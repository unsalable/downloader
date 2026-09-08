//! Filesystem layout.
//!
//! Everything the app writes lives under one roaming-data root so uninstalling
//! is a single directory removal, with the exception of downloads themselves,
//! which go to the user's own Downloads folder by default.

use std::path::{Path, PathBuf};

use once_cell::sync::OnceCell;

use crate::error::{AppError, AppResult};

const APP_DIR_NAME: &str = "UniversalDownloader";

static ROOT: OnceCell<PathBuf> = OnceCell::new();

/// `%APPDATA%\UniversalDownloader`, created on first access.
pub fn root() -> AppResult<&'static Path> {
    let path = ROOT.get_or_try_init(|| {
        let base = dirs::data_dir()
            .ok_or_else(|| AppError::Other("could not locate the application data folder".into()))?;
        let dir = base.join(APP_DIR_NAME);
        std::fs::create_dir_all(&dir)?;
        Ok::<PathBuf, AppError>(dir)
    })?;
    Ok(path.as_path())
}

fn subdir(name: &str) -> AppResult<PathBuf> {
    let dir = root()?.join(name);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Managed copies of yt-dlp / ffmpeg.
pub fn tools_dir() -> AppResult<PathBuf> {
    subdir("tools")
}

pub fn cache_dir() -> AppResult<PathBuf> {
    subdir("cache")
}

pub fn thumbnail_cache_dir() -> AppResult<PathBuf> {
    let dir = cache_dir()?.join("thumbnails");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn metadata_cache_dir() -> AppResult<PathBuf> {
    let dir = cache_dir()?.join("metadata");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn logs_dir() -> AppResult<PathBuf> {
    subdir("logs")
}

/// Scratch space for in-flight downloads. Partial files live here so an
/// interrupted download never leaves a half-written file in the user's
/// Downloads folder.
pub fn temp_dir() -> AppResult<PathBuf> {
    subdir("temp")
}

pub fn database_path() -> AppResult<PathBuf> {
    Ok(root()?.join("library.db"))
}

/// The user's own Downloads folder, falling back to the home directory.
pub fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Remove temp artifacts left behind by a hard crash. Files belonging to a
/// resumable download are recognised by their `.part` sidecar and preserved.
pub fn sweep_temp(max_age_secs: u64) -> AppResult<u64> {
    let dir = temp_dir()?;
    let now = std::time::SystemTime::now();
    let mut removed = 0u64;

    for entry in std::fs::read_dir(&dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        // `.part` and its `.meta` sidecar are resume state, not garbage.
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("part") | Some("meta")
        ) {
            continue;
        }
        let age = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if age > max_age_secs && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }

    Ok(removed)
}
