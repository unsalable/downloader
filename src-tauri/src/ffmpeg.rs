//! FFmpeg operations: merging split streams, converting, and the editor's
//! export passes.
//!
//! FFmpeg is only invoked when there is no other way to produce the requested
//! file. A merge is a stream copy -- no re-encoding, so it is I/O bound and
//! finishes in seconds. Re-encoding only happens when a container genuinely
//! cannot hold the source codecs, or when the user has asked for something a
//! copy cannot give them, and it is logged when it does.
//!
//! This module builds argument vectors and runs them. It decides nothing about
//! what should be exported: which ranges are kept, where a cut is allowed to
//! land and what the result is called all belong to [`crate::export`], which is
//! also the only place that knows a job may be several processes long.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::downloader::control::TaskControl;
use crate::downloader::plan;
use crate::error::{AppError, AppResult};
use crate::model::{
    AspectRatio, AudioCodec, EditSegment, ExportOptions, ExportQuality, FrameFit, VideoCodec,
};
use crate::{log_debug, log_info, process, tools};

/// Reported while FFmpeg works, so a long merge is not a frozen progress bar.
#[derive(Debug, Clone)]
pub struct FfmpegProgress {
    pub percent: Option<f64>,
    pub bytes_written: u64,
}

fn base_args() -> Vec<String> {
    vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
    ]
}

/// Interleave a separate video and audio stream into one file.
///
/// A stream copy is the goal and the normal outcome: it is I/O bound and
/// finishes in seconds. It is only possible when the output container accepts
/// both codecs, though -- WebM takes VP8/VP9/AV1 with Opus or Vorbis and
/// nothing else -- so when the user has asked for a container that cannot hold
/// one of the streams, that stream, and only that stream, is re-encoded.
#[allow(clippy::too_many_arguments)]
pub async fn merge(
    video: &Path,
    audio: &Path,
    output: &Path,
    video_codec: Option<&str>,
    audio_codec: Option<&str>,
    duration_sec: Option<f64>,
    control: Arc<TaskControl>,
    on_progress: &mut (dyn FnMut(FfmpegProgress) + Send),
) -> AppResult<()> {
    let target = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mkv")
        .to_ascii_lowercase();

    let video_fits = plan::container_holds_video(&target, video_codec.unwrap_or(""));
    let audio_fits = plan::container_holds_audio(&target, audio_codec.unwrap_or(""));

    if video_fits && audio_fits {
        let args = merge_args(video, audio, output, &target, true, true);
        match run_with_progress(&args, duration_sec, Arc::clone(&control), on_progress).await {
            Ok(()) => return Ok(()),
            Err(AppError::Canceled) => return Err(AppError::Canceled),
            Err(err) => {
                // The codecs the source reported were not the whole story.
                // Rather than failing, re-encode into what the container does
                // accept -- slower, but it produces the file that was asked for.
                log_info!(
                    "ffmpeg",
                    "copying both streams into .{target} failed ({err}); re-encoding instead"
                );
            }
        }
    } else {
        log_info!(
            "ffmpeg",
            ".{target} cannot hold {} + {}; re-encoding the streams it rejects",
            video_codec.unwrap_or("?"),
            audio_codec.unwrap_or("?")
        );
    }

    let args = merge_args(video, audio, output, &target, video_fits, audio_fits);
    run_with_progress(&args, duration_sec, control, on_progress).await
}

fn merge_args(
    video: &Path,
    audio: &Path,
    output: &Path,
    target: &str,
    copy_video: bool,
    copy_audio: bool,
) -> Vec<String> {
    let mut args = base_args();
    args.extend([
        "-i".into(),
        video.to_string_lossy().into_owned(),
        "-i".into(),
        audio.to_string_lossy().into_owned(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "1:a:0".into(),
    ]);

    if copy_video {
        args.extend(["-c:v".into(), "copy".into()]);
    } else if target == "webm" {
        args.extend([
            "-c:v".into(),
            "libvpx-vp9".into(),
            "-crf".into(),
            "31".into(),
            "-b:v".into(),
            "0".into(),
        ]);
    } else {
        args.extend([
            "-c:v".into(),
            "libx264".into(),
            "-crf".into(),
            "20".into(),
            "-preset".into(),
            "veryfast".into(),
        ]);
    }

    if copy_audio {
        args.extend(["-c:a".into(), "copy".into()]);
    } else if target == "webm" {
        args.extend(["-c:a".into(), "libopus".into(), "-b:a".into(), "160k".into()]);
    } else {
        args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()]);
    }

    // MP4 needs its index at the front for the file to be seekable before it is
    // fully written -- cheap here, and it makes the result behave in players.
    if target == "mp4" {
        args.push("-movflags".into());
        args.push("+faststart".into());
    }

    args.push(output.to_string_lossy().into_owned());
    args
}

/// Repackage or re-encode into `target_container`.
pub async fn convert(
    input: &Path,
    output: &Path,
    duration_sec: Option<f64>,
    source_audio_bitrate: Option<f64>,
    hardware_acceleration: bool,
    control: Arc<TaskControl>,
    on_progress: &mut (dyn FnMut(FfmpegProgress) + Send),
) -> AppResult<()> {
    let target = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();

    // A picture has no container to repackage into: a copy would only put the
    // same JPEG bytes in a file named .png. It is always encoded.
    if is_image_target(&target) {
        let args = image_conversion_args(input, output, &target);
        return run_with_progress(&args, None, control, on_progress).await;
    }

    // Try a stream copy first: if the codecs already fit the container this is
    // a fast repackage rather than a re-encode.
    let copy_args = conversion_args(input, output, &target, source_audio_bitrate, true, false);
    match run_with_progress(&copy_args, duration_sec, Arc::clone(&control), on_progress).await {
        Ok(()) => return Ok(()),
        Err(AppError::Canceled) => return Err(AppError::Canceled),
        Err(err) => {
            log_info!(
                "ffmpeg",
                "stream copy into .{target} was rejected, re-encoding instead: {err}"
            );
        }
    }

    let encode_args = conversion_args(
        input,
        output,
        &target,
        source_audio_bitrate,
        false,
        hardware_acceleration,
    );
    run_with_progress(&encode_args, duration_sec, control, on_progress).await
}

fn conversion_args(
    input: &Path,
    output: &Path,
    target: &str,
    source_audio_bitrate: Option<f64>,
    copy_only: bool,
    hardware_acceleration: bool,
) -> Vec<String> {
    let mut args = base_args();
    args.push("-i".into());
    args.push(input.to_string_lossy().into_owned());

    let audio_only = matches!(target, "mp3" | "m4a" | "aac" | "wav" | "opus" | "ogg" | "flac");
    if audio_only {
        // Drop video and any cover art rather than letting it become a stream
        // the target container may not accept.
        args.push("-vn".into());
    }

    if copy_only {
        args.push("-c".into());
        args.push("copy".into());
    } else if audio_only {
        // Round to a standard rate at or below the source, so a 128 kbps source
        // is never "upconverted" into a larger file with no added quality.
        let bitrate = source_audio_bitrate
            .map(|value| match value {
                v if v >= 320.0 => "320k",
                v if v >= 256.0 => "256k",
                v if v >= 192.0 => "192k",
                v if v >= 160.0 => "160k",
                _ => "128k",
            })
            .unwrap_or("192k");

        match target {
            "mp3" => args.extend(["-c:a".into(), "libmp3lame".into(), "-b:a".into(), bitrate.into()]),
            "wav" => args.extend(["-c:a".into(), "pcm_s16le".into()]),
            "opus" | "ogg" => args.extend(["-c:a".into(), "libopus".into(), "-b:a".into(), bitrate.into()]),
            "flac" => args.extend(["-c:a".into(), "flac".into()]),
            _ => args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), bitrate.into()]),
        }
    } else {
        match target {
            "webm" => args.extend([
                "-c:v".into(),
                "libvpx-vp9".into(),
                "-crf".into(),
                "31".into(),
                "-b:v".into(),
                "0".into(),
                "-c:a".into(),
                "libopus".into(),
            ]),
            _ => {
                // The hardware encoder is tried first when enabled; it is an
                // order of magnitude cheaper than libx264 on the CPU.
                if hardware_acceleration {
                    args.extend(["-c:v".into(), "h264_nvenc".into(), "-cq".into(), "23".into()]);
                } else {
                    args.extend([
                        "-c:v".into(),
                        "libx264".into(),
                        "-crf".into(),
                        "20".into(),
                        "-preset".into(),
                        "veryfast".into(),
                    ]);
                }
                args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()]);
            }
        }
    }

    if target == "mp4" {
        args.push("-movflags".into());
        args.push("+faststart".into());
    }

    args.push(output.to_string_lossy().into_owned());
    args
}

fn is_image_target(target: &str) -> bool {
    matches!(target, "jpg" | "jpeg" | "png" | "webp")
}

/// Write the first frame of `input` as a single picture. Animated sources
/// (a GIF, an animated WebP) become their first frame, which is what a
/// still-image format can hold.
fn image_conversion_args(input: &Path, output: &Path, target: &str) -> Vec<String> {
    let mut args = base_args();
    args.push("-i".into());
    args.push(input.to_string_lossy().into_owned());
    args.extend(["-frames:v".into(), "1".into(), "-an".into()]);

    match target {
        "png" => args.extend(["-c:v".into(), "png".into()]),
        "webp" => args.extend([
            "-c:v".into(),
            "libwebp".into(),
            "-quality".into(),
            "90".into(),
        ]),
        // JPEG has no alpha and wants full-range 4:2:0 for the widest support.
        _ => args.extend([
            "-c:v".into(),
            "mjpeg".into(),
            "-q:v".into(),
            "2".into(),
            "-pix_fmt".into(),
            "yuvj420p".into(),
        ]),
    }

    // The image muxer otherwise expects a numbered sequence of files.
    args.extend(["-update".into(), "1".into()]);
    args.push(output.to_string_lossy().into_owned());
    args
}

// -- the editor's export ----------------------------------------------------

/// One process's slice of a job made of several of them.
///
/// Each FFmpeg process reports its own 0..100, so a lossless export of four
/// kept ranges would fill the bar five times over. A slice rescales a pass's
/// own figure into the part of the whole job that pass is responsible for, and
/// the slices a job hands out add up to one bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassSlice {
    start: f64,
    span: f64,
}

impl PassSlice {
    /// A job that is one process from beginning to end.
    pub fn whole() -> Self {
        Self {
            start: 0.0,
            span: 100.0,
        }
    }

    pub fn new(start: f64, span: f64) -> Self {
        let start = start.clamp(0.0, 100.0);
        Self {
            start,
            span: span.clamp(0.0, 100.0 - start),
        }
    }

    /// Where a pass's own percentage falls in the job as a whole.
    pub fn place(self, local: Option<f64>) -> Option<f64> {
        local.map(|value| {
            (self.start + value.clamp(0.0, 100.0) / 100.0 * self.span).clamp(0.0, 100.0)
        })
    }
}

/// Slices for a run of passes that together own `span` of the bar, each pass
/// taking a share in proportion to how much of the output it produces.
///
/// Weighting by output rather than by pass count is what keeps the bar from
/// lurching: a job of one four-second range and one forty-second range would
/// otherwise spend half its bar on a tenth of the work.
pub fn weighted_slices(weights: &[f64], start: f64, span: f64) -> Vec<PassSlice> {
    let total: f64 = weights.iter().filter(|value| value.is_finite()).sum();
    let mut offset = start;
    weights
        .iter()
        .map(|weight| {
            // An unusable set of weights still has to produce a bar that moves
            // forward, so the passes fall back to equal shares.
            let share = if total > 0.0 && weight.is_finite() {
                weight / total
            } else {
                1.0 / weights.len().max(1) as f64
            };
            let slice = PassSlice::new(offset, span * share);
            offset += span * share;
            slice
        })
        .collect()
}

/// What an export does about a source brighter than Rec. 709.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneMap {
    /// Not asked for, or the source is not HDR -- in which case there is
    /// nothing to map and the setting means nothing.
    None,
    /// Into linear light, through Hable, and back out to Rec. 709.
    Hable,
    /// What is left when this FFmpeg has no `zscale` or no `tonemap`: the
    /// picture is converted to 8-bit 4:2:0 and nothing else. The highlights
    /// stay where they were, so a bright sky still washes out, and that is
    /// worth saying plainly rather than calling it a tone map.
    Plain,
}

