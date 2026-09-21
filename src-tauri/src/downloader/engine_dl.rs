//! Downloads that the engine has to perform itself.
//!
//! Used for segmented protocols (HLS, DASH) where a single ranged GET is not
//! enough -- the stream is a manifest plus hundreds of fragments. Rather than
//! reimplementing that, the work is handed to the engine and its progress is
//! read back through a machine-readable template, so the UI shows the same real
//! byte counts, speed and ETA as the native path.
//!
//! Pause is implemented as "stop the process": the engine keeps its `.part` and
//! fragment state, so resuming continues where it left off.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::downloader::control::TaskControl;
use crate::downloader::http::ProgressSample;
use crate::error::{AppError, AppResult};
use crate::providers::engine;
use crate::settings::Settings;
use crate::{log_debug, logging, process, tools};

/// A sentinel-prefixed, space-separated progress line. Parsing this is far more
/// robust than scraping the human-readable progress bar.
const PROGRESS_TEMPLATE: &str = "download:@P %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s %(progress.eta)s";

pub struct EngineDownload<'a> {
    pub url: &'a str,
    pub format_selector: &'a str,
    pub target: &'a Path,
    /// Set when the engine is expected to merge two streams itself.
    pub merge_container: Option<&'a str>,
}

/// Download once with nothing behind it, and -- only for a refusal a linked
/// browser might answer -- once more with that browser's session.
///
/// The gate is the same one `analyze` uses, for the same reason: a public video
/// is the overwhelmingly common case and must never cause the stored session to
/// be written to disk. `--continue` means the second attempt picks up whatever
/// the first managed to write, so nothing is downloaded twice.
pub async fn run(
    options: EngineDownload<'_>,
    settings: &Settings,
    control: Arc<TaskControl>,
    on_progress: &mut (dyn FnMut(ProgressSample) + Send),
) -> AppResult<u64> {
    let refusal = match attempt(&options, settings, &control, on_progress, None).await {
        Ok(size) => return Ok(size),
        Err(err) => err,
    };

    let Some(session) = engine::session_for(&refusal, options.url, settings) else {
        return Err(refusal);
    };

    let jar = session.path().to_string_lossy().into_owned();
    let retried = attempt(&options, settings, &control, on_progress, Some(jar.as_str())).await;

    // The engine rewrites the jar it was handed, carrying over whatever Google
    // rotated while the download ran; folding that back is what keeps the
    // session usable without the browser ever being opened again. Only after a
    // run that succeeded, though -- the jar a refused run leaves behind may be
    // a signed-out one, and storing that over a good session would break the
    // next download rather than help it. Failing to fold back costs freshness,
    // not this download, so it does not become the user's problem.
    if retried.is_ok() {
        session.fold_back();
    }

    retried
}

async fn attempt(
    options: &EngineDownload<'_>,
    settings: &Settings,
    control: &Arc<TaskControl>,
    on_progress: &mut (dyn FnMut(ProgressSample) + Send),
    cookies: Option<&str>,
) -> AppResult<u64> {
    let binary = tools::require_engine()?;
    let mut args = engine::base_args(settings);

    args.push("--newline".into());
    args.push("--progress".into());
    args.push("--progress-template".into());
    args.push(PROGRESS_TEMPLATE.into());
    // The engine otherwise prints a line for every block it reads, many times
    // a second on a fast link, and on a phone formatting those in Python and
    // parsing them here is real work for lines that are mostly thrown away.
    // The yt-dlp this app installs has the option; a copy chosen by hand on
    // the desktop might not.
    #[cfg(target_os = "android")]
    {
        args.push("--progress-delta".into());
        args.push("0.5".into());
    }
    args.push("--no-playlist".into());
    // Resume whatever a previous run left behind.
    args.push("--continue".into());
    args.push("-f".into());
    args.push(options.format_selector.to_string());
    args.push("-o".into());
    args.push(engine::output_template(options.target));

    if let Some(container) = options.merge_container {
        args.push("--merge-output-format".into());
        args.push(container.to_string());
    }

    // Point the engine at the same FFmpeg the app manages, so it never picks up
    // a different copy from PATH.
    if let Some(location) = tools::ffmpeg_location() {
        args.push("--ffmpeg-location".into());
        args.push(location);
    }

    if let Some(jar) = cookies {
        args.push("--cookies".into());
        args.push(jar.to_string());
    }

    args.push(options.url.to_string());
    engine::log_engine_invocation(&args);

    let mut child = process::command(&binary)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| AppError::Other(format!("the engine could not be started: {err}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Other("the engine produced no output stream".into()))?;
    let stderr = child.stderr.take();

    // stderr is drained on its own task: a full pipe buffer would otherwise
    // deadlock the child while we are busy reading stdout. Each line is cut
    // down as it arrives, so the session can never be in the text that the
    // failure message is later built from.
    let stderr_handle = tokio::spawn(async move {
        let mut collected = String::new();
        if let Some(stderr) = stderr {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if collected.len() < 8192 {
                    collected.push_str(&logging::redact(&line));
                    collected.push('\n');
                }
            }
        }
        collected
    });

    let mut reader = BufReader::new(stdout).lines();
    let mut last_sample: Option<ProgressSample> = None;
    let mut last_emit = Instant::now();
    let mut poll = tokio::time::interval(Duration::from_millis(250));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let interrupted = loop {
        tokio::select! {
            line = reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        if let Some(sample) = parse_progress(&line) {
                            // Rate-limit the emission, not the parsing: the
                            // engine can print several times a second.
                            if last_emit.elapsed() >= Duration::from_millis(400) {
                                on_progress(sample.clone());
                                last_emit = Instant::now();
                            }
                            last_sample = Some(sample);
                        } else if !line.trim().is_empty() {
                            log_debug!("engine-dl", "{}", logging::redact(line.trim()));
                        }
                    }
                    Ok(None) => break false,
                    Err(err) => {
                        let _ = child.kill().await;
                        return Err(AppError::Io(format!("could not read engine output: {err}")));
                    }
                }
            }
            // Polls the control flag even while the engine is quiet, so a pause
            // during a slow fragment does not wait for the next output line.
            _ = poll.tick() => {
                if control.interrupted() {
                    break true;
                }
            }
        }
    };

    if interrupted {
        let _ = child.kill().await;
        let _ = child.wait().await;
        stderr_handle.abort();
        return Err(AppError::Canceled);
    }

    let status = child
        .wait()
        .await
        .map_err(|err| AppError::Other(format!("the engine did not exit cleanly: {err}")))?;
    let stderr_text = stderr_handle.await.unwrap_or_default();

    if !status.success() {
        if control.interrupted() {
            return Err(AppError::Canceled);
        }
        return Err(engine::classify_engine_error(&stderr_text));
    }

    if let Some(sample) = last_sample {
        on_progress(sample);
    }

    // The engine may have chosen a different extension (a merge to mkv, an
    // audio extraction). Find what it actually wrote.
    let written = resolve_output(options.target)?;
    let size = std::fs::metadata(&written).map(|m| m.len()).unwrap_or(0);
    Ok(size)
}

