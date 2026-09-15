//! Converting media files that are already on disk.
//!
//! This is the only part of the app that starts from a local file rather than a
//! URL, but it reuses the same machinery: FFmpeg is driven through
//! [`crate::ffmpeg::run_with_progress`], and a job is interrupted through the
//! same [`TaskControl`] a download uses.
//!
//! Two rules shape everything here:
//!
//!   1. **Repackage before re-encoding.** Moving an H.264 track from MKV into
//!      MP4 is a copy: it takes seconds and loses nothing. Only when the target
//!      container genuinely cannot hold the source codecs -- or the user asked
//!      for a smaller frame or a different bitrate -- is anything re-encoded.
//!   2. **Never write over the source.** The output name is made unique in its
//!      directory at the moment the job runs, so converting `clip.mkv` to MP4
//!      beside itself produces `clip.mp4`, and doing it twice produces
//!      `clip (2).mp4` rather than silently replacing the first one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use regex::Regex;
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::downloader::control::TaskControl;
use crate::downloader::plan;
use crate::error::{AppError, AppResult};
use crate::ffmpeg::{self, FfmpegProgress};
use crate::model::{
    ConvertFormatInfo, ConvertJob, ConvertKind, ConvertOptions, ConvertProgressEvent,
    ConvertQuality, ConvertRequest, ConvertStatus, MediaProbe,
};
use crate::settings::Settings;
use crate::{filename, log_info, log_warn, process, tools, util};

pub const EVENT_CHANGED: &str = "convert://changed";
pub const EVENT_PROGRESS: &str = "convert://progress";

/// Containers offered as a target. Anything not on one of these two lists is
/// refused: the arguments FFmpeg needs differ per container, and an unchecked
/// extension would reach the command line as an output path.
const VIDEO_FORMATS: [&str; 5] = ["mp4", "mkv", "webm", "mov", "avi"];
const AUDIO_FORMATS: [&str; 7] = ["mp3", "m4a", "aac", "wav", "flac", "opus", "ogg"];

pub fn format_kind(target: &str) -> Option<ConvertKind> {
    let target = target.trim().to_ascii_lowercase();
    if VIDEO_FORMATS.contains(&target.as_str()) {
        Some(ConvertKind::Video)
    } else if AUDIO_FORMATS.contains(&target.as_str()) {
        Some(ConvertKind::Audio)
    } else {
        None
    }
}

/// The list the UI renders. Names live in the translation dictionary; this is
/// only the set of ids, so the two sides cannot offer different formats.
pub fn catalogue() -> Vec<ConvertFormatInfo> {
    VIDEO_FORMATS
        .iter()
        .map(|id| (id, ConvertKind::Video))
        .chain(AUDIO_FORMATS.iter().map(|id| (id, ConvertKind::Audio)))
        .map(|(id, kind)| ConvertFormatInfo {
            id: (*id).to_string(),
            kind,
        })
        .collect()
}

// -- probing ---------------------------------------------------------------

fn ffprobe_path() -> Option<PathBuf> {
    // The managed FFmpeg install puts ffprobe next to ffmpeg, and a system
    // install almost always does too.
    if let Some(ffmpeg) = tools::ffmpeg_path() {
        let sibling = ffmpeg.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else if cfg!(target_os = "android") {
            "libffprobe.so"
        } else {
            "ffprobe"
        });
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    process::which("ffprobe")
}

/// Read what a local file actually contains.
///
/// ffprobe is asked first because its JSON is unambiguous. When it is not on
/// disk -- a system FFmpeg installed without it -- the same facts are recovered
/// from the banner `ffmpeg -i` prints, which is less precise but enough to show
/// the file and to drive a progress bar.
pub async fn probe(path: &str) -> AppResult<MediaProbe> {
    let file = Path::new(path);
    let meta = std::fs::metadata(file)
        .map_err(|err| AppError::Io(format!("{path} could not be read: {err}")))?;
    if !meta.is_file() {
        return Err(AppError::Io(format!("{path} is not a file")));
    }

    let mut probe = MediaProbe {
        path: path.to_string(),
        file_name: file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string()),
        container: file
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase(),
        size_bytes: meta.len(),
        duration_sec: None,
        width: None,
        height: None,
        fps: None,
        video_codec: None,
        audio_codec: None,
        audio_bitrate_kbps: None,
        has_video: false,
        has_audio: false,
    };

    if let Some(binary) = ffprobe_path() {
        match probe_with_ffprobe(&binary, path, &mut probe).await {
            Ok(()) => return Ok(probe),
            Err(err) => log_warn!("convert", "ffprobe could not read {path}: {err}"),
        }
    }

    probe_with_ffmpeg(path, &mut probe).await?;
    Ok(probe)
}

