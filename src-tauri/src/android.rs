//! Android integration.
//!
//! Three things work differently on a phone, and all of them are handled here
//! so the rest of the crate keeps its desktop shape:
//!
//! 1. **Where things live.** There is no `%APPDATA%`. The Kotlin side reports
//!    the app's private files directory and the shared Downloads folder, and
//!    `paths` is told about both before anything opens the database.
//! 2. **How tools run.** Since Android 10 an app may not execute a file from
//!    its own data directory. The only executable location is the APK's native
//!    library directory, so Python, FFmpeg, ffprobe and QuickJS ship there as
//!    `lib*.so` files (the packaging the youtubedl-android project maintains).
//!    The libraries they link against ship zipped beside them and are unpacked
//!    once per install; `command` then starts a tool with the environment that
//!    lets it find them. yt-dlp itself is a Python zipapp, fetched like on the
//!    desktop and run by that interpreter.
//! 3. **What only the OS can do.** Keeping downloads alive once the app is in
//!    the background, opening a finished file in another app, picking files to
//!    convert and receiving links shared from other apps all go through the
//!    `BridgePlugin` Kotlin class.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tauri::plugin::mobile::PluginInvokeError;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Listener, Manager, Wry};
use tokio::process::Command;
use tokio::sync::watch;

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::{converter, export, log_info, log_warn, paths, queue, range};

const PLUGIN_PACKAGE: &str = "io.universaldownloader.app";
const PLUGIN_CLASS: &str = "BridgePlugin";

/// Downloads land in a folder of their own inside the shared Downloads
/// directory, which is where a phone's file manager and gallery look.
const DOWNLOAD_FOLDER: &str = "Universal Downloader";

const PYTHON: &str = "libpython.so";
const PYTHON_ARCHIVE: &str = "libpython.zip.so";
const FFMPEG: &str = "libffmpeg.so";
const FFMPEG_ARCHIVE: &str = "libffmpeg.zip.so";
const QUICKJS: &str = "libqjs.so";

/// Kept in its own directory: the unpacked runtime is replaced whole.
const PYTHON_STARTUP: &str = "python-startup";
const SITECUSTOMIZE: &str = include_str!("sitecustomize.py");

/// The code `BridgePlugin.installApk` rejects with when the user did not let
/// this app install others.
const INSTALL_PERMISSION_DENIED: &str = "INSTALL_PERMISSION_DENIED";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Environment {
    native_library_dir: PathBuf,
    files_dir: PathBuf,
    cache_dir: PathBuf,
    downloads_dir: PathBuf,
}

static ENVIRONMENT: OnceCell<Environment> = OnceCell::new();

pub struct Bridge(PluginHandle<Wry>);

impl Bridge {
    fn call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        payload: impl Serialize,
    ) -> AppResult<T> {
        self.0
            .run_mobile_plugin(command, payload)
            .map_err(|err| match err {
                PluginInvokeError::InvokeRejected(ref response)
                    if response.code.as_deref() == Some(INSTALL_PERMISSION_DENIED) =>
                {
                    AppError::Permission("installing apps from this source is not allowed".into())
                }
                _ => AppError::Other(format!("{command} failed: {err}")),
            })
    }
}

/// Registers the Kotlin half. Plugins are set up before the app's own `setup`,
/// which is what lets `init` query it from there.
pub fn plugin() -> TauriPlugin<Wry> {
    Builder::new("bridge")
        .setup(|app, api| {
            let handle = api.register_android_plugin(PLUGIN_PACKAGE, PLUGIN_CLASS)?;
            app.manage(Bridge(handle));
            Ok(())
        })
        .build()
}

fn environment() -> &'static Environment {
    ENVIRONMENT
        .get()
        .expect("android::init runs during setup, before any tool is started")
}

/// Resolve the platform directories. Must run before `paths::root()` is read.
pub fn init(app: &AppHandle) -> AppResult<()> {
    let env: Environment = app.state::<Bridge>().call("environment", ())?;
    paths::set_platform_dirs(env.files_dir.clone(), env.downloads_dir.join(DOWNLOAD_FOLDER))?;
    let _ = ENVIRONMENT.set(env);
    Ok(())
}

// -- the bundled runtime ------------------------------------------------------

fn native_lib(name: &str) -> PathBuf {
    environment().native_library_dir.join(name)
}

pub fn ffmpeg_binary() -> PathBuf {
    native_lib(FFMPEG)
}

pub fn quickjs_binary() -> PathBuf {
    native_lib(QUICKJS)
}

fn runtime_dir(name: &str) -> AppResult<PathBuf> {
    Ok(paths::tools_dir()?.join("runtime").join(name))
}

