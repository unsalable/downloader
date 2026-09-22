//! Discovery and installation of the external tools.
//!
//! None of them is bundled in the installer: yt-dlp changes weekly (a bundled
//! copy would be stale the month after release), FFmpeg is GPL, which is
//! cleanest to keep as a separate process the user opts into, and the
//! JavaScript runtime is 40 MB that plenty of installs never need. All are
//! fetched once, on request, into the app's own data directory -- and a copy
//! the user already has on PATH is preferred over downloading anything at all.
//!
//! Android differs in two respects: nothing an app downloads may be executed
//! there, so FFmpeg ships inside the APK and is always present, and the
//! JavaScript runtime is not a tool at all -- the APK carries QuickJS, which
//! `android.rs` hands the engine directly. yt-dlp is still fetched on request
//! -- it is a Python program, and the interpreter that runs it is the part the
//! APK carries.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::time::Duration;

use futures_util::StreamExt;
use once_cell::sync::Lazy;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::{AppError, AppResult};
use crate::model::{ToolKind, ToolSource, ToolStatus, ToolsState};
use crate::settings::Settings;
use crate::{log_info, log_warn, paths, process};

#[cfg(not(target_os = "android"))]
const ENGINE_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";

/// The platform-independent zipapp, run by the Python the APK bundles.
#[cfg(target_os = "android")]
const ENGINE_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp";

/// The shared build is 77 MB against 170 MB for the static one, and the extra
/// DLLs land in the same directory as the executable.
#[cfg(not(target_os = "android"))]
const FFMPEG_URL: &str =
    "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl-shared.zip";

#[cfg(not(target_os = "android"))]
const ENGINE_EXE: &str = "yt-dlp.exe";
#[cfg(target_os = "android")]
const ENGINE_EXE: &str = "yt-dlp";
#[cfg(not(target_os = "android"))]
const FFMPEG_EXE: &str = "ffmpeg.exe";

/// Deno rather than Node or Bun: it is the one runtime yt-dlp looks for by
/// itself, so it is the best-supported path and keeps working even if this app
/// stops naming it. The Windows release is a single executable in a zip.
#[cfg(not(target_os = "android"))]
const JS_RUNTIME_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";

#[cfg(not(target_os = "android"))]
const JS_RUNTIME_EXE: &str = "deno.exe";

/// The managed runtime's own directory. Not `runtime`, which on Android is
/// where the libraries unpacked from the APK go.
#[cfg(not(target_os = "android"))]
const JS_RUNTIME_DIR: &str = "js";

static STATE: Lazy<RwLock<ToolsState>> = Lazy::new(|| RwLock::new(blank_state()));

fn blank_state() -> ToolsState {
    ToolsState {
        engine: ToolStatus::missing(ToolKind::Engine),
        ffmpeg: ToolStatus::missing(ToolKind::Ffmpeg),
        #[cfg(not(target_os = "android"))]
        js_runtime: ToolStatus::missing(ToolKind::JsRuntime),
    }
}

/// Serialises discovery. A pass runs a subprocess per tool and then publishes
/// what it saw, so two overlapping passes let the slower one publish a view from
/// before the faster one's install finished -- a tool the UI had just shown as
/// installed would then read as missing to everything else until a restart.
static DISCOVERY: Lazy<AsyncMutex<()>> = Lazy::new(|| AsyncMutex::new(()));

/// Set once the first pass has published, so the placeholder "missing" state
/// above is never handed out as if it were an answer.
static DISCOVERED: AtomicBool = AtomicBool::new(false);

/// A version check is normally well under a second, but the first run of a
/// freshly downloaded yt-dlp unpacks itself and is scanned on the way, which on
/// a slow machine can take tens of seconds. This only bounds a tool that hangs.
const VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(90);

/// Pauses between attempts to start a tool that has just been written to disk.
/// Real-time antivirus scanning commonly holds a new executable for a few
/// seconds, and giving up on the first refusal left a good install reading as
/// missing until the app was restarted and discovery ran again.
const VERIFY_BACKOFF_SECS: [u64; 6] = [1, 2, 3, 5, 8, 13];

/// Marks a previous copy that was renamed aside during an update.
const REPLACED_MARKER: &str = ".replaced-";

