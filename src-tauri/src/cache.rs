//! Thumbnail and metadata cache.
//!
//! Thumbnails are fetched once and kept on disk, then handed to the webview as
//! a data URL. That keeps the strict content-security policy intact (no remote
//! image origins) and means re-opening History does not re-contact anyone's CDN.
//!
//! The cache is bounded: once it exceeds the configured limit, the
//! least-recently-used files are dropped.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::error::{AppError, AppResult};
use crate::model::CacheStats;
use crate::settings::Settings;
use crate::{log_debug, paths, util};

/// A thumbnail larger than this is almost certainly not a thumbnail.
const MAX_THUMBNAIL_BYTES: usize = 6 * 1024 * 1024;

fn extension_for(content_type: &str, url: &str) -> &'static str {
    let lower = content_type.to_ascii_lowercase();
    if lower.contains("png") {
        "png"
    } else if lower.contains("webp") {
        "webp"
    } else if lower.contains("gif") {
        "gif"
    } else if lower.contains("avif") {
        "avif"
    } else if lower.contains("jpeg") || lower.contains("jpg") {
        "jpg"
    } else if url.contains(".png") {
        "png"
    } else if url.contains(".webp") {
        "webp"
    } else {
        "jpg"
    }
}

fn mime_for(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        _ => "image/jpeg",
    }
}

fn cached_path(url: &str, extension: &str) -> AppResult<PathBuf> {
    Ok(paths::thumbnail_cache_dir()?.join(format!("{}.{extension}", util::hash_key(url))))
}

fn find_cached(url: &str) -> Option<(PathBuf, &'static str)> {
    for extension in ["jpg", "png", "webp", "gif", "avif"] {
        if let Ok(path) = cached_path(url, extension) {
            if path.is_file() {
                return Some((path, extension));
            }
        }
    }
    None
}

/// Return the thumbnail as a data URL, fetching and caching it if needed.
pub async fn thumbnail_data_url(url: &str, settings: &Settings) -> AppResult<String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(AppError::InvalidUrl("not a fetchable image address".into()));
    }

    if let Some((path, extension)) = find_cached(url) {
        // Touching the file keeps LRU eviction honest.
        let _ = filetime_touch(&path);
        let bytes = std::fs::read(&path)?;
        return Ok(encode_data_url(mime_for(extension), &bytes));
    }

    let client = crate::net::client(settings)?;
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(AppError::from_status(
            response.status().as_u16(),
            "the thumbnail could not be fetched",
        ));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = response.bytes().await?;
    if bytes.len() > MAX_THUMBNAIL_BYTES {
        return Err(AppError::Other("the thumbnail is unexpectedly large".into()));
    }

    let extension = extension_for(&content_type, url);
    if let Ok(path) = cached_path(url, extension) {
        let _ = std::fs::write(&path, &bytes);
    }

    enforce_limit(settings.cache_limit_mb);
    Ok(encode_data_url(mime_for(extension), &bytes))
}

fn encode_data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64_encode(bytes))
}

/// Small standard-alphabet base64 encoder. A dependency for one function that
/// runs a few times per screen would not earn its place.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

fn filetime_touch(path: &std::path::Path) -> std::io::Result<()> {
    // Reopening for append with no write updates the access pattern enough for
    // the eviction heuristic without rewriting the file.
    let file = std::fs::OpenOptions::new().append(true).open(path)?;
    file.set_len(file.metadata()?.len())?;
    Ok(())
}

pub fn stats() -> AppResult<CacheStats> {
    let mut stats = CacheStats::default();

    if let Ok(dir) = paths::thumbnail_cache_dir() {
        for entry in std::fs::read_dir(dir)?.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    stats.thumbnail_count += 1;
                    stats.thumbnail_bytes += meta.len();
                }
            }
        }
    }

    if let Ok(dir) = paths::metadata_cache_dir() {
        for entry in std::fs::read_dir(dir)?.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    stats.metadata_count += 1;
                    stats.total_bytes += meta.len();
                }
            }
        }
    }

    stats.total_bytes += stats.thumbnail_bytes;
    Ok(stats)
}

pub fn clear() -> AppResult<()> {
    for dir in [paths::thumbnail_cache_dir()?, paths::metadata_cache_dir()?] {
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// Drop the oldest files until the cache fits inside its budget.
pub fn enforce_limit(limit_mb: u64) {
    let budget = limit_mb.saturating_mul(1024 * 1024);
    let Ok(dir) = paths::thumbnail_cache_dir() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };

    let mut files: Vec<(PathBuf, u64, SystemTime)> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((entry.path(), meta.len(), meta.modified().ok()?))
        })
        .collect();

    let total: u64 = files.iter().map(|(_, size, _)| size).sum();
    if total <= budget {
        return;
    }

    files.sort_by_key(|(_, _, modified)| *modified);
    let mut freed = 0u64;
    for (path, size, _) in files {
        if total - freed <= budget {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            freed += size;
        }
    }
    log_debug!("cache", "evicted {freed} bytes to stay under {budget}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_high_bytes() {
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64_encode(&[0x00, 0x00, 0x00]), "AAAA");
    }

    #[test]
    fn content_type_decides_the_extension_before_the_url_does() {
        assert_eq!(extension_for("image/png", "https://x/a.jpg"), "png");
        assert_eq!(extension_for("image/webp", "https://x/a"), "webp");
        assert_eq!(extension_for("", "https://x/a.png"), "png");
        assert_eq!(extension_for("", "https://x/a"), "jpg");
    }

    #[test]
    fn every_extension_maps_to_a_mime_type() {
        assert_eq!(mime_for("png"), "image/png");
        assert_eq!(mime_for("webp"), "image/webp");
        assert_eq!(mime_for("unknown"), "image/jpeg");
    }

    #[test]
    fn a_data_url_is_well_formed() {
        assert_eq!(
            encode_data_url("image/png", b"foo"),
            "data:image/png;base64,Zm9v"
        );
    }
}
