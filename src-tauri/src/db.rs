//! SQLite persistence: settings, history and the durable queue.
//!
//! The database is intentionally tiny. It is opened once, guarded by a mutex
//! (every statement here is sub-millisecond), and runs in WAL mode so a write
//! never blocks the UI thread's reads.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

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

    pub fn history_count(&self) -> AppResult<u32> {
        let conn = self.lock()?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM downloads", [], |row| row.get(0))?;
        Ok(count as u32)
    }

    pub fn history_delete(&self, id: i64) -> AppResult<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn history_clear(&self) -> AppResult<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM downloads", [])?;
        Ok(())
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