pub fn snapshot() -> ToolsState {
    STATE
        .read()
        .map(|state| state.clone())
        .unwrap_or_else(|_| blank_state())
}

pub fn engine_path() -> Option<PathBuf> {
    let state = STATE.read().ok()?;
    state
        .engine
        .available
        .then(|| state.engine.path.as_ref().map(PathBuf::from))
        .flatten()
}

pub fn ffmpeg_path() -> Option<PathBuf> {
    let state = STATE.read().ok()?;
    state
        .ffmpeg
        .available
        .then(|| state.ffmpeg.path.as_ref().map(PathBuf::from))
        .flatten()
}

/// The JavaScript runtime discovery settled on, if there is one.
///
/// There is deliberately no `require_js_runtime`: every download that worked
/// before this tool existed still works without it, so a missing runtime is
/// never a reason to refuse a link -- it only costs the formats that have to be
/// deciphered, which is the signed-in half of YouTube.
#[cfg(not(target_os = "android"))]
pub fn js_runtime_path() -> Option<PathBuf> {
    let state = STATE.read().ok()?;
    state
        .js_runtime
        .available
        .then(|| state.js_runtime.path.as_ref().map(PathBuf::from))
        .flatten()
}

pub fn require_engine() -> AppResult<PathBuf> {
    engine_path().ok_or(AppError::EngineMissing)
}

pub fn require_ffmpeg() -> AppResult<PathBuf> {
    ffmpeg_path().ok_or(AppError::FfmpegMissing)
}

/// What yt-dlp is given as `--ffmpeg-location`, so it uses the same FFmpeg the
/// app does rather than whatever is on PATH.
///
/// Normally that is the containing directory. On Android the binaries are
/// named `libffmpeg.so` and `libffprobe.so`, which yt-dlp would not find by
/// looking in a directory; handed the file itself, it takes the name as given
/// and derives the ffprobe beside it by substituting the word.
pub fn ffmpeg_location() -> Option<String> {
    let ffmpeg = ffmpeg_path()?;
    #[cfg(target_os = "android")]
    let location = Some(ffmpeg.as_path());
    #[cfg(not(target_os = "android"))]
    let location = ffmpeg.parent();
    location.map(|path| path.to_string_lossy().into_owned())
}

fn managed_engine() -> AppResult<PathBuf> {
    Ok(paths::tools_dir()?.join(ENGINE_EXE))
}

#[cfg(not(target_os = "android"))]
fn managed_ffmpeg() -> AppResult<PathBuf> {
    Ok(paths::tools_dir()?.join("ffmpeg").join(FFMPEG_EXE))
}

#[cfg(not(target_os = "android"))]
fn managed_js_runtime() -> AppResult<PathBuf> {
    Ok(paths::tools_dir()?.join(JS_RUNTIME_DIR).join(JS_RUNTIME_EXE))
}

/// Re-detect every tool and publish the result. Called after a custom path
/// setting changes and whenever the user asks for a fresh check.
pub async fn refresh(settings: &Settings) -> ToolsState {
    let _pass = DISCOVERY.lock().await;
    discover_all(settings).await
}

/// The discovered state, waiting for the first pass rather than returning the
/// placeholder. Startup discovery runs in the background, so the interface can
/// ask before it has finished; this is what keeps that race from showing an
/// installed tool as missing.
pub async fn discovered(settings: &Settings) -> ToolsState {
    if DISCOVERED.load(Ordering::SeqCst) {
        return snapshot();
    }
    let _pass = DISCOVERY.lock().await;
    if DISCOVERED.load(Ordering::SeqCst) {
        return snapshot();
    }
    discover_all(settings).await
}

/// The caller holds `DISCOVERY`.
async fn discover_all(settings: &Settings) -> ToolsState {
    // Neither tool can start until the libraries they link against have been
    // unpacked from the APK. That is a no-op on every launch but the first
    // after an install or update.
    #[cfg(target_os = "android")]
    prepare_android_runtime(false).await;

    let engine = detect_kind(ToolKind::Engine, settings).await;
    let ffmpeg = detect_kind(ToolKind::Ffmpeg, settings).await;
    #[cfg(not(target_os = "android"))]
    let js_runtime = detect_kind(ToolKind::JsRuntime, settings).await;

    let state = publish(|state| {
        state.engine = engine;
        state.ffmpeg = ffmpeg;
        #[cfg(not(target_os = "android"))]
        {
            state.js_runtime = js_runtime;
        }
    });
    DISCOVERED.store(true, Ordering::SeqCst);
    state
}