/// Bringing an HDR source down to Rec. 709.
///
/// Every input side is stated rather than read. Measured on this build, a
/// stream properly tagged smpte2084 / bt2020nc still answers `zscale=t=linear`
/// with "no path between colorspaces" and exits 127; it resolves only once
/// `tin=` is given, and the stage after the linearisation needs `pin=` for the
/// same reason. Stating the input also says what the setting means: the user
/// has told us this source is PQ, and a source that is not gets `ToneMap::None`
/// instead of being crushed by a curve it never needed.
const TONE_MAP_CHAIN: &str = "zscale=tin=smpte2084:t=linear:npl=100,format=gbrpf32le,\
zscale=pin=bt2020:p=bt709,tonemap=tonemap=hable:desat=0,\
zscale=tin=linear:t=bt709:m=bt709:r=tv";

/// Containers that keep their index at the end unless told otherwise, which
/// leaves the file unseekable until the last byte of it is written.
fn wants_faststart(container: &str) -> bool {
    matches!(container, "mp4" | "m4v" | "mov" | "m4a")
}

/// A time on the command line, at the precision it was worked out to.
///
/// Three decimals used to be enough and is not: a keyframe at 8.333333 written
/// as `8.333` is a third of a millisecond *below* the keyframe, and a backward
/// seek that lands below a keyframe resolves to the one before it -- measured,
/// a 8.533 s cut arriving as 16.867 s with a whole extra group of pictures in
/// front of it. Six decimals is what ffprobe prints these times as in the first
/// place, so the number written is the number that was measured.
fn seek_value(seconds: f64) -> String {
    format!("{seconds:.6}")
}

/// One kept range, copied through.
///
/// `-ss` and `-to` both go before `-i` so that FFmpeg seeks to the start rather
/// than reading and discarding everything ahead of it, which on a long film is
/// the difference between a second and a minute.
///
/// `seek_sec` is where to aim the seek, which is not the same number as the
/// range's start: it is a point a little way inside the keyframe's own group of
/// pictures, because FFmpeg's backward seek can otherwise land on the keyframe
/// before it. Working out where the cut really lands belongs to
/// [`crate::export`] and not here, because that is also where the interface is
/// told, before the user commits to it.
///
/// Only the first video track and the audio tracks come along, each optional.
/// An editor that refused a file for carrying a subtitle track or an attached
/// cover image would be refusing the ordinary case. See [`copied_streams`] for
/// what `keep_audio` does.
pub fn lossless_single_args(
    input: &Path,
    output: &Path,
    container: &str,
    seek_sec: f64,
    end_sec: f64,
    keep_audio: bool,
) -> Vec<String> {
    let mut args = base_args();
    args.extend([
        "-ss".into(),
        seek_value(seek_sec),
        "-to".into(),
        seek_value(end_sec),
        "-i".into(),
        input.to_string_lossy().into_owned(),
    ]);
    args.extend(copied_streams(keep_audio));
    args.extend([
        // Title, creation date and the rest belong to the film, not to the
        // range of it that was kept.
        "-map_metadata".into(),
        "0".into(),
        "-c".into(),
        "copy".into(),
        // A range that starts mid-stream carries timestamps that no longer
        // begin at zero. Left alone they make players open the file at the
        // wrong point, or refuse it.
        "-avoid_negative_ts".into(),
        "make_zero".into(),
    ]);
    if wants_faststart(container) {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.push(output.to_string_lossy().into_owned());
    args
}

/// Which streams a copy carries: the first video track, and the audio tracks
/// unless the sound is being dropped.
///
/// Dropping it is a copy's to do as much as a re-encode's. Leaving a track out
/// decodes nothing, so turning the sound off is no reason to give up a lossless
/// export -- and a track that is never mapped is also never checked against a
/// container that could not have held it. `-an` is said as well as the map
/// being left out, so that the intent survives anyone adding a map later.
fn copied_streams(keep_audio: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["-map".into(), "0:v:0?".into()];
    if keep_audio {
        args.extend(["-map".into(), "0:a?".into()]);
    } else {
        args.push("-an".into());
    }
    args
}

/// The muxer and extension one cut pass writes, which is what decides whether
/// the join afterwards can be a stream copy at all.
///
/// MPEG-TS is what the concat demuxer was built for: every packet carries its
/// own headers, so two files can be laid end to end without either of them
/// being re-written. It can only hold a codec that has an elementary-stream
/// form, though, which among what this app is handed means H.264 and HEVC.
/// Anything else goes to Matroska, which joins slightly less accurately --
/// measured, 9.588 s against 9.528 s for TS on the same ideal 9.500 -- but
/// holds everything.
///
/// The audio has a veto for the same reason, and it is the sharper one: MPEG-TS
/// asked to carry a codec it has no stream type for does not refuse, it writes
/// the track as private data and exits zero, and the join afterwards finds no
/// audio at all. An export that quietly came out silent is worse than one that
/// took the slower muxer.
pub fn segment_format(
    video_codec: Option<&str>,
    audio_codec: Option<&str>,
) -> (&'static str, &'static str) {
    match annex_b_filter(video_codec.unwrap_or("")) {
        Some(_) if mpegts_holds_audio(audio_codec) => ("mpegts", "ts"),
        _ => ("matroska", "mkv"),
    }
}

/// Whether MPEG-TS has a stream type for this audio.
///
/// `None` is a file with no audio track, where there is nothing to lose. A
/// codec the probe named but this list does not know is treated as one TS
/// cannot hold, because being wrong in that direction costs a slower muxer and
/// being wrong in the other costs the sound.
fn mpegts_holds_audio(codec: Option<&str>) -> bool {
    let Some(codec) = codec else {
        return true;
    };
    let codec = codec.trim().to_ascii_lowercase();
    [
        "aac", "mp4a", "mp3", "mp2", "mp1", "ac3", "ac-3", "eac3", "ec-3", "dts", "truehd", "opus",
    ]
    .iter()
    .any(|family| codec.starts_with(family))
}

/// The bitstream filter that turns a length-prefixed stream into the Annex B
/// form MPEG-TS carries. `None` for a codec that has no such form.
fn annex_b_filter(codec: &str) -> Option<&'static str> {
    let codec = codec.trim().to_ascii_lowercase();
    if codec.starts_with("h264") || codec.starts_with("avc") || codec.starts_with("h.264") {
        Some("h264_mp4toannexb")
    } else if codec.starts_with("hevc")
        || codec.starts_with("h265")
        || codec.starts_with("hvc")
        || codec.starts_with("hev")
    {
        Some("hevc_mp4toannexb")
    } else {
        None
    }
}

/// One kept range of a multi-range lossless export, written to scratch.
///
/// The obvious way to do this is to skip the cut passes altogether and give the
/// concat demuxer `inpoint` and `outpoint` for each range. Do not: measured on
/// this build, every join then carries 45 lead-in frames crushed into three
/// milliseconds, because the demuxer hands the muxer everything from the
/// keyframe before `inpoint` and only re-stamps it. Cutting first and joining
/// whole files afterwards is the slower shape and the correct one.
///
/// `seek_sec` is the aimed-at seek rather than the range's start, and
/// `keep_audio` whether the sound comes along, exactly as in
/// [`lossless_single_args`].
pub fn lossless_cut_args(
    input: &Path,
    output: &Path,
    muxer: &str,
    seek_sec: f64,
    end_sec: f64,
    video_codec: Option<&str>,
    keep_audio: bool,
) -> Vec<String> {
    let mut args = base_args();
    args.extend([
        "-ss".into(),
        seek_value(seek_sec),
        "-to".into(),
        seek_value(end_sec),
        "-i".into(),
        input.to_string_lossy().into_owned(),
    ]);
    args.extend(copied_streams(keep_audio));
    args.extend([
        "-c".into(),
        "copy".into(),
        "-avoid_negative_ts".into(),
        "make_zero".into(),
    ]);
    if let Some(filter) = annex_b_filter(video_codec.unwrap_or("")) {
        args.extend(["-bsf:v".into(), filter.into()]);
    }
    args.extend(["-f".into(), muxer.to_string()]);
    args.push(output.to_string_lossy().into_owned());
    args
}

/// The concat demuxer's list file.
///
/// One line per piece, in order. The paths are absolute and written with
/// forward slashes, which the demuxer takes on Windows as well; an apostrophe
/// in a file name closes the quoting, so it is written as the four characters
/// `'\''` -- close, escape one, reopen.
pub fn concat_list_text(segments: &[PathBuf]) -> String {
    segments
        .iter()
        .map(|path| {
            let text = path.to_string_lossy().replace('\\', "/").replace('\'', "'\\''");
            format!("file '{text}'\n")
        })
        .collect()
}

/// Lay the cut pieces end to end without re-writing a byte of them.
///
/// `keep_audio` is the cut passes' own. The pieces of a muted export carry no
/// sound to begin with; saying so here as well is what keeps the join from
/// ever being the step that brings it back.
pub fn concat_args(list: &Path, output: &Path, container: &str, keep_audio: bool) -> Vec<String> {
    let mut args = base_args();
    args.extend([
        "-f".into(),
        "concat".into(),
        // The list holds absolute paths, which the demuxer refuses by default.
        // They are paths this run wrote itself, into the app's own scratch
        // directory, and the manager has checked that before getting here.
        "-safe".into(),
        "0".into(),
        "-i".into(),
        list.to_string_lossy().into_owned(),
    ]);
    args.extend(copied_streams(keep_audio));
    args.extend(["-c".into(), "copy".into()]);
    if wants_faststart(container) {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.push(output.to_string_lossy().into_owned());
    args
}

/// Everything a re-encode needs that [`ExportOptions`] does not carry.
pub struct ExportPlan<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    /// In order, non-overlapping, and already clamped to the source.
    pub segments: &'a [EditSegment],
    pub options: &'a ExportOptions,
    /// The source frame once its pixels are square -- the frame the graph's
    /// first stage actually produces, not the one the file stores. A 720x576
    /// frame with a 64:45 pixel arrives here as 1024x576, so that the tier
    /// ceiling is measured against the picture the user is looking at rather
    /// than against columns the graph is about to expand. `None` when the probe
    /// could not read it, which costs only the choice of tier: the graph
    /// normalises the shape either way.
    pub source_frame: Option<(u32, u32)>,
    /// False when the source has no audio, or the user asked for none. Getting
    /// this wrong is not a degraded export but a dead one: `[0:a]` on a silent
    /// file exits 127, and so does `concat` with `a=1` and no audio branch.
    pub has_audio: bool,
    /// The source's frame rate as the probe read it, which only SVT-AV1 is
    /// told (see [`svt_frame_rate_args`]).
    pub source_fps: Option<f64>,
    /// Resolved by [`crate::tools::preferred_encoder`], which has run the
    /// encoder rather than believing `-encoders`.
    pub video_encoder: &'a str,
    pub tone_map: ToneMap,
}

