//! Discovery and installation of the two external tools.
//!
//! Neither tool is bundled in the installer: yt-dlp changes weekly (a bundled
//! copy would be stale the month after release) and FFmpeg is GPL, which is
//! cleanest to keep as a separate process the user opts into. Both are fetched
//! once, on request, into the app's own data directory -- and a copy the user
//! already has on PATH is preferred over downloading anything at all.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use futures_util::StreamExt;
use once_cell::sync::Lazy;

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

/// Re-detect both tools and cache the result. Called at startup, after an
/// install, and whenever a custom path setting changes.
pub async fn refresh(settings: &Settings) -> ToolsState {
    let engine = detect(
        ToolKind::Engine,
        settings.engine_path.as_deref(),
        managed_engine().ok(),
        "yt-dlp",
        &["--version".to_string()],
    )
    .await;

    let ffmpeg = detect(
        ToolKind::Ffmpeg,
        settings.ffmpeg_path.as_deref(),
        managed_ffmpeg().ok(),
        "ffmpeg",
        &["-version".to_string()],
    )
    .await;

    let state = ToolsState { engine, ffmpeg };
    if let Ok(mut guard) = STATE.write() {
        *guard = state.clone();
    }
    state
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
        match process::run(&path, version_args).await {
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
    match kind {
        ToolKind::Engine => install_engine(settings, on_progress).await?,
        ToolKind::Ffmpeg => install_ffmpeg(settings, on_progress).await?,
    }

    on_progress(0, None, "verifying");
    let state = refresh(settings).await;
    let status = match kind {
        ToolKind::Engine => state.engine,
        ToolKind::Ffmpeg => state.ffmpeg,
    };

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

async fn install_engine(settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    let target = managed_engine()?;
    let temp = target.with_extension("download");

    download_to_file(ENGINE_URL, &temp, settings, on_progress).await?;

    // Replace atomically enough: remove-then-rename, so a failed download never
    // leaves a truncated executable in place of a working one.
    if target.exists() {
        std::fs::remove_file(&target)?;
    }
    std::fs::rename(&temp, &target)?;
    Ok(())
}

async fn install_ffmpeg(settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    let dir = paths::tools_dir()?.join("ffmpeg");
    std::fs::create_dir_all(&dir)?;
    let archive = paths::tools_dir()?.join("ffmpeg-download.zip");

    download_to_file(FFMPEG_URL, &archive, settings, on_progress).await?;

    on_progress(0, None, "extracting");
    let extract_result = extract_ffmpeg(&archive, &dir);
    let _ = std::fs::remove_file(&archive);
    extract_result
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

async fn download_to_file(
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