fn publish(update: impl FnOnce(&mut ToolsState)) -> ToolsState {
    match STATE.write() {
        Ok(mut guard) => {
            update(&mut guard);
            guard.clone()
        }
        Err(_) => snapshot(),
    }
}

fn slot(state: &mut ToolsState, kind: ToolKind) -> &mut ToolStatus {
    match kind {
        ToolKind::Engine => &mut state.engine,
        ToolKind::Ffmpeg => &mut state.ffmpeg,
        #[cfg(not(target_os = "android"))]
        ToolKind::JsRuntime => &mut state.js_runtime,
    }
}

async fn detect_kind(kind: ToolKind, settings: &Settings) -> ToolStatus {
    match kind {
        ToolKind::Engine => {
            detect(kind, engine_candidates(settings), &["--version".to_string()]).await
        }
        ToolKind::Ffmpeg => {
            detect(kind, ffmpeg_candidates(settings), &["-version".to_string()]).await
        }
        #[cfg(not(target_os = "android"))]
        ToolKind::JsRuntime => {
            detect(kind, js_runtime_candidates(), &["--version".to_string()]).await
        }
    }
}

/// Order matters: an explicit choice wins, then the copy this app manages,
/// then whatever the system already provides.
#[cfg(not(target_os = "android"))]
fn engine_candidates(settings: &Settings) -> Vec<(PathBuf, ToolSource)> {
    ordered_candidates(settings.engine_path.as_deref(), managed_engine().ok(), "yt-dlp")
}

#[cfg(not(target_os = "android"))]
fn ffmpeg_candidates(settings: &Settings) -> Vec<(PathBuf, ToolSource)> {
    ordered_candidates(settings.ffmpeg_path.as_deref(), managed_ffmpeg().ok(), "ffmpeg")
}

/// The copy this app installed, then any runtime the machine already has.
///
/// There is no custom-path setting to honour here, so the order is only those
/// two. Node and Bun are looked for beside Deno because the engine is happy
/// with any of them once it is told where one is, and a machine that has one
/// should not be asked to fetch 40 MB to learn nothing new.
#[cfg(not(target_os = "android"))]
fn js_runtime_candidates() -> Vec<(PathBuf, ToolSource)> {
    managed_js_runtime()
        .ok()
        .map(|path| (path, ToolSource::Managed))
        .into_iter()
        .chain(
            ["deno", "node", "bun"]
                .iter()
                .filter_map(|name| Some((process::which(name)?, ToolSource::System))),
        )
        .collect()
}

