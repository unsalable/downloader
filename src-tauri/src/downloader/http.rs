//! The resumable HTTP fetcher.
//!
//! This is the path taken for ordinary progressive streams -- which is most of
//! them. Bytes are written straight to a `.part` file, so a paused or crashed
//! download resumes with a `Range` request instead of starting over, and the
//! user's Downloads folder never sees a partial file.
//!
//! Transfers are issued as a sequence of bounded ranged requests rather than
//! one long GET. That is not an optimisation detail -- several large media
//! hosts throttle a single sustained connection hard (measured on one CDN: 35
//! kB/s sustained versus 3.3 MB/s when the same bytes are requested in chunks).
//! A server that ignores `Range` simply answers 200 with the whole body, which
//! is handled as the single-request case.

use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{ACCEPT_RANGES, CONTENT_RANGE, RANGE};
use reqwest::StatusCode;

use crate::downloader::control::TaskControl;
use crate::downloader::speed::SpeedTracker;
use crate::error::{AppError, AppResult};
use crate::{log_debug, log_warn};

/// Bytes requested per ranged request. Large enough that per-request overhead
/// is negligible, small enough to keep a throttling host from ramping up.
const CHUNK_BYTES: u64 = 8 * 1024 * 1024;
const LOW_RESOURCE_CHUNK_BYTES: u64 = 2 * 1024 * 1024;

/// Written to disk in 1 MiB batches; larger buffers stop helping and start
/// costing memory per concurrent download.
const BUFFER_BYTES: usize = 1024 * 1024;
const LOW_RESOURCE_BUFFER_BYTES: usize = 128 * 1024;

/// A stalled connection is retried rather than failed. Most interruptions on a
/// long download are transient.
const MAX_STALL_RETRIES: u32 = 4;

// Low-resource mode has to mean *less* of everything, not more.
const _: () = assert!(LOW_RESOURCE_CHUNK_BYTES < CHUNK_BYTES);
const _: () = assert!(LOW_RESOURCE_BUFFER_BYTES < BUFFER_BYTES);

#[derive(Debug, Clone)]
pub struct ProgressSample {
    pub received: u64,
    pub total: Option<u64>,
    pub speed_bps: f64,
    pub eta_sec: Option<f64>,
    pub percent: Option<f64>,
    pub resumable: bool,
}

pub struct FetchOptions<'a> {
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    /// Final destination. The transfer writes to `<target>.part` and renames on
    /// success.
    pub target: &'a Path,
    pub low_resource: bool,
}

/// Outcome of one ranged request.
struct Span {
    bytes: u64,
    /// Total size of the resource, when the response revealed it.
    total: Option<u64>,
    resumable: bool,
    /// The server ignored `Range` and streamed the entire body, so there is
    /// nothing left to request.
    whole_body: bool,
}

/// Download `url` into `options.target`, resuming if a partial file is present.
/// Returns the number of bytes in the completed file.
pub async fn fetch(
    client: &reqwest::Client,
    options: FetchOptions<'_>,
    control: Arc<TaskControl>,
    on_progress: &mut (dyn FnMut(ProgressSample) + Send),
) -> AppResult<u64> {
    let part_path = part_path(options.target);
    let chunk_size = if options.low_resource {
        LOW_RESOURCE_CHUNK_BYTES
    } else {
        CHUNK_BYTES
    };

    let already_on_disk = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
    // The tracker lives across chunks so speed and ETA stay continuous instead
    // of resetting every few megabytes.
    let mut tracker = SpeedTracker::new(already_on_disk);
    let mut total: Option<u64> = None;
    let mut resumable = false;
    let mut attempt = 0u32;
    let mut last_emit = Instant::now();

    emit(on_progress, &tracker, total, resumable);

    loop {
        if control.interrupted() {
            return Err(AppError::Canceled);
        }

        let received = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
        if total.is_some_and(|size| received >= size) {
            break;
        }

        let span = fetch_span(
            client,
            &options,
            &part_path,
            received,
            chunk_size,
            &control,
            &mut tracker,
            &mut last_emit,
            on_progress,
        )
        .await;

        match span {
            Ok(span) => {
                attempt = 0;
                if let Some(size) = span.total {
                    total = Some(size);
                }
                resumable = span.resumable;

                if span.whole_body {
                    break;
                }
                if span.bytes == 0 {
                    // A range request that returns nothing would loop forever.
                    if total.is_none() {
                        break;
                    }
                    return Err(AppError::Network(
                        "the server stopped sending data before the file was complete".into(),
                    ));
                }
            }
            Err(AppError::Canceled) => return Err(AppError::Canceled),
            Err(err) => {
                attempt += 1;
                if attempt > MAX_STALL_RETRIES || !is_retryable(&err) {
                    return Err(err);
                }
                log_warn!("http", "attempt {attempt} failed at {received} bytes: {err}");
                tokio::time::sleep(Duration::from_millis(400 * u64::from(attempt))).await;
            }
        }
    }

    let written = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
    if let Some(expected) = total {
        if written < expected {
            return Err(AppError::Network(format!(
                "the connection closed after {written} of {expected} bytes"
            )));
        }
    }

    emit(on_progress, &tracker, total.or(Some(written)), resumable);
    finalize(&part_path, options.target)?;
    Ok(written)
}

fn is_retryable(err: &AppError) -> bool {
    matches!(err, AppError::Network(_) | AppError::Io(_))
}