async fn probe_with_ffprobe(binary: &Path, path: &str, out: &mut MediaProbe) -> AppResult<()> {
    let args: Vec<String> = vec![
        "-v".into(),
        "error".into(),
        "-print_format".into(),
        "json".into(),
        "-show_format".into(),
        "-show_streams".into(),
        path.to_string(),
    ];

    let output = process::run(binary, &args).await?;
    if !output.success() {
        return Err(AppError::Other(format!(
            "ffprobe exited with {:?}: {}",
            output.status,
            output.stderr.lines().next().unwrap_or("no detail").trim()
        )));
    }

    let json: serde_json::Value =
        serde_json::from_str(&output.stdout).map_err(|err| AppError::Parse(err.to_string()))?;

    out.duration_sec = json
        .pointer("/format/duration")
        .and_then(|value| value.as_str())
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| *value > 0.0);

    let Some(streams) = json.get("streams").and_then(|value| value.as_array()) else {
        return Ok(());
    };

    for stream in streams {
        match stream.get("codec_type").and_then(|value| value.as_str()) {
            // Cover art is stored as a video stream with a single frame.
            // Treating it as video would make an MP3 look like a film.
            Some("video") if !is_cover_art(stream) => {
                if out.has_video {
                    continue;
                }
                out.has_video = true;
                out.video_codec = stream
                    .get("codec_name")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                out.width = stream
                    .get("width")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u32);
                out.height = stream
                    .get("height")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u32);
                out.fps = stream
                    .get("r_frame_rate")
                    .and_then(|value| value.as_str())
                    .and_then(parse_rational);
            }
            Some("audio") => {
                if out.has_audio {
                    continue;
                }
                out.has_audio = true;
                out.audio_codec = stream
                    .get("codec_name")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                out.audio_bitrate_kbps = stream
                    .get("bit_rate")
                    .and_then(|value| value.as_str())
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|bits| bits / 1000.0)
                    .filter(|value| *value > 0.0);
            }
            _ => {}
        }
    }

    if !out.has_video && !out.has_audio {
        return Err(AppError::Other("the file contains no media streams".into()));
    }
    Ok(())
}

/// A still image packed into an audio file: one frame, and FFmpeg marks it.
fn is_cover_art(stream: &serde_json::Value) -> bool {
    stream
        .pointer("/disposition/attached_pic")
        .and_then(|value| value.as_u64())
        == Some(1)
}

/// "30000/1001" -> 29.97. Returns None for the "0/0" FFmpeg reports when a
/// stream has no meaningful frame rate.
fn parse_rational(value: &str) -> Option<f64> {
    let (num, den) = value.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    if den == 0.0 || num == 0.0 {
        return None;
    }
    Some(num / den)
}

static DURATION_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Duration:\s*(\d+):(\d{2}):(\d{2})\.(\d+)").expect("valid regex"));
static VIDEO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Stream #\d+:\d+.*: Video: (\w+).*?, (\d{2,5})x(\d{2,5})").expect("valid regex")
});
static AUDIO_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Stream #\d+:\d+.*: Audio: (\w+)").expect("valid regex"));
static AUDIO_BITRATE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r": Audio:.*?, (\d+) kb/s").expect("valid regex"));

/// Fallback probe. `ffmpeg -i` with no output file prints what it found and
/// exits non-zero, which is expected here rather than a failure.
async fn probe_with_ffmpeg(path: &str, out: &mut MediaProbe) -> AppResult<()> {
    let binary = tools::require_ffmpeg()?;
    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-i".into(),
        path.to_string(),
    ];
    let output = process::run(&binary, &args).await?;
    let text = output.stderr;

    if let Some(caps) = DURATION_RE.captures(&text) {
        let hours: f64 = caps[1].parse().unwrap_or(0.0);
        let minutes: f64 = caps[2].parse().unwrap_or(0.0);
        let seconds: f64 = caps[3].parse().unwrap_or(0.0);
        let fraction: f64 = format!("0.{}", &caps[4]).parse().unwrap_or(0.0);
        let total = hours * 3600.0 + minutes * 60.0 + seconds + fraction;
        out.duration_sec = (total > 0.0).then_some(total);
    }

    if let Some(caps) = VIDEO_RE.captures(&text) {
        out.has_video = true;
        out.video_codec = Some(caps[1].to_string());
        out.width = caps[2].parse().ok();
        out.height = caps[3].parse().ok();
    }
    if let Some(caps) = AUDIO_RE.captures(&text) {
        out.has_audio = true;
        out.audio_codec = Some(caps[1].to_string());
    }
    if let Some(caps) = AUDIO_BITRATE_RE.captures(&text) {
        out.audio_bitrate_kbps = caps[1].parse().ok();
    }

    if !out.has_video && !out.has_audio {
        return Err(AppError::Other(
            "no audio or video stream could be read from this file".into(),
        ));
    }
    Ok(())
}

// -- argument construction -------------------------------------------------

/// Whether a plain repackage is worth attempting at all.
///
/// FFmpeg would reject an impossible copy on its own, but an attempt costs a
/// process launch -- and asking for a smaller frame or a specific bitrate is a
/// request to re-encode by definition, which a copy would silently ignore.
fn copy_is_plausible(
    target: &str,
    kind: ConvertKind,
    probe: &MediaProbe,
    options: &ConvertOptions,
) -> bool {
    if !options.allow_stream_copy {
        return false;
    }
    match kind {
        ConvertKind::Video => {
            if options.max_height.is_some() {
                return false;
            }
            let vcodec = probe.video_codec.as_deref().unwrap_or("");
            let acodec = probe.audio_codec.as_deref().unwrap_or("");
            plan::container_holds_video(target, vcodec)
                && plan::container_holds_audio(target, acodec)
        }
        ConvertKind::Audio => {
            if options.audio_bitrate_kbps.is_some() {
                return false;
            }
            probe.has_audio && audio_codec_fits(target, probe.audio_codec.as_deref().unwrap_or(""))
        }
    }
}