/// Every kept range, cut and joined and re-shaped, in one process.
///
/// One process rather than several because a re-encode has to decode the source
/// anyway: doing it once and cutting inside the filter graph costs a single
/// pass over the file, and leaves nothing on disk to clean up afterwards.
pub fn reencode_args(plan: &ExportPlan<'_>) -> AppResult<Vec<String>> {
    if plan.segments.is_empty() {
        return Err(AppError::Other("there is nothing marked to export".into()));
    }

    let options = plan.options;
    let container = options.container.as_str();
    let audio = (!options.mute && plan.has_audio).then_some(options.audio_codec);
    container_accepts(container, options.video_codec, audio)?;

    let canvas = canvas_frame(options.aspect, plan.source_frame, options.max_height);

    let mut args = base_args();
    // Hardware decoding is a different thing from hardware encoding and worth
    // having on its own: the frames come back in system memory, so the filters
    // below are the same filters either way, and `auto` falls back to the
    // software decoder rather than failing when there is nothing to use.
    args.extend(["-hwaccel".into(), "auto".into()]);

    // A `trim` filter throws frames away only after the decoder has made them,
    // so without a seek a ten-second cut from the end of a two-hour film
    // decodes the whole film first -- measured, about 21 ms per second of
    // source skipped. Seeking a second short of the first kept frame skips that
    // decode; the second of padding is what leaves the accurate seek room to
    // land, and the graph's times move with it so the output is unchanged.
    let seek = (plan.segments[0].start_sec - 1.0).max(0.0);
    if seek > 0.0 {
        args.extend(["-ss".into(), seek_value(seek)]);
    }
    args.extend(["-i".into(), plan.input.to_string_lossy().into_owned()]);

    // `-filter_complex_script` was removed from this build, so the graph goes
    // in as one argument. It is a single argv element, never a shell string,
    // and commas inside an expression are quoted so the filter parser does not
    // read them as the end of an argument.
    args.extend([
        "-filter_complex".into(),
        reencode_filter(plan, canvas, audio.is_some(), seek),
    ]);

    args.extend(["-map".into(), "[vout]".into()]);
    match audio {
        Some(_) => args.extend(["-map".into(), "[ac]".into()]),
        None => args.push("-an".into()),
    }

    args.extend(["-c:v".into(), plan.video_encoder.to_string()]);
    // One or the other and never both: each encoder has a single rate control,
    // and a constant-quality flag left beside a bitrate either overrides it or
    // is silently ignored, depending on the encoder.
    args.extend(match options.video_bitrate_kbps {
        Some(kbps) => bitrate_args(plan.video_encoder, kbps),
        None => quality_args(
            plan.video_encoder,
            quality_value(options.video_codec, options.quality),
        ),
    });
    if plan.video_encoder == "libsvtav1" {
        args.extend(svt_frame_rate_args(options.fps.or(plan.source_fps)));
    }

    // HEVC written into an MP4 or a MOV is tagged `hev1` unless told otherwise,
    // and QuickTime, Safari and Windows all refuse to play that. `hvc1` is the
    // same bytes under a name they accept.
    if options.video_codec == VideoCodec::H265 && matches!(container, "mp4" | "m4v" | "mov") {
        args.extend(["-tag:v".into(), "hvc1".into()]);
    }

    if let Some(codec) = audio {
        args.extend(["-c:a".into(), audio_encoder(codec).into()]);
        // FLAC is lossless, so a bitrate is not a thing to ask it for.
        if codec != AudioCodec::Flac {
            let kbps = options.audio_bitrate_kbps.unwrap_or(192).clamp(32, 512);
            args.extend(["-b:a".into(), format!("{kbps}k")]);
        }
    }

    if wants_faststart(container) {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }

    args.push(plan.output.to_string_lossy().into_owned());
    Ok(args)
}

/// `seek` is the input seek the caller put before `-i`, which the graph's times
/// are relative to: the first frame the decoder hands over is that point, not
/// the start of the file.
fn reencode_filter(
    plan: &ExportPlan<'_>,
    canvas: Option<(u32, u32)>,
    with_audio: bool,
    seek: f64,
) -> String {
    let mut graph = String::new();

    for (index, segment) in plan.segments.iter().enumerate() {
        let (start, end) = (segment.start_sec - seek, segment.end_sec - seek);
        // `setpts=PTS-STARTPTS` on every branch is what makes the pieces
        // joinable: each one has to begin at zero on its own clock before
        // `concat` can lay the next one after it.
        graph.push_str(&format!(
            "[0:v]trim=start={start:.3}:end={end:.3},setpts=PTS-STARTPTS[v{index}];"
        ));
        if with_audio {
            graph.push_str(&format!(
                "[0:a]atrim=start={start:.3}:end={end:.3},asetpts=PTS-STARTPTS[a{index}];"
            ));
        }
    }

    for index in 0..plan.segments.len() {
        graph.push_str(&format!("[v{index}]"));
        if with_audio {
            graph.push_str(&format!("[a{index}]"));
        }
    }

    // The gain goes on once, after the join, so that every range gets the same
    // one and the join itself sees the source's own levels. Whatever the audio
    // passes through, it leaves the graph as `[ac]`, which is the label the
    // caller maps.
    let gain = if with_audio { plan.options.gain() } else { None };
    let joined_audio = match (with_audio, gain) {
        (false, _) => "",
        (true, None) => "[ac]",
        (true, Some(_)) => "[acat]",
    };
    graph.push_str(&format!(
        "concat=n={}:v=1:a={}[vc]{joined_audio};",
        plan.segments.len(),
        u8::from(with_audio),
    ));
    if let Some(gain) = gain {
        graph.push_str(&format!("[acat]volume={gain:.3}[ac];"));
    }

    graph.push_str("[vc]");
    graph.push_str(&video_chain(plan, canvas));
    graph.push_str("[vout]");
    graph
}

/// The four stages every re-encoded frame goes through, each one earned by a
/// measured failure of the graph without it.
fn video_chain(plan: &ExportPlan<'_>, canvas: Option<(u32, u32)>) -> String {
    let mut stages: Vec<String> = Vec::new();

    // Square the pixels first. A source stored 720x576 with a 64:45 pixel is a
    // 16:9 picture, and without this the "16:9" button would hand back a file
    // measuring 2.37:1. Odd dimensions also stop libx264 dead, and this is
    // where they stop being odd, so it belongs in every graph -- including the
    // one that changes no shape at all.
    stages.push("scale=w='trunc(iw*max(1,sar)/2)*2':h='trunc(ih/min(1,sar)/2)*2'".into());
    stages.push("setsar=1".into());

    match plan.tone_map {
        ToneMap::None => {}
        ToneMap::Hable => stages.push(TONE_MAP_CHAIN.into()),
        ToneMap::Plain => stages.push("format=yuv420p".into()),
    }

    match canvas {
        Some((width, height)) => match plan.options.fit {
            // The frame is poured into the canvas and the overflow is taken
            // off. Deriving the size inside the crop instead gives an even but
            // inexact frame: 202x360 is 0.5611, not 0.5625.
            FrameFit::Fill => {
                stages.push(format!(
                    "scale={width}:{height}:force_original_aspect_ratio=increase:force_divisible_by=2"
                ));
                stages.push(format!("crop={width}:{height}"));
            }
            FrameFit::Fit => {
                stages.push(format!(
                    "scale={width}:{height}:force_original_aspect_ratio=decrease:force_divisible_by=2"
                ));
                stages.push(format!("pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black"));
            }
        },
        // No canvas: the frame keeps its own shape, capped in height if asked.
        None => {
            if let Some(height) = plan.options.max_height {
                stages.push(format!("scale=-2:'min({height},ih)'"));
            }
        }
    }

    // A rate the user asked for is exact in both directions with this and only
    // with this -- dropping frames for a slower rate, repeating them for a
    // faster one. Without a request the source's own rate is kept.
    if let Some(fps) = plan.options.fps.filter(|value| *value > 0.0) {
        stages.push(format!("fps={fps}"));
    }

    // `force_original_aspect_ratio` leaves the sample aspect at 5120:5121
    // rather than 1:1, which is a picture very slightly the wrong shape in
    // every player that honours it.
    stages.push("setsar=1".into());
    stages.push("format=yuv420p".into());

    stages.join(",")
}

/// The frame sizes an aspect button offers, named by the short side.
const FRAME_TIERS: [u32; 6] = [2160, 1440, 1080, 720, 480, 360];

/// The exported frame for a ratio: whole numbers, both even, and exactly the
/// ratio the button names.
///
/// The ratio is doubled first when either side is odd, so that every multiple
/// of it is a frame libx264 will accept. The largest tier that fits inside the
/// source frame wins, because a crop cannot invent detail the source never had;
/// when even the smallest tier is bigger than the source, the source itself
/// sets the size.
fn canvas_frame(
    aspect: AspectRatio,
    source: Option<(u32, u32)>,
    max_height: Option<u32>,
) -> Option<(u32, u32)> {
    let (ratio_w, ratio_h) = aspect.parts()?;
    let (unit_w, unit_h) = if ratio_w % 2 == 1 || ratio_h % 2 == 1 {
        (ratio_w * 2, ratio_h * 2)
    } else {
        (ratio_w, ratio_h)
    };
    let short = unit_w.min(unit_h).max(1);

    let limits = [
        source.filter(|(w, h)| *w > 0 && *h > 0).map(|(w, h)| (w / unit_w).min(h / unit_h)),
        max_height.map(|height| height / unit_h),
    ];
    let limit = limits.into_iter().flatten().min();

    let tier = FRAME_TIERS
        .iter()
        .map(|tier| ((*tier as f64) / (short as f64)).round() as u32)
        .find(|step| *step > 0 && limit.is_none_or(|ceiling| *step <= ceiling));

    let step = tier.or(limit).unwrap_or(1).max(1);
    Some((unit_w * step, unit_h * step))
}

/// Constant-quality value for a codec family. The scales are not shared: 23 is
/// an ordinary H.264 file and a very good VP9 one.
fn quality_value(codec: VideoCodec, quality: ExportQuality) -> u32 {
    match codec {
        VideoCodec::H264 => match quality {
            ExportQuality::Maximum => 16,
            ExportQuality::High => 19,
            ExportQuality::Balanced => 23,
            ExportQuality::Small => 28,
        },
        VideoCodec::H265 => match quality {
            ExportQuality::Maximum => 18,
            ExportQuality::High => 22,
            ExportQuality::Balanced => 26,
            ExportQuality::Small => 31,
        },
        VideoCodec::Vp9 => match quality {
            ExportQuality::Maximum => 24,
            ExportQuality::High => 28,
            ExportQuality::Balanced => 33,
            ExportQuality::Small => 38,
        },
        VideoCodec::Av1 => match quality {
            ExportQuality::Maximum => 22,
            ExportQuality::High => 27,
            ExportQuality::Balanced => 32,
            ExportQuality::Small => 40,
        },
    }
}

/// How each encoder family is asked for a constant quality.
///
/// These cannot be shared. `-crf` on an NVENC encoder is ignored rather than
/// refused, which is the worst of both: the run succeeds and the setting does
/// nothing, and the file comes out at whatever bitrate the default was.
fn quality_args(encoder: &str, value: u32) -> Vec<String> {
    if encoder.ends_with("_nvenc") {
        vec![
            "-preset".into(),
            "p5".into(),
            "-tune".into(),
            "hq".into(),
            "-rc".into(),
            "vbr".into(),
            "-cq".into(),
            value.to_string(),
            // Without this the constant-quality target is still capped by a
            // default bitrate, and a detailed frame is starved to stay under it.
            "-b:v".into(),
            "0".into(),
        ]
    } else if encoder.ends_with("_qsv") {
        vec!["-global_quality".into(), value.to_string()]
    } else if encoder.ends_with("_amf") {
        // Untested here: this machine advertises AMF and has no runtime for it,
        // so every AMF export falls through to the CPU encoder before reaching
        // a frame. These are what the encoder documents for constant quality.
        vec![
            "-rc".into(),
            "cqp".into(),
            "-qp_i".into(),
            value.to_string(),
            "-qp_p".into(),
            value.to_string(),
        ]
    } else if encoder == "libvpx-vp9" {
        // VP9 reads `-crf` as a ceiling unless the bitrate target is cleared,
        // and then produces a file far below the quality asked for.
        vec![
            "-crf".into(),
            value.to_string(),
            "-b:v".into(),
            "0".into(),
        ]
    } else if encoder == "libaom-av1" {
        // The same rule as libvpx: older builds read `-crf` beside a bitrate
        // target as a ceiling, and a cleared target is what makes it a
        // constant quality on all of them.
        let mut args = vec!["-crf".into(), value.to_string(), "-b:v".into(), "0".into()];
        args.extend(speed_args(encoder));
        args
    } else if encoder == "librav1e" {
        // rav1e has no `-crf`, only the quantiser itself, and that runs to 255
        // rather than to 63.
        let mut args = vec!["-qp".into(), rav1e_quantizer(value).to_string()];
        args.extend(speed_args(encoder));
        args
    } else {
        vec!["-crf".into(), value.to_string()]
    }
}

/// An AV1 quality value from the 0-63 scale SVT-AV1 and libaom share, carried
/// onto rav1e's 0-255 one.
///
/// Four to one is not an approximation: libaom turns its 0-63 into the codec's
/// own quantiser index by the same table, which is four steps per value and
/// ends on 255, and rav1e's scale is that index directly.
fn rav1e_quantizer(value: u32) -> u32 {
    value.saturating_mul(4).min(255)
}

/// What an encoder is told about speed, for the two whose defaults suit an
/// archive better than somebody waiting on a phone.
///
/// Both are the processor AV1 an export falls back to where SVT-AV1 will not
/// open, which is most likely on a phone, and both spend most of their time
/// idle on the processor's cores unless told otherwise. Measured on two seconds
/// of 720p: libaom at its own default took 113 s, and 1.9 s at speed 6 with
/// `-row-mt` spreading each frame over the cores -- 6 being the fastest its
/// ordinary mode accepts on every version still in use. rav1e works in parallel
/// across tiles, and asking for more of them is what made the difference:
/// 10.6 s at its defaults, 9.3 s at speed 8, and 3.6 s at speed 8 in eight
/// tiles -- a number it lowers by itself on a frame too small to hold that
/// many. Every other encoder keeps its own default, which is already what it is
/// normally asked for.
///
/// Shared with [`crate::tools::encoder_runs`], so that an encoder is only ever
/// chosen having been run with the settings it will be given.
pub(crate) fn speed_args(encoder: &str) -> Vec<String> {
    match encoder {
        "libaom-av1" => vec!["-cpu-used".into(), "6".into(), "-row-mt".into(), "1".into()],
        "librav1e" => vec!["-speed".into(), "8".into(), "-tiles".into(), "8".into()],
        _ => Vec::new(),
    }
}