#[allow(clippy::too_many_arguments)]
async fn fetch_span(
    client: &reqwest::Client,
    options: &FetchOptions<'_>,
    part_path: &Path,
    start: u64,
    chunk_size: u64,
    control: &Arc<TaskControl>,
    tracker: &mut SpeedTracker,
    last_emit: &mut Instant,
    on_progress: &mut (dyn FnMut(ProgressSample) + Send),
) -> AppResult<Span> {
    let mut request = client.get(options.url);
    for (name, value) in options.headers {
        request = request.header(name, value);
    }
    // Always bounded, even from zero: an open-ended range is what triggers the
    // throttling this chunking exists to avoid.
    request = request.header(RANGE, format!("bytes={}-{}", start, start + chunk_size - 1));

    let response = request.send().await?;
    let status = response.status();

    // 416 means the partial file is at or past what the server will serve.
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        log_debug!("http", "range rejected at {start}; treating the file as complete");
        return Ok(Span {
            bytes: 0,
            total: Some(start),
            resumable: true,
            whole_body: true,
        });
    }

    if !status.is_success() {
        return Err(AppError::from_status(
            status.as_u16(),
            status.canonical_reason().unwrap_or("request failed"),
        ));
    }

    let headers = response.headers();
    let partial = status == StatusCode::PARTIAL_CONTENT;
    let resumable = partial
        || headers
            .get(ACCEPT_RANGES)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("bytes"));

    // With 206 the full size is in Content-Range; with 200 the body is the
    // whole resource and Content-Length describes all of it.
    let total = if partial {
        headers
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit('/').next()?.parse::<u64>().ok())
    } else {
        response.content_length()
    };

    // A server that ignores Range answers 200 with everything from byte zero,
    // so whatever was on disk must be discarded rather than appended to.
    let write_at = if partial { start } else { 0 };
    if !partial && start > 0 {
        log_debug!("http", "server ignored the range request; restarting from zero");
        *tracker = SpeedTracker::new(0);
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        // Truncation is decided below, per response: a 206 appends, a 200
        // starts over. Never on open.
        .truncate(false)
        .open(part_path)?;

    if write_at > 0 {
        file.seek(SeekFrom::Start(write_at))?;
    } else {
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
    }

    let buffer_capacity = if options.low_resource {
        LOW_RESOURCE_BUFFER_BYTES
    } else {
        BUFFER_BYTES
    };
    let mut buffer: Vec<u8> = Vec::with_capacity(buffer_capacity);
    let mut stream = response.bytes_stream();
    let mut span_bytes = 0u64;

    loop {
        // Checked before each chunk, so a pause or cancel takes effect within
        // one chunk. Buffered bytes are flushed first: they are what a later
        // resume will pick up from.
        if control.interrupted() {
            flush(&mut file, &mut buffer)?;
            file.sync_data()?;
            return Err(AppError::Canceled);
        }

        let Some(chunk) = stream.next().await else {
            break;
        };
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                flush(&mut file, &mut buffer)?;
                file.sync_data()?;
                return Err(err.into());
            }
        };

        buffer.extend_from_slice(&chunk);
        if buffer.len() >= buffer_capacity {
            flush(&mut file, &mut buffer)?;
        }

        span_bytes += chunk.len() as u64;
        let window_closed = tracker.record(chunk.len() as u64);
        if window_closed || last_emit.elapsed() >= Duration::from_millis(500) {
            emit(on_progress, tracker, total, resumable);
            *last_emit = Instant::now();
        }
    }

    flush(&mut file, &mut buffer)?;
    file.sync_data()?;
    drop(file);

    emit(on_progress, tracker, total, resumable);

    Ok(Span {
        bytes: span_bytes,
        total,
        resumable,
        // Only a 200 means the response carried the entire resource.
        whole_body: !partial,
    })
}

fn emit(
    on_progress: &mut (dyn FnMut(ProgressSample) + Send),
    tracker: &SpeedTracker,
    total: Option<u64>,
    resumable: bool,
) {
    on_progress(ProgressSample {
        received: tracker.received(),
        total,
        speed_bps: tracker.speed_bps(),
        eta_sec: tracker.eta_sec(total),
        percent: tracker.percent(total),
        resumable,
    });
}

fn flush(file: &mut std::fs::File, buffer: &mut Vec<u8>) -> AppResult<()> {
    if buffer.is_empty() {
        return Ok(());
    }
    file.write_all(buffer)?;
    buffer.clear();
    Ok(())
}

pub fn part_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

fn finalize(part_path: &Path, target: &Path) -> AppResult<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if target.exists() {
        std::fs::remove_file(target)?;
    }
    std::fs::rename(part_path, target)?;
    Ok(())
}

/// Drop the partial file for a download the user cancelled outright.
pub fn discard_partial(target: &Path) {
    let _ = std::fs::remove_file(part_path(target));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_path_appends_rather_than_replacing_the_extension() {
        let path = part_path(Path::new(r"C:\d\video.mp4"));
        assert!(path.to_string_lossy().ends_with("video.mp4.part"));
    }

    #[test]
    fn network_and_io_failures_are_retryable_but_access_failures_are_not() {
        assert!(is_retryable(&AppError::Network("x".into())));
        assert!(is_retryable(&AppError::Io("x".into())));
        assert!(!is_retryable(&AppError::Forbidden {
            status: 403,
            detail: "x".into()
        }));
        assert!(!is_retryable(&AppError::Canceled));
    }

}