#[cfg(not(target_os = "android"))]
fn ordered_candidates(
    custom: Option<&str>,
    managed: Option<PathBuf>,
    path_name: &str,
) -> Vec<(PathBuf, ToolSource)> {
    [
        custom.map(|value| (PathBuf::from(value), ToolSource::Custom)),
        managed.map(|value| (value, ToolSource::Managed)),
        process::which(path_name).map(|value| (value, ToolSource::System)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A custom path cannot be honoured on Android: a file the user points at
/// would not be allowed to run. Only the managed copy counts.
#[cfg(target_os = "android")]
fn engine_candidates(_settings: &Settings) -> Vec<(PathBuf, ToolSource)> {
    managed_engine()
        .ok()
        .map(|path| (path, ToolSource::Managed))
        .into_iter()
        .collect()
}

#[cfg(target_os = "android")]
fn ffmpeg_candidates(_settings: &Settings) -> Vec<(PathBuf, ToolSource)> {
    vec![(crate::android::ffmpeg_binary(), ToolSource::Bundled)]
}

#[cfg(target_os = "android")]
async fn prepare_android_runtime(force: bool) {
    match tokio::task::spawn_blocking(move || crate::android::prepare_runtime(force)).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => log_warn!("tools", "the bundled runtime could not be unpacked: {err}"),
        Err(err) => log_warn!("tools", "unpacking the bundled runtime panicked: {err}"),
    }
}

// -- remembering what a version check answered ------------------------------

/// What a version check saw, recorded against the file it saw it in.
///
/// Only the engine is remembered, and it is the reason this exists: yt-dlp on
/// Windows is a 17.8 MB PyInstaller bundle that unpacks itself to a temporary
/// directory every time it starts, so asking it its version costs the better
/// part of a second. That second used to be spent on every launch, with the
/// link field greyed out for the whole of it, to be told what the last launch
/// was already told. FFmpeg is not cached: it answers in tens of milliseconds,
/// and the shared build depends on DLLs beside it that an unchanged
/// `ffmpeg.exe` would not notice the loss of.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
struct ProbedVersion {
    size: u64,
    modified_ms: i64,
    version: String,
}

fn probe_cache_path() -> AppResult<PathBuf> {
    Ok(paths::cache_dir()?.join("engine-version.json"))
}

/// Size and modification time, which together are what make a file the same
/// file. A path whose metadata will not be read is simply never remembered.
fn fingerprint(path: &Path) -> Option<(u64, i64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((metadata.len(), modified))
}

static PROBE_CACHE: Lazy<RwLock<HashMap<String, ProbedVersion>>> =
    Lazy::new(|| RwLock::new(load_probe_cache()));

fn load_probe_cache() -> HashMap<String, ProbedVersion> {
    probe_cache_path()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The version remembered for `path`, if that path still holds the file it was
/// read from. An install, an update, or the user pointing the setting somewhere
/// else all move the size or the modification time, and so all re-run the check.
fn remembered_version(path: &Path) -> Option<String> {
    let (size, modified_ms) = fingerprint(path)?;
    let cache = PROBE_CACHE.read().ok()?;
    let entry = cache.get(path.to_string_lossy().as_ref())?;
    (entry.size == size && entry.modified_ms == modified_ms).then(|| entry.version.clone())
}

/// Remember what this file answered, on disk, so the next launch starts knowing
/// it. A cache that cannot be written costs the check again and nothing else.
fn remember_version(path: &Path, version: &str) {
    let Some((size, modified_ms)) = fingerprint(path) else {
        return;
    };
    let entry = ProbedVersion {
        size,
        modified_ms,
        version: version.to_owned(),
    };
    let key = path.to_string_lossy().into_owned();

    let snapshot = {
        let Ok(mut cache) = PROBE_CACHE.write() else {
            return;
        };
        if cache.get(&key) == Some(&entry) {
            return;
        }
        // One tool, one entry: a path that moved leaves nothing behind.
        cache.clear();
        cache.insert(key, entry);
        cache.clone()
    };

    if let (Ok(path), Ok(bytes)) = (probe_cache_path(), serde_json::to_vec(&snapshot)) {
        let _ = std::fs::write(path, bytes);
    }
}

async fn detect(
    kind: ToolKind,
    candidates: Vec<(PathBuf, ToolSource)>,
    version_args: &[String],
) -> ToolStatus {
    for (path, source) in candidates {
        if !path.is_file() {
            continue;
        }
        // A binary that has not changed answers what it answered last time, and
        // for the engine that answer is a second of the user's launch.
        if kind == ToolKind::Engine {
            if let Some(version) = remembered_version(&path) {
                return ToolStatus {
                    name: kind,
                    available: true,
                    path: Some(path.to_string_lossy().into_owned()),
                    version: Some(version),
                    source,
                };
            }
        }
        match process::run_with_timeout(&path, version_args, VERSION_CHECK_TIMEOUT).await {
            Ok(output) if output.success() => {
                let version = parse_version(kind, &output.stdout);
                if kind == ToolKind::Engine {
                    remember_version(&path, &version);
                }
                return ToolStatus {
                    name: kind,
                    available: true,
                    path: Some(path.to_string_lossy().into_owned()),
                    version: Some(version),
                    source,
                };
            }
            Ok(output) => {
                log_warn!(
                    "tools",
                    "{} at {} exited with {:?}: {}",
                    kind.as_str(),
                    path.display(),
                    output.status,
                    output.stderr.lines().next().unwrap_or("")
                );
            }
            Err(err) => {
                log_warn!("tools", "{} at {} is not runnable: {err}", kind.as_str(), path.display());
            }
        }
    }

    ToolStatus::missing(kind)
}

fn parse_version(kind: ToolKind, stdout: &str) -> String {
    let first = stdout.lines().next().unwrap_or("").trim();
    match kind {
        // yt-dlp prints just the version, e.g. "2026.08.24".
        ToolKind::Engine => first.to_string(),
        // ffmpeg prints "ffmpeg version n7.1-... Copyright ...".
        ToolKind::Ffmpeg => first
            .split_whitespace()
            .nth(2)
            .unwrap_or(first)
            .trim_start_matches('n')
            .to_string(),
        // Three runtimes, three shapes: "deno 2.9.7 (stable, ...)", Node's
        // bare "v22.11.0", Bun's bare "1.1.0". The first word that starts
        // with a digit is the version in all of them.
        #[cfg(not(target_os = "android"))]
        ToolKind::JsRuntime => first
            .split_whitespace()
            .map(|word| word.trim_start_matches('v'))
            .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
            .unwrap_or(first)
            .to_string(),
    }
}

/// Progress callback: `(received, total_or_none, stage)`.
pub type InstallProgress<'a> = &'a (dyn Fn(u64, Option<u64>, &str) + Send + Sync);

pub async fn install(
    kind: ToolKind,
    settings: &Settings,
    on_progress: InstallProgress<'_>,
) -> AppResult<ToolStatus> {
    sweep_replaced();

    match kind {
        ToolKind::Engine => install_engine(settings, on_progress).await?,
        ToolKind::Ffmpeg => install_ffmpeg(settings, on_progress).await?,
        #[cfg(not(target_os = "android"))]
        ToolKind::JsRuntime => install_js_runtime(settings, on_progress).await?,
    }

    on_progress(0, None, "verifying");
    let status = verify(kind, settings).await;

    if !status.available {
        return Err(AppError::Other(format!(
            "{} was downloaded but could not be started",
            kind.as_str()
        )));
    }

    on_progress(0, None, "done");
    log_info!("tools", "installed {} {:?}", kind.as_str(), status.version);
    Ok(status)
}

/// Detect one tool that has just been installed, allowing for the delay before
/// a new executable is allowed to run.
///
/// Only this tool's entry is published. Re-detecting the other one here would
/// race that tool's own install when both run at once, which is how a finished
/// install used to be overwritten with a stale "missing".
async fn verify(kind: ToolKind, settings: &Settings) -> ToolStatus {
    let mut delays = VERIFY_BACKOFF_SECS.iter();
    loop {
        let status = {
            let _pass = DISCOVERY.lock().await;
            detect_kind(kind, settings).await
        };

        if status.available {
            publish(|state| *slot(state, kind) = status.clone());
            return status;
        }

        let Some(secs) = delays.next() else {
            publish(|state| *slot(state, kind) = status.clone());
            return status;
        };
        log_warn!(
            "tools",
            "{} is not runnable yet, checking again in {secs}s",
            kind.as_str()
        );
        tokio::time::sleep(Duration::from_secs(*secs)).await;
    }
}

async fn install_engine(settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    let target = managed_engine()?;
    let temp = target.with_extension("download");

    download_to_file(ENGINE_URL, &temp, settings, on_progress).await?;
    replace_path(&temp, &target)
}

/// FFmpeg is part of the APK, so "installing" it can only mean unpacking its
/// libraries again -- which is also the repair for a damaged copy.
#[cfg(target_os = "android")]
async fn install_ffmpeg(_settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    on_progress(0, None, "extracting");
    tokio::task::spawn_blocking(|| crate::android::prepare_runtime(true))
        .await
        .map_err(|err| AppError::Other(format!("unpacking FFmpeg failed: {err}")))?
}

#[cfg(not(target_os = "android"))]
async fn install_ffmpeg(settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    let tools_dir = paths::tools_dir()?;
    let dir = tools_dir.join("ffmpeg");
    let staging = tools_dir.join("ffmpeg.staging");
    let archive = tools_dir.join("ffmpeg-download.zip");

    download_to_file(FFMPEG_URL, &archive, settings, on_progress).await?;

    on_progress(0, None, "extracting");
    // Extracted beside the live copy and swapped in whole, so discovery never
    // sees an ffmpeg.exe whose DLLs have not landed yet, and a failed
    // extraction leaves a working install untouched.
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let extracted = extract_ffmpeg(&archive, &staging);
    let _ = std::fs::remove_file(&archive);
    if let Err(err) = extracted {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }

    replace_path(&staging, &dir)
}

/// The same shape as the FFmpeg install: fetch the zip, unpack it beside the
/// live copy, swap the directory in whole. One executable rather than a
/// directory's worth would fit in a plain file swap, but a release that one day
/// ships a DLL beside it would then land half-installed.
#[cfg(not(target_os = "android"))]
async fn install_js_runtime(settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    let tools_dir = paths::tools_dir()?;
    let dir = tools_dir.join(JS_RUNTIME_DIR);
    let staging = tools_dir.join(format!("{JS_RUNTIME_DIR}.staging"));
    let archive = tools_dir.join("js-runtime-download.zip");

    download_to_file(JS_RUNTIME_URL, &archive, settings, on_progress).await?;

    on_progress(0, None, "extracting");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let extracted = extract_js_runtime(&archive, &staging);
    let _ = std::fs::remove_file(&archive);
    if let Err(err) = extracted {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }

    replace_path(&staging, &dir)
}

/// Move `fresh` into place at `target`, keeping the previous copy until the
/// move has succeeded.
///
/// The old copy is renamed aside rather than deleted: Windows lets a running
/// executable be renamed but not removed, so an update does not fail just
/// because a download happens to be using the tool.
fn replace_path(fresh: &Path, target: &Path) -> AppResult<()> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tool");
    let previous = target.with_file_name(format!(
        "{name}{REPLACED_MARKER}{}",
        chrono::Utc::now().timestamp_millis()
    ));

    let had_previous = target.exists();
    if had_previous {
        std::fs::rename(target, &previous).map_err(|err| {
            AppError::Other(format!(
                "{} is in use and could not be replaced: {err}",
                target.display()
            ))
        })?;
    }

    if let Err(err) = std::fs::rename(fresh, target) {
        if had_previous {
            let _ = std::fs::rename(&previous, target);
        }
        return Err(err.into());
    }

    if had_previous {
        remove_path(&previous);
    }
    Ok(())
}

/// Previous copies that were still running when they were replaced.
fn sweep_replaced() {
    let Ok(dir) = paths::tools_dir() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().contains(REPLACED_MARKER) {
            remove_path(&entry.path());
        }
    }
}

fn remove_path(path: &Path) {
    let _ = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
}

/// Pull just the executables and their DLLs out of the release archive.
/// `ffplay` is skipped: nothing in this app plays media back.
#[cfg(not(target_os = "android"))]
fn extract_ffmpeg(archive: &Path, dest: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|err| AppError::Other(format!("the FFmpeg archive could not be read: {err}")))?;

    let mut extracted = 0usize;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|err| AppError::Other(format!("the FFmpeg archive is damaged: {err}")))?;

        if entry.is_dir() {
            continue;
        }
        // `enclosed_name` rejects entries that would escape the destination
        // directory -- zip-slip protection, since this archive is remote input.
        let Some(entry_path) = entry.enclosed_name() else {
            continue;
        };
        let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let in_bin = entry_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("bin");

        if !in_bin || name.eq_ignore_ascii_case("ffplay.exe") {
            continue;
        }

        let out_path = dest.join(name);
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        out.flush()?;
        extracted += 1;
    }

    if extracted == 0 {
        return Err(AppError::Other(
            "the FFmpeg archive did not contain the expected files".into(),
        ));
    }
    Ok(())
}