/// Which source codecs each audio container can take without re-encoding.
fn audio_codec_fits(target: &str, codec: &str) -> bool {
    let codec = codec.trim().to_ascii_lowercase();
    if codec.is_empty() {
        return false;
    }
    match target {
        "mp3" => codec.starts_with("mp3"),
        "m4a" | "aac" => {
            codec.starts_with("aac") || codec.starts_with("mp4a") || codec.starts_with("alac")
        }
        "opus" => codec.starts_with("opus"),
        "ogg" => codec.starts_with("vorbis") || codec.starts_with("opus"),
        "flac" => codec.starts_with("flac"),
        "wav" => codec.starts_with("pcm"),
        _ => false,
    }
}

/// Round to a standard rate at or below the source, so a 128 kbps file is never
/// "upconverted" into a bigger one carrying no more detail.
fn audio_bitrate(
    target: &str,
    probe: &MediaProbe,
    options: &ConvertOptions,
) -> Option<&'static str> {
    if matches!(target, "wav" | "flac") {
        return None;
    }
    if let Some(requested) = options.audio_bitrate_kbps {
        return Some(match requested {
            value if value >= 320 => "320k",
            value if value >= 256 => "256k",
            value if value >= 192 => "192k",
            value if value >= 160 => "160k",
            value if value >= 128 => "128k",
            _ => "96k",
        });
    }
    Some(
        probe
            .audio_bitrate_kbps
            .map(|value| match value {
                value if value >= 320.0 => "320k",
                value if value >= 256.0 => "256k",
                value if value >= 192.0 => "192k",
                value if value >= 160.0 => "160k",
                _ => "128k",
            })
            .unwrap_or("192k"),
    )
}

/// Constant-quality value for the video encoder. Lower is better looking and
/// larger; these are the usual "visually lossless / good / small" points, and
/// VP9's scale is not H.264's.
fn crf_for(target: &str, quality: ConvertQuality) -> &'static str {
    match (target, quality) {
        ("webm", ConvertQuality::High) => "28",
        ("webm", ConvertQuality::Balanced) => "33",
        ("webm", ConvertQuality::Small) => "38",
        (_, ConvertQuality::High) => "18",
        (_, ConvertQuality::Balanced) => "23",
        (_, ConvertQuality::Small) => "28",
    }
}

/// Which audio encoder belongs in a video container.
fn video_container_audio(target: &str) -> &'static str {
    match target {
        "webm" => "libopus",
        // AVI predates AAC; MP3 is what players expect to find in one.
        "avi" => "libmp3lame",
        _ => "aac",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// Repackage: no encoder is named at all.
    Copy,
    /// Re-encode on the GPU.
    Hardware,
    /// Re-encode on the CPU.
    Software,
}

fn build_args(
    input: &Path,
    output: &Path,
    target: &str,
    kind: ConvertKind,
    probe: &MediaProbe,
    options: &ConvertOptions,
    pass: Pass,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
    ];

    // Only the first video and audio stream are carried over, and both are
    // optional. Copying a subtitle or data stream the target cannot hold would
    // fail a repackage that would otherwise have worked.
    match kind {
        ConvertKind::Video => {
            args.extend(["-map".into(), "0:v:0?".into(), "-map".into(), "0:a:0?".into()]);
        }
        ConvertKind::Audio => {
            args.extend(["-map".into(), "0:a:0?".into(), "-vn".into()]);
        }
    }
    args.extend(["-sn".into(), "-dn".into()]);

    if pass == Pass::Copy {
        args.extend(["-c".into(), "copy".into()]);
    } else if kind == ConvertKind::Audio {
        match target {
            "mp3" => args.extend(["-c:a".into(), "libmp3lame".into()]),
            "wav" => args.extend(["-c:a".into(), "pcm_s16le".into()]),
            "flac" => args.extend(["-c:a".into(), "flac".into()]),
            "opus" => args.extend(["-c:a".into(), "libopus".into()]),
            "ogg" => args.extend(["-c:a".into(), "libvorbis".into()]),
            _ => args.extend(["-c:a".into(), "aac".into()]),
        }
        if let Some(bitrate) = audio_bitrate(target, probe, options) {
            args.extend(["-b:a".into(), bitrate.into()]);
        }
    } else {
        // Cap the height without ever enlarging a smaller source, and keep the
        // width even -- H.264 cannot encode odd dimensions in 4:2:0.
        if let Some(height) = options.max_height {
            args.extend(["-vf".into(), format!("scale=-2:'min({height},ih)'")]);
        }

        match (target, pass) {
            ("webm", _) => args.extend([
                "-c:v".into(),
                "libvpx-vp9".into(),
                "-crf".into(),
                crf_for(target, options.quality).into(),
                "-b:v".into(),
                "0".into(),
                // VP9 uses one thread per tile without this, which is the
                // difference between slow and unusable on a long file.
                "-row-mt".into(),
                "1".into(),
            ]),
            (_, Pass::Hardware) => args.extend([
                "-c:v".into(),
                "h264_nvenc".into(),
                "-rc".into(),
                "vbr".into(),
                "-cq".into(),
                crf_for(target, options.quality).into(),
                "-b:v".into(),
                "0".into(),
            ]),
            _ => args.extend([
                "-c:v".into(),
                "libx264".into(),
                "-crf".into(),
                crf_for(target, options.quality).into(),
                "-preset".into(),
                "veryfast".into(),
                // Chroma subsampling every player understands. Some sources are
                // 10-bit or 4:4:4, which many hardware decoders refuse.
                "-pix_fmt".into(),
                "yuv420p".into(),
            ]),
        }

        args.extend(["-c:a".into(), video_container_audio(target).into()]);
        if let Some(bitrate) = audio_bitrate(target, probe, options) {
            args.extend(["-b:a".into(), bitrate.into()]);
        }
    }

    // MP4 and its relatives keep their index at the end unless told otherwise,
    // which leaves a file unseekable until it is completely written.
    if matches!(target, "mp4" | "mov" | "m4a") {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }

    args.push(output.to_string_lossy().into_owned());
    args
}