/// The frame rate SVT-AV1 is to plan its rate and keyframes around, said
/// outright.
///
/// It will not open above 240 frames a second, and the FFmpeg a phone runs
/// (7.1) hands it no rate at all out of `concat`, which every re-encode passes
/// through: the encoder then read one off the graph's time base, a million a
/// second, and the export failed before its first frame -- every AV1 export on
/// a phone. The desktop's newer FFmpeg carries the rate through the join, and
/// there this only repeats it. The frames keep their own times either way;
/// only the encoder's arithmetic reads the number, so a source whose rate the
/// probe could not read is planned as the common 30.
fn svt_frame_rate_args(fps: Option<f64>) -> Vec<String> {
    let fps = fps
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= 240.0)
        .unwrap_or(30.0);
    let millis = (fps * 1000.0).round() as u64;
    vec!["-svtav1-params".into(), format!("fps-num={millis}:fps-denom=1000")]
}

/// How each encoder family is asked for an average bitrate instead of a
/// quality.
///
/// A variable rate throughout, never a constant one: the average is the number
/// the user chose, and a peak half as high again lets a busy scene borrow from
/// a still one without the file drifting from the size that number implies. The
/// buffer is two seconds' worth at the average, which is what the peak is
/// policed over.
///
/// None of these may carry a constant-quality flag from [`quality_args`]. On
/// libvpx and libaom a leftover `-crf` turns the average into a ceiling,
/// SVT-AV1 only switches to its bitrate mode when no `-crf` is given, rav1e
/// holds a leftover `-qp` over any rate, and on the hardware encoders a
/// leftover `-cq` or `-qp_*` wins over the bitrate outright.
///
/// Every codec keeps a way to do this on the processor, as it does for
/// quality: the retry that follows a failed hardware encode rebuilds its
/// arguments through here, so a file that asked for a size gets that size from
/// whichever encoder finishes it.
fn bitrate_args(encoder: &str, kbps: u32) -> Vec<String> {
    // Worked out wide so that no rate the checks let through can overflow.
    let average = format!("{kbps}k");
    let peak = format!("{}k", u64::from(kbps) * 3 / 2);
    let buffer = format!("{}k", u64::from(kbps) * 2);

    if encoder.ends_with("_nvenc") {
        // The preset and tune are the same as for quality; only the rate
        // control changes, and `-b:v` is now the target rather than `0`.
        vec![
            "-preset".into(),
            "p5".into(),
            "-tune".into(),
            "hq".into(),
            "-rc".into(),
            "vbr".into(),
            "-b:v".into(),
            average,
            "-maxrate".into(),
            peak,
            "-bufsize".into(),
            buffer,
        ]
    } else if encoder.ends_with("_qsv") {
        // A peak above the average is what makes QSV choose its variable mode;
        // equal, it would be a constant rate.
        vec!["-b:v".into(), average, "-maxrate".into(), peak]
    } else if encoder.ends_with("_amf") {
        // Untested for the same reason as its quality arguments.
        vec![
            "-rc".into(),
            "vbr_peak".into(),
            "-b:v".into(),
            average,
            "-maxrate".into(),
            peak,
        ]
    } else if encoder == "libx264" || encoder == "libx265" {
        // x264 and x265 ignore a peak that has no buffer to measure it over.
        vec![
            "-b:v".into(),
            average,
            "-maxrate".into(),
            peak,
            "-bufsize".into(),
            buffer,
        ]
    } else if encoder == "libsvtav1" {
        // A bare `-b:v` is SVT-AV1's variable rate, as it is libvpx's below.
        // Unlike the others it also has a ceiling, and past it the encoder
        // will not open at all -- measured: "the target bit rate must be
        // between [0, 100000] kbps", exit 127, nothing written. It is the
        // processor's AV1, with nothing after it to fall back to, so a rate
        // above the ceiling is handed the ceiling.
        vec!["-b:v".into(), format!("{}k", kbps.min(SVT_AV1_MAX_KBPS))]
    } else if encoder == "libaom-av1" || encoder == "librav1e" {
        // A bare `-b:v` is the variable rate of both, as it is libvpx's. The
        // speed they are given for a quality comes along: a size is a
        // different target, not a reason to run slower.
        let mut args = vec!["-b:v".into(), average];
        args.extend(speed_args(encoder));
        args
    } else {
        // libvpx-vp9 reads a bare `-b:v` as a variable rate, and so does a
        // phone's MediaCodec encoder. It is also the one spelling every FFmpeg
        // encoder understands, which makes it the right answer for an encoder
        // this list has not met.
        vec!["-b:v".into(), average]
    }
}

/// The most SVT-AV1 will open with, in kilobits a second. The hardware AV1
/// encoders take the full range the export allows.
const SVT_AV1_MAX_KBPS: u32 = 100_000;

fn audio_encoder(codec: AudioCodec) -> &'static str {
    match codec {
        AudioCodec::Aac => "aac",
        AudioCodec::Opus => "libopus",
        AudioCodec::Mp3 => "libmp3lame",
        AudioCodec::Flac => "flac",
    }
}

fn video_codec_label(codec: VideoCodec) -> &'static str {
    match codec {
        VideoCodec::H264 => "H.264",
        VideoCodec::H265 => "HEVC",
        VideoCodec::Vp9 => "VP9",
        VideoCodec::Av1 => "AV1",
    }
}

fn audio_codec_label(codec: AudioCodec) -> &'static str {
    match codec {
        AudioCodec::Aac => "AAC",
        AudioCodec::Opus => "Opus",
        AudioCodec::Mp3 => "MP3",
        AudioCodec::Flac => "FLAC",
    }
}

/// Refuse a pairing the container cannot hold, here, in a sentence.
///
/// FFmpeg would refuse it too, eventually, several minutes into an encode and
/// in its own words. Saying so before the first frame is the difference
/// between a mistake and a wasted afternoon.
fn container_accepts(
    container: &str,
    video: VideoCodec,
    audio: Option<AudioCodec>,
) -> AppResult<()> {
    let video_ok = match container {
        "webm" => matches!(video, VideoCodec::Vp9 | VideoCodec::Av1),
        "mov" => matches!(video, VideoCodec::H264 | VideoCodec::H265),
        "mp4" | "m4v" => matches!(video, VideoCodec::H264 | VideoCodec::H265 | VideoCodec::Av1),
        // Matroska was designed to hold anything, and does.
        _ => true,
    };
    if !video_ok {
        return Err(AppError::Other(format!(
            "a .{container} file cannot hold {} video",
            video_codec_label(video)
        )));
    }

    let Some(audio) = audio else {
        return Ok(());
    };
    let audio_ok = match container {
        "webm" => matches!(audio, AudioCodec::Opus),
        "mov" => matches!(audio, AudioCodec::Aac),
        "mp4" | "m4v" => matches!(
            audio,
            AudioCodec::Aac | AudioCodec::Opus | AudioCodec::Mp3
        ),
        _ => true,
    };
    if !audio_ok {
        return Err(AppError::Other(format!(
            "a .{container} file cannot hold {} audio",
            audio_codec_label(audio)
        )));
    }
    Ok(())
}

/// Run FFmpeg with the given arguments, reporting progress as it works.
///
/// Shared with the standalone converter, which builds different arguments but
/// needs exactly this: a child process that reports where it has got to and
/// stops promptly when the task is cancelled.
pub(crate) async fn run_with_progress(
    args: &[String],
    duration_sec: Option<f64>,
    control: Arc<TaskControl>,
    on_progress: &mut (dyn FnMut(FfmpegProgress) + Send),
) -> AppResult<()> {
    run_pass(args, duration_sec, PassSlice::whole(), control, on_progress).await
}