/// Unpack the libraries Python and FFmpeg link against, and put the app's own
/// start-up code for Python beside them.
///
/// Each archive is stamped with the size, modification time and location of
/// the copy it came from. An app update installs the APK's libraries afresh,
/// which changes all three, so a stale runtime is replaced without having to
/// know anything about version numbers.
pub fn prepare_runtime(force: bool) -> AppResult<()> {
    unpack(PYTHON_ARCHIVE, "python", force)?;
    unpack(FFMPEG_ARCHIVE, "ffmpeg", force)?;
    write_python_startup()
}

/// Python imports a `sitecustomize` module from its path before the program it
/// runs; this app's gives the engine the same DNS fallback that the app's own
/// requests have (see `net::dns`). It is written whenever it differs from the
/// copy built into this version of the app.
fn write_python_startup() -> AppResult<()> {
    let dir = runtime_dir(PYTHON_STARTUP)?;
    let file = dir.join("sitecustomize.py");
    if std::fs::read(&file).ok().as_deref() == Some(SITECUSTOMIZE.as_bytes()) {
        return Ok(());
    }
    std::fs::create_dir_all(&dir)?;
    let staging = file.with_extension("staging");
    std::fs::write(&staging, SITECUSTOMIZE)?;
    std::fs::rename(&staging, &file)?;
    Ok(())
}

fn unpack(archive_name: &str, dir_name: &str, force: bool) -> AppResult<()> {
    let archive = native_lib(archive_name);
    let meta = std::fs::metadata(&archive).map_err(|err| {
        AppError::Other(format!("{} is missing from the app: {err}", archive.display()))
    })?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|age| age.as_secs())
        .unwrap_or(0);
    let stamp = format!("{}:{}:{}", meta.len(), modified, archive.display());

    let dest = runtime_dir(dir_name)?;
    let stamp_file = dest.join(".stamp");
    if !force && std::fs::read_to_string(&stamp_file).ok().as_deref() == Some(stamp.as_str()) {
        return Ok(());
    }

    log_info!("android", "unpacking {archive_name}");
    let started = std::time::Instant::now();

    // Unpacked beside the live copy and swapped in whole, so a tool never
    // starts against half a set of libraries.
    let staging = dest.with_extension("staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    if let Err(err) = extract(&archive, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }
    std::fs::write(staging.join(".stamp"), &stamp)?;

    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&staging, &dest)?;

    log_info!(
        "android",
        "unpacked {archive_name} in {} ms",
        started.elapsed().as_millis()
    );
    Ok(())
}

/// Extract an archive that carries Unix symlinks: a library such as `libz.so`
/// is stored as a link to `libz.so.1.3.1`, and the loader resolves the name it
/// was linked against, so the links have to be recreated rather than written
/// out as small text files.
fn extract(archive: &Path, dest: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|err| AppError::Other(format!("{} could not be read: {err}", archive.display())))?;

    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|err| AppError::Other(format!("{} is damaged: {err}", archive.display())))?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let out = dest.join(relative);

        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if entry.is_symlink() {
            let mut target = String::new();
            entry.read_to_string(&mut target)?;
            // Only links that stay inside the archive's own tree are honoured.
            if target.starts_with('/') || target.split('/').any(|part| part == "..") {
                continue;
            }
            let _ = std::fs::remove_file(&out);
            std::os::unix::fs::symlink(target, &out)?;
        } else {
            let mut writer = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut writer)?;
        }
    }
    Ok(())
}

/// Build the command that runs `program`.
///
/// A binary from the native library directory is started directly. Anything
/// else is a Python program -- in practice yt-dlp -- and is handed to the
/// bundled interpreter. Both get the same environment: the unpacked library
/// directories, the interpreter's home, certificate bundle and start-up code,
/// and a writable temp directory. FFmpeg is started by yt-dlp as well as by
/// the app, so one environment has to suit both.
pub fn command(program: &Path) -> Command {
    let env = environment();

    let mut cmd = if program.starts_with(&env.native_library_dir) {
        Command::new(program)
    } else {
        let mut cmd = Command::new(native_lib(PYTHON));
        cmd.arg(program);
        cmd
    };

    let python = runtime_dir("python").unwrap_or_default();
    let ffmpeg = runtime_dir("ffmpeg").unwrap_or_default();
    let library_path = format!(
        "{}:{}",
        python.join("usr/lib").display(),
        ffmpeg.join("usr/lib").display()
    );
    let search_path = match std::env::var("PATH") {
        Ok(existing) if !existing.is_empty() => {
            format!("{existing}:{}", env.native_library_dir.display())
        }
        _ => env.native_library_dir.display().to_string(),
    };

    // The phone's resolver has lately been failing where the public ones
    // answered: the engine asks those first instead of waiting on it again.
    if crate::net::dns::prefer_fallback() {
        cmd.env("UD_DNS_FALLBACK_FIRST", "1");
    }

    cmd.env("LD_LIBRARY_PATH", library_path)
        .env("PYTHONHOME", python.join("usr"))
        .env("PYTHONPATH", runtime_dir(PYTHON_STARTUP).unwrap_or_default())
        .env("SSL_CERT_FILE", python.join("usr/etc/tls/cert.pem"))
        .env("HOME", &env.files_dir)
        .env("TMPDIR", &env.cache_dir)
        .env("PATH", search_path)
        // Titles are rarely ASCII; without this Python falls back to the C
        // locale and fails on the first one that is not.
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    cmd
}

