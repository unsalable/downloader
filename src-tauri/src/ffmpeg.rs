//! FFmpeg operations: merging split streams and user-requested conversion.
//!
//! FFmpeg is only invoked when there is no other way to produce the requested
//! file. A merge is a stream copy -- no re-encoding, so it is I/O bound and
//! finishes in seconds. Re-encoding only happens when a container genuinely
//! cannot hold the source codecs, and it is logged when it does.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::downloader::control::TaskControl;
use crate::downloader::plan;
use crate::error::{AppError, AppResult};
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
    let binary = tools::require_ffmpeg()?;

    // `-progress pipe:1` writes machine-readable key=value lines, which is far
    // steadier to parse than the status line FFmpeg normally prints to stderr.
    let mut full_args: Vec<String> = vec!["-progress".into(), "pipe:1".into()];
    full_args.extend_from_slice(args);

    let mut child = process::command(&binary)
        .args(&full_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| AppError::Other(format!("FFmpeg could not be started: {err}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Other("FFmpeg produced no output stream".into()))?;
    let stderr = child.stderr.take();

    let stderr_handle = tokio::spawn(async move {
        let mut collected = String::new();
        if let Some(stderr) = stderr {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if collected.len() < 4096 {
                    collected.push_str(&line);
                    collected.push('\n');
                }
            }
        }
        collected
    });

    let mut reader = BufReader::new(stdout).lines();
    let mut bytes_written = 0u64;
    let mut last_emit = Instant::now();
    let mut poll = tokio::time::interval(Duration::from_millis(250));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let interrupted = loop {
        tokio::select! {
            line = reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let Some((key, value)) = line.split_once('=') else { continue };
                        match key.trim() {
                            "total_size" => {
                                bytes_written = value.trim().parse().unwrap_or(bytes_written);
                            }
                            "out_time_us" | "out_time_ms" => {
                                // Despite its name, out_time_ms is microseconds.
                                let micros: f64 = value.trim().parse().unwrap_or(0.0);
                                let percent = duration_sec
                                    .filter(|d| *d > 0.0)
                                    .map(|d| (micros / 1_000_000.0 / d * 100.0).clamp(0.0, 100.0));
                                if last_emit.elapsed() >= Duration::from_millis(300) {
                                    on_progress(FfmpegProgress { percent, bytes_written });
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
        stderr_handle.abort();
        return Err(AppError::Canceled);
    }

    let status = child
        .wait()
        .await
        .map_err(|err| AppError::Other(format!("FFmpeg did not exit cleanly: {err}")))?;
    let stderr_text = stderr_handle.await.unwrap_or_default();

    if !status.success() {
        log_debug!("ffmpeg", "failed: {}", stderr_text.trim());
        return Err(AppError::Other(format!(
            "FFmpeg exited with {}: {}",
            status.code().unwrap_or(-1),
            stderr_text.lines().next().unwrap_or("no detail").trim()
        )));
    }

    on_progress(FfmpegProgress {
        percent: Some(100.0),
        bytes_written,
    });
    Ok(())
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
}