/// yt-dlp writes to the requested path when it can, but a container change
/// leaves a sibling with a different extension.
pub fn resolve_output(target: &Path) -> AppResult<std::path::PathBuf> {
    if target.exists() {
        return Ok(target.to_path_buf());
    }

    let (Some(dir), Some(stem)) = (target.parent(), target.file_stem()) else {
        return Err(AppError::Io("the download produced no file".into()));
    };

    let mut best: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.file_stem() != Some(stem) || !path.is_file() {
            continue;
        }
        // Skip the engine's own scratch files.
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("part") | Some("ytdl") | Some("temp")
        ) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(_, best_time)| modified > *best_time) {
            best = Some((path, modified));
        }
    }

    best.map(|(path, _)| path)
        .ok_or_else(|| AppError::Io("the download produced no file".into()))
}

/// Parse one `@P` line. Every field may be the literal "NA" when the engine
/// does not know it yet, which is why each is parsed independently.
fn parse_progress(line: &str) -> Option<ProgressSample> {
    let rest = line.strip_prefix("@P")?.trim();
    let mut parts = rest.split_whitespace();

    let downloaded = parse_number(parts.next()?)? as u64;
    let total = parse_number(parts.next().unwrap_or("NA")).map(|value| value as u64);
    let estimate = parse_number(parts.next().unwrap_or("NA")).map(|value| value as u64);
    let speed = parse_number(parts.next().unwrap_or("NA")).unwrap_or(0.0);
    let eta = parse_number(parts.next().unwrap_or("NA"));

    let total = total.or(estimate).filter(|value| *value > 0);
    let percent = total.map(|total| (downloaded as f64 / total as f64 * 100.0).clamp(0.0, 100.0));

    Some(ProgressSample {
        received: downloaded,
        total,
        speed_bps: speed.max(0.0),
        eta_sec: eta.filter(|value| *value >= 0.0),
        percent,
        // A segmented download resumes by re-running the engine, which picks up
        // its own fragment state.
        resumable: true,
    })
}

fn parse_number(value: &str) -> Option<f64> {
    if value.is_empty() || value == "NA" || value == "None" {
        return None;
    }
    value.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_complete_progress_line() {
        let sample = parse_progress("@P 1048576 10485760 NA 524288.0 18").unwrap();
        assert_eq!(sample.received, 1_048_576);
        assert_eq!(sample.total, Some(10_485_760));
        assert_eq!(sample.speed_bps, 524_288.0);
        assert_eq!(sample.eta_sec, Some(18.0));
        assert_eq!(sample.percent, Some(10.0));
    }

    #[test]
    fn falls_back_to_the_estimated_total() {
        let sample = parse_progress("@P 500 NA 1000 100.0 5").unwrap();
        assert_eq!(sample.total, Some(1000));
        assert_eq!(sample.percent, Some(50.0));
    }

    #[test]
    fn an_unknown_total_yields_no_percentage() {
        let sample = parse_progress("@P 500 NA NA NA NA").unwrap();
        assert_eq!(sample.total, None);
        assert_eq!(sample.percent, None);
        assert_eq!(sample.speed_bps, 0.0);
        assert_eq!(sample.eta_sec, None);
    }

    #[test]
    fn a_zero_total_is_treated_as_unknown() {
        let sample = parse_progress("@P 500 0 0 NA NA").unwrap();
        assert_eq!(sample.total, None);
    }

    #[test]
    fn non_progress_output_is_ignored() {
        assert!(parse_progress("[download] Destination: video.mp4").is_none());
        assert!(parse_progress("").is_none());
        assert!(parse_progress("@P").is_none());
    }

    #[test]
    fn percent_is_clamped_when_the_total_was_underestimated() {
        let sample = parse_progress("@P 1200 1000 NA 1.0 0").unwrap();
        assert_eq!(sample.percent, Some(100.0));
    }
}