// -- the job list ----------------------------------------------------------

/// Conversions run one at a time. Encoding saturates every core it is given, so
/// a second job running alongside finishes neither any sooner.
const MAX_CONCURRENT: u32 = 1;

pub struct ConvertManager {
    app: AppHandle,
    settings: Arc<Mutex<Settings>>,
    jobs: Mutex<Vec<ConvertJob>>,
    controls: Mutex<HashMap<String, Arc<TaskControl>>>,
    running: AtomicU32,
    wake: Notify,
}

impl ConvertManager {
    pub fn new(app: AppHandle, settings: Arc<Mutex<Settings>>) -> Arc<Self> {
        Arc::new(Self {
            app,
            settings,
            jobs: Mutex::new(Vec::new()),
            controls: Mutex::new(HashMap::new()),
            running: AtomicU32::new(0),
            wake: Notify::new(),
        })
    }

    fn settings(&self) -> Settings {
        self.settings
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn with_jobs<T>(&self, f: impl FnOnce(&mut Vec<ConvertJob>) -> T) -> T {
        let mut guard = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    pub fn list(&self) -> Vec<ConvertJob> {
        self.with_jobs(|jobs| jobs.clone())
    }

    pub fn active_count(&self) -> u32 {
        self.with_jobs(|jobs| jobs.iter().filter(|job| !job.status.is_terminal()).count() as u32)
    }

    /// Queue one job per input file.
    ///
    /// Every file is probed first: the duration is what turns FFmpeg's output
    /// into a percentage, and a file that cannot be read is worth reporting now
    /// rather than after it reaches the front of the queue.
    pub async fn enqueue(self: &Arc<Self>, request: ConvertRequest) -> AppResult<Vec<ConvertJob>> {
        let target = request.options.target_format.trim().to_ascii_lowercase();
        let Some(kind) = format_kind(&target) else {
            return Err(AppError::Other(format!(
                "{target} is not a format this app can write"
            )));
        };
        if request.input_paths.is_empty() {
            return Err(AppError::Other("no files were given to convert".into()));
        }
        // Fail before queueing anything, rather than filling the list with jobs
        // that will each fail the same way.
        tools::require_ffmpeg()?;

        let mut options = request.options.clone();
        options.target_format = target;

        // On Android a picked file is a private copy in the app's cache, so
        // "beside the source" would be a folder the user cannot reach.
        #[cfg(target_os = "android")]
        if options.output_dir.as_deref().is_none_or(|dir| dir.trim().is_empty()) {
            let dir = self.settings().download_dir;
            std::fs::create_dir_all(&dir).map_err(|err| {
                AppError::Permission(format!("{dir} could not be created: {err}"))
            })?;
            options.output_dir = Some(dir);
        }

        normalize_output_dir(&mut options)?;

        let mut created = Vec::with_capacity(request.input_paths.len());
        for path in &request.input_paths {
            let probed = probe(path).await?;
            let job = ConvertJob {
                id: util::new_id("cv"),
                input_path: probed.path.clone(),
                input_name: probed.file_name.clone(),
                input_size_bytes: probed.size_bytes,
                output_path: None,
                output_size_bytes: None,
                kind,
                status: ConvertStatus::Queued,
                percent: None,
                duration_sec: probed.duration_sec,
                stream_copied: false,
                error: None,
                created_at: util::now_ms(),
                started_at: None,
                completed_at: None,
                options: options.clone(),
            };
            self.with_jobs(|jobs| jobs.push(job.clone()));
            created.push(job);
        }

        self.emit_changed();
        self.wake.notify_one();
        Ok(created)
    }

    pub fn cancel(self: &Arc<Self>, id: &str) {
        if let Some(control) = self.control_for(id) {
            control.cancel();
        }
        // A job that has not started yet has no control handle, so its status is
        // set directly.
        self.update_job(id, |job| {
            if !job.status.is_terminal() {
                job.status = ConvertStatus::Canceled;
                job.percent = None;
                job.completed_at = Some(util::now_ms());
            }
        });
        self.emit_changed();
        self.wake.notify_one();
    }

    pub fn retry(self: &Arc<Self>, id: &str) {
        self.update_job(id, |job| {
            if job.status.is_terminal() {
                job.status = ConvertStatus::Queued;
                job.error = None;
                job.percent = None;
                job.output_path = None;
                job.output_size_bytes = None;
                job.stream_copied = false;
                job.completed_at = None;
            }
        });
        self.emit_changed();
        self.wake.notify_one();
    }

    pub fn remove(self: &Arc<Self>, id: &str) {
        if let Some(control) = self.control_for(id) {
            control.cancel();
        }
        self.with_jobs(|jobs| jobs.retain(|job| job.id != id));
        self.controls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        self.emit_changed();
    }

    pub fn clear_finished(self: &Arc<Self>) {
        self.with_jobs(|jobs| jobs.retain(|job| !job.status.is_terminal()));
        self.emit_changed();
    }

    /// Stop everything on the way out, so no encoder outlives the window.
    pub fn shutdown(&self) {
        let controls = self.controls.lock().unwrap_or_else(|e| e.into_inner());
        for control in controls.values() {
            control.cancel();
        }
    }

    // -- scheduling --------------------------------------------------------

    /// Long-lived admission loop, started once at app setup.
    pub fn spawn_scheduler(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            loop {
                if manager.running.load(Ordering::Acquire) < MAX_CONCURRENT {
                    if let Some(job) = manager.take_next_queued() {
                        manager.running.fetch_add(1, Ordering::AcqRel);
                        let worker = Arc::clone(&manager);
                        tauri::async_runtime::spawn(async move {
                            worker.run_job(job).await;
                            worker.running.fetch_sub(1, Ordering::AcqRel);
                            worker.wake.notify_one();
                        });
                        continue;
                    }
                }
                manager.wake.notified().await;
            }
        });
    }

