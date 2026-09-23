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
//!
//! A fetch of part of a link is the one thing here that is not that shape. The
//! engine hands that transfer to an FFmpeg of its own -- `--downloader native`
//! is accepted and then silently overridden -- which makes the real downloader
//! a grandchild, costs the progress readout entirely, and is why every run
//! started here is started as a family that can be killed as one.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::downloader::control::TaskControl;
use crate::downloader::http::ProgressSample;
use crate::downloader::plan::KeptRange;
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
    /// Fetch only this much of the source. `None` is the whole of it.
    pub section: Option<KeptRange>,
    /// Re-encode around the cuts so the fetch begins on the frame that was
    /// asked for. Only means anything alongside a section.
    pub force_keyframes: bool,
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

    // A section is fetched by FFmpeg, and the engine simply gives up if there
    // is none: "You have requested downloading the video partially, but ffmpeg
    // is not installed". Asking first turns that into the card the app already
    // knows how to draw, with the Install button on it.
    if options.section.is_some() {
        tools::require_ffmpeg()?;
    }
    clear_stale_part(options);

    let args = download_args(options, engine::base_args(settings), cookies);
    engine::log_engine_invocation(&args);

    let mut tree = process::spawn_tree(
        process::command(&binary)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;

    let stdout = tree
        .child
        .stdout
        .take()
        .ok_or_else(|| AppError::Other("the engine produced no output stream".into()))?;
    let stderr = tree.child.stderr.take();

    // stderr is drained on its own task: a full pipe buffer would otherwise
    // deadlock the child while we are busy reading stdout. Each line is cut
    // down as it arrives, so the session can never be in the text that the
    // failure message is later built from.
    let stderr_handle = tokio::spawn(async move {
        let mut collected = Diagnostics::default();
        if let Some(stderr) = stderr {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                collected.push(&logging::redact(&line));
            }
        }
        collected.into_text()
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
                        tree.kill().await;
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
        // The whole family, not just the engine. A cancelled ranged fetch that
        // killed only the parent would leave the FFmpeg underneath it
        // downloading and writing to a file nobody is waiting for any more.
        tree.kill().await;
        stderr_handle.abort();
        return Err(AppError::Canceled);
    }

    let status = tree
        .child
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

/// Everything after the shared arguments: what to fetch, how much of it, and
/// where to put it.
///
/// Split out from the run so the vector can be read in a test. What goes on an
/// engine command line is most of what this module decides, and a flag that
/// quietly stopped being passed would show up as a download that is merely
/// slower or larger rather than as a failure.
fn download_args(
    options: &EngineDownload<'_>,
    base: Vec<String>,
    cookies: Option<&str>,
) -> Vec<String> {
    let mut args = base;

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

    match options.section {
        // The star is what makes this a time range rather than a pattern
        // matched against the chapter titles: without it, "0-10" is a regular
        // expression that matches no chapter and fetches nothing.
        Some(range) => {
            args.push("--download-sections".into());
            args.push(format!(
                "*{}-{}",
                timestamp(range.start_sec),
                timestamp(range.end_sec)
            ));
            // Resuming means nothing here. The transfer is an FFmpeg process
            // writing a fresh `.part` from the first byte, so `--continue`
            // cannot pick anything up -- and a half file left by an earlier
            // run of a different range would be a file of the wrong film.
            args.push("--no-continue".into());
            if options.force_keyframes {
                // Replaces the stream copy with a real re-encode so the cut
                // begins on the frame that was asked for rather than on the
                // keyframe before it. Roughly twice the time and a third more
                // bytes, which is why it is a choice and not the default.
                args.push("--force-keyframes-at-cuts".into());
            }
        }
        // Resume whatever a previous run left behind.
        None => args.push("--continue".into()),
    }

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
    args
}

/// A mark as `--download-sections` reads it. Plain seconds with milliseconds,
/// which this engine parses alongside `MM:SS` and `HH:MM:SS`, and which cannot
/// be misread whatever the length of the film.
fn timestamp(seconds: f64) -> String {
    format!("{:.3}", seconds.max(0.0))
}

/// Take away a partial file a section fetch would neither resume nor replace.
///
/// A ranged transfer has no resumable state: FFmpeg writes a fresh `.part` from
/// its first byte, which is why `--no-continue` goes with the section. What is
/// left over from a run that was cancelled halfway is therefore never picked up
/// and never overwritten either -- it simply sits there, and on the next run of
/// a *different* range it is a half file of the wrong piece of the film under a
/// name that looks like this one's.
///
/// Every scratch file is looked for rather than one, because the name is not
/// always the one that was asked for: measured, a fetch written to `job.mp4`
/// whose streams merged into WebM leaves `job.mp4.webm.part` behind.
fn clear_stale_part(options: &EngineDownload<'_>) {
    if options.section.is_none() {
        return;
    }
    let (Some(dir), Some(name)) = (options.target.parent(), options.target.file_name()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let name = name.to_string_lossy().into_owned();
    for entry in entries.flatten() {
        let found = entry.file_name().to_string_lossy().into_owned();
        if found.starts_with(&name) && found.ends_with(".part") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// What the engine had to say about a run, kept so that the part which explains
/// the failure survives however much else there is.
///
/// A plain capped buffer does not survive a ranged fetch. The transfer goes
/// through FFmpeg, whose banner is the first thing on the pipe and whose
/// `frame=` lines then arrive several times a second; the cap is reached long
/// before anything goes wrong, so the real `ERROR:` never gets stored and the
/// message the user is shown is the version number of FFmpeg.
#[derive(Default)]
struct Diagnostics {
    /// The first reported error, kept whole and kept regardless. This is the
    /// line the classification and the message are both built from.
    reported: Option<String>,
    /// The beginning of everything else, for the log.
    context: String,
}

impl Diagnostics {
    /// How much surrounding output is worth keeping. Enough to explain a
    /// failure that never says "ERROR:", small enough that a file producing a
    /// warning per frame cannot grow it without bound.
    const CONTEXT_LIMIT: usize = 8192;

    /// As much of one message as is worth reading. The engine's own errors are
    /// a sentence; anything far longer than this is a tool quoting a signed URL
    /// back at us.
    const LINE_LIMIT: usize = 2048;

    fn push(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.reported.is_none()
            && (trimmed.starts_with("ERROR:") || trimmed.starts_with("error:"))
        {
            self.reported = Some(clipped(trimmed, Self::LINE_LIMIT));
            return;
        }
        // The remaining budget rather than the whole line. FFmpeg's status is
        // one line that it rewrites with carriage returns rather than newlines,
        // so a transfer of any length arrives here as a single line of
        // unbounded size -- measured, 6 KB of it in eight seconds.
        let room = Self::CONTEXT_LIMIT.saturating_sub(self.context.len());
        if room == 0 {
            return;
        }
        self.context.push_str(&clipped(trimmed, room));
        self.context.push('\n');
    }

    /// The reported error first, so that whoever reads this finds it whether
    /// they take the first line or search the whole text for a phrase.
    fn into_text(self) -> String {
        match self.reported {
            Some(reported) => format!("{reported}\n{}", self.context),
            None => self.context,
        }
    }
}

/// The first `limit` bytes of `text`, cut on a character rather than through
/// one. Slicing a string at a byte offset that lands inside a multi-byte
/// character panics, and a title in any language but English is full of them.
fn clipped(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let end = (0..=limit)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);
    text[..end].to_owned()
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

    fn options(section: Option<KeptRange>, force_keyframes: bool) -> EngineDownload<'static> {
        EngineDownload {
            url: "https://example.test/watch?v=x",
            format_selector: "137+140",
            target: Path::new("C:\\temp\\job.e.mp4"),
            merge_container: Some("mp4"),
            section,
            force_keyframes,
        }
    }

    fn args_for(section: Option<KeptRange>, force_keyframes: bool) -> Vec<String> {
        download_args(&options(section, force_keyframes), Vec::new(), None)
    }

    fn value_after(args: &[String], key: &str) -> Option<String> {
        args.iter()
            .position(|arg| arg == key)
            .and_then(|index| args.get(index + 1).cloned())
    }

    fn range(start: f64, end: f64) -> KeptRange {
        KeptRange {
            start_sec: start,
            end_sec: end,
        }
    }

    #[test]
    fn a_section_is_asked_for_as_a_time_range_and_not_as_a_chapter() {
        let args = args_for(Some(range(30.0, 40.0)), false);
        // The star is the whole difference: without it this is a pattern
        // matched against chapter titles, and it matches none of them.
        assert_eq!(
            value_after(&args, "--download-sections").as_deref(),
            Some("*30.000-40.000")
        );
    }

    #[test]
    fn a_section_replaces_resuming_rather_than_joining_it() {
        let args = args_for(Some(range(30.0, 40.0)), false);
        assert!(args.iter().any(|arg| arg == "--no-continue"));
        assert!(
            !args.iter().any(|arg| arg == "--continue"),
            "a section is written from the first byte by an FFmpeg that has \
             nothing to resume"
        );
    }

    #[test]
    fn a_whole_download_still_resumes_and_asks_for_no_section() {
        let args = args_for(None, false);
        assert!(args.iter().any(|arg| arg == "--continue"));
        assert!(!args.iter().any(|arg| arg == "--no-continue"));
        assert!(!args.iter().any(|arg| arg == "--download-sections"));
    }

    #[test]
    fn an_exact_cut_is_only_asked_for_alongside_a_section() {
        assert!(args_for(Some(range(30.0, 40.0)), true)
            .iter()
            .any(|arg| arg == "--force-keyframes-at-cuts"));
        assert!(!args_for(Some(range(30.0, 40.0)), false)
            .iter()
            .any(|arg| arg == "--force-keyframes-at-cuts"));
        // Nothing to force keyframes at, so the flag would only cost a
        // re-encode of the whole video.
        assert!(!args_for(None, true)
            .iter()
            .any(|arg| arg == "--force-keyframes-at-cuts"));
    }

    #[test]
    fn a_mark_is_written_in_seconds_to_the_millisecond() {
        assert_eq!(timestamp(0.0), "0.000");
        assert_eq!(timestamp(4.8), "4.800");
        assert_eq!(timestamp(3661.25), "3661.250");
        // A mark that arrived below zero is the start of the file, not a
        // negative offset the engine would refuse to parse.
        assert_eq!(timestamp(-2.0), "0.000");
    }

    #[test]
    fn the_link_is_always_the_final_argument() {
        for section in [None, Some(range(30.0, 40.0))] {
            let args = args_for(section, true);
            assert_eq!(args.last().unwrap(), "https://example.test/watch?v=x");
        }
    }

    #[test]
    fn a_session_is_only_passed_when_there_is_one_to_pass() {
        let args = download_args(&options(None, false), Vec::new(), Some("C:\\temp\\jar.txt"));
        assert_eq!(
            value_after(&args, "--cookies").as_deref(),
            Some("C:\\temp\\jar.txt")
        );
        assert!(!args_for(None, false).iter().any(|arg| arg == "--cookies"));
    }

    #[test]
    fn a_half_file_of_an_earlier_range_is_taken_away_before_a_new_one_starts() {
        let dir = std::env::temp_dir().join("ud-engine-part");
        let _ = std::fs::create_dir_all(&dir);
        let target = dir.join("job.e.mp4");

        // Both spellings the engine uses: the name as asked for, and the name
        // with the container it actually merged into appended.
        let plain = dir.join("job.e.mp4.part");
        let remuxed = dir.join("job.e.mp4.webm.part");
        let finished = dir.join("job.e.mp4.webm");
        for path in [&plain, &remuxed, &finished] {
            std::fs::write(path, b"half a film").unwrap();
        }

        let mut options = options(Some(range(30.0, 40.0)), false);
        options.target = &target;
        clear_stale_part(&options);

        assert!(!plain.exists());
        assert!(!remuxed.exists());
        assert!(
            finished.exists(),
            "a finished file is somebody's result, not scratch"
        );
        let _ = std::fs::remove_file(&finished);
    }

    #[test]
    fn a_whole_download_keeps_its_partial_file_to_resume_from() {
        let dir = std::env::temp_dir().join("ud-engine-part-kept");
        let _ = std::fs::create_dir_all(&dir);
        let target = dir.join("job.e.mp4");
        let part = dir.join("job.e.mp4.part");
        std::fs::write(&part, b"half a film").unwrap();

        let mut options = options(None, false);
        options.target = &target;
        clear_stale_part(&options);

        assert!(part.exists(), "this is exactly what --continue picks up");
        let _ = std::fs::remove_file(&part);
    }

    #[test]
    fn the_reported_error_survives_a_flood_of_frame_lines() {
        // What a ranged fetch actually puts on the pipe: FFmpeg's banner
        // first, then a line per frame for as long as it runs, and the one
        // line that explains the failure somewhere after the cap.
        let mut log = Diagnostics::default();
        log.push("ffmpeg version N-126740-g1234567 Copyright (c) 2000-2026");
        for index in 0..4000 {
            log.push(&format!(
                "frame= {index} fps=120 q=-1.0 size=  2048kB time=00:00:0{}.00 bitrate=N/A",
                index % 10
            ));
        }
        log.push("ERROR: unable to download video data: HTTP Error 403: Forbidden");

        let text = log.into_text();
        assert!(
            text.starts_with("ERROR: unable to download video data"),
            "the explanation has to be the first line, not the version number"
        );
        assert!(text.contains("403"));
        assert_eq!(engine::classify_engine_error(&text).code(), "forbidden");
    }

    #[test]
    fn one_endless_status_line_cannot_grow_the_buffer_without_bound() {
        // FFmpeg rewrites its status with carriage returns rather than
        // newlines, so however long the transfer runs it is one line here.
        let mut log = Diagnostics::default();
        log.push(&"frame= 1 fps=45 size= 256KiB\r".repeat(4000));
        log.push("ERROR: unable to download video data: HTTP Error 403: Forbidden");
        let text = log.into_text();
        assert!(text.starts_with("ERROR: unable to download"));
        assert!(text.len() < 16_384, "{} bytes", text.len());
    }

    #[test]
    fn a_line_is_never_cut_through_a_character() {
        // A title in any language but English is full of multi-byte
        // characters, and slicing one in half is a panic rather than a
        // truncated message.
        let text = "é".repeat(100);
        for limit in 0..=200 {
            let cut = clipped(&text, limit);
            assert!(cut.len() <= limit);
            assert!(text.starts_with(&cut));
        }
    }

    #[test]
    fn the_first_reported_error_is_the_one_that_is_kept() {
        let mut log = Diagnostics::default();
        log.push("ERROR: unable to download video data: HTTP Error 403: Forbidden");
        log.push("ERROR: Postprocessing: Conversion failed");
        assert!(log.into_text().starts_with("ERROR: unable to download"));
    }

    #[test]
    fn a_failure_that_never_says_error_is_still_reported() {
        let mut log = Diagnostics::default();
        log.push("");
        log.push("  You have requested downloading the video partially, but ffmpeg is not installed. Aborting.  ");
        let text = log.into_text();
        assert_eq!(
            engine::classify_engine_error(&text).code(),
            "ffmpegMissing",
            "the one sentence a ranged fetch fails with has to survive"
        );
    }

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
