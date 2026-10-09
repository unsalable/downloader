//! SQLite persistence: settings, history and the durable queue.
//!
//! The database is intentionally tiny. It is opened once, guarded by a mutex
//! (every statement here is sub-millisecond), and runs in WAL mode so a write
//! never blocks the UI thread's reads.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::model::{DownloadRequest, DownloadTask, HistoryEntry, PlatformId};
use crate::settings::Settings;

const SCHEMA_VERSION: i32 = 1;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| AppError::Database("database lock was poisoned".into()))
    }

    fn migrate(&self) -> AppResult<()> {
        let conn = self.lock()?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS settings (
                    key   TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS downloads (
                    id            INTEGER PRIMARY KEY AUTOINCREMENT,
                    url           TEXT    NOT NULL,
                    title         TEXT    NOT NULL,
                    platform      TEXT    NOT NULL,
                    thumbnail_url TEXT,
                    file_path     TEXT    NOT NULL,
                    container     TEXT    NOT NULL,
                    quality_label TEXT    NOT NULL,
                    file_size     INTEGER,
                    created_at    INTEGER NOT NULL,
                    status        TEXT    NOT NULL,
                    request_json  TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_downloads_created
                    ON downloads (created_at DESC);

                CREATE TABLE IF NOT EXISTS queue (
                    id         TEXT    PRIMARY KEY,
                    position   INTEGER NOT NULL,
                    task_json  TEXT    NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                "#,
            )?;
        }

        if version != SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    // -- settings ----------------------------------------------------------

    pub fn load_settings(&self) -> AppResult<Settings> {
        let conn = self.lock()?;
        Ok(read_settings(&conn)?)
    }

    /// Whether settings have ever been saved. `load_settings` writes nothing,
    /// so until something is saved the app is still on its first launch after
    /// install, running on defaults nobody has chosen.
    pub fn settings_saved(&self) -> AppResult<bool> {
        let conn = self.lock()?;
        let found: Option<i64> = conn
            .query_row("SELECT 1 FROM settings WHERE key = 'app'", [], |row| row.get(0))
            .optional()?;
        Ok(found.is_some())
    }

    pub fn save_settings(&self, settings: &Settings) -> AppResult<()> {
        let json = serde_json::to_string(settings)?;
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('app', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![json],
        )?;
        Ok(())
    }

    // -- history -----------------------------------------------------------

    pub fn history_insert(&self, entry: &HistoryEntry) -> AppResult<i64> {
        let request_json = entry
            .request
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO downloads
                (url, title, platform, thumbnail_url, file_path, container,
                 quality_label, file_size, created_at, status, request_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.url,
                entry.title,
                platform_to_str(entry.platform),
                entry.thumbnail_url,
                entry.file_path,
                entry.container,
                entry.quality_label,
                entry.file_size.map(|v| v as i64),
                entry.created_at,
                entry.status,
                request_json,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn history_list(&self, query: Option<&str>, limit: u32, offset: u32) -> AppResult<Vec<HistoryEntry>> {
        let conn = self.lock()?;
        let sql = if query.is_some() {
            "SELECT id, url, title, platform, thumbnail_url, file_path, container,
                    quality_label, file_size, created_at, status, request_json
             FROM downloads
             WHERE title LIKE ?1 OR url LIKE ?1
             ORDER BY created_at DESC LIMIT ?2 OFFSET ?3"
        } else {
            "SELECT id, url, title, platform, thumbnail_url, file_path, container,
                    quality_label, file_size, created_at, status, request_json
             FROM downloads
             ORDER BY created_at DESC LIMIT ?2 OFFSET ?3"
        };

        let mut stmt = conn.prepare(sql)?;
        let pattern = query.map(|q| format!("%{q}%")).unwrap_or_default();
        let rows = stmt.query_map(params![pattern, limit, offset], |row| {
            let request_json: Option<String> = row.get(11)?;
            let file_path: String = row.get(5)?;
            Ok(HistoryEntry {
                id: row.get(0)?,
                url: row.get(1)?,
                title: row.get(2)?,
                platform: platform_from_str(&row.get::<_, String>(3)?),
                thumbnail_url: row.get(4)?,
                // Checked at read time: files get moved and deleted outside the
                // app, so a stored flag would go stale immediately.
                file_exists: Path::new(&file_path).exists(),
                file_path,
                container: row.get(6)?,
                quality_label: row.get(7)?,
                file_size: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
                created_at: row.get(9)?,
                status: row.get(10)?,
                request: request_json
                    .and_then(|json| serde_json::from_str::<DownloadRequest>(&json).ok()),
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// How many entries were recorded at or after `since` (ms since the epoch),
    /// or in all, when it is `None`. Two fixed statements rather than one with
    /// `?1 IS NULL OR ...`, as `history_list` does it: both forms are answered
    /// from `idx_downloads_created` alone.
    pub fn history_count(&self, since: Option<i64>) -> AppResult<u32> {
        let conn = self.lock()?;
        let count: i64 = match since {
            Some(since) => conn.query_row(
                "SELECT COUNT(*) FROM downloads WHERE created_at >= ?1",
                params![since],
                |row| row.get(0),
            )?,
            None => conn.query_row("SELECT COUNT(*) FROM downloads", [], |row| row.get(0))?,
        };
        Ok(count as u32)
    }

    pub fn history_delete(&self, id: i64) -> AppResult<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Remove the entries recorded at or after `since` (all of them when `None`)
    /// and say how many went. Rows only: the files they point at are the user's,
    /// and stay exactly where they are.
    pub fn history_clear(&self, since: Option<i64>) -> AppResult<u32> {
        let conn = self.lock()?;
        let removed = match since {
            Some(since) => {
                conn.execute("DELETE FROM downloads WHERE created_at >= ?1", params![since])?
            }
            None => conn.execute("DELETE FROM downloads", [])?,
        };
        Ok(removed as u32)
    }

    // -- queue -------------------------------------------------------------

    /// Replaces the persisted queue wholesale. Called on every queue mutation;
    /// the list is short enough that a diff would be more code than value.
    pub fn queue_replace(&self, tasks: &[DownloadTask]) -> AppResult<()> {
        let now = crate::util::now_ms();
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM queue", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO queue (id, position, task_json, updated_at) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (index, task) in tasks.iter().enumerate() {
                stmt.execute(params![
                    task.id,
                    index as i64,
                    serde_json::to_string(task)?,
                    now
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn queue_load(&self) -> AppResult<Vec<DownloadTask>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT task_json FROM queue ORDER BY position ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

        let mut tasks = Vec::new();
        for row in rows {
            // Skip rather than fail: one unreadable row should not cost the
            // user their whole queue.
            if let Ok(task) = serde_json::from_str::<DownloadTask>(&row?) {
                tasks.push(task);
            }
        }
        Ok(tasks)
    }

    pub fn vacuum(&self) -> AppResult<()> {
        let conn = self.lock()?;
        conn.execute_batch("VACUUM")?;
        Ok(())
    }
}

/// The settings as `load_settings` reads them, for a process that is not the
/// app: the browser link's host, which reads the user's defaults to work out
/// what a download would fetch.
///
/// The app may well be running and holding this file, and the host must not
/// disturb it, so the file is opened read-only and is never created or
/// migrated: a missing database is an app that has not run yet, and its
/// defaults are the answer. A lock the app holds is waited on briefly rather
/// than for as long as the app holds it. Any failure at all -- no file, a
/// locked one, one that is not a database -- reads as the defaults, which is
/// what the app itself would be running on.
pub fn load_settings_read_only(path: &Path) -> Settings {
    let read = || -> rusqlite::Result<Settings> {
        let conn = open_read_only(path)?;
        conn.busy_timeout(Duration::from_millis(1500))?;
        read_settings(&conn)
    };

    let mut settings = read().unwrap_or_default();
    settings.sanitize();
    settings
}

/// Open `path` for reading, creating nothing beside it.
///
/// A database in WAL mode is read through its `-wal` and `-shm` files, and
/// SQLite creates them when they are missing -- for a read-only connection as
/// well, which then cannot remove them again (measured). They are only missing
/// when nothing has the database open and every write has reached the main
/// file, so in that case the main file is all there is to read, and it is read
/// as it stands (`immutable`) without them. An address SQLite will not take as
/// a URI -- a profile on a network share -- is read the ordinary way.
fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    // `SQLITE_OPEN_URI` is left out of the ordinary open on purpose: that is a
    // path, and read as a URI a `?` or `#` in it would mean something else.
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    if path.is_file() && !Path::new(&wal).exists() {
        if let Ok(mut uri) = reqwest::Url::from_file_path(path) {
            uri.set_query(Some("immutable=1"));
            if let Ok(conn) = Connection::open_with_flags(uri.as_str(), flags | OpenFlags::SQLITE_OPEN_URI) {
                return Ok(conn);
            }
        }
    }
    Connection::open_with_flags(path, flags)
}

/// The one row settings live in, parsed and made safe.
fn read_settings(conn: &Connection) -> rusqlite::Result<Settings> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = 'app'", [], |row| {
            row.get(0)
        })
        .optional()?;

    let mut settings = match raw {
        // A config that fails to parse (downgrade, hand edit) should not
        // block startup -- fall back to defaults and let the user re-save.
        Some(json) => serde_json::from_str::<Settings>(&json).unwrap_or_default(),
        None => Settings::default(),
    };
    settings.sanitize();
    Ok(settings)
}

pub fn platform_to_str(platform: PlatformId) -> &'static str {
    platform.slug()
}

pub fn platform_from_str(value: &str) -> PlatformId {
    match value {
        "youtube" => PlatformId::Youtube,
        "tiktok" => PlatformId::Tiktok,
        "instagram" => PlatformId::Instagram,
        "x" | "twitter" => PlatformId::Twitter,
        "reddit" => PlatformId::Reddit,
        "facebook" => PlatformId::Facebook,
        "twitch" => PlatformId::Twitch,
        "pinterest" => PlatformId::Pinterest,
        "vimeo" => PlatformId::Vimeo,
        "dailymotion" => PlatformId::Dailymotion,
        "soundcloud" => PlatformId::Soundcloud,
        "direct" => PlatformId::Direct,
        "web" | "generic" => PlatformId::Generic,
        _ => PlatformId::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::HistoryRange;

    #[test]
    fn settings_count_as_saved_only_once_something_is_saved() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        assert!(!db.settings_saved().unwrap(), "a fresh install has saved nothing");
        // Reading hands back the defaults without writing them.
        let defaults = db.load_settings().unwrap();
        assert!(!db.settings_saved().unwrap());

        db.save_settings(&Settings {
            language: "tr".into(),
            ..defaults
        })
        .unwrap();
        assert!(db.settings_saved().unwrap());
        assert_eq!(db.load_settings().unwrap().language, "tr");
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ud-db-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn chosen() -> Settings {
        Settings {
            default_mode: crate::model::DownloadMode::Audio,
            default_quality: crate::model::QualityPreference::AudioBitrate { kbps: 192 },
            default_container: Some("mp3".into()),
            language: "tr".into(),
            ..Settings::default()
        }
    }

    #[test]
    fn another_process_reads_what_the_app_saved_without_writing_a_byte() {
        let dir = scratch("read-only");
        let path = dir.join("library.db");
        let db = Database::open(&path).unwrap();
        db.save_settings(&chosen()).unwrap();

        // While the app holds the file, as it does whenever it is running.
        let read = load_settings_read_only(&path);
        assert_eq!(read.default_mode, crate::model::DownloadMode::Audio);
        assert_eq!(read.default_quality, chosen().default_quality);
        assert_eq!(read.default_container.as_deref(), Some("mp3"));
        assert_eq!(read.language, "tr");

        // And once it has closed it, which takes its `-wal` and `-shm` away.
        // Nothing is put back in their place.
        drop(db);
        assert_eq!(files_in(&dir), ["library.db"]);
        let before = std::fs::read(&path).unwrap();
        let read = load_settings_read_only(&path);
        assert_eq!(read.default_container.as_deref(), Some("mp3"));
        assert_eq!(read.language, "tr");
        assert_eq!(std::fs::read(&path).unwrap(), before, "the database was written to");
        assert_eq!(files_in(&dir), ["library.db"], "files were created beside the database");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_folder_name_a_uri_would_misread_is_read_as_the_folder_it_is() {
        // Spaces, a `#`, a `%` and letters outside ASCII: everything the
        // address SQLite is handed has to escape.
        let dir = scratch("Melikşah's 100% #1 folder");
        let path = dir.join("library.db");
        let db = Database::open(&path).unwrap();
        db.save_settings(&chosen()).unwrap();
        drop(db);

        assert_eq!(load_settings_read_only(&path).default_container.as_deref(), Some("mp3"));
        // Read as it stands, not through a WAL it had to create.
        assert_eq!(files_in(&dir), ["library.db"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_database_that_is_not_there_is_not_created_and_reads_as_the_defaults() {
        let dir = scratch("missing");
        let path = dir.join("library.db");

        let read = load_settings_read_only(&path);
        assert_eq!(read.default_mode, Settings::default().default_mode);
        assert_eq!(read.default_container, None);
        assert!(!path.exists(), "the database was created");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "something was created beside it");

        // Nor is anything that is not a database read as one.
        std::fs::write(&path, b"not a database").unwrap();
        assert_eq!(load_settings_read_only(&path).language, "en");
        assert_eq!(std::fs::read(&path).unwrap(), b"not a database");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_database_from_before_any_settings_reads_as_the_defaults() {
        let dir = scratch("empty");
        let path = dir.join("library.db");
        drop(Database::open(&path).unwrap());

        let read = load_settings_read_only(&path);
        assert_eq!(read.language, "en");
        assert_eq!(read.default_container, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- clearing the history by range ---------------------------------------

    const HOUR: i64 = 3_600_000;

    fn entry(created_at: i64, file_path: &str) -> HistoryEntry {
        HistoryEntry {
            id: 0,
            url: "https://www.youtube.com/watch?v=a".into(),
            title: format!("at {created_at}"),
            platform: PlatformId::Youtube,
            thumbnail_url: None,
            file_path: file_path.into(),
            file_exists: true,
            container: "mp4".into(),
            quality_label: "1080p".into(),
            file_size: Some(1),
            created_at,
            status: "completed".into(),
            request: None,
        }
    }

    #[test]
    fn a_range_counts_and_clears_only_what_is_newer_than_its_start() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let now = crate::util::now_ms();
        for hours in [1, 23, 25, 144, 192, 720] {
            db.history_insert(&entry(now - hours * HOUR, "")).unwrap();
        }

        let day = HistoryRange::Day.cutoff(now);
        let week = HistoryRange::Week.cutoff(now);
        assert_eq!(db.history_count(day).unwrap(), 2);
        assert_eq!(db.history_count(week).unwrap(), 4);
        assert_eq!(db.history_count(None).unwrap(), 6);

        // The day goes, and everything older than it stays, in its order.
        assert_eq!(db.history_clear(day).unwrap(), 2);
        let ages: Vec<i64> = db
            .history_list(None, 10, 0)
            .unwrap()
            .iter()
            .map(|entry| (now - entry.created_at) / HOUR)
            .collect();
        assert_eq!(ages, [25, 144, 192, 720]);

        // The week takes what the day left of it, and all of it the rest.
        assert_eq!(db.history_clear(week).unwrap(), 2);
        assert_eq!(db.history_clear(None).unwrap(), 2);
        assert_eq!(db.history_count(None).unwrap(), 0);
    }

    #[test]
    fn an_entry_exactly_at_the_cutoff_belongs_to_the_range() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        let cutoff = 1_800_000_000_000 - 24 * HOUR;
        db.history_insert(&entry(cutoff, "")).unwrap();
        db.history_insert(&entry(cutoff - 1, "")).unwrap();

        assert_eq!(db.history_count(Some(cutoff)).unwrap(), 1);
        assert_eq!(db.history_clear(Some(cutoff)).unwrap(), 1);
        let left = db.history_list(None, 10, 0).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].created_at, cutoff - 1);
    }

    #[test]
    fn clearing_the_history_leaves_the_files_where_they_are() {
        // The database is kept in memory, so nothing in the folder is held open
        // when it is removed at the end -- on Windows an open file would keep
        // the folder there.
        let dir = scratch("clear-keeps-files");
        let clip = dir.join("clip.mp4");
        std::fs::write(&clip, b"video").unwrap();
        let path = clip.to_string_lossy();

        let db = Database::open(Path::new(":memory:")).unwrap();
        let now = crate::util::now_ms();
        db.history_insert(&entry(now, &path)).unwrap();
        db.history_insert(&entry(1, &path)).unwrap();

        // A range, and then the rest: neither reaches the file.
        assert_eq!(db.history_clear(HistoryRange::Day.cutoff(now)).unwrap(), 1);
        assert_eq!(db.history_clear(None).unwrap(), 1);
        assert_eq!(std::fs::read(&clip).unwrap(), b"video");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clearing_an_empty_range_removes_nothing() {
        // What `clear_history` reads as nothing to compact afterwards.
        let db = Database::open(Path::new(":memory:")).unwrap();
        let now = crate::util::now_ms();
        db.history_insert(&entry(now - 8 * 24 * HOUR, "")).unwrap();
        db.history_insert(&entry(now - 30 * 24 * HOUR, "")).unwrap();

        assert_eq!(db.history_clear(HistoryRange::Day.cutoff(now)).unwrap(), 0);
        assert_eq!(db.history_count(None).unwrap(), 2);
    }
}