/// One FFmpeg process, reporting its share of a job that may be several.
///
/// Nothing here says the run has finished. A job of five processes would
/// otherwise announce completion five times, and the last of those would be a
/// lie about the four still to come; whoever owns the job publishes the final
/// figure once, when there is actually nothing left to do.
pub(crate) async fn run_pass(
    args: &[String],
    duration_sec: Option<f64>,
    slice: PassSlice,
    control: Arc<TaskControl>,
    on_progress: &mut (dyn FnMut(FfmpegProgress) + Send),
) -> AppResult<()> {
    let binary = tools::require_ffmpeg()?;

    // `-progress` writes machine-readable key=value lines, which is far
    // steadier to parse than the status line FFmpeg normally prints. It goes to
    // stderr rather than stdout so that stdout stays the child's own: a pass
    // that writes raw samples to a pipe instead of to a file would otherwise
    // find progress text spliced into the middle of them.
    let mut full_args: Vec<String> = vec!["-progress".into(), "pipe:2".into()];
    full_args.extend_from_slice(args);

    let mut child = process::command(&binary)
        .args(&full_args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| AppError::Other(format!("FFmpeg could not be started: {err}")))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Other("FFmpeg produced no output stream".into()))?;

    let mut reader = BufReader::new(stderr).lines();
    let mut trouble = String::new();
    let mut bytes_written = 0u64;
    let mut last_emit = Instant::now();
    let mut poll = tokio::time::interval(Duration::from_millis(250));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let interrupted = loop {
        tokio::select! {
            line = reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let Some((key, value)) = progress_field(&line) else {
                            // Everything that is not a progress field is what
                            // FFmpeg has to say about why this will not work,
                            // and it is the only account of the failure there
                            // is. The cap is there so a file that produces a
                            // warning per frame cannot grow without bound.
                            // SVT-AV1 opens with a banner it prints whatever
                            // the log level, and the first line of this is
                            // what a failure is reported as: it said
                            // "Svt[info]: -----" instead of the reason.
                            if !line.trim().is_empty()
                                && !line.starts_with("Svt[info]")
                                && trouble.len() < 4096
                            {
                                trouble.push_str(line.trim());
                                trouble.push('\n');
                            }
                            continue;
                        };
                        match key {
                            "total_size" => {
                                bytes_written = value.parse().unwrap_or(bytes_written);
                            }
                            "out_time_us" | "out_time_ms" => {
                                let Some(micros) = progress_micros(value) else {
                                    continue;
                                };
                                let local = duration_sec
                                    .filter(|d| *d > 0.0)
                                    .map(|d| (micros / 1_000_000.0 / d * 100.0).clamp(0.0, 100.0));
                                if last_emit.elapsed() >= Duration::from_millis(300) {
                                    on_progress(FfmpegProgress {
                                        percent: slice.place(local),
                                        bytes_written,
                                    });
                                    last_emit = Instant::now();
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(None) => break false,
                    Err(_) => break false,
                }
            }
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
        return Err(AppError::Canceled);
    }

    let status = child
        .wait()
        .await
        .map_err(|err| AppError::Other(format!("FFmpeg did not exit cleanly: {err}")))?;

    if !status.success() {
        log_debug!("ffmpeg", "failed: {}", trouble.trim());
        return Err(AppError::Other(format!(
            "FFmpeg exited with {}: {}",
            status.code().unwrap_or(-1),
            trouble.lines().next().unwrap_or("no detail").trim()
        )));
    }

    Ok(())
}

/// Split one line of stderr into a progress field, or say it is not one.
///
/// Progress and diagnostics share the pipe now, so the two have to be told
/// apart by shape. A progress line is one known key, no spaces, an equals sign
/// and a value; FFmpeg's own messages begin with a bracketed source and are
/// never mistaken for one.
fn progress_field(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let known = matches!(
        key,
        "frame"
            | "fps"
            | "bitrate"
            | "total_size"
            | "out_time_us"
            | "out_time_ms"
            | "out_time"
            | "dup_frames"
            | "drop_frames"
            | "speed"
            | "progress"
    ) || key.starts_with("stream_");
    known.then(|| (key, value.trim()))
}

/// How far into the output FFmpeg has got, in microseconds despite what the
/// `out_time_ms` key is called.
///
/// `None` for the `N/A` an encoder with a lookahead reports while it drains --
/// VP9 and AV1 both do it, in the last second of a run. It means there is no
/// new timestamp yet, not that the file has gone back to the start, and read as
/// zero it drops the bar from ninety-odd to nothing and leaves it there, since
/// the pass emits nothing after it.
fn progress_micros(value: &str) -> Option<f64> {
    value.parse::<f64>().ok().filter(|micros| micros.is_finite())
}

/// Scratch path for an intermediate stream, kept out of the user's folder.
pub fn intermediate_path(dir: &Path, task_id: &str, role: &str, extension: &str) -> PathBuf {
    dir.join(format!("{task_id}.{role}.{extension}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_for(target: &str, copy_only: bool, hw: bool) -> Vec<String> {
        conversion_args(
            Path::new("in.mkv"),
            Path::new(&format!("out.{target}")),
            target,
            Some(192.0),
            copy_only,
            hw,
        )
    }

    #[test]
    fn a_copy_pass_never_names_an_encoder() {
        let args = args_for("mp3", true, false);
        assert!(args.windows(2).any(|w| w[0] == "-c" && w[1] == "copy"));
        assert!(!args.iter().any(|a| a == "libmp3lame"));
    }

    #[test]
    fn audio_targets_drop_the_video_stream() {
        for target in ["mp3", "m4a", "wav", "opus", "flac"] {
            assert!(args_for(target, false, false).iter().any(|a| a == "-vn"), "{target}");
        }
    }

    #[test]
    fn video_targets_keep_the_video_stream() {
        assert!(!args_for("mp4", false, false).iter().any(|a| a == "-vn"));
        assert!(!args_for("webm", false, false).iter().any(|a| a == "-vn"));
    }

    #[test]
    fn each_audio_container_selects_its_encoder() {
        let codec_for = |target: &str| {
            let args = args_for(target, false, false);
            let index = args.iter().position(|a| a == "-c:a").unwrap();
            args[index + 1].clone()
        };
        assert_eq!(codec_for("mp3"), "libmp3lame");
        assert_eq!(codec_for("wav"), "pcm_s16le");
        assert_eq!(codec_for("opus"), "libopus");
        assert_eq!(codec_for("flac"), "flac");
        assert_eq!(codec_for("m4a"), "aac");
    }

    #[test]
    fn the_audio_bitrate_never_exceeds_the_source() {
        let args = conversion_args(
            Path::new("in.m4a"),
            Path::new("out.mp3"),
            "mp3",
            Some(128.0),
            false,
            false,
        );
        let index = args.iter().position(|a| a == "-b:a").unwrap();
        assert_eq!(args[index + 1], "128k");
    }

    #[test]
    fn hardware_acceleration_switches_the_video_encoder() {
        let index_of = |args: &[String], key: &str| {
            args.iter().position(|a| a == key).map(|i| args[i + 1].clone())
        };
        assert_eq!(index_of(&args_for("mp4", false, true), "-c:v").as_deref(), Some("h264_nvenc"));
        assert_eq!(index_of(&args_for("mp4", false, false), "-c:v").as_deref(), Some("libx264"));
    }

    #[test]
    fn mp4_output_is_made_seekable() {
        assert!(args_for("mp4", true, false).iter().any(|a| a == "+faststart"));
        assert!(!args_for("webm", true, false).iter().any(|a| a == "+faststart"));
    }

    fn merge_for(target: &str, vcodec: &str, acodec: &str) -> Vec<String> {
        let output = format!("out.{target}");
        merge_args(
            Path::new("v.tmp"),
            Path::new("a.tmp"),
            Path::new(&output),
            target,
            plan::container_holds_video(target, vcodec),
            plan::container_holds_audio(target, acodec),
        )
    }

    fn codec_after(args: &[String], key: &str) -> String {
        let index = args.iter().position(|a| a == key).expect(key);
        args[index + 1].clone()
    }

    #[test]
    fn a_merge_the_container_accepts_copies_both_streams() {
        let args = merge_for("webm", "vp9", "opus");
        assert_eq!(codec_after(&args, "-c:v"), "copy");
        assert_eq!(codec_after(&args, "-c:a"), "copy");

        let args = merge_for("mp4", "avc1.640028", "mp4a.40.2");
        assert_eq!(codec_after(&args, "-c:v"), "copy");
        assert_eq!(codec_after(&args, "-c:a"), "copy");
    }

    #[test]
    fn only_the_stream_the_container_rejects_is_re_encoded() {
        // WebM has no place for AAC, but the VP9 video is still copied.
        let args = merge_for("webm", "vp9", "mp4a.40.2");
        assert_eq!(codec_after(&args, "-c:v"), "copy");
        assert_eq!(codec_after(&args, "-c:a"), "libopus");

        // MP4 has no place for VP9, and Opus is re-encoded to AAC with it.
        let args = merge_for("mp4", "vp9", "opus");
        assert_eq!(codec_after(&args, "-c:v"), "libx264");
        assert_eq!(codec_after(&args, "-c:a"), "aac");
    }

    #[test]
    fn matroska_never_forces_a_re_encode() {
        let args = merge_for("mkv", "vp9", "mp4a.40.2");
        assert_eq!(codec_after(&args, "-c:v"), "copy");
        assert_eq!(codec_after(&args, "-c:a"), "copy");
    }

    #[test]
    fn a_merged_mp4_is_made_seekable_and_other_containers_are_not() {
        assert!(merge_for("mp4", "avc1", "mp4a").iter().any(|a| a == "+faststart"));
        assert!(!merge_for("webm", "vp9", "opus").iter().any(|a| a == "+faststart"));
    }

    #[test]
    fn a_merge_takes_one_stream_from_each_input() {
        let args = merge_for("mkv", "vp9", "opus");
        assert!(args.windows(2).any(|w| w[0] == "-map" && w[1] == "0:v:0"));
        assert!(args.windows(2).any(|w| w[0] == "-map" && w[1] == "1:a:0"));
        assert_eq!(args.last().unwrap(), "out.mkv");
    }

    #[test]
    fn the_output_path_is_always_the_final_argument() {
        let args = args_for("mp4", true, false);
        assert_eq!(args.last().unwrap(), "out.mp4");
    }

    #[test]
    fn a_picture_is_encoded_into_the_format_asked_for_never_copied() {
        let encoder_for = |target: &str| {
            let output = format!("out.{target}");
            let args = image_conversion_args(Path::new("in.jpg"), Path::new(&output), target);
            assert!(!args.iter().any(|a| a == "copy"), "{target}");
            assert!(args.windows(2).any(|w| w[0] == "-frames:v" && w[1] == "1"), "{target}");
            assert_eq!(args.last().unwrap(), &output);
            codec_after(&args, "-c:v")
        };
        assert_eq!(encoder_for("png"), "png");
        assert_eq!(encoder_for("jpg"), "mjpeg");
        assert_eq!(encoder_for("webp"), "libwebp");
    }

    #[test]
    fn only_still_image_formats_take_the_picture_path() {
        for target in ["jpg", "jpeg", "png", "webp"] {
            assert!(is_image_target(target), "{target}");
        }
        for target in ["mp4", "webm", "mp3", "gif"] {
            assert!(!is_image_target(target), "{target}");
        }
    }

    // -- the editor's export -----------------------------------------------

    fn segment(start: f64, end: f64) -> EditSegment {
        EditSegment {
            start_sec: start,
            end_sec: end,
        }
    }

    fn export_plan<'a>(
        options: &'a ExportOptions,
        segments: &'a [EditSegment],
        source: Option<(u32, u32)>,
        has_audio: bool,
        encoder: &'a str,
    ) -> ExportPlan<'a> {
        ExportPlan {
            input: Path::new("in.mp4"),
            output: Path::new("out.mp4"),
            segments,
            options,
            source_frame: source,
            has_audio,
            source_fps: Some(29.97),
            video_encoder: encoder,
            tone_map: ToneMap::None,
        }
    }

    fn graph_of(args: &[String]) -> String {
        value_after(args, "-filter_complex").expect("a filter graph")
    }

    fn value_after(args: &[String], key: &str) -> Option<String> {
        args.iter()
            .position(|arg| arg == key)
            .map(|index| args[index + 1].clone())
    }

    #[test]
    fn one_kept_range_is_copied_and_seeks_before_it_reads() {
        let args = lossless_single_args(
            Path::new("in.mp4"),
            Path::new("out.mp4"),
            "mp4",
            4.8,
            8.0,
            true,
        );
        let ss = args.iter().position(|a| a == "-ss").expect("-ss");
        let input = args.iter().position(|a| a == "-i").expect("-i");
        assert!(ss < input, "-ss has to come before -i or the seek is wasted");
        assert_eq!(value_after(&args, "-ss").as_deref(), Some("4.800000"));
        assert_eq!(value_after(&args, "-to").as_deref(), Some("8.000000"));
        assert_eq!(value_after(&args, "-c").as_deref(), Some("copy"));
        assert_eq!(value_after(&args, "-map_metadata").as_deref(), Some("0"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-avoid_negative_ts" && w[1] == "make_zero"));
        assert!(args.iter().any(|a| a == "+faststart"));
        assert_eq!(args.last().unwrap(), "out.mp4");
    }

    #[test]
    fn a_seek_keeps_every_decimal_the_keyframe_had() {
        // Keyframe times on an NTSC-rate file are 1.668333, 0.667333 and the
        // like. Written as "1.668" the seek sits below the keyframe, and a
        // backward seek that lands below a keyframe resolves to the one before
        // it -- measured on a 29.97 fps file, 2.202 s of marked film arriving
        // as 2.535 s, opening a third of a second before the mark.
        let args = lossless_single_args(
            Path::new("in.mp4"),
            Path::new("out.mp4"),
            "mp4",
            1.668333,
            3.668333,
            true,
        );
        assert_eq!(value_after(&args, "-ss").as_deref(), Some("1.668333"));
        assert_eq!(value_after(&args, "-to").as_deref(), Some("3.668333"));

        let args = lossless_cut_args(
            Path::new("in.mp4"),
            Path::new("job.seg0.ts"),
            "mpegts",
            8.483333,
            16.666667,
            Some("h264"),
            true,
        );
        assert_eq!(value_after(&args, "-ss").as_deref(), Some("8.483333"));
        assert_eq!(value_after(&args, "-to").as_deref(), Some("16.666667"));
    }

    #[test]
    fn a_copied_range_carries_no_encoder_and_no_filter() {
        let args = lossless_single_args(
            Path::new("in.mkv"),
            Path::new("out.mkv"),
            "mkv",
            0.0,
            1.0,
            true,
        );
        assert!(!args.iter().any(|a| a == "-filter_complex"));
        assert!(!args.iter().any(|a| a.starts_with("libx")));
        assert!(!args.iter().any(|a| a == "+faststart"));
    }

    #[test]
    fn a_cut_pass_writes_transport_stream_only_for_a_codec_that_has_one() {
        assert_eq!(segment_format(Some("h264"), Some("aac")), ("mpegts", "ts"));
        assert_eq!(
            segment_format(Some("avc1.640028"), Some("mp4a.40.2")),
            ("mpegts", "ts")
        );
        assert_eq!(segment_format(Some("hevc"), Some("ac3")), ("mpegts", "ts"));
        assert_eq!(segment_format(Some("vp9"), Some("opus")), ("matroska", "mkv"));
        assert_eq!(segment_format(Some("av1"), Some("opus")), ("matroska", "mkv"));
        assert_eq!(segment_format(None, None), ("matroska", "mkv"));
        // A silent file has nothing to lose either way.
        assert_eq!(segment_format(Some("h264"), None), ("mpegts", "ts"));
    }

    #[test]
    fn a_cut_pass_leaves_transport_stream_for_audio_it_cannot_hold() {
        // MPEG-TS does not refuse these, it writes them as private data and
        // exits zero, and the join afterwards finds no audio at all.
        for codec in ["flac", "vorbis", "pcm_s16le", "alac"] {
            assert_eq!(
                segment_format(Some("h264"), Some(codec)),
                ("matroska", "mkv"),
                "{codec} cannot go through a transport stream"
            );
        }
        // A codec the probe could not name is treated the same way: the slower
        // muxer costs a little, a silent export costs the whole file.
        assert_eq!(segment_format(Some("h264"), Some("")), ("matroska", "mkv"));
        // The ordinary case is unchanged.
        assert_eq!(segment_format(Some("h264"), Some("eac3")), ("mpegts", "ts"));
    }

    #[test]
    fn a_cut_pass_names_the_bitstream_filter_its_codec_needs() {
        let args = lossless_cut_args(
            Path::new("in.mp4"),
            Path::new("job.seg0.ts"),
            "mpegts",
            1.6,
            3.2,
            Some("h264"),
            true,
        );
        assert_eq!(value_after(&args, "-bsf:v").as_deref(), Some("h264_mp4toannexb"));
        assert_eq!(value_after(&args, "-f").as_deref(), Some("mpegts"));
        assert_eq!(value_after(&args, "-c").as_deref(), Some("copy"));

        let args = lossless_cut_args(
            Path::new("in.mp4"),
            Path::new("job.seg0.ts"),
            "mpegts",
            1.6,
            3.2,
            Some("hevc"),
            true,
        );
        assert_eq!(value_after(&args, "-bsf:v").as_deref(), Some("hevc_mp4toannexb"));

        let args = lossless_cut_args(
            Path::new("in.webm"),
            Path::new("job.seg0.mkv"),
            "matroska",
            1.6,
            3.2,
            Some("vp9"),
            true,
        );
        assert!(!args.iter().any(|a| a == "-bsf:v"));
        assert_eq!(value_after(&args, "-f").as_deref(), Some("matroska"));
    }

    #[test]
    fn the_join_lays_whole_files_end_to_end_rather_than_seeking_inside_them() {
        let args = concat_args(Path::new("job.list.txt"), Path::new("out.mp4"), "mp4", true);
        assert_eq!(value_after(&args, "-f").as_deref(), Some("concat"));
        assert_eq!(value_after(&args, "-safe").as_deref(), Some("0"));
        assert_eq!(value_after(&args, "-c").as_deref(), Some("copy"));
        // The demuxer's own seeking is what crushes 45 frames into 3 ms at
        // every join, so it must never appear here.
        assert!(!args.iter().any(|a| a == "-inpoint" || a == "-outpoint"));
        assert!(args.iter().any(|a| a == "+faststart"));
    }

    /// Every map a copy makes, in order.
    fn maps_of(args: &[String]) -> Vec<String> {
        args.windows(2)
            .filter(|w| w[0] == "-map")
            .map(|w| w[1].clone())
            .collect()
    }

    #[test]
    fn a_copy_can_leave_the_sound_behind_without_decoding_anything() {
        let every_pass = |keep_audio: bool| {
            [
                lossless_single_args(
                    Path::new("in.mp4"),
                    Path::new("out.mp4"),
                    "mp4",
                    1.6,
                    3.2,
                    keep_audio,
                ),
                lossless_cut_args(
                    Path::new("in.mp4"),
                    Path::new("job.seg0.ts"),
                    "mpegts",
                    1.6,
                    3.2,
                    Some("h264"),
                    keep_audio,
                ),
                concat_args(
                    Path::new("job.list.txt"),
                    Path::new("out.mp4"),
                    "mp4",
                    keep_audio,
                ),
            ]
        };

        for args in every_pass(false) {
            assert_eq!(maps_of(&args), ["0:v:0?"], "{args:?}");
            assert!(args.iter().any(|a| a == "-an"), "{args:?}");
            // Still a copy: dropping a track is not a reason to encode one.
            assert_eq!(value_after(&args, "-c").as_deref(), Some("copy"));
            assert!(!args
                .iter()
                .any(|a| a == "-c:v" || a == "-c:a" || a == "-filter_complex"));
        }
        for args in every_pass(true) {
            assert_eq!(maps_of(&args), ["0:v:0?", "0:a?"], "{args:?}");
            assert!(!args.iter().any(|a| a == "-an"), "{args:?}");
        }
    }

    #[test]
    fn a_list_file_uses_forward_slashes_and_escapes_an_apostrophe() {
        let text = concat_list_text(&[
            PathBuf::from(r"C:\Users\mel\temp\job.seg0.ts"),
            PathBuf::from(r"C:\Users\mel\it's here\job.seg1.ts"),
        ]);
        assert_eq!(
            text,
            "file 'C:/Users/mel/temp/job.seg0.ts'\nfile 'C:/Users/mel/it'\\''s here/job.seg1.ts'\n"
        );
    }

    #[test]
    fn every_re_encode_squares_the_pixels_before_anything_else() {
        let mut options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            ..ExportOptions::default()
        };
        let segments = [segment(1.0, 3.0)];

        for aspect in [
            AspectRatio::Source,
            AspectRatio::Widescreen,
            AspectRatio::Square,
        ] {
            options.aspect = aspect;
            let plan = export_plan(&options, &segments, Some((1920, 1080)), true, "libx264");
            let graph = graph_of(&reencode_args(&plan).unwrap());
            assert!(
                graph.contains("scale=w='trunc(iw*max(1,sar)/2)*2':h='trunc(ih/min(1,sar)/2)*2'"),
                "{aspect:?}: {graph}"
            );
            assert!(graph.ends_with("setsar=1,format=yuv420p[vout]"), "{graph}");
        }
    }

    #[test]
    fn an_aspect_button_crops_to_fill_and_pads_to_fit() {
        let segments = [segment(0.0, 2.0)];
        let mut options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            aspect: AspectRatio::Widescreen,
            ..ExportOptions::default()
        };

        let plan = export_plan(&options, &segments, Some((1920, 1080)), true, "libx264");
        let graph = graph_of(&reencode_args(&plan).unwrap());
        assert!(graph.contains(
            "scale=1920:1080:force_original_aspect_ratio=increase:force_divisible_by=2,crop=1920:1080"
        ));

        options.fit = FrameFit::Fit;
        let plan = export_plan(&options, &segments, Some((1920, 1080)), true, "libx264");
        let graph = graph_of(&reencode_args(&plan).unwrap());
        assert!(graph.contains(
            "scale=1920:1080:force_original_aspect_ratio=decrease:force_divisible_by=2,pad=1920:1080:(ow-iw)/2:(oh-ih)/2:color=black"
        ));
    }

    #[test]
    fn a_frame_is_exactly_the_ratio_the_button_names() {
        let exact = |aspect: AspectRatio, source: (u32, u32)| {
            let (width, height) = canvas_frame(aspect, Some(source), None).expect("a canvas");
            let (rw, rh) = aspect.parts().unwrap();
            assert_eq!(
                width * rh,
                height * rw,
                "{aspect:?} from {source:?} gave {width}x{height}"
            );
            assert_eq!(width % 2, 0, "{width} is odd");
            assert_eq!(height % 2, 0, "{height} is odd");
            (width, height)
        };

        assert_eq!(exact(AspectRatio::Widescreen, (1920, 1080)), (1920, 1080));
        assert_eq!(exact(AspectRatio::Classic, (1920, 1080)), (1440, 1080));
        assert_eq!(exact(AspectRatio::Tall, (1920, 1080)), (1728, 1080));
        assert_eq!(exact(AspectRatio::Square, (1920, 1080)), (1080, 1080));
        assert_eq!(exact(AspectRatio::Portrait, (1080, 1920)), (1080, 1920));
        assert_eq!(canvas_frame(AspectRatio::Source, Some((1920, 1080)), None), None);
    }

    #[test]
    fn a_tier_never_reaches_past_the_source_or_the_cap() {
        // 720p is the largest whole 16:9 tier a 1280x720 source can fill.
        assert_eq!(
            canvas_frame(AspectRatio::Widescreen, Some((1280, 720)), None),
            Some((1280, 720))
        );
        // A cap in height is a cap, never an invitation to enlarge.
        assert_eq!(
            canvas_frame(AspectRatio::Widescreen, Some((1920, 1080)), Some(720)),
            Some((1280, 720))
        );
        // Smaller than every tier: the source itself sets the frame, and it is
        // still exactly the ratio.
        let (width, height) =
            canvas_frame(AspectRatio::Widescreen, Some((320, 180)), None).unwrap();
        assert_eq!(width * 9, height * 16);
        assert!(width <= 320 && height <= 180, "{width}x{height}");
    }

    #[test]
    fn an_anamorphic_source_is_measured_by_the_picture_and_not_by_the_file() {
        // A 1440x1080 master with a 4:3 pixel is already a 16:9 picture, and
        // the graph widens it to 1920x1080 before anything is sized. Measured
        // against the stored 1440 columns instead, the 16:9 button handed back
        // 1280x720 -- half the lines gone for a button that changed nothing
        // about the shape. `source_frame` now arrives already squared.
        assert_eq!(
            canvas_frame(AspectRatio::Widescreen, Some((1920, 1080)), None),
            Some((1920, 1080))
        );
        // The 720x576 PAL case, squared to 1024x576: the largest whole tier
        // that fits is 864x486, not the 640x360 the stored frame allowed.
        assert_eq!(
            canvas_frame(AspectRatio::Widescreen, Some((1024, 576)), None),
            Some((864, 486))
        );
    }

    #[test]
    fn a_silent_source_grows_no_audio_branch_at_all() {
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            ..ExportOptions::default()
        };
        let segments = [segment(1.0, 2.0), segment(4.0, 5.0)];
        let plan = export_plan(&options, &segments, Some((1280, 720)), false, "libx264");
        let args = reencode_args(&plan).unwrap();
        let graph = graph_of(&args);

        assert!(!graph.contains("[0:a]"), "{graph}");
        assert!(!graph.contains("atrim"), "{graph}");
        assert!(graph.contains("concat=n=2:v=1:a=0[vc];"), "{graph}");
        assert!(args.iter().any(|a| a == "-an"));
        assert!(!args.iter().any(|a| a == "-c:a"));
    }

    #[test]
    fn muting_a_source_that_has_audio_drops_it_the_same_way() {
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            mute: true,
            ..ExportOptions::default()
        };
        let segments = [segment(1.0, 2.0)];
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        let args = reencode_args(&plan).unwrap();
        assert!(!graph_of(&args).contains("atrim"));
        assert!(args.iter().any(|a| a == "-an"));
    }

    #[test]
    fn every_kept_range_is_trimmed_and_restamped_before_it_is_joined() {
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            ..ExportOptions::default()
        };
        let segments = [segment(1.0, 3.0), segment(10.0, 12.5)];
        let plan = export_plan(&options, &segments, Some((1920, 1080)), true, "libx264");
        let graph = graph_of(&reencode_args(&plan).unwrap());

        assert!(graph.starts_with(
            "[0:v]trim=start=1.000:end=3.000,setpts=PTS-STARTPTS[v0];\
[0:a]atrim=start=1.000:end=3.000,asetpts=PTS-STARTPTS[a0];\
[0:v]trim=start=10.000:end=12.500,setpts=PTS-STARTPTS[v1];\
[0:a]atrim=start=10.000:end=12.500,asetpts=PTS-STARTPTS[a1];\
[v0][a0][v1][a1]concat=n=2:v=1:a=1[vc][ac];"
        ), "{graph}");
    }

    #[test]
    fn a_range_late_in_a_film_is_seeked_to_and_not_decoded_up_to() {
        // `trim` throws frames away only after the decoder has made them, so
        // without a seek a cut at 1:55 of a two-hour film decodes the whole
        // film first -- measured, 2963 ms against 680 ms for the same five
        // seconds. The graph's times move with the seek, so the output is the
        // same frames it always was.
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            ..ExportOptions::default()
        };
        let segments = [segment(60.0, 65.0), segment(115.0, 120.0)];
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        let args = reencode_args(&plan).unwrap();

        let ss = args.iter().position(|a| a == "-ss").expect("-ss");
        let input = args.iter().position(|a| a == "-i").expect("-i");
        let hwaccel = args.iter().position(|a| a == "-hwaccel").expect("-hwaccel");
        assert!(ss < input, "-ss after -i reads the whole file anyway");
        assert!(hwaccel < input, "a decoder option after -i is ignored");
        assert_eq!(args[ss + 1], "59.000000");

        let graph = graph_of(&args);
        assert!(graph.starts_with("[0:v]trim=start=1.000:end=6.000"), "{graph}");
        assert!(graph.contains("[0:v]trim=start=56.000:end=61.000"), "{graph}");

        // A film cut from its opening has nothing to skip, and an -ss of zero
        // would only cost the seek.
        let segments = [segment(0.4, 3.0)];
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        let args = reencode_args(&plan).unwrap();
        assert!(!args.iter().any(|a| a == "-ss"));
        assert!(graph_of(&args).starts_with("[0:v]trim=start=0.400:end=3.000"));
    }

    #[test]
    fn the_whole_graph_is_one_argument() {
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            ..ExportOptions::default()
        };
        let segments = [segment(1.0, 3.0), segment(4.0, 6.0)];
        let plan = export_plan(&options, &segments, Some((1920, 1080)), true, "libx264");
        let args = reencode_args(&plan).unwrap();

        assert_eq!(args.iter().filter(|a| *a == "-filter_complex").count(), 1);
        assert!(!args.iter().any(|a| a == "-filter_complex_script"));
        assert!(args.iter().filter(|a| a.contains("concat=n=2")).count() == 1);
    }

    #[test]
    fn a_requested_rate_is_forced_and_an_unrequested_one_is_left_alone() {
        let mut options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            ..ExportOptions::default()
        };
        let segments = [segment(0.0, 2.0)];

        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        assert!(!graph_of(&reencode_args(&plan).unwrap()).contains("fps="));

        options.fps = Some(25.0);
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        assert!(graph_of(&reencode_args(&plan).unwrap()).contains(",fps=25,setsar=1,format=yuv420p"));
    }

    #[test]
    fn tone_mapping_states_every_input_side_and_says_so_when_it_cannot() {
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            tone_map_sdr: true,
            ..ExportOptions::default()
        };
        let segments = [segment(0.0, 2.0)];

        let mut plan = export_plan(&options, &segments, Some((1920, 1080)), true, "libx264");
        plan.tone_map = ToneMap::Hable;
        let graph = graph_of(&reencode_args(&plan).unwrap());
        assert!(graph.contains("zscale=tin=smpte2084:t=linear:npl=100"), "{graph}");
        assert!(graph.contains("tonemap=tonemap=hable:desat=0"), "{graph}");

        plan.tone_map = ToneMap::Plain;
        let graph = graph_of(&reencode_args(&plan).unwrap());
        assert!(!graph.contains("tonemap"), "{graph}");
        assert!(graph.contains("format=yuv420p"), "{graph}");
    }

    #[test]
    fn each_encoder_family_is_asked_for_quality_in_its_own_words() {
        assert_eq!(quality_args("libx264", 23), vec!["-crf", "23"]);
        assert_eq!(quality_args("libx265", 26), vec!["-crf", "26"]);
        assert_eq!(quality_args("libsvtav1", 32), vec!["-crf", "32"]);
        assert_eq!(
            quality_args("libvpx-vp9", 33),
            vec!["-crf", "33", "-b:v", "0"]
        );
        assert_eq!(
            quality_args("libaom-av1", 32),
            vec!["-crf", "32", "-b:v", "0", "-cpu-used", "6", "-row-mt", "1"]
        );
        assert_eq!(
            quality_args("librav1e", 32),
            vec!["-qp", "128", "-speed", "8", "-tiles", "8"]
        );
        assert_eq!(
            quality_args("h264_nvenc", 23),
            vec!["-preset", "p5", "-tune", "hq", "-rc", "vbr", "-cq", "23", "-b:v", "0"]
        );
        assert_eq!(quality_args("hevc_qsv", 26), vec!["-global_quality", "26"]);
        assert_eq!(
            quality_args("h264_amf", 23),
            vec!["-rc", "cqp", "-qp_i", "23", "-qp_p", "23"]
        );
    }

    #[test]
    fn quality_moves_on_a_scale_that_belongs_to_the_codec() {
        assert_eq!(quality_value(VideoCodec::H264, ExportQuality::Balanced), 23);
        assert_eq!(quality_value(VideoCodec::Vp9, ExportQuality::Balanced), 33);
        assert!(
            quality_value(VideoCodec::H264, ExportQuality::Maximum)
                < quality_value(VideoCodec::H264, ExportQuality::Small)
        );
    }

    /// The flags that ask for a constant quality, in every family's spelling.
    /// Any of them beside a bitrate either overrides it or turns it into a
    /// ceiling, so none may survive the switch.
    const QUALITY_FLAGS: [&str; 6] = ["-crf", "-cq", "-global_quality", "-qp", "-qp_i", "-qp_p"];

    fn assert_no_quality_flag(args: &[String], encoder: &str) {
        for flag in QUALITY_FLAGS {
            assert!(!args.iter().any(|a| a == flag), "{encoder} kept {flag}: {args:?}");
        }
        assert!(
            !args.windows(2).any(|w| w[0] == "-b:v" && w[1] == "0"),
            "{encoder} kept the cleared target: {args:?}"
        );
    }

    #[test]
    fn each_encoder_family_is_asked_for_a_bitrate_in_its_own_words() {
        assert_eq!(
            bitrate_args("libx264", 4000),
            vec!["-b:v", "4000k", "-maxrate", "6000k", "-bufsize", "8000k"]
        );
        assert_eq!(
            bitrate_args("libx265", 2500),
            vec!["-b:v", "2500k", "-maxrate", "3750k", "-bufsize", "5000k"]
        );
        assert_eq!(bitrate_args("libvpx-vp9", 3000), vec!["-b:v", "3000k"]);
        assert_eq!(bitrate_args("libsvtav1", 3000), vec!["-b:v", "3000k"]);
        assert_eq!(
            bitrate_args("libaom-av1", 3000),
            vec!["-b:v", "3000k", "-cpu-used", "6", "-row-mt", "1"]
        );
        assert_eq!(
            bitrate_args("librav1e", 3000),
            vec!["-b:v", "3000k", "-speed", "8", "-tiles", "8"]
        );
        assert_eq!(
            bitrate_args("h264_nvenc", 4000),
            vec![
                "-preset", "p5", "-tune", "hq", "-rc", "vbr", "-b:v", "4000k", "-maxrate",
                "6000k", "-bufsize", "8000k"
            ]
        );
        assert_eq!(
            bitrate_args("hevc_qsv", 4000),
            vec!["-b:v", "4000k", "-maxrate", "6000k"]
        );
        assert_eq!(
            bitrate_args("h264_amf", 4000),
            vec!["-rc", "vbr_peak", "-b:v", "4000k", "-maxrate", "6000k"]
        );
        assert_eq!(bitrate_args("h264_mediacodec", 4000), vec!["-b:v", "4000k"]);

        for encoder in [
            "libx264",
            "libx265",
            "libvpx-vp9",
            "libsvtav1",
            "libaom-av1",
            "librav1e",
            "h264_nvenc",
            "hevc_nvenc",
            "av1_nvenc",
            "h264_qsv",
            "av1_qsv",
            "hevc_amf",
            "hevc_mediacodec",
        ] {
            assert_no_quality_flag(&bitrate_args(encoder, 4000), encoder);
        }
    }

    #[test]
    fn every_codec_can_be_given_a_bitrate_on_the_processor() {
        // The retry after a hardware encoder gives up lands here, so a codec
        // without a processor spelling would lose the size the user asked for
        // exactly when the export had already gone wrong once. Every one of
        // them, because which one a machine has depends on its FFmpeg.
        for codec in [VideoCodec::H264, VideoCodec::H265, VideoCodec::Vp9, VideoCodec::Av1] {
            for &encoder in tools::processor_encoders(codec) {
                let args = bitrate_args(encoder, 1500);
                assert_eq!(value_after(&args, "-b:v").as_deref(), Some("1500k"), "{encoder}");
                assert_no_quality_flag(&args, encoder);
            }
        }
    }

    #[test]
    fn every_processor_encoder_is_asked_for_a_quality_it_understands() {
        // An encoder handed a flag it does not have refuses to open, and one
        // handed nothing encodes at whatever its default happens to be. Both
        // would pass unnoticed on a machine that never reaches that encoder,
        // which for libaom and rav1e is every desktop.
        for codec in [VideoCodec::H264, VideoCodec::H265, VideoCodec::Vp9, VideoCodec::Av1] {
            for &encoder in tools::processor_encoders(codec) {
                let args = quality_args(encoder, quality_value(codec, ExportQuality::Balanced));
                let named = if encoder == "librav1e" { "-qp" } else { "-crf" };
                assert!(args.iter().any(|a| a == named), "{encoder}: {args:?}");
            }
        }
    }

    #[test]
    fn rav1e_is_asked_for_the_same_quality_on_its_own_scale() {
        for quality in [
            ExportQuality::Maximum,
            ExportQuality::High,
            ExportQuality::Balanced,
            ExportQuality::Small,
        ] {
            let value = quality_value(VideoCodec::Av1, quality);
            assert_eq!(rav1e_quantizer(value), value * 4, "{quality:?}");
        }
        // The ends of the scale land on the ends of the other one, and nothing
        // a caller could pass leaves it.
        assert_eq!(rav1e_quantizer(0), 0);
        assert_eq!(rav1e_quantizer(63), 252);
        assert_eq!(rav1e_quantizer(64), 255);
        assert_eq!(rav1e_quantizer(u32::MAX), 255);
    }

    #[test]
    fn only_the_fallback_av1_encoders_are_told_how_fast_to_go() {
        assert_eq!(speed_args("libaom-av1"), ["-cpu-used", "6", "-row-mt", "1"]);
        assert_eq!(speed_args("librav1e"), ["-speed", "8", "-tiles", "8"]);
        for encoder in ["libx264", "libsvtav1", "libvpx-vp9", "av1_nvenc", "h264_mediacodec"] {
            assert!(speed_args(encoder).is_empty(), "{encoder}");
        }
    }

    #[test]
    fn svt_av1_is_told_the_frame_rate_the_join_loses() {
        let mut options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            video_codec: VideoCodec::Av1,
            ..ExportOptions::default()
        };
        let segments = [segment(0.0, 12.0)];
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libsvtav1");
        let params = value_after(&reencode_args(&plan).unwrap(), "-svtav1-params");
        assert_eq!(params.as_deref(), Some("fps-num=29970:fps-denom=1000"));

        // A rate the user picked is the one the file will have.
        options.fps = Some(25.0);
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libsvtav1");
        let params = value_after(&reencode_args(&plan).unwrap(), "-svtav1-params");
        assert_eq!(params.as_deref(), Some("fps-num=25000:fps-denom=1000"));

        // Nothing it would refuse, and nothing for anyone else.
        assert_eq!(svt_frame_rate_args(None)[1], "fps-num=30000:fps-denom=1000");
        assert_eq!(svt_frame_rate_args(Some(1_000_000.0))[1], "fps-num=30000:fps-denom=1000");
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libaom-av1");
        assert!(value_after(&reencode_args(&plan).unwrap(), "-svtav1-params").is_none());
    }

    #[test]
    fn the_largest_rate_allowed_does_not_overflow_its_peak() {
        let args = bitrate_args("libx264", 300_000);
        assert_eq!(value_after(&args, "-maxrate").as_deref(), Some("450000k"));
        assert_eq!(value_after(&args, "-bufsize").as_deref(), Some("600000k"));
        // And nothing a caller could pass does either.
        let args = bitrate_args("libx264", u32::MAX);
        assert_eq!(value_after(&args, "-bufsize").as_deref(), Some("8589934590k"));
    }

    #[test]
    fn the_processor_av1_is_never_asked_for_more_than_it_will_open_with() {
        // Past 100 Mb/s SVT-AV1 refuses to start, and it is where a failed
        // hardware AV1 export lands, so the top of the range has to be its top.
        assert_eq!(bitrate_args("libsvtav1", 300_000), vec!["-b:v", "100000k"]);
        assert_eq!(bitrate_args("libsvtav1", 100_000), vec!["-b:v", "100000k"]);
        assert_eq!(bitrate_args("libsvtav1", 99_999), vec!["-b:v", "99999k"]);
        // The hardware encoders take the whole range and are not held to it.
        for encoder in ["av1_nvenc", "av1_qsv"] {
            let args = bitrate_args(encoder, 300_000);
            assert_eq!(
                value_after(&args, "-b:v").as_deref(),
                Some("300000k"),
                "{encoder}"
            );
        }
    }

    /// Everything between `-c:v` and the next stream's settings: the video's
    /// rate control, as the export would run it.
    fn video_rate_args(args: &[String]) -> Vec<String> {
        let start = args.iter().position(|a| a == "-c:v").expect("-c:v") + 2;
        let end = args[start..]
            .iter()
            // SVT-AV1's frame rate follows its rate control and is not part of it.
            .position(|a| {
                a == "-svtav1-params" || a == "-tag:v" || a == "-c:a" || a == "-an" || a == "-movflags"
            })
            .map_or(args.len() - 1, |offset| start + offset);
        args[start..end].to_vec()
    }

    #[test]
    fn a_chosen_bitrate_replaces_the_quality_and_leaves_nothing_of_it() {
        let segments = [segment(0.0, 2.0)];
        for (container, codec, encoder) in [
            ("mp4", VideoCodec::H264, "libx264"),
            ("mp4", VideoCodec::H264, "h264_nvenc"),
            ("mov", VideoCodec::H265, "hevc_qsv"),
            ("webm", VideoCodec::Vp9, "libvpx-vp9"),
            ("mkv", VideoCodec::Av1, "libsvtav1"),
            ("mp4", VideoCodec::Av1, "libaom-av1"),
            ("webm", VideoCodec::Av1, "librav1e"),
        ] {
            let audio_codec = if container == "webm" {
                AudioCodec::Opus
            } else {
                AudioCodec::Aac
            };
            let options = ExportOptions {
                mode: crate::model::ExportMode::Reencode,
                container: container.into(),
                video_codec: codec,
                audio_codec,
                video_bitrate_kbps: Some(2400),
                ..ExportOptions::default()
            };
            let plan = export_plan(&options, &segments, Some((1280, 720)), true, encoder);
            let args = reencode_args(&plan).unwrap();

            assert_eq!(video_rate_args(&args), bitrate_args(encoder, 2400), "{encoder}");
            assert_no_quality_flag(&args, encoder);
            // The audio's own bitrate is a different flag and is untouched.
            assert_eq!(value_after(&args, "-b:a").as_deref(), Some("192k"), "{encoder}");
        }
    }

    #[test]
    fn no_chosen_bitrate_keeps_the_quality_exactly_as_it_was() {
        let segments = [segment(0.0, 2.0)];
        for (codec, encoder) in [
            (VideoCodec::H264, "libx264"),
            (VideoCodec::H264, "h264_nvenc"),
            (VideoCodec::H265, "hevc_amf"),
            (VideoCodec::Av1, "libsvtav1"),
            (VideoCodec::Av1, "libaom-av1"),
            (VideoCodec::Av1, "librav1e"),
        ] {
            let options = ExportOptions {
                mode: crate::model::ExportMode::Reencode,
                container: "mkv".into(),
                video_codec: codec,
                quality: ExportQuality::High,
                ..ExportOptions::default()
            };
            let plan = export_plan(&options, &segments, Some((1280, 720)), true, encoder);
            let args = reencode_args(&plan).unwrap();
            assert_eq!(
                video_rate_args(&args),
                quality_args(encoder, quality_value(codec, ExportQuality::High)),
                "{encoder}"
            );
        }
    }

    #[test]
    fn a_volume_is_applied_once_after_the_join() {
        let base = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            volume: 1.5,
            ..ExportOptions::default()
        };

        // One range: the join has a single input, and the gain still follows
        // it rather than going onto the range itself.
        let one = [segment(1.0, 3.0)];
        let plan = export_plan(&base, &one, Some((1280, 720)), true, "libx264");
        let args = reencode_args(&plan).unwrap();
        let graph = graph_of(&args);
        assert!(
            graph.contains("[v0][a0]concat=n=1:v=1:a=1[vc][acat];[acat]volume=1.500[ac];"),
            "{graph}"
        );
        assert_eq!(value_after(&args, "-map").as_deref(), Some("[vout]"));
        assert!(args.windows(2).any(|w| w[0] == "-map" && w[1] == "[ac]"));

        // Three ranges: still one gain, on the joined track.
        let three = [segment(1.0, 2.0), segment(4.0, 5.0), segment(7.0, 8.0)];
        let plan = export_plan(&base, &three, Some((1280, 720)), true, "libx264");
        let graph = graph_of(&reencode_args(&plan).unwrap());
        assert!(
            graph.contains(
                "[v0][a0][v1][a1][v2][a2]concat=n=3:v=1:a=1[vc][acat];[acat]volume=1.500[ac];[vc]"
            ),
            "{graph}"
        );
        assert_eq!(graph.matches("volume=").count(), 1, "{graph}");
        // Every label the graph makes is used, and the one mapped is made once.
        assert_eq!(graph.matches("[acat]").count(), 2, "{graph}");
        assert_eq!(graph.matches("[ac]").count(), 1, "{graph}");
    }

    #[test]
    fn silence_is_a_track_and_unity_is_no_filter_at_all() {
        let segments = [segment(1.0, 3.0), segment(4.0, 6.0)];

        // Zero keeps a silent track, which a player expecting sound still finds.
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            volume: 0.0,
            ..ExportOptions::default()
        };
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        let args = reencode_args(&plan).unwrap();
        assert!(graph_of(&args).contains("[acat]volume=0.000[ac];"));
        assert_eq!(value_after(&args, "-c:a").as_deref(), Some("aac"));
        assert!(!args.iter().any(|a| a == "-an"));

        // Unity, and anything close enough to it that the ear could not tell,
        // leaves the graph exactly as it was before there was a volume.
        for volume in [1.0, 1.004, 0.996, f64::NAN] {
            let options = ExportOptions {
                mode: crate::model::ExportMode::Reencode,
                volume,
                ..ExportOptions::default()
            };
            let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
            let graph = graph_of(&reencode_args(&plan).unwrap());
            assert!(!graph.contains("volume"), "{volume}: {graph}");
            assert!(graph.contains("concat=n=2:v=1:a=1[vc][ac];[vc]"), "{volume}: {graph}");
        }
    }

    #[test]
    fn a_volume_on_a_track_that_is_not_there_changes_nothing() {
        let segments = [segment(1.0, 3.0)];
        for (mute, has_audio) in [(true, true), (false, false)] {
            let options = ExportOptions {
                mode: crate::model::ExportMode::Reencode,
                volume: 1.8,
                mute,
                ..ExportOptions::default()
            };
            let plan = export_plan(&options, &segments, Some((1280, 720)), has_audio, "libx264");
            let args = reencode_args(&plan).unwrap();
            let graph = graph_of(&args);
            assert!(!graph.contains("volume"), "{graph}");
            assert!(!graph.contains("[acat]"), "{graph}");
            assert!(graph.contains("concat=n=1:v=1:a=0[vc];"), "{graph}");
            assert!(args.iter().any(|a| a == "-an"));
        }
    }

    #[test]
    fn hevc_in_an_mp4_is_tagged_so_that_a_player_will_open_it() {
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            video_codec: VideoCodec::H265,
            container: "mp4".into(),
            ..ExportOptions::default()
        };
        let segments = [segment(0.0, 2.0)];
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx265");
        assert_eq!(
            value_after(&reencode_args(&plan).unwrap(), "-tag:v").as_deref(),
            Some("hvc1")
        );

        let options = ExportOptions {
            container: "mkv".into(),
            ..options
        };
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx265");
        assert!(!reencode_args(&plan).unwrap().iter().any(|a| a == "-tag:v"));
    }

    #[test]
    fn an_impossible_pairing_is_refused_before_the_first_frame() {
        assert!(container_accepts("webm", VideoCodec::H264, Some(AudioCodec::Opus)).is_err());
        assert!(container_accepts("webm", VideoCodec::Vp9, Some(AudioCodec::Aac)).is_err());
        assert!(container_accepts("mov", VideoCodec::Vp9, Some(AudioCodec::Aac)).is_err());
        assert!(container_accepts("mov", VideoCodec::H264, Some(AudioCodec::Opus)).is_err());
        assert!(container_accepts("mp4", VideoCodec::Vp9, Some(AudioCodec::Aac)).is_err());

        assert!(container_accepts("webm", VideoCodec::Vp9, Some(AudioCodec::Opus)).is_ok());
        assert!(container_accepts("mov", VideoCodec::H265, Some(AudioCodec::Aac)).is_ok());
        assert!(container_accepts("mkv", VideoCodec::Av1, Some(AudioCodec::Flac)).is_ok());
        // A muted export has no audio to disagree about.
        assert!(container_accepts("webm", VideoCodec::Vp9, None).is_ok());
    }

    #[test]
    fn a_muted_export_still_refuses_a_container_that_cannot_hold_the_picture() {
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            container: "webm".into(),
            video_codec: VideoCodec::H264,
            mute: true,
            ..ExportOptions::default()
        };
        let segments = [segment(0.0, 2.0)];
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        assert!(reencode_args(&plan).is_err());
    }

    #[test]
    fn lossless_audio_carries_no_bitrate_and_the_rest_do() {
        let base = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            container: "mkv".into(),
            ..ExportOptions::default()
        };
        let segments = [segment(0.0, 2.0)];

        let options = ExportOptions {
            audio_codec: AudioCodec::Flac,
            ..base.clone()
        };
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        let args = reencode_args(&plan).unwrap();
        assert_eq!(value_after(&args, "-c:a").as_deref(), Some("flac"));
        assert!(!args.iter().any(|a| a == "-b:a"));

        let options = ExportOptions {
            audio_codec: AudioCodec::Opus,
            audio_bitrate_kbps: Some(160),
            ..base
        };
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        let args = reencode_args(&plan).unwrap();
        assert_eq!(value_after(&args, "-c:a").as_deref(), Some("libopus"));
        assert_eq!(value_after(&args, "-b:a").as_deref(), Some("160k"));
    }

    #[test]
    fn hardware_decoding_is_asked_for_separately_from_hardware_encoding() {
        let options = ExportOptions {
            mode: crate::model::ExportMode::Reencode,
            ..ExportOptions::default()
        };
        let segments = [segment(0.0, 2.0)];
        let plan = export_plan(&options, &segments, Some((1280, 720)), true, "libx264");
        let args = reencode_args(&plan).unwrap();

        let hwaccel = args.iter().position(|a| a == "-hwaccel").expect("-hwaccel");
        let input = args.iter().position(|a| a == "-i").expect("-i");
        assert!(hwaccel < input, "a decoder option after -i is ignored");
        assert_eq!(args[hwaccel + 1], "auto");
        assert_eq!(value_after(&args, "-c:v").as_deref(), Some("libx264"));
    }

    #[test]
    fn nothing_marked_is_refused_rather_than_run() {
        let options = ExportOptions::default();
        let plan = export_plan(&options, &[], Some((1280, 720)), true, "libx264");
        assert!(reencode_args(&plan).is_err());
    }

    #[test]
    fn a_pass_reports_where_the_whole_job_has_got_to() {
        let whole = PassSlice::whole();
        assert_eq!(whole.place(Some(0.0)), Some(0.0));
        assert_eq!(whole.place(Some(100.0)), Some(100.0));
        assert_eq!(whole.place(None), None);

        let second_half = PassSlice::new(50.0, 50.0);
        assert_eq!(second_half.place(Some(0.0)), Some(50.0));
        assert_eq!(second_half.place(Some(50.0)), Some(75.0));
        assert_eq!(second_half.place(Some(100.0)), Some(100.0));
    }

    #[test]
    fn the_cut_passes_share_their_half_by_how_much_each_one_writes() {
        let slices = weighted_slices(&[1.0, 3.0], 0.0, 50.0);
        assert_eq!(slices[0], PassSlice::new(0.0, 12.5));
        assert_eq!(slices[1], PassSlice::new(12.5, 37.5));
        assert_eq!(slices[1].place(Some(100.0)), Some(50.0));
    }

    #[test]
    fn weights_that_say_nothing_still_move_the_bar_forward() {
        let slices = weighted_slices(&[0.0, 0.0], 0.0, 50.0);
        assert_eq!(slices[0], PassSlice::new(0.0, 25.0));
        assert_eq!(slices[1], PassSlice::new(25.0, 25.0));
    }

    #[test]
    fn a_progress_field_is_told_apart_from_a_complaint() {
        assert_eq!(progress_field("out_time_us=3065034"), Some(("out_time_us", "3065034")));
        assert_eq!(progress_field("frame=  123"), Some(("frame", "123")));
        assert_eq!(progress_field("progress=end"), Some(("progress", "end")));
        assert_eq!(progress_field("stream_0_0_q=28.0"), Some(("stream_0_0_q", "28.0")));

        assert_eq!(progress_field("[AMF @ 01ac] DLL amfrt64.dll failed to open"), None);
        assert_eq!(
            progress_field("[Parsed_zscale_7 @ 00000208] code 3074: no path between colorspaces"),
            None
        );
        assert_eq!(progress_field("Error opening output file out.mp4."), None);
        assert_eq!(progress_field("no equals sign here"), None);
    }

    #[test]
    fn a_timestamp_that_is_not_there_yet_is_not_a_timestamp_of_zero() {
        assert_eq!(progress_micros("8797000"), Some(8_797_000.0));
        // What VP9 and AV1 report while they drain, in the last second of a
        // run. Read as zero it sends the bar from ninety-odd back to nothing.
        assert_eq!(progress_micros("N/A"), None);
        assert_eq!(progress_micros(""), None);
        assert_eq!(progress_micros("inf"), None);
    }
}