// -- work in the background ---------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundWork {
    active: bool,
    downloads: u32,
    conversions: u32,
    language: String,
}

/// Keep a foreground service running while anything is downloading or
/// converting.
///
/// Without one, Android freezes a backgrounded app within seconds, and a
/// download the user left running would simply stop when the screen turned
/// off. The service only exists while there is work, so an idle app leaves
/// no notification behind.
///
/// The editor's two jobs are work in the same sense and are counted in the
/// same two numbers, so that the notification needs no words of its own: a
/// link brought into the editor is a download, and an export is a conversion
/// of the file that is open. Both count from the moment they start until the
/// state they publish says they ended, finished or failed or cancelled.
///
/// State changes are funnelled through a watch channel to one task: the queue
/// can change state from any thread, including the one Android would need to
/// run the call on, and only the latest state matters anyway. An export and a
/// fetch publish their progress through the same event as their state, several
/// times a second, so a count that has not changed is dropped before it wakes
/// anything.
pub fn keep_alive_while_busy(app: &AppHandle) {
    let (tx, mut rx) = watch::channel((0u32, 0u32));
    let tx = Arc::new(tx);

    for event in [
        queue::EVENT_CHANGED,
        converter::EVENT_CHANGED,
        export::EVENT_CHANGED,
        range::EVENT_CHANGED,
    ] {
        let app_handle = app.clone();
        let tx = Arc::clone(&tx);
        app.listen(event, move |_| {
            let Some(state) = app_handle.try_state::<AppState>() else {
                return;
            };
            // Counted inside the channel's lock rather than before taking it.
            // Four sources report here from their own threads, and a count
            // read first and sent second can land after a newer one: a
            // download's last event read while an export was still running,
            // sent just after the export's own "finished", would leave the
            // notification saying it is running with nothing left to correct
            // it.
            tx.send_if_modified(|current| {
                let counts = work_counts(&state);
                let changed = *current != counts;
                *current = counts;
                changed
            });
        });
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut running = false;
        while rx.changed().await.is_ok() {
            let (downloads, conversions) = *rx.borrow_and_update();
            let active = downloads + conversions > 0;
            // Nothing running and nothing to stop.
            if !active && !running {
                continue;
            }
            running = active;

            let language = app
                .try_state::<AppState>()
                .map(|state| state.settings().language)
                .unwrap_or_else(|| "en".into());
            let handle = app.clone();
            let work = BackgroundWork {
                active,
                downloads,
                conversions,
                language,
            };
            let result = tauri::async_runtime::spawn_blocking(move || {
                handle
                    .state::<Bridge>()
                    .call::<serde_json::Value>("setBackgroundWork", work)
            })
            .await;
            if let Ok(Err(err)) = result {
                log_warn!("android", "background work could not be updated: {err}");
            }
        }
    });
}

/// What the notification counts, as `(downloads, conversions)`.
fn work_counts(state: &AppState) -> (u32, u32) {
    let downloads =
        state.queue.active_count() + state.queue.queued_count() + state.fetcher.active_count();
    let conversions = state.converter.active_count() + state.exporter.active_count();
    (downloads, conversions)
}

/// Tell the media index about a finished file.
///
/// A file written by path into shared storage is on disk but not in the index
/// that the gallery, music players and the system file picker read from, so
/// without this a finished download would seem not to exist anywhere but in
/// this app.
pub fn announce_media(app: &AppHandle, path: &str) {
    let app = app.clone();
    let path = path.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let result = app
            .state::<Bridge>()
            .call::<serde_json::Value>("scanFile", PathArgs { path: path.clone() });
        if let Err(err) = result {
            log_warn!("android", "{path} could not be added to the media index: {err}");
        }
    });
}

/// Let the WebView stream one file to the editor's picture.
///
/// The asset protocol's answers cannot be played from on a phone past the first
/// of them (see `MediaStreamClient.kt`), so the Kotlin side answers for the
/// files allowed here, and only for those. Called straight from the command,
/// as `init` is: the answer must be in place before the `<video>` asks.
pub fn allow_media(app: &AppHandle, path: &Path) -> AppResult<()> {
    app.state::<Bridge>()
        .call::<serde_json::Value>("allowMedia", PathArgs { path: path.to_string_lossy().into_owned() })
        .map(|_| ())
}

