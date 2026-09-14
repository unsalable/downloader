//! Discovery and installation of the two external tools.
//!
//! Neither tool is bundled in the installer: yt-dlp changes weekly (a bundled
//! copy would be stale the month after release) and FFmpeg is GPL, which is
//! cleanest to keep as a separate process the user opts into. Both are fetched
//! once, on request, into the app's own data directory -- and a copy the user
//! already has on PATH is preferred over downloading anything at all.

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

const ENGINE_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";

/// The shared build is 77 MB against 170 MB for the static one, and the extra
/// DLLs land in the same directory as the executable.
const FFMPEG_URL: &str =
    "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl-shared.zip";

const ENGINE_EXE: &str = "yt-dlp.exe";
const FFMPEG_EXE: &str = "ffmpeg.exe";

static STATE: Lazy<RwLock<ToolsState>> = Lazy::new(|| {
    RwLock::new(ToolsState {
        engine: ToolStatus::missing(ToolKind::Engine),
        ffmpeg: ToolStatus::missing(ToolKind::Ffmpeg),
    })
});

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
        .unwrap_or_else(|_| ToolsState {
            engine: ToolStatus::missing(ToolKind::Engine),
            ffmpeg: ToolStatus::missing(ToolKind::Ffmpeg),
        })
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

pub fn require_engine() -> AppResult<PathBuf> {
    engine_path().ok_or(AppError::EngineMissing)
}

pub fn require_ffmpeg() -> AppResult<PathBuf> {
    ffmpeg_path().ok_or(AppError::FfmpegMissing)
}

fn managed_engine() -> AppResult<PathBuf> {
    Ok(paths::tools_dir()?.join(ENGINE_EXE))
}

fn managed_ffmpeg() -> AppResult<PathBuf> {
    Ok(paths::tools_dir()?.join("ffmpeg").join(FFMPEG_EXE))
}

/// Re-detect both tools and publish the result. Called after a custom path
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
    let engine = detect_kind(ToolKind::Engine, settings).await;
    let ffmpeg = detect_kind(ToolKind::Ffmpeg, settings).await;

    let state = publish(|state| {
        state.engine = engine;
        state.ffmpeg = ffmpeg;
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
    }
}

async fn detect_kind(kind: ToolKind, settings: &Settings) -> ToolStatus {
    match kind {
        ToolKind::Engine => {
            detect(
                kind,
                settings.engine_path.as_deref(),
                managed_engine().ok(),
                "yt-dlp",
                &["--version".to_string()],
            )
            .await
        }
        ToolKind::Ffmpeg => {
            detect(
                kind,
                settings.ffmpeg_path.as_deref(),
                managed_ffmpeg().ok(),
                "ffmpeg",
                &["-version".to_string()],
            )
            .await
        }
    }
}

async fn detect(
    kind: ToolKind,
    custom: Option<&str>,
    managed: Option<PathBuf>,
    path_name: &str,
    version_args: &[String],
) -> ToolStatus {
    // Order matters: an explicit choice wins, then the copy this app manages,
    // then whatever the system already provides.
    let candidates: Vec<(PathBuf, ToolSource)> = [
        custom.map(|value| (PathBuf::from(value), ToolSource::Custom)),
        managed.map(|value| (value, ToolSource::Managed)),
        process::which(path_name).map(|value| (value, ToolSource::System)),
    ]
    .into_iter()
    .flatten()
    .collect();

    for (path, source) in candidates {
        if !path.is_file() {
            continue;
        }
        match process::run_with_timeout(&path, version_args, VERSION_CHECK_TIMEOUT).await {
            Ok(output) if output.success() => {
                return ToolStatus {
                    name: kind,
                    available: true,
                    path: Some(path.to_string_lossy().into_owned()),
                    version: Some(parse_version(kind, &output.stdout)),
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

/// Stream `url` to `target`, removing the partial file if the transfer fails.
async fn download_to_file(
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