    fn take_next_queued(self: &Arc<Self>) -> Option<ConvertJob> {
        self.with_jobs(|jobs| {
            let job = jobs
                .iter_mut()
                .find(|job| job.status == ConvertStatus::Queued)?;
            job.status = ConvertStatus::Running;
            job.started_at = Some(util::now_ms());
            Some(job.clone())
        })
    }

    async fn run_job(self: &Arc<Self>, job: ConvertJob) {
        let control = TaskControl::shared();
        self.controls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(job.id.clone(), Arc::clone(&control));

        // The job was marked running before this task was spawned, so it can
        // already have been cancelled in between. Checking after the control is
        // registered is what closes that window: a cancel arriving from here on
        // finds the handle and interrupts FFmpeg itself.
        let still_running = self.with_jobs(|jobs| {
            jobs.iter()
                .any(|entry| entry.id == job.id && entry.status == ConvertStatus::Running)
        });
        if !still_running {
            self.controls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&job.id);
            return;
        }
        self.emit_changed();

        let result = self.execute(&job, Arc::clone(&control)).await;

        self.controls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job.id);

        match result {
            Ok(outcome) => self.complete(&job.id, outcome),
            Err(AppError::Canceled) => self.settle_cancel(&job.id),
            Err(err) => self.fail(&job.id, err),
        }
    }

    /// Run the passes in order until one produces a file.
    ///
    /// A repackage is tried when the container can hold what the source already
    /// is; the GPU encoder is tried when the user has it enabled, because it is
    /// absent on most machines without an NVIDIA card and refusing to fall back
    /// would turn a preference into a hard failure; the software encoder always
    /// comes last and is what actually guarantees a result.
    async fn execute(
        self: &Arc<Self>,
        job: &ConvertJob,
        control: Arc<TaskControl>,
    ) -> AppResult<Outcome> {
        let input = PathBuf::from(&job.input_path);
        if !input.is_file() {
            return Err(AppError::Io(format!(
                "{} is no longer on disk",
                job.input_name
            )));
        }

        // The file may have been probed minutes ago; re-reading it now keeps the
        // codec decisions honest and costs one short process.
        let probed = probe(&job.input_path).await?;
        let target = job.options.target_format.as_str();

        let dir = match job.options.output_dir.as_deref() {
            Some(dir) => PathBuf::from(dir),
            None => input
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
        };
        std::fs::create_dir_all(&dir).map_err(|err| {
            AppError::Permission(format!("{} could not be created: {err}", dir.display()))
        })?;

        let stem = filename::sanitize_component(
            &input
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        let stem = if stem.is_empty() {
            "converted".to_string()
        } else {
            stem
        };
        let output = filename::unique_path(&dir, &stem, target);

        let settings = self.settings();
        let mut passes: Vec<Pass> = Vec::with_capacity(3);
        if copy_is_plausible(target, job.kind, &probed, &job.options) {
            passes.push(Pass::Copy);
        }
        if job.kind == ConvertKind::Video && target != "webm" && settings.hardware_acceleration {
            passes.push(Pass::Hardware);
        }
        passes.push(Pass::Software);

        let mut last_error: Option<AppError> = None;
        for pass in passes {
            let args = build_args(&input, &output, target, job.kind, &probed, &job.options, pass);
            let mut sink = {
                let manager = Arc::clone(self);
                let id = job.id.clone();
                move |progress: FfmpegProgress| manager.on_progress(&id, progress)
            };

            match ffmpeg::run_with_progress(
                &args,
                probed.duration_sec,
                Arc::clone(&control),
                &mut sink,
            )
            .await
            {
                Ok(()) => {
                    let size = std::fs::metadata(&output).map(|meta| meta.len()).ok();
                    // FFmpeg can exit cleanly having written nothing when the
                    // mapping matched no stream. That is not a success.
                    if size.unwrap_or(0) == 0 {
                        let _ = std::fs::remove_file(&output);
                        last_error =
                            Some(AppError::Other("the conversion produced an empty file".into()));
                        continue;
                    }
                    return Ok(Outcome {
                        output_path: output,
                        output_size_bytes: size,
                        stream_copied: pass == Pass::Copy,
                    });
                }
                Err(AppError::Canceled) => {
                    let _ = std::fs::remove_file(&output);
                    return Err(AppError::Canceled);
                }
                Err(err) => {
                    log_info!(
                        "convert",
                        "{} as {pass:?} into .{target} failed: {err}",
                        job.input_name
                    );
                    // A half-written file from a rejected pass must not be left
                    // where the next pass is about to write.
                    let _ = std::fs::remove_file(&output);
                    last_error = Some(err);
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| AppError::Other("the conversion could not be started".into())))
    }

    fn complete(self: &Arc<Self>, id: &str, outcome: Outcome) {
        let path = outcome.output_path.to_string_lossy().into_owned();
        self.update_job(id, |job| {
            job.status = ConvertStatus::Completed;
            job.percent = Some(100.0);
            job.output_path = Some(path.clone());
            job.output_size_bytes = outcome.output_size_bytes;
            job.stream_copied = outcome.stream_copied;
            job.completed_at = Some(util::now_ms());
        });
        log_info!("convert", "wrote {path}");
        #[cfg(target_os = "android")]
        crate::android::announce_media(&self.app, &path);
        self.emit_job(id);
        self.emit_changed();
    }

    fn fail(self: &Arc<Self>, id: &str, err: AppError) {
        let info = err.to_info();
        log_warn!("convert", "job {id} failed: {err}");
        self.update_job(id, |job| {
            job.status = ConvertStatus::Failed;
            job.error = Some(info.clone());
            job.percent = None;
            job.completed_at = Some(util::now_ms());
        });
        self.emit_job(id);
        self.emit_changed();
    }

    fn settle_cancel(self: &Arc<Self>, id: &str) {
        self.update_job(id, |job| {
            job.status = ConvertStatus::Canceled;
            job.percent = None;
            job.completed_at = Some(util::now_ms());
        });
        self.emit_job(id);
        self.emit_changed();
    }

    fn on_progress(self: &Arc<Self>, id: &str, progress: FfmpegProgress) {
        let updated = self.update_job(id, |job| {
            if job.status == ConvertStatus::Running {
                job.percent = progress.percent;
            }
        });

        // Progress is its own lightweight event: a running conversion should not
        // re-render the whole list several times a second.
        match updated {
            Some(job) if job.status == ConvertStatus::Running => {
                let _ = self.app.emit(
                    EVENT_PROGRESS,
                    ConvertProgressEvent {
                        id: job.id,
                        status: job.status,
                        percent: job.percent,
                        output_path: job.output_path,
                        output_size_bytes: job.output_size_bytes,
                        stream_copied: job.stream_copied,
                        error: job.error,
                    },
                );
            }
            _ => {}
        }
    }

    fn update_job(&self, id: &str, f: impl FnOnce(&mut ConvertJob)) -> Option<ConvertJob> {
        self.with_jobs(|jobs| {
            let job = jobs.iter_mut().find(|job| job.id == id)?;
            f(job);
            Some(job.clone())
        })
    }

    fn control_for(&self, id: &str) -> Option<Arc<TaskControl>> {
        self.controls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    fn emit_changed(&self) {
        let _ = self.app.emit(EVENT_CHANGED, self.list());
    }

    fn emit_job(&self, id: &str) {
        let Some(job) = self.with_jobs(|jobs| jobs.iter().find(|job| job.id == id).cloned()) else {
            return;
        };
        let _ = self.app.emit(
            EVENT_PROGRESS,
            ConvertProgressEvent {
                id: job.id,
                status: job.status,
                percent: job.percent,
                output_path: job.output_path,
                output_size_bytes: job.output_size_bytes,
                stream_copied: job.stream_copied,
                error: job.error,
            },
        );
    }
}

struct Outcome {
    output_path: PathBuf,
    output_size_bytes: Option<u64>,
    stream_copied: bool,
}

/// An empty or missing folder means "beside the source file"; anything else has
/// to exist and be a directory before a job is accepted.
fn normalize_output_dir(options: &mut ConvertOptions) -> AppResult<()> {
    let Some(dir) = options
        .output_dir
        .as_ref()
        .map(|value| value.trim().to_string())
    else {
        return Ok(());
    };
    if dir.is_empty() {
        options.output_dir = None;
        return Ok(());
    }
    if !Path::new(&dir).is_dir() {
        return Err(AppError::Io(format!("{dir} is not a folder")));
    }
    options.output_dir = Some(dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_probe() -> MediaProbe {
        MediaProbe {
            path: "in.mkv".into(),
            file_name: "in.mkv".into(),
            container: "mkv".into(),
            size_bytes: 1024,
            duration_sec: Some(60.0),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30.0),
            video_codec: Some("h264".into()),
            audio_codec: Some("aac".into()),
            audio_bitrate_kbps: Some(192.0),
            has_video: true,
            has_audio: true,
        }
    }

    fn options(target: &str) -> ConvertOptions {
        ConvertOptions {
            target_format: target.into(),
            output_dir: None,
            quality: ConvertQuality::Balanced,
            max_height: None,
            audio_bitrate_kbps: None,
            allow_stream_copy: true,
        }
    }

    fn args_for(target: &str, pass: Pass, options: &ConvertOptions) -> Vec<String> {
        let kind = format_kind(target).expect("a known format");
        build_args(
            Path::new("in.mkv"),
            Path::new(&format!("out.{target}")),
            target,
            kind,
            &sample_probe(),
            options,
            pass,
        )
    }

    fn value_after(args: &[String], key: &str) -> Option<String> {
        args.iter()
            .position(|arg| arg == key)
            .map(|index| args[index + 1].clone())
    }

    #[test]
    fn only_known_formats_are_offered() {
        assert_eq!(format_kind("mp4"), Some(ConvertKind::Video));
        assert_eq!(format_kind("MP3"), Some(ConvertKind::Audio));
        assert_eq!(format_kind("exe"), None);
        assert_eq!(format_kind("../evil"), None);
        assert_eq!(catalogue().len(), VIDEO_FORMATS.len() + AUDIO_FORMATS.len());
    }

    #[test]
    fn a_copy_pass_never_names_an_encoder() {
        let args = args_for("mp4", Pass::Copy, &options("mp4"));
        assert_eq!(value_after(&args, "-c").as_deref(), Some("copy"));
        assert!(!args.iter().any(|arg| arg == "libx264"));
    }

    #[test]
    fn h264_and_aac_can_be_repackaged_into_mp4_but_not_webm() {
        let probe = sample_probe();
        assert!(copy_is_plausible("mp4", ConvertKind::Video, &probe, &options("mp4")));
        assert!(copy_is_plausible("mkv", ConvertKind::Video, &probe, &options("mkv")));
        assert!(!copy_is_plausible("webm", ConvertKind::Video, &probe, &options("webm")));
    }

    #[test]
    fn aac_audio_is_repackaged_into_m4a_and_re_encoded_into_mp3() {
        let probe = sample_probe();
        assert!(copy_is_plausible("m4a", ConvertKind::Audio, &probe, &options("m4a")));
        assert!(!copy_is_plausible("mp3", ConvertKind::Audio, &probe, &options("mp3")));
    }

    #[test]
    fn asking_for_a_smaller_frame_rules_out_a_repackage() {
        let mut opts = options("mp4");
        opts.max_height = Some(720);
        assert!(!copy_is_plausible("mp4", ConvertKind::Video, &sample_probe(), &opts));
    }

    #[test]
    fn asking_for_a_bitrate_rules_out_an_audio_repackage() {
        let mut opts = options("m4a");
        opts.audio_bitrate_kbps = Some(128);
        assert!(!copy_is_plausible("m4a", ConvertKind::Audio, &sample_probe(), &opts));
    }

    #[test]
    fn a_repackage_can_be_turned_off_entirely() {
        let mut opts = options("mp4");
        opts.allow_stream_copy = false;
        assert!(!copy_is_plausible("mp4", ConvertKind::Video, &sample_probe(), &opts));
    }

    #[test]
    fn audio_targets_drop_the_video_stream() {
        for target in AUDIO_FORMATS {
            let args = args_for(target, Pass::Software, &options(target));
            assert!(args.iter().any(|arg| arg == "-vn"), "{target}");
            assert!(
                args.windows(2).any(|w| w[0] == "-map" && w[1] == "0:a:0?"),
                "{target}"
            );
        }
    }

    #[test]
    fn video_targets_keep_both_streams_and_drop_the_rest() {
        for target in VIDEO_FORMATS {
            let args = args_for(target, Pass::Software, &options(target));
            assert!(!args.iter().any(|arg| arg == "-vn"), "{target}");
            assert!(
                args.windows(2).any(|w| w[0] == "-map" && w[1] == "0:v:0?"),
                "{target}"
            );
            assert!(args.iter().any(|arg| arg == "-sn"), "{target}");
        }
    }

    #[test]
    fn each_audio_container_selects_its_encoder() {
        let codec_for =
            |target: &str| value_after(&args_for(target, Pass::Software, &options(target)), "-c:a");
        assert_eq!(codec_for("mp3").as_deref(), Some("libmp3lame"));
        assert_eq!(codec_for("wav").as_deref(), Some("pcm_s16le"));
        assert_eq!(codec_for("flac").as_deref(), Some("flac"));
        assert_eq!(codec_for("opus").as_deref(), Some("libopus"));
        assert_eq!(codec_for("ogg").as_deref(), Some("libvorbis"));
        assert_eq!(codec_for("m4a").as_deref(), Some("aac"));
        assert_eq!(codec_for("aac").as_deref(), Some("aac"));
    }

    #[test]
    fn lossless_audio_targets_carry_no_bitrate() {
        for target in ["wav", "flac"] {
            let args = args_for(target, Pass::Software, &options(target));
            assert!(!args.iter().any(|arg| arg == "-b:a"), "{target}");
        }
    }

    #[test]
    fn the_audio_bitrate_never_exceeds_the_source() {
        let mut probe = sample_probe();
        probe.audio_bitrate_kbps = Some(128.0);
        assert_eq!(audio_bitrate("mp3", &probe, &options("mp3")), Some("128k"));
    }

    #[test]
    fn an_explicit_bitrate_wins_over_the_source() {
        let mut opts = options("mp3");
        opts.audio_bitrate_kbps = Some(320);
        assert_eq!(audio_bitrate("mp3", &sample_probe(), &opts), Some("320k"));
    }

    #[test]
    fn each_video_container_gets_audio_it_can_hold() {
        let codec_for =
            |target: &str| value_after(&args_for(target, Pass::Software, &options(target)), "-c:a");
        assert_eq!(codec_for("mp4").as_deref(), Some("aac"));
        assert_eq!(codec_for("mkv").as_deref(), Some("aac"));
        assert_eq!(codec_for("webm").as_deref(), Some("libopus"));
        assert_eq!(codec_for("avi").as_deref(), Some("libmp3lame"));
    }

    #[test]
    fn webm_always_uses_vp9_even_when_the_gpu_is_available() {
        let args = args_for("webm", Pass::Hardware, &options("webm"));
        assert_eq!(value_after(&args, "-c:v").as_deref(), Some("libvpx-vp9"));
    }

    #[test]
    fn the_gpu_pass_and_the_cpu_pass_name_different_encoders() {
        assert_eq!(
            value_after(&args_for("mp4", Pass::Hardware, &options("mp4")), "-c:v").as_deref(),
            Some("h264_nvenc")
        );
        assert_eq!(
            value_after(&args_for("mp4", Pass::Software, &options("mp4")), "-c:v").as_deref(),
            Some("libx264")
        );
    }

    #[test]
    fn a_smaller_target_is_a_downscale_that_never_enlarges() {
        let mut opts = options("mp4");
        opts.max_height = Some(720);
        let filter = value_after(&args_for("mp4", Pass::Software, &opts), "-vf").expect("-vf");
        assert_eq!(filter, "scale=-2:'min(720,ih)'");
    }

    #[test]
    fn quality_moves_the_constant_rate_factor() {
        assert_eq!(crf_for("mp4", ConvertQuality::High), "18");
        assert_eq!(crf_for("mp4", ConvertQuality::Small), "28");
        assert_eq!(crf_for("webm", ConvertQuality::Balanced), "33");
    }

    #[test]
    fn mp4_output_is_made_seekable_and_matroska_is_left_alone() {
        assert!(args_for("mp4", Pass::Copy, &options("mp4"))
            .iter()
            .any(|arg| arg == "+faststart"));
        assert!(!args_for("mkv", Pass::Copy, &options("mkv"))
            .iter()
            .any(|arg| arg == "+faststart"));
    }

    #[test]
    fn the_output_path_is_always_the_final_argument() {
        let args = args_for("mp4", Pass::Software, &options("mp4"));
        assert_eq!(args.last().unwrap(), "out.mp4");
    }

    #[test]
    fn a_frame_rate_of_zero_over_zero_is_not_a_frame_rate() {
        assert_eq!(
            parse_rational("30000/1001").map(|value| (value * 100.0).round()),
            Some(2997.0)
        );
        assert_eq!(parse_rational("0/0"), None);
        assert_eq!(parse_rational("nonsense"), None);
    }

    #[test]
    fn an_output_folder_that_does_not_exist_is_refused() {
        let mut opts = options("mp4");
        opts.output_dir = Some("Z:\\definitely\\not\\here".into());
        assert!(normalize_output_dir(&mut opts).is_err());
    }

    #[test]
    fn a_blank_output_folder_means_beside_the_source() {
        let mut opts = options("mp4");
        opts.output_dir = Some("   ".into());
        assert!(normalize_output_dir(&mut opts).is_ok());
        assert_eq!(opts.output_dir, None);
    }
}