// -- commands -----------------------------------------------------------------

#[derive(Serialize)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
struct PickedFiles {
    paths: Vec<String>,
}

#[derive(Deserialize)]
struct SharedText {
    text: Option<String>,
}

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    call: impl FnOnce(&Bridge) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(move || call(&app.state::<Bridge>()))
        .await
        .map_err(|err| AppError::Other(format!("the platform call did not complete: {err}")))?
}

/// Open a finished file in whichever app handles its type.
pub async fn open_file(app: AppHandle, path: String) -> AppResult<()> {
    blocking(app, move |bridge| {
        bridge.call::<serde_json::Value>("openFile", PathArgs { path })
    })
    .await
    .map(|_| ())
}

/// There is no folder window to reveal a file in; the system's Downloads view
/// is the closest thing, and it is where the file is.
pub async fn open_downloads(app: AppHandle) -> AppResult<()> {
    blocking(app, |bridge| bridge.call::<serde_json::Value>("openDownloads", ()))
        .await
        .map(|_| ())
}

/// The app's page in the system settings, where it is allowed to use Wi-Fi and
/// mobile data.
pub async fn open_app_settings(app: AppHandle) -> AppResult<()> {
    blocking(app, |bridge| bridge.call::<serde_json::Value>("openAppSettings", ()))
        .await
        .map(|_| ())
}

/// What Android knows about this app's connection.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    /// The phone has a network, whether or not this app may use it.
    pub connected: bool,
    /// Android is not letting this app use the network: data restrictions,
    /// battery rules, or a VPN that blocks connections without it.
    pub blocked: bool,
    #[serde(default)]
    pub vpn: bool,
    /// The network has been checked to reach the internet.
    #[serde(default)]
    pub validated: bool,
    /// The Private DNS server in use: a host name, or "automatic".
    #[serde(default)]
    pub private_dns: Option<String>,
    #[serde(default)]
    pub data_saver: bool,
}

impl NetworkStatus {
    /// One line for the technical details, e.g. "connected, VPN, Private DNS
    /// dns.example".
    pub fn describe(&self) -> String {
        let state = match (self.connected, self.blocked, self.validated) {
            (false, _, _) => "no network",
            (true, true, _) => "connected, blocked for this app",
            (true, false, true) => "connected",
            (true, false, false) => "connected, internet not confirmed",
        };
        let mut facts = vec![state.to_string()];
        if self.vpn {
            facts.push("VPN".into());
        }
        if let Some(server) = &self.private_dns {
            facts.push(format!("Private DNS {server}"));
        }
        if self.data_saver {
            facts.push("Data Saver".into());
        }
        facts.join(", ")
    }
}

pub async fn network_status(app: AppHandle) -> AppResult<NetworkStatus> {
    blocking(app, |bridge| bridge.call::<NetworkStatus>("networkStatus", ())).await
}

/// Let the user choose media files to convert.
///
/// The picker hands back content URIs, which neither FFmpeg nor `std::fs` can
/// open, so the Kotlin side copies each choice into the cache and returns
/// those paths instead.
pub async fn pick_media_files(app: AppHandle) -> AppResult<Vec<String>> {
    blocking(app, |bridge| bridge.call::<PickedFiles>("pickMediaFiles", ()))
        .await
        .map(|picked| picked.paths)
}

#[derive(Serialize)]
struct SystemBars {
    dark: bool,
}

/// The status and navigation bars sit over the page's own background, so their
/// icons have to follow the app's theme rather than the system's.
pub async fn set_system_bars(app: AppHandle, dark: bool) -> AppResult<()> {
    blocking(app, move |bridge| {
        bridge.call::<serde_json::Value>("setSystemBarsTheme", SystemBars { dark })
    })
    .await
    .map(|_| ())
}

/// Where a downloaded update waits for the installer. It has to be inside the
/// cache directory, which is what the file provider shares with the installer.
pub fn update_dir() -> PathBuf {
    environment().cache_dir.join("updates")
}

/// Hand a downloaded APK to the system installer, first sending the user to
/// allow installs from this app if they have not yet.
pub async fn install_apk(app: AppHandle, path: String) -> AppResult<()> {
    blocking(app, move |bridge| {
        bridge.call::<serde_json::Value>("installApk", PathArgs { path })
    })
    .await
    .map(|_| ())
}

/// A link shared to the app from another one, if one is waiting.
pub async fn take_shared_text(app: AppHandle) -> AppResult<Option<String>> {
    blocking(app, |bridge| bridge.call::<SharedText>("takeSharedText", ()))
        .await
        .map(|shared| shared.text)
}