/// Pull the runtime out of its release archive.
///
/// That zip holds exactly one file today, `deno.exe`. Everything at the root is
/// taken rather than that one name, so a release that grows a library beside
/// the executable still installs whole; a subdirectory is ignored, since
/// nothing the engine is handed would look for one. The install fails outright
/// if the executable itself was not among them -- an empty `js` directory would
/// otherwise read as a runtime that merely refuses to start.
#[cfg(not(target_os = "android"))]
fn extract_js_runtime(archive: &Path, dest: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|err| {
        AppError::Other(format!(
            "the JavaScript runtime archive could not be read: {err}"
        ))
    })?;

    let mut found_runtime = false;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|err| {
            AppError::Other(format!("the JavaScript runtime archive is damaged: {err}"))
        })?;

        if entry.is_dir() {
            continue;
        }
        // Remote input again, so the same zip-slip guard as above.
        let Some(entry_path) = entry.enclosed_name() else {
            continue;
        };
        if !at_archive_root(&entry_path) {
            continue;
        }
        let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let mut out = std::fs::File::create(dest.join(name))?;
        std::io::copy(&mut entry, &mut out)?;
        out.flush()?;
        found_runtime |= name.eq_ignore_ascii_case(JS_RUNTIME_EXE);
    }

    if !found_runtime {
        return Err(AppError::Other(
            "the JavaScript runtime archive did not contain the runtime".into(),
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn at_archive_root(path: &Path) -> bool {
    path.components().count() == 1
}

/// Stream `url` to `target`, removing the partial file if the transfer fails.
pub(crate) async fn download_to_file(
    url: &str,
    target: &Path,
    settings: &Settings,
    on_progress: InstallProgress<'_>,
) -> AppResult<()> {
    let result = stream_to_file(url, target, settings, on_progress).await;
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

async fn stream_to_file(
    url: &str,
    target: &Path,
    settings: &Settings,
    on_progress: InstallProgress<'_>,
) -> AppResult<()> {
    let client = crate::net::client(settings)?;
    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(AppError::from_status(
            response.status().as_u16(),
            format!("could not download from {url}"),
        ));
    }

    let total = response.content_length();
    let mut file = std::fs::File::create(target)?;
    let mut stream = response.bytes_stream();
    let mut received = 0u64;
    let mut last_report = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        received += chunk.len() as u64;

        // Report a few times a second, not once per chunk.
        if last_report.elapsed().as_millis() >= 120 {
            on_progress(received, total, "downloading");
            last_report = std::time::Instant::now();
        }
    }

    file.flush()?;
    on_progress(received, total, "downloading");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ud-tools-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn replacing_a_file_keeps_only_the_new_copy() {
        let dir = scratch("file");
        let target = dir.join("tool.exe");
        let fresh = dir.join("tool.download");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&fresh, b"new").unwrap();

        replace_path(&fresh, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!fresh.exists());
        let leftovers = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(leftovers, 1, "the previous copy should have been removed");
    }

    #[test]
    fn replacing_a_directory_swaps_it_whole() {
        let dir = scratch("dir");
        let target = dir.join("ffmpeg");
        let staging = dir.join("ffmpeg.staging");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("stale.dll"), b"old").unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("ffmpeg.exe"), b"new").unwrap();

        replace_path(&staging, &target).unwrap();

        assert!(target.join("ffmpeg.exe").is_file());
        assert!(!target.join("stale.dll").exists());
        assert!(!staging.exists());
    }

    #[test]
    fn a_failed_swap_puts_the_previous_copy_back() {
        let dir = scratch("restore");
        let target = dir.join("tool.exe");
        std::fs::write(&target, b"old").unwrap();

        let missing = dir.join("never-downloaded");
        assert!(replace_path(&missing, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }

    /// The name the TypeScript side is written against. Renaming the variant
    /// without renaming it there would leave the runtime installing into a
    /// card that never updates.
    #[test]
    #[cfg(not(target_os = "android"))]
    fn the_js_runtime_is_called_what_the_interface_calls_it() {
        assert_eq!(ToolKind::JsRuntime.as_str(), "jsRuntime");
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn a_js_runtime_reports_a_version_whichever_runtime_it_is() {
        let version = |stdout: &str| parse_version(ToolKind::JsRuntime, stdout);

        assert_eq!(
            version("deno 2.9.7 (stable, release, x86_64-pc-windows-msvc)\nv8 14.2\n"),
            "2.9.7"
        );
        assert_eq!(version("v22.11.0\n"), "22.11.0");
        assert_eq!(version("1.1.0\n"), "1.1.0");

        // Nothing recognisable rather than a panic or an empty card.
        assert_eq!(version("some runtime\n"), "some runtime");
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn only_the_top_of_the_runtime_archive_is_unpacked() {
        assert!(at_archive_root(Path::new("deno.exe")));
        assert!(at_archive_root(Path::new("LICENSE.md")));
        assert!(!at_archive_root(Path::new("bin/deno.exe")));
        assert!(!at_archive_root(Path::new("deno/bin/deno.exe")));
    }

    #[test]
    fn first_install_needs_no_previous_copy() {
        let dir = scratch("first");
        let target = dir.join("tool.exe");
        let fresh = dir.join("tool.download");
        std::fs::write(&fresh, b"new").unwrap();

        replace_path(&fresh, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }
}
