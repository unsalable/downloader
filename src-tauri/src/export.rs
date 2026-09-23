//! Keeping several ranges of one local file and joining them into one result.
//!
//! This is deliberately not built on [`crate::converter`], even though both
//! drive FFmpeg over a file the user picked. A conversion is a batch: several
//! files are staged, a format is chosen for all of them, and the results land
//! in a list that outlives the screen. An export is the opposite -- one file,
//! open in front of the user, with marks they are still moving. There is only
//! ever one of them, so there is no queue here: starting an export while one is
//! running replaces it, which is what pressing the button again means.
//!
//! What this module owns, and [`crate::ffmpeg`] deliberately does not, is every
//! decision: which ranges survive the checks below, where a copied cut is
//! allowed to begin, how many processes the job is and what share of the bar
//! each one gets, and what the file ends up called.
//!
//! Six things FFmpeg will accept and then write rubbish for are refused here
//! instead: a range with no length or the wrong way round, a range that starts
//! after the file ends, a range that runs past the end, ranges that overlap or
//! arrive out of order, a join of anything this run did not write itself, and
//! an aspect ratio asked of a copy, which cannot square a non-square pixel and
//! so would hand back a picture the wrong shape.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use regex::Regex;
use tauri::{AppHandle, Emitter};

use crate::downloader::control::TaskControl;
use crate::downloader::plan;
use crate::error::{AppError, AppResult};
use crate::ffmpeg::{self, ExportPlan, PassSlice, ToneMap};
use crate::model::{
    AspectRatio, EditSegment, ExportMode, ExportOptions, ExportRequest, ExportState, ExportStatus,
    MediaProbe,
};
use crate::settings::Settings;
use crate::{converter, filename, log_info, log_warn, paths, process, tools, util};

pub const EVENT_CHANGED: &str = "editor://export";

/// The shortest range worth keeping. Below this the two marks are one mark the
/// user has not finished dragging, and FFmpeg would be asked for a piece of
/// film with no frames in it.
const MIN_SEGMENT_SEC: f64 = 0.05;

/// How much of the bar the cutting half of a lossless export owns.
///
/// The cuts are stream copies and the join re-writes every byte of them again,
/// so the two halves cost roughly the same and are given the same share.
const CUT_SHARE: f64 = 50.0;

/// Containers an export may be written into. An extension that reached the
/// command line unchecked would be an output path nobody chose.
const CONTAINERS: [&str; 4] = ["mp4", "mkv", "mov", "webm"];

/// The range a chosen video bitrate is held to, in kilobits a second. Why these
/// two is said where they are applied, in [`checked_options`].
const MIN_VIDEO_KBPS: u32 = 100;
const MAX_VIDEO_KBPS: u32 = 300_000;

pub struct ExportManager {
    app: AppHandle,
    settings: Arc<Mutex<Settings>>,
    state: Mutex<ExportState>,
    /// The export that is running, so a later press can interrupt it.
    control: Mutex<Option<Arc<TaskControl>>>,
}

/// Everything one export knows about itself, settled before the first process
/// starts so that nothing below has to re-read the request.
struct Job {
    id: String,
    input: PathBuf,
    output: PathBuf,
    segments: Vec<EditSegment>,
    /// One per segment, in step with it, and only for a lossless export: where
    /// that range's input seek is aimed, which is a little past its start. See
    /// [`seek_for`].
    seeks: Vec<f64>,
    options: ExportOptions,
    probe: MediaProbe,
    hardware: bool,
}

impl Job {
    /// Where to seek for the range at `index`. A re-encode carries no seeks of
    /// its own -- it cuts inside the filter graph -- so the start stands in.
    fn seek_at(&self, index: usize) -> f64 {
        self.seeks
            .get(index)
            .copied()
            .unwrap_or_else(|| self.segments[index].start_sec)
    }

    /// What FFmpeg reports is how far into the output it has got, so the whole
    /// job is the length of what is kept -- not the length of the source, which
    /// would leave a short export of a long film stuck near zero.
    fn kept_sec(&self) -> f64 {
        self.segments.iter().map(EditSegment::length).sum()
    }
}

impl ExportManager {
    pub fn new(app: AppHandle, settings: Arc<Mutex<Settings>>) -> Arc<Self> {
        Arc::new(Self {
            app,
            settings,
            state: Mutex::new(ExportState::default()),
            control: Mutex::new(None),
        })
    }

    fn settings(&self) -> Settings {
        self.settings
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn state(&self) -> ExportState {
        self.state
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// How many exports are running: one or none.
    ///
    /// Read by [`crate::android`], which keeps the app alive in the background
    /// while there is work. It is worked out from the published state, so it
    /// goes back to zero at the same moment the screen hears the export ended
    /// -- however it ended.
    pub fn active_count(&self) -> u32 {
        u32::from(self.state().status.is_active())
    }

    /// Where an export of `input` goes when no folder was chosen, if beside it
    /// will not do: the folder downloads go to. `None` means beside it will.
    /// See [`export_dir`] for which sources those are.
    ///
    /// Asked by the export itself and by the inspector's location row, through
    /// [`crate::commands::export_default_dir`], so the row can only ever name
    /// the folder the file will really be written into.
    pub fn default_dir(&self, input: &Path) -> Option<String> {
        out_of_reach(input, cfg!(target_os = "android"), paths::root().ok())
            .then(|| self.settings().download_dir)
    }

    /// Replace the published state and tell the interface about it. Every
    /// change goes through here, so the screen cannot drift from the truth.
    fn publish(&self, update: impl FnOnce(&mut ExportState)) {
        let next = {
            let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
            update(&mut guard);
            guard.clone()
        };
        let _ = self.app.emit(EVENT_CHANGED, &next);
    }

    /// Interrupt the running export, if there is one. The task itself publishes
    /// the result: FFmpeg has to be given the chance to die first.
    pub fn cancel(&self) {
        let control = self
            .control
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        if let Some(control) = control {
            control.cancel();
        }
    }

    /// Stop anything in flight because the app is closing.
    pub fn shutdown(&self) {
        self.cancel();
    }

    /// Begin an export, replacing whatever was running.
    ///
    /// The marks are checked against what is actually in the file rather than
    /// trusted: the interface clamps them to the duration it was shown, and
    /// that duration came from a probe of a file that may since have been
    /// replaced on disk.
    pub async fn start(self: &Arc<Self>, request: ExportRequest) -> AppResult<()> {
        // Nothing below works without it, and this is a far clearer way to
        // learn that than a failure to spawn a process.
        tools::require_ffmpeg()?;

        let probe = converter::probe(&request.input_path).await?;
        let options = checked_options(request.options)?;
        let mut segments = plan_segments(&request.segments, probe.duration_sec)?;

        let mut seeks: Vec<f64> = Vec::new();
        if options.mode == ExportMode::Lossless {
            lossless_is_possible(&options, &probe)?;
            // A copied video stream can only begin at a keyframe, so this is
            // where the ranges stop being what was asked for and become what
            // can be delivered. Snapping can push one range back onto the one
            // before it, which is why they are merged again afterwards.
            let marks = keyframes(&request.input_path).await?;
            for segment in &mut segments {
                segment.start_sec = snap_to_keyframe(segment.start_sec, &marks);
            }
            segments = merge_overlaps(segments);
            // The seek is worked out here and not in the argument builder,
            // because this is the only place that knows where the keyframes
            // are. Everything else -- `-to`, the progress weights, the promised
            // length -- goes on using the range's real start.
            seeks = segments
                .iter()
                .map(|segment| seek_for(segment.start_sec, segment.end_sec, &marks))
                .collect();
        }

        let input = PathBuf::from(&request.input_path);
        let dir = export_dir(
            &input,
            request.output_dir.as_deref(),
            self.default_dir(&input).as_deref(),
        )?;
        let output = output_path(&input, &dir, &options, &segments)?;

        // A press while an export is running means "this one instead", so the
        // old one is interrupted before the new state is published over it.
        self.cancel();

        let control = Arc::new(TaskControl::new());
        *self.control.lock().unwrap_or_else(|err| err.into_inner()) = Some(Arc::clone(&control));

        self.publish(|state| {
            *state = ExportState {
                status: ExportStatus::Running,
                percent: Some(0.0),
                output_path: None,
                error: None,
            };
        });

        let job = Job {
            id: util::new_id("ex"),
            input,
            output,
            segments,
            seeks,
            hardware: options.hardware && self.settings().hardware_acceleration,
            options,
            probe,
        };

        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let outcome = manager.execute(&job, Arc::clone(&control)).await;

            if !reclaim(&manager.control, &control) {
                return;
            }

            match outcome {
                Ok(()) => {
                    let path = job.output.to_string_lossy().into_owned();
                    log_info!("export", "wrote {path}");
                    // Written by path into shared storage, which the gallery
                    // does not watch: without this the export would exist
                    // nowhere but in this app until the phone next rescanned.
                    #[cfg(target_os = "android")]
                    crate::android::announce_media(&manager.app, &path);
                    manager.publish(|state| {
                        *state = ExportState {
                            status: ExportStatus::Completed,
                            percent: Some(100.0),
                            output_path: Some(path),
                            error: None,
                        };
                    });
                }
                Err(AppError::Canceled) => {
                    // A half-written file is not a result, and leaving it
                    // beside the source would look like one.
                    let _ = std::fs::remove_file(&job.output);
                    manager.publish(|state| {
                        *state = ExportState {
                            status: ExportStatus::Canceled,
                            percent: None,
                            output_path: None,
                            error: None,
                        };
                    });
                }
                Err(err) => {
                    let _ = std::fs::remove_file(&job.output);
                    log_warn!("export", "the export failed: {err}");
                    manager.publish(|state| {
                        *state = ExportState {
                            status: ExportStatus::Failed,
                            percent: None,
                            output_path: None,
                            error: Some(err.to_info()),
                        };
                    });
                }
            }
        });

        Ok(())
    }

    async fn execute(self: &Arc<Self>, job: &Job, control: Arc<TaskControl>) -> AppResult<()> {
        match job.options.mode {
            ExportMode::Lossless => self.run_lossless(job, control).await,
            ExportMode::Reencode => self.run_reencode(job, control).await,
        }
    }

    /// One range is one process; several are one cut pass each and a join.
    async fn run_lossless(
        self: &Arc<Self>,
        job: &Job,
        control: Arc<TaskControl>,
    ) -> AppResult<()> {
        let keep_audio = !job.options.mute;
        if let [only] = job.segments.as_slice() {
            let args = ffmpeg::lossless_single_args(
                &job.input,
                &job.output,
                &job.options.container,
                job.seek_at(0),
                only.end_sec,
                keep_audio,
            );
            return self
                .pass(&args, Some(only.length()), PassSlice::whole(), control)
                .await;
        }

        let scratch = paths::temp_dir()?;
        let codec = job.probe.video_codec.as_deref();
        // Sound that is being left out has no say in how the pieces are
        // written: a track the transport stream could not carry is not a
        // reason to take the slower muxer when it is not going in at all.
        let audio = job.probe.audio_codec.as_deref().filter(|_| keep_audio);
        let (muxer, extension) = ffmpeg::segment_format(codec, audio);
        // The cuts share the first half of the bar in proportion to how much
        // film each of them writes; the join owns the second half.
        let weights: Vec<f64> = job.segments.iter().map(EditSegment::length).collect();
        let slices = ffmpeg::weighted_slices(&weights, 0.0, CUT_SHARE);

        let mut pieces: Vec<PathBuf> = Vec::with_capacity(job.segments.len());
        let mut outcome = Ok(());
        for (index, segment) in job.segments.iter().enumerate() {
            // `intermediate_path` under the app's own temp directory, so that
            // the sweep at startup clears whatever a crash leaves behind.
            let piece = ffmpeg::intermediate_path(&scratch, &job.id, &format!("seg{index}"), extension);
            let args = ffmpeg::lossless_cut_args(
                &job.input,
                &piece,
                muxer,
                job.seek_at(index),
                segment.end_sec,
                codec,
                keep_audio,
            );
            pieces.push(piece);
            outcome = self
                .pass(
                    &args,
                    Some(segment.length()),
                    slices[index],
                    Arc::clone(&control),
                )
                .await;
            if outcome.is_err() {
                break;
            }
        }

        let list = scratch.join(format!("{}.list.txt", job.id));
        if outcome.is_ok() {
            outcome = ensure_produced(&pieces, &scratch).and_then(|()| {
                std::fs::write(&list, ffmpeg::concat_list_text(&pieces)).map_err(AppError::from)
            });
        }
        if outcome.is_ok() {
            let args = ffmpeg::concat_args(&list, &job.output, &job.options.container, keep_audio);
            outcome = self
                .pass(
                    &args,
                    Some(job.kept_sec()),
                    PassSlice::new(CUT_SHARE, 100.0 - CUT_SHARE),
                    control,
                )
                .await;
        }

        for piece in &pieces {
            let _ = std::fs::remove_file(piece);
        }
        let _ = std::fs::remove_file(&list);
        outcome
    }

    /// Every range, cut and joined and re-shaped, in one process.
    async fn run_reencode(
        self: &Arc<Self>,
        job: &Job,
        control: Arc<TaskControl>,
    ) -> AppResult<()> {
        let tone_map = tone_map_for(&job.probe, &job.options).await;
        let encoder = tools::preferred_encoder(job.options.video_codec, job.hardware).await;
        let processor = tools::processor_encoder(job.options.video_codec).await;
        let kept = job.kept_sec();

        let args = reencode_args(job, &encoder, tone_map)?;
        match self
            .pass(&args, Some(kept), PassSlice::whole(), Arc::clone(&control))
            .await
        {
            Ok(()) => Ok(()),
            Err(AppError::Canceled) => Err(AppError::Canceled),
            Err(err) if encoder != processor => {
                // The encoder opened on a test pattern and still gave up on
                // this film -- a frame size it will not take, a driver that
                // fell over. The processor is slower and always works.
                log_info!(
                    "export",
                    "{encoder} gave up on this file ({err}); encoding on the processor instead"
                );
                let _ = std::fs::remove_file(&job.output);
                let args = reencode_args(job, processor, tone_map)?;
                self.pass(&args, Some(kept), PassSlice::whole(), control).await
            }
            Err(err) => Err(err),
        }
    }

    async fn pass(
        self: &Arc<Self>,
        args: &[String],
        duration_sec: Option<f64>,
        slice: PassSlice,
        control: Arc<TaskControl>,
    ) -> AppResult<()> {
        let manager = Arc::clone(self);
        let mut sink = move |update: ffmpeg::FfmpegProgress| manager.on_progress(update);
        ffmpeg::run_pass(args, duration_sec, slice, control, &mut sink).await
    }

    fn on_progress(&self, update: ffmpeg::FfmpegProgress) {
        self.publish(|state| {
            // A late tick from an export that has already finished must not put
            // a running bar back on a finished screen.
            if state.status != ExportStatus::Running {
                return;
            }
            // `-movflags +faststart` re-writes a large MP4 once the last frame
            // is in, and FFmpeg reports nothing at all while it does. A bar
            // that reached 100 and then sat there would be saying the file was
            // ready; one short of the end says what is actually true.
            state.percent = update.percent.map(|value| value.min(99.0));
        });
    }
}

fn reencode_args(job: &Job, encoder: &str, tone_map: ToneMap) -> AppResult<Vec<String>> {
    ffmpeg::reencode_args(&ExportPlan {
        input: &job.input,
        output: &job.output,
        segments: &job.segments,
        options: &job.options,
        source_frame: job.probe.width.zip(job.probe.height).map(|(width, height)| {
            // The graph squares the pixels before anything else, so the frame
            // a tier is measured against is the squared one. Measured against
            // the stored frame instead, the 16:9 button on a 1440x1080 master
            // with a 4:3 pixel -- a picture that is already 16:9 -- hands back
            // 1280x720, throwing away half the lines for a button that changed
            // nothing about the shape.
            square_pixels(width, height, job.probe.pixel_aspect.unwrap_or(1.0))
        }),
        has_audio: job.probe.has_audio,
        source_fps: job.probe.fps,
        video_encoder: encoder,
        tone_map,
    })
}

/// Take the running-export handle back, and say whether it was still this
/// export's to take.
///
/// A press that replaced this export installed its own handle before this task
/// noticed the flag it was cancelled by, and from that moment the state on
/// screen and the file on disk both belong to the newer one. Clearing the
/// handle regardless would leave the live export impossible to cancel, and
/// publishing over it would freeze its progress bar; worse, the tidy-up that
/// follows deletes the output file, which until the last pass of a multi-range
/// export finishes is the very path the newer one is writing.
fn reclaim(control: &Mutex<Option<Arc<TaskControl>>>, ours: &Arc<TaskControl>) -> bool {
    let mut guard = control.lock().unwrap_or_else(|err| err.into_inner());
    let still_ours = guard
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, ours));
    if still_ours {
        *guard = None;
    }
    still_ours
}

/// The frame once its pixels are square, which is the frame the export's filter
/// graph opens by producing.
///
/// The arithmetic is deliberately the one in that graph's first `scale`: a wide
/// pixel widens the frame and a tall one heightens it, never both, and each
/// side comes down to an even number because libx264 will not take an odd one.
/// A ratio that is not a positive finite number is read as square, which is
/// what an unread one means anyway.
///
/// The product is rounded to a whole pixel before it is made even. 1440 times
/// the nearest double to 4/3 is a ten-thousandth of a billionth short of 1920,
/// and truncating that would hand back 1918 -- one pixel under the 16:9 tier,
/// which is the whole point of measuring this.
fn square_pixels(width: u32, height: u32, pixel_aspect: f64) -> (u32, u32) {
    let sar = if pixel_aspect.is_finite() && pixel_aspect > 0.0 {
        pixel_aspect
    } else {
        1.0
    };
    let even = |value: f64| (value.round() / 2.0).trunc() as u32 * 2;
    (
        even(f64::from(width) * sar.max(1.0)),
        even(f64::from(height) / sar.min(1.0)),
    )
}

/// Whether there is anything to tone map, and whether this build can.
async fn tone_map_for(probe: &MediaProbe, options: &ExportOptions) -> ToneMap {
    if !options.tone_map_sdr || !source_is_hdr(&probe.path).await {
        return ToneMap::None;
    }
    if tools::filter_available("zscale").await && tools::filter_available("tonemap").await {
        ToneMap::Hable
    } else {
        // Without zscale there is no way into linear light, and a tone curve
        // applied outside it is not a tone map. The picture is converted and
        // the highlights stay where they are, which is worse than the real
        // thing and better than claiming to have done it.
        log_warn!(
            "export",
            "this FFmpeg has no zscale or no tonemap; converting the picture without mapping it"
        );
        ToneMap::Plain
    }
}

/// Whether the source is brighter than Rec. 709 can hold.
///
/// Asked of the file rather than taken from the setting: the setting says the
/// user would like an SDR result, and on a source that is already SDR that is
/// a request to do nothing, not a request to run a curve over it.
async fn source_is_hdr(path: &str) -> bool {
    let Some(binary) = converter::ffprobe_path() else {
        return false;
    };
    let args: Vec<String> = vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "v:0".into(),
        "-show_entries".into(),
        "stream=color_transfer".into(),
        "-of".into(),
        "csv=p=0".into(),
        path.to_owned(),
    ];
    match process::run(&binary, &args).await {
        Ok(output) => output
            .stdout
            .lines()
            .any(|line| is_hdr_transfer(line.trim().trim_end_matches(','))),
        Err(_) => false,
    }
}

/// The two transfer curves that mean HDR in practice: perceptual quantiser,
/// and the hybrid log-gamma broadcasters use.
fn is_hdr_transfer(transfer: &str) -> bool {
    matches!(transfer, "smpte2084" | "arib-std-b67")
}

// -- what an export is allowed to be ----------------------------------------

/// The container, checked and lowercased; the rate and the gain, held to what
/// FFmpeg will take.
///
/// The two numbers are clamped rather than refused, because both come off a
/// control the user drags and neither has a wrong answer, only an extreme one.
/// Below 100 kb/s no encoder here produces a picture anyone would keep, and
/// 300 Mb/s is past what any of them can spend on a frame. A gain that is not a
/// number is taken to mean no change: FFmpeg would refuse the graph over it, and
/// a failed export is a worse answer than an untouched volume.
fn checked_options(mut options: ExportOptions) -> AppResult<ExportOptions> {
    options.container = options.container.trim().to_ascii_lowercase();
    if !CONTAINERS.contains(&options.container.as_str()) {
        return Err(AppError::Other(format!(
            "{} is not a container this app can write",
            options.container
        )));
    }
    options.video_bitrate_kbps = options
        .video_bitrate_kbps
        .map(|kbps| kbps.clamp(MIN_VIDEO_KBPS, MAX_VIDEO_KBPS));
    options.volume = if options.volume.is_finite() {
        options.volume.clamp(0.0, 2.0)
    } else {
        1.0
    };
    Ok(options)
}

/// What a stream copy cannot do, said before the export rather than after it.
///
/// A copy leaves the frames exactly as they were found, so everything the
/// editor offers beyond where the cuts fall and whether the sound comes along
/// is a reason to leave this mode. The aspect buttons are the sharpest case: a
/// source stored with a non-square pixel is only the shape it looks like once
/// the pixels have been squared, and squaring them means decoding them.
///
/// Dropping the sound is the one change a copy can make, because leaving a
/// track out decodes nothing. Once it is dropped, nothing about it can stand in
/// the way either: not a volume the slider still holds, which a re-encode would
/// not apply to a track it is not writing, and not a codec the container could
/// not have held.
fn lossless_is_possible(options: &ExportOptions, probe: &MediaProbe) -> AppResult<()> {
    let refusal = |what: &str| {
        Err(AppError::Other(format!(
            "{what} means re-encoding; a copied export can only choose where the cuts fall \
             and whether the sound comes along"
        )))
    };
    if options.aspect != AspectRatio::Source {
        return refusal("changing the shape of the frame");
    }
    if options.max_height.is_some() {
        return refusal("changing the size of the frame");
    }
    if options.fps.is_some() {
        return refusal("changing the frame rate");
    }
    let keeps_sound = probe.has_audio && !options.mute;
    if keeps_sound && options.gain().is_some() {
        return refusal("changing the volume");
    }

    let container = options.container.as_str();
    let video = probe.video_codec.as_deref().unwrap_or("");
    let audio = probe
        .audio_codec
        .as_deref()
        .filter(|_| keeps_sound)
        .unwrap_or("");
    if !plan::container_holds_video(container, video)
        || !plan::container_holds_audio(container, audio)
    {
        return Err(AppError::Other(format!(
            "a .{container} file cannot hold these streams without re-encoding them"
        )));
    }
    Ok(())
}

/// The ranges as they will actually be exported.
///
/// Sorted, clamped to the file, merged where they touch, and refused where they
/// say nothing. FFmpeg accepts every one of these mistakes and writes something
/// for them -- an empty file, a file of the whole film, a file with a piece
/// repeated -- so none of them can be left to it.
fn plan_segments(segments: &[EditSegment], duration: Option<f64>) -> AppResult<Vec<EditSegment>> {
    if segments.is_empty() {
        return Err(AppError::Other("there is nothing marked to export".into()));
    }

    let duration = duration.filter(|value| value.is_finite() && *value > 0.0);
    let mut kept = Vec::with_capacity(segments.len());

    for segment in segments {
        if !segment.start_sec.is_finite() || !segment.end_sec.is_finite() {
            return Err(AppError::Other("one of the marks is not a time".into()));
        }
        let start = segment.start_sec.max(0.0);
        if duration.is_some_and(|duration| start >= duration) {
            return Err(AppError::Other(
                "one of the ranges starts after the file ends".into(),
            ));
        }
        let end = match duration {
            Some(duration) => segment.end_sec.min(duration),
            None => segment.end_sec,
        };
        // Catches both the range with no length and the range the wrong way
        // round, which FFmpeg turns into an empty file and a whole film.
        if end - start < MIN_SEGMENT_SEC {
            return Err(AppError::Other(
                "one of the ranges is too short to hold a frame".into(),
            ));
        }
        kept.push(EditSegment {
            start_sec: start,
            end_sec: end,
        });
    }

    kept.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    Ok(merge_overlaps(kept))
}

/// Fold ranges that overlap or touch into one.
///
/// Two ranges that meet are one range, and exporting them as two would put a
/// join in the middle of a continuous piece of film -- which on a copy means a
/// visible stutter where none was asked for.
fn merge_overlaps(sorted: Vec<EditSegment>) -> Vec<EditSegment> {
    let mut merged: Vec<EditSegment> = Vec::with_capacity(sorted.len());
    for segment in sorted {
        match merged.last_mut() {
            Some(previous) if segment.start_sec <= previous.end_sec => {
                previous.end_sec = previous.end_sec.max(segment.end_sec);
            }
            _ => merged.push(segment),
        }
    }
    merged
}

/// Only files this run wrote, and wrote something into, are ever joined.
///
/// The concat demuxer is handed `-safe 0`, which means it reads whatever
/// absolute path the list gives it. The list is built here and nowhere else,
/// from paths this run made in the app's own scratch directory -- and this is
/// the check that says so, and catches the one way it could go wrong in
/// practice: a cut pass that exited cleanly having written nothing at all.
fn ensure_produced(pieces: &[PathBuf], scratch: &Path) -> AppResult<()> {
    for piece in pieces {
        if piece.parent() != Some(scratch) {
            return Err(AppError::Other(
                "an export tried to join a file it did not make".into(),
            ));
        }
        if std::fs::metadata(piece).map(|meta| meta.len()).unwrap_or(0) == 0 {
            return Err(AppError::Other("one of the ranges came out empty".into()));
        }
    }
    Ok(())
}

/// The folder an export is written into.
///
/// The one the user chose, if they chose one; otherwise beside the source,
/// except where the source sits somewhere the user cannot keep a file. On
/// Android that is every file the editor opens: the picker copies what it is
/// handed into the app's cache, and a fetched link lands in the app's own
/// temporary folder. Beside either of those is a folder no file manager or
/// gallery can reach, and one the system empties when it runs short of space.
/// On the desktop it is a file inside the app's own folder, which is where a
/// link brought in with the editor's "from a link" is fetched to: that
/// temporary folder is emptied by the sweep at startup and by clearing
/// temporary files in Settings. Either way an export written beside the source
/// would be finished, and then gone. `shared` is where it goes instead -- the
/// folder downloads go to, as for a conversion -- and is `None` whenever beside
/// the source is a place the user already knows; see
/// [`ExportManager::default_dir`].
fn export_dir(input: &Path, chosen: Option<&str>, shared: Option<&str>) -> AppResult<PathBuf> {
    let named = |dir: Option<&str>| {
        dir.map(str::trim)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = named(chosen).or_else(|| named(shared)) {
        return Ok(dir);
    }
    input
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::Io("that file has nowhere beside it to write to".into()))
}

/// Whether beside `input` is somewhere an export would not be kept: anywhere
/// at all on a phone, and inside the app's own folder, `root`, elsewhere. The
/// reasons are [`export_dir`]'s.
fn out_of_reach(input: &Path, phone: bool, root: Option<&Path>) -> bool {
    phone || root.is_some_and(|root| lies_within(input, root))
}

/// Whether `path` is inside `dir`, the app's own folder.
///
/// Asked of the path on disk first, which settles a different case or an old
/// short name for the same folder, and then of the two as written, which is
/// all there is to go on for a file that is not there any more. Compared by
/// component, so a folder that merely begins with the same letters --
/// `UniversalDownloader Backup` beside `UniversalDownloader` -- is not inside
/// it, and on Windows without regard to case, as its file system does.
fn lies_within(path: &Path, dir: &Path) -> bool {
    if let (Ok(path), Ok(dir)) = (std::fs::canonicalize(path), std::fs::canonicalize(dir)) {
        if starts_with_folded(&path, &dir) {
            return true;
        }
    }
    starts_with_folded(path, dir)
}

fn starts_with_folded(path: &Path, dir: &Path) -> bool {
    let fold = |part: std::path::Component<'_>| {
        let text = part.as_os_str().to_string_lossy();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text.into_owned()
        }
    };
    let mut parts = path.components().map(fold);
    dir.components().map(fold).all(|part| parts.next() == Some(part))
}

/// This app's own range at the end of a name: two stamps, and the "(2)" a
/// collision added after them, if one did. See [`stamp`].
static RANGE_SUFFIX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r" [0-9]+m[0-9]{2}(?:\.[0-9])?s-[0-9]+m[0-9]{2}(?:\.[0-9])?s(?: \([0-9]+\))?$")
        .expect("valid regex")
});

/// A stem without the range this app wrote into it, if it wrote one.
///
/// An export of an export, or of a piece fetched from a link, is named from a
/// stem that already ends in the range it was cut from. Adding the new one
/// after it stacked them -- "holiday 0m00s-0m12s 0m00s-0m05.2s" -- and one more
/// pass stacked a third. The new range replaces the old one instead, so a name
/// carries one range however many times the film has been cut: the one this
/// file was taken at, from the file it was taken from. A name that then matches
/// one already in the folder is given a "(2)", as any other would be.
fn without_range(stem: &str) -> &str {
    match RANGE_SUFFIX.find(stem) {
        Some(found) if found.start() > 0 => &stem[..found.start()],
        _ => stem,
    }
}

/// Where the export is written, inside `dir`, which is made if it is missing.
///
/// The kept range goes in the name because it is the only thing that
/// distinguishes two exports of the same film, and it says so in digits rather
/// than in a word that would have to be translated. Several ranges are named by
/// the span they were taken from: the alternative is a name listing every mark,
/// which on eight ranges is not a file name any more.
fn output_path(
    input: &Path,
    dir: &Path,
    options: &ExportOptions,
    segments: &[EditSegment],
) -> AppResult<PathBuf> {
    if !dir.is_dir() {
        std::fs::create_dir_all(dir).map_err(|err| {
            AppError::Permission(format!("{} could not be created: {err}", dir.display()))
        })?;
    }

    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(without_range)
        .unwrap_or("clip");
    let first = segments.first().map(|s| s.start_sec).unwrap_or(0.0);
    let last = segments.last().map(|s| s.end_sec).unwrap_or(0.0);

    let named = format!(
        "{} {}-{}",
        filename::truncate_stem(stem, 120),
        stamp(first),
        stamp(last)
    );
    Ok(filename::unique_path(
        dir,
        &filename::sanitize_component(&named),
        &options.container,
    ))
}

/// A mark, as a file name can hold it: minutes and seconds, no colon -- which
/// Windows will not take in a name -- and no decimal point unless the tenth of
/// a second is what tells two exports apart.
///
/// Shared with [`crate::range`], which names a fetched piece of a link the same
/// way for the same reason. Two spellings of the same thing in one folder would
/// read as two different things.
pub(crate) fn stamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let rest = seconds - (minutes as f64) * 60.0;
    if (rest - rest.round()).abs() < 0.05 {
        format!("{minutes}m{:02}s", rest.round() as u64)
    } else {
        format!("{minutes}m{rest:04.1}s")
    }
}

// -- keyframes --------------------------------------------------------------

/// Where a copied cut is allowed to begin, in seconds, remembered per file.
///
/// Read once per opened file: a five-minute film answers with 150 marks in
/// 0.15 s, which is cheap enough to do on demand and far too expensive to do
/// per keystroke while someone drags a handle.
static KEYFRAMES: Lazy<Mutex<HashMap<String, Remembered>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// A keyframe list together with the file it was read from, so that a file
/// replaced on disk under the same name is read again rather than believed.
#[derive(Clone)]
struct Remembered {
    size: u64,
    modified_ms: i64,
    times: Arc<Vec<f64>>,
}

/// The keyframe times of a file.
///
/// Served to the interface as its own command, because the consequence of
/// snapping is something the user has to be able to see before they commit to
/// it. The failure a snapped start causes is not corruption -- the keyframe is
/// in the file and decodes perfectly -- it is surplus footage: measured, half a
/// second asked for arrived as 1.002667 s, beginning earlier than the mark.
pub async fn keyframes(path: &str) -> AppResult<Vec<f64>> {
    let print = fingerprint(Path::new(path));
    if let Some(known) = remembered(path) {
        if print == Some((known.size, known.modified_ms)) {
            return Ok(known.times.as_ref().clone());
        }
    }

    let binary = converter::ffprobe_path().ok_or_else(|| {
        AppError::Other("ffprobe is needed to read where the cuts can land".into())
    })?;
    let args: Vec<String> = vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "v:0".into(),
        // Decoding only the keyframes is what makes this fast enough to ask
        // for: everything between them is skipped without being read.
        "-skip_frame".into(),
        "nokey".into(),
        "-show_entries".into(),
        "frame=pts_time".into(),
        "-of".into(),
        "csv=p=0".into(),
        path.to_owned(),
    ];

    let output = process::run(&binary, &args).await?;
    if !output.success() {
        return Err(AppError::Other(format!(
            "the keyframes could not be read: {}",
            output.stderr.lines().next().unwrap_or("no detail").trim()
        )));
    }

    let times = parse_keyframes(&output.stdout);
    if let Some((size, modified_ms)) = print {
        if let Ok(mut cache) = KEYFRAMES.lock() {
            // One file at a time is open in the editor, so the map never needs
            // to hold more than the one it was last asked about.
            cache.clear();
            cache.insert(
                path.to_owned(),
                Remembered {
                    size,
                    modified_ms,
                    times: Arc::new(times.clone()),
                },
            );
        }
    }
    Ok(times)
}

fn remembered(path: &str) -> Option<Remembered> {
    KEYFRAMES.lock().ok()?.get(path).cloned()
}

/// Size and modification time, which together are what make a file the same
/// file. A file whose metadata will not be read is simply never remembered.
fn fingerprint(path: &Path) -> Option<(u64, i64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((metadata.len(), modified))
}

/// One time per line.
///
/// The first row comes back as `0.000000,` on this build -- `csv=p=0` prints a
/// trailing separator for the first frame and not for the rest -- so the line
/// is cut at the first separator rather than parsed whole.
fn parse_keyframes(stdout: &str) -> Vec<f64> {
    let mut times: Vec<f64> = stdout
        .lines()
        .filter_map(|line| line.split(',').next())
        .map(str::trim)
        .filter_map(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    times
}

/// Move a start back to the keyframe at or before it.
///
/// The tolerance is there because a mark the interface took from this same list
/// arrives as the nearest double rather than the exact one, and a mark that is
/// a keyframe must not be pushed back to the one before it.
fn snap_to_keyframe(start: f64, keyframes: &[f64]) -> f64 {
    keyframes
        .iter()
        .rev()
        .find(|mark| **mark <= start + 0.001)
        .copied()
        .unwrap_or(start)
}

/// How far past a keyframe an input seek is aimed.
///
/// FFmpeg backs a seek up by 3/23 s before handing it to a demuxer that cannot
/// seek by presentation time -- Matroska and WebM -- when the stream has
/// B-frames, so a `-ss` landing exactly on a keyframe resolves to the keyframe
/// before it and a whole group of pictures the user cut out comes back:
/// measured, 8.53 s of marked film arriving as 16.87 s. A third of a second
/// would be safe for every container; 0.15 s is enough for the backoff and
/// small enough to stay inside the short groups an NTSC file has.
const SEEK_LEAD_SEC: f64 = 0.15;

/// Where to aim the input seek for a range that starts on a keyframe.
///
/// A seek inside the group of pictures still resolves backwards to the keyframe
/// that opens it, so nothing is lost by aiming late; aiming exactly costs a
/// whole group. The lead-in is halved against the room there actually is, so a
/// range or a group shorter than it degrades to today's behaviour rather than
/// skipping past the keyframe it was meant to land on.
fn seek_for(start: f64, end: f64, keyframes: &[f64]) -> f64 {
    // Only a start that is itself a keyframe can be aimed past. Anywhere else
    // -- a mark before the first keyframe, or a file with no keyframes to read
    // at all, which is what an audio-only export is -- moving the seek forward
    // would skip whatever lies between and lose film the user marked.
    if !keyframes.iter().any(|mark| (mark - start).abs() <= 0.001) {
        return start;
    }
    let next = keyframes
        .iter()
        .copied()
        .find(|mark| *mark > start + 0.001)
        .unwrap_or(end)
        .min(end);
    let room = ((next - start) * 0.5).max(0.0);
    start + room.min(SEEK_LEAD_SEC)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(start: f64, end: f64) -> EditSegment {
        EditSegment {
            start_sec: start,
            end_sec: end,
        }
    }

    fn probe_of(container: &str, video: &str, audio: &str) -> MediaProbe {
        MediaProbe {
            path: format!("in.{container}"),
            file_name: format!("in.{container}"),
            container: container.into(),
            size_bytes: 1024,
            duration_sec: Some(60.0),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30.0),
            pixel_aspect: Some(1.0),
            video_duration_sec: Some(60.0),
            video_codec: Some(video.into()),
            audio_codec: Some(audio.into()),
            audio_bitrate_kbps: Some(192.0),
            has_video: true,
            has_audio: true,
        }
    }

    #[test]
    fn stamps_a_whole_second_without_a_decimal() {
        assert_eq!(stamp(0.0), "0m00s");
        assert_eq!(stamp(9.0), "0m09s");
        assert_eq!(stamp(61.0), "1m01s");
        assert_eq!(stamp(3600.0), "60m00s");
    }

    #[test]
    fn keeps_the_tenth_when_it_is_the_difference() {
        assert_eq!(stamp(12.4), "0m12.4s");
        assert_eq!(stamp(72.5), "1m12.5s");
    }

    #[test]
    fn a_stamp_never_needs_sanitising() {
        for value in [0.0, 1.5, 59.9, 60.0, 4321.25] {
            let text = stamp(value);
            assert_eq!(filename::sanitize_component(&text), text);
        }
    }

    #[test]
    fn nothing_marked_is_refused() {
        assert!(plan_segments(&[], Some(60.0)).is_err());
    }

    #[test]
    fn a_range_with_no_length_or_the_wrong_way_round_is_refused() {
        assert!(plan_segments(&[segment(10.0, 10.0)], Some(60.0)).is_err());
        assert!(plan_segments(&[segment(10.0, 10.02)], Some(60.0)).is_err());
        assert!(plan_segments(&[segment(10.0, 4.0)], Some(60.0)).is_err());
        assert!(plan_segments(&[segment(f64::NAN, 4.0)], Some(60.0)).is_err());
    }

    #[test]
    fn a_range_that_starts_after_the_file_ends_is_refused() {
        assert!(plan_segments(&[segment(60.0, 70.0)], Some(60.0)).is_err());
        assert!(plan_segments(&[segment(80.0, 90.0)], Some(60.0)).is_err());
    }

    #[test]
    fn a_range_that_runs_past_the_end_is_clamped_to_it() {
        let kept = plan_segments(&[segment(50.0, 90.0)], Some(60.0)).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].end_sec, 60.0);

        // Nothing to clamp against leaves the mark as it was asked for.
        let kept = plan_segments(&[segment(50.0, 90.0)], None).unwrap();
        assert_eq!(kept[0].end_sec, 90.0);
    }

    #[test]
    fn ranges_are_put_in_order_and_the_ones_that_meet_are_joined() {
        let kept = plan_segments(
            &[segment(20.0, 25.0), segment(1.0, 3.0), segment(2.5, 8.0)],
            Some(60.0),
        )
        .unwrap();
        assert_eq!(kept.len(), 2);
        assert_eq!((kept[0].start_sec, kept[0].end_sec), (1.0, 8.0));
        assert_eq!((kept[1].start_sec, kept[1].end_sec), (20.0, 25.0));
    }

    #[test]
    fn a_range_wholly_inside_another_disappears_into_it() {
        let kept = plan_segments(&[segment(1.0, 20.0), segment(5.0, 6.0)], Some(60.0)).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].start_sec, kept[0].end_sec), (1.0, 20.0));
    }

    #[test]
    fn ranges_that_only_touch_become_one() {
        let kept = plan_segments(&[segment(1.0, 4.0), segment(4.0, 9.0)], Some(60.0)).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].start_sec, kept[0].end_sec), (1.0, 9.0));
    }

    #[test]
    fn a_copy_refuses_everything_it_cannot_actually_do() {
        let probe = probe_of("mp4", "h264", "aac");
        let base = ExportOptions {
            mode: ExportMode::Lossless,
            container: "mp4".into(),
            ..ExportOptions::default()
        };
        assert!(lossless_is_possible(&base, &probe).is_ok());

        // The sharpest of them: a non-square pixel cannot be squared by a copy,
        // so the button would hand back a picture the wrong shape.
        assert!(lossless_is_possible(
            &ExportOptions {
                aspect: AspectRatio::Widescreen,
                ..base.clone()
            },
            &probe
        )
        .is_err());
        assert!(lossless_is_possible(
            &ExportOptions {
                max_height: Some(720),
                ..base.clone()
            },
            &probe
        )
        .is_err());
        assert!(lossless_is_possible(
            &ExportOptions {
                fps: Some(25.0),
                ..base.clone()
            },
            &probe
        )
        .is_err());

        // WebM has no place for H.264, whatever the mode says.
        assert!(lossless_is_possible(
            &ExportOptions {
                container: "webm".into(),
                ..base
            },
            &probe
        )
        .is_err());
    }

    #[test]
    fn a_copy_refuses_exactly_the_volumes_a_re_encode_would_apply() {
        let probe = probe_of("mp4", "h264", "aac");
        let base = ExportOptions {
            mode: ExportMode::Lossless,
            container: "mp4".into(),
            ..ExportOptions::default()
        };
        // Louder, quieter and silent all mean decoding the sound.
        for volume in [1.5, 0.5, 0.0, 2.0] {
            let options = ExportOptions {
                volume,
                ..base.clone()
            };
            let refused = lossless_is_possible(&options, &probe).unwrap_err();
            assert!(refused.to_string().contains("volume"), "{volume}: {refused}");
        }
        // Unity, and the rounding a slider leaves near it, are a copy's to keep.
        for volume in [1.0, 1.004, 0.996] {
            let options = ExportOptions {
                volume,
                ..base.clone()
            };
            assert!(lossless_is_possible(&options, &probe).is_ok(), "{volume}");
        }
        // A chosen bitrate is simply not a copy's concern: nothing is encoded
        // for it to apply to, so it is not a reason to refuse one.
        let options = ExportOptions {
            video_bitrate_kbps: Some(4000),
            ..base
        };
        assert!(lossless_is_possible(&options, &probe).is_ok());
    }

    #[test]
    fn a_copy_can_leave_the_sound_behind() {
        let base = ExportOptions {
            mode: ExportMode::Lossless,
            container: "mp4".into(),
            mute: true,
            ..ExportOptions::default()
        };
        let probe = probe_of("mp4", "h264", "aac");
        assert!(lossless_is_possible(&base, &probe).is_ok());

        // A volume the slider still holds is not applied to a track that is
        // not written, by a re-encode either, so it rules nothing out.
        let louder = ExportOptions {
            volume: 1.5,
            ..base.clone()
        };
        assert!(lossless_is_possible(&louder, &probe).is_ok());
        assert!(lossless_is_possible(
            &ExportOptions {
                mute: false,
                ..louder
            },
            &probe
        )
        .is_err());

        // Nor is a sound the container could not have held: Vorbis has no
        // place in an MP4, and it is not going into one.
        let vorbis = probe_of("mp4", "h264", "vorbis");
        assert!(lossless_is_possible(&base, &vorbis).is_ok());
        assert!(lossless_is_possible(
            &ExportOptions {
                mute: false,
                ..base.clone()
            },
            &vorbis
        )
        .is_err());

        // Whatever happens to the sound, the picture still has to fit.
        assert!(lossless_is_possible(
            &ExportOptions {
                container: "webm".into(),
                ..base
            },
            &probe
        )
        .is_err());
    }

    #[test]
    fn a_silent_source_has_no_volume_to_refuse() {
        let options = ExportOptions {
            mode: ExportMode::Lossless,
            container: "mp4".into(),
            volume: 0.5,
            ..ExportOptions::default()
        };
        let silent = MediaProbe {
            audio_codec: None,
            audio_bitrate_kbps: None,
            has_audio: false,
            ..probe_of("mp4", "h264", "aac")
        };
        assert!(lossless_is_possible(&options, &silent).is_ok());
    }

    #[test]
    fn a_rate_and_a_gain_are_held_to_what_ffmpeg_will_take() {
        let checked = |bitrate: Option<u32>, volume: f64| {
            checked_options(ExportOptions {
                video_bitrate_kbps: bitrate,
                volume,
                ..ExportOptions::default()
            })
            .unwrap()
        };

        assert_eq!(checked(None, 1.0).video_bitrate_kbps, None);
        assert_eq!(checked(Some(8000), 1.0).video_bitrate_kbps, Some(8000));
        assert_eq!(checked(Some(0), 1.0).video_bitrate_kbps, Some(MIN_VIDEO_KBPS));
        assert_eq!(checked(Some(99), 1.0).video_bitrate_kbps, Some(100));
        assert_eq!(
            checked(Some(u32::MAX), 1.0).video_bitrate_kbps,
            Some(MAX_VIDEO_KBPS)
        );

        assert_eq!(checked(None, 0.75).volume, 0.75);
        assert_eq!(checked(None, 0.0).volume, 0.0);
        assert_eq!(checked(None, -3.0).volume, 0.0);
        assert_eq!(checked(None, 7.0).volume, 2.0);
        // Not a number is no change, rather than a graph FFmpeg would refuse.
        assert_eq!(checked(None, f64::NAN).volume, 1.0);
        assert_eq!(checked(None, f64::INFINITY).volume, 1.0);
        assert_eq!(checked(None, f64::NEG_INFINITY).volume, 1.0);
    }

    /// Options as the interface sent them before it had a bitrate or a volume
    /// to send.
    const OPTIONS_WITHOUT_RATE_OR_GAIN: &str = r#"{
        "mode": "reencode",
        "container": "mp4",
        "videoCodec": "h264",
        "quality": "balanced",
        "fps": null,
        "maxHeight": null,
        "aspect": "source",
        "fit": "fill",
        "mute": false,
        "audioCodec": "aac",
        "audioBitrateKbps": 192,
        "toneMapSdr": false,
        "hardware": true
    }"#;

    #[test]
    fn options_from_an_older_interface_still_arrive_and_change_nothing() {
        let options: ExportOptions = serde_json::from_str(OPTIONS_WITHOUT_RATE_OR_GAIN).unwrap();
        assert_eq!(options.video_bitrate_kbps, None);
        assert_eq!(options.volume, 1.0);
        assert_eq!(options.gain(), None);
        // The fields it did send are still read.
        assert_eq!(options.mode, ExportMode::Reencode);
        assert!(options.hardware);
    }

    #[test]
    fn a_rate_and_a_gain_come_through_under_the_names_the_interface_uses() {
        let mut value: serde_json::Value = serde_json::from_str(OPTIONS_WITHOUT_RATE_OR_GAIN).unwrap();
        value["videoBitrateKbps"] = serde_json::json!(6000);
        value["volume"] = serde_json::json!(0.4);
        let options: ExportOptions = serde_json::from_value(value).unwrap();
        assert_eq!(options.video_bitrate_kbps, Some(6000));
        assert_eq!(options.volume, 0.4);

        // And back out again unchanged, under the same names.
        let echoed = serde_json::to_value(&options).unwrap();
        assert_eq!(echoed["videoBitrateKbps"], serde_json::json!(6000));
        assert_eq!(echoed["volume"], serde_json::json!(0.4));
        let again: ExportOptions = serde_json::from_value(echoed).unwrap();
        assert_eq!(again.video_bitrate_kbps, Some(6000));
        assert_eq!(again.volume, 0.4);

        // An explicit null is the interface saying "use the quality instead".
        let mut value: serde_json::Value = serde_json::from_str(OPTIONS_WITHOUT_RATE_OR_GAIN).unwrap();
        value["videoBitrateKbps"] = serde_json::Value::Null;
        let options: ExportOptions = serde_json::from_value(value).unwrap();
        assert_eq!(options.video_bitrate_kbps, None);
    }

    #[test]
    fn only_containers_this_app_writes_are_accepted() {
        for container in ["mp4", "MKV", " mov ", "webm"] {
            let options = ExportOptions {
                container: container.into(),
                ..ExportOptions::default()
            };
            let checked = checked_options(options).expect(container);
            assert_eq!(checked.container, container.trim().to_ascii_lowercase());
        }
        for container in ["exe", "../evil", "", "avi"] {
            let options = ExportOptions {
                container: container.into(),
                ..ExportOptions::default()
            };
            assert!(checked_options(options).is_err(), "{container}");
        }
    }

    #[test]
    fn a_start_is_moved_back_to_the_keyframe_at_or_before_it() {
        let marks = [0.0, 1.6, 3.2, 4.8];
        assert_eq!(snap_to_keyframe(0.0, &marks), 0.0);
        assert_eq!(snap_to_keyframe(2.0, &marks), 1.6);
        assert_eq!(snap_to_keyframe(3.2, &marks), 3.2);
        assert_eq!(snap_to_keyframe(9.0, &marks), 4.8);
        // Nothing to snap to leaves the mark alone rather than moving it to zero.
        assert_eq!(snap_to_keyframe(2.0, &[]), 2.0);
        assert_eq!(snap_to_keyframe(1.0, &[5.0]), 1.0);
    }

    #[test]
    fn a_seek_aims_inside_the_group_of_pictures_rather_than_at_its_edge() {
        // A seek landing exactly on a keyframe resolves to the keyframe before
        // it on Matroska and WebM, because FFmpeg backs the target up by 3/23 s
        // first: measured, a 8.53 s cut arriving as 16.87 s from an .mkv, and
        // the same marks on the same content in an .mp4 coming out right.
        let marks = [0.0, 8.333333, 16.666667, 25.0];
        let seek = seek_for(8.333333, 16.666667, &marks);
        assert!(
            seek > 8.333333 + 0.13 && seek < 16.666667,
            "{seek} has to clear the backoff and stay inside the group"
        );

        // Half a group is the ceiling, so a short one is not overshot. An NTSC
        // file's groups are a third of a second.
        let marks = [0.0, 0.333667, 0.667333, 1.001];
        let seek = seek_for(0.333667, 1.001, &marks);
        assert!(seek > 0.333667 && seek < 0.667333, "{seek}");

        // A range that ends before the next keyframe is bounded by its own end,
        // never seeked past it.
        assert!(seek_for(1.0, 1.05, &[0.0, 1.0, 9.0]) < 1.05);
        // The last group has no keyframe after it to aim before.
        let seek = seek_for(25.0, 30.0, &[0.0, 8.333333, 16.666667, 25.0]);
        assert!(seek > 25.0 && seek < 30.0, "{seek}");
        // A file with no keyframes to read is an audio-only one, and a seek
        // moved forward there would simply cut the first moment off.
        assert_eq!(seek_for(2.0, 12.0, &[]), 2.0);
        // A mark before the first keyframe is not on one, so it stays put.
        assert_eq!(seek_for(0.0, 5.0, &[0.04, 8.0]), 0.0);
    }

    #[test]
    fn a_frame_is_measured_after_its_pixels_are_squared() {
        // A 1440x1080 master with a 4:3 pixel is a 1920x1080 picture, and a
        // 720x576 one with a 64:45 pixel is 1024x576. Measured as stored, the
        // 16:9 button shrank both.
        assert_eq!(square_pixels(1440, 1080, 4.0 / 3.0), (1920, 1080));
        assert_eq!(square_pixels(720, 576, 64.0 / 45.0), (1024, 576));
        // A tall pixel heightens the frame instead of narrowing it, so no
        // detail is thrown away to make the shape right.
        assert_eq!(square_pixels(1920, 1080, 0.5), (1920, 2160));
        // Square pixels, and a ratio nothing could be read from, are both the
        // frame as it stands.
        assert_eq!(square_pixels(1920, 1080, 1.0), (1920, 1080));
        assert_eq!(square_pixels(1920, 1080, f64::NAN), (1920, 1080));
        assert_eq!(square_pixels(1920, 1080, 0.0), (1920, 1080));
        // Both sides come back even, which is all libx264 will take.
        let (width, height) = square_pixels(711, 479, 1.0);
        assert_eq!((width % 2, height % 2), (0, 0));

        // Every ratio a real file states, against the frame it states it for.
        // One pixel short here drops the export a whole tier, so the rounding
        // is checked and not assumed.
        for (width, height, num, den, want) in [
            (1440u32, 1080u32, 4u32, 3u32, (1920u32, 1080u32)),
            (720, 576, 64, 45, (1024, 576)),
            // NTSC widescreen: 853.33 columns, and the even one below it is
            // what the graph's own `trunc(.../2)*2` produces too.
            (720, 480, 32, 27, (852, 480)),
            (1280, 1080, 3, 2, (1920, 1080)),
            (960, 720, 4, 3, (1280, 720)),
        ] {
            let sar = f64::from(num) / f64::from(den);
            assert_eq!(
                square_pixels(width, height, sar),
                want,
                "{width}x{height} with a {num}:{den} pixel"
            );
        }
    }

    #[test]
    fn an_export_that_was_replaced_leaves_the_newer_one_alone() {
        let control: Mutex<Option<Arc<TaskControl>>> = Mutex::new(None);
        let first = Arc::new(TaskControl::new());
        *control.lock().unwrap() = Some(Arc::clone(&first));

        // The ordinary end: the handle was ours and it is cleared.
        assert!(reclaim(&control, &first));
        assert!(control.lock().unwrap().is_none());

        // A press meaning "this one instead" cancels the first and installs
        // its own handle; the first only notices on its next poll, and by then
        // the screen and the output file belong to the second.
        let second = Arc::new(TaskControl::new());
        *control.lock().unwrap() = Some(Arc::clone(&second));
        assert!(!reclaim(&control, &first));
        assert!(
            control.lock().unwrap().is_some(),
            "the live export has to stay cancellable"
        );
        assert!(reclaim(&control, &second));
    }

    #[test]
    fn snapping_can_fold_two_ranges_into_one() {
        let marks = [0.0, 1.6, 3.2];
        let mut kept = plan_segments(&[segment(1.8, 2.2), segment(2.4, 3.0)], Some(60.0)).unwrap();
        for segment in &mut kept {
            segment.start_sec = snap_to_keyframe(segment.start_sec, &marks);
        }
        let kept = merge_overlaps(kept);
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].start_sec, kept[0].end_sec), (1.6, 3.0));
    }

    #[test]
    fn a_keyframe_list_survives_the_trailing_separator_on_the_first_row() {
        let times = parse_keyframes("0.000000,\n1.600000\n3.200000\n\n");
        assert_eq!(times, vec![0.0, 1.6, 3.2]);
    }

    #[test]
    fn a_keyframe_list_comes_back_in_order_and_without_repeats() {
        let times = parse_keyframes("3.2\n1.6\n1.6\nN/A\n-1\n0.0\n");
        assert_eq!(times, vec![0.0, 1.6, 3.2]);
    }

    #[test]
    fn only_a_genuinely_bright_source_is_tone_mapped() {
        assert!(is_hdr_transfer("smpte2084"));
        assert!(is_hdr_transfer("arib-std-b67"));
        assert!(!is_hdr_transfer("bt709"));
        assert!(!is_hdr_transfer("unknown"));
        assert!(!is_hdr_transfer(""));
    }

    #[test]
    fn nothing_outside_the_scratch_directory_is_ever_joined() {
        let scratch = std::env::temp_dir().join("ud-export-tests");
        let _ = std::fs::create_dir_all(&scratch);
        let elsewhere = scratch.join("elsewhere");
        let _ = std::fs::create_dir_all(&elsewhere);

        let stray = elsewhere.join("someone-elses.ts");
        std::fs::write(&stray, b"not ours").unwrap();
        assert!(ensure_produced(&[stray], &scratch).is_err());

        let empty = scratch.join("empty.ts");
        std::fs::write(&empty, b"").unwrap();
        assert!(ensure_produced(std::slice::from_ref(&empty), &scratch).is_err());

        std::fs::write(&empty, b"something").unwrap();
        assert!(ensure_produced(std::slice::from_ref(&empty), &scratch).is_ok());
    }

    #[test]
    fn the_name_carries_the_span_that_was_kept() {
        let dir = std::env::temp_dir().join("ud-export-name");
        let _ = std::fs::create_dir_all(&dir);
        let options = ExportOptions {
            container: "mp4".into(),
            ..ExportOptions::default()
        };
        let path = output_path(
            &PathBuf::from("C:\\films\\holiday.mkv"),
            &dir,
            &options,
            &[segment(4.8, 8.0), segment(61.0, 72.5)],
        )
        .unwrap();

        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("holiday 0m04.8s-1m12.5s"), "{name}");
        assert!(name.ends_with(".mp4"), "{name}");
        assert_eq!(path.parent(), Some(dir.as_path()));
    }

    #[test]
    fn an_export_lands_beside_its_source_unless_that_is_out_of_sight() {
        let source = Path::new("/data/user/0/app/cache/picked/holiday.mp4");
        let downloads = "/storage/emulated/0/Download/Universal Downloader";

        // The desktop: beside the source, as it always was.
        assert_eq!(
            export_dir(source, None, None).unwrap(),
            PathBuf::from("/data/user/0/app/cache/picked")
        );
        // The phone: the source is a private copy, so the export goes where a
        // download would, and never into the cache the system empties.
        assert_eq!(
            export_dir(source, None, Some(downloads)).unwrap(),
            PathBuf::from(downloads)
        );
        // A folder the user chose wins on both, and blank is not a choice.
        assert_eq!(
            export_dir(source, Some(" /sdcard/Movies "), Some(downloads)).unwrap(),
            PathBuf::from("/sdcard/Movies")
        );
        assert_eq!(
            export_dir(source, Some("  "), Some(downloads)).unwrap(),
            PathBuf::from(downloads)
        );
        assert_eq!(
            export_dir(source, Some(""), None).unwrap(),
            PathBuf::from("/data/user/0/app/cache/picked")
        );
        // Nothing to fall back on is still an error rather than a guess.
        assert!(export_dir(Path::new("/"), None, None).is_err());
    }

    #[test]
    fn a_desktop_export_of_the_apps_own_file_goes_where_downloads_go() {
        let root = Path::new("/Users/someone/AppData/Roaming/UniversalDownloader");
        let downloads = "/Users/someone/Downloads";
        let decide = |source: &Path, chosen: Option<&str>| {
            let shared = out_of_reach(source, false, Some(root)).then_some(downloads);
            export_dir(source, chosen, shared).unwrap()
        };
        let fetched = root.join("temp").join("holiday 0m00s-0m12s.mp4");

        // A clip fetched from a link sits in the temporary folder the sweep
        // empties, so its export must not be written beside it.
        assert_eq!(decide(&fetched, None), PathBuf::from(downloads));
        assert_eq!(decide(&root.join("cache").join("x.mp4"), None), PathBuf::from(downloads));
        // A file of the user's own stays where it is.
        assert_eq!(
            decide(Path::new("/Users/someone/Videos/holiday.mp4"), None),
            PathBuf::from("/Users/someone/Videos")
        );
        // A folder that only begins with the same name is somewhere else.
        assert_eq!(
            decide(
                Path::new("/Users/someone/AppData/Roaming/UniversalDownloader Backup/a.mp4"),
                None
            ),
            PathBuf::from("/Users/someone/AppData/Roaming/UniversalDownloader Backup")
        );
        // A folder the user chose is still the answer, even for the app's own file.
        assert_eq!(decide(&fetched, Some("/Volumes/Cuts")), PathBuf::from("/Volumes/Cuts"));
        // No app folder to compare with leaves the desktop as it was.
        assert!(!out_of_reach(&fetched, false, None));
        // And a phone never writes beside a source, wherever it is.
        assert!(out_of_reach(Path::new("/storage/emulated/0/DCIM/a.mp4"), true, None));
    }

    #[test]
    fn the_apps_folder_is_matched_by_component_and_on_windows_by_any_case() {
        let root = Path::new("/Users/someone/AppData/Roaming/UniversalDownloader");
        assert!(lies_within(&root.join("temp").join("a.mp4"), root));
        assert!(!lies_within(Path::new("/Users/someone/AppData/Roaming/a.mp4"), root));
        assert!(!lies_within(
            Path::new("/Users/someone/AppData/Roaming/UniversalDownloaderX/a.mp4"),
            root
        ));
        if cfg!(windows) {
            // What a file dialog or the webview hands back is not always spelt
            // the way the app spelt its own folder.
            let root = Path::new(r"C:\Users\someone\AppData\Roaming\UniversalDownloader");
            assert!(lies_within(
                Path::new("c:/users/someone/appdata/roaming/universaldownloader/temp/a.mp4"),
                root
            ));
            assert!(!lies_within(
                Path::new("D:/Users/someone/AppData/Roaming/UniversalDownloader/a.mp4"),
                root
            ));
        }
    }

    #[test]
    fn a_range_this_app_wrote_is_taken_off_the_end_of_a_name() {
        assert_eq!(without_range("holiday 0m00s-0m12s"), "holiday");
        assert_eq!(without_range("holiday 0m00s-0m05.2s"), "holiday");
        assert_eq!(without_range("holiday 1m12.5s-60m00s"), "holiday");
        // With the "(2)" a collision added after it, which is not the new file's.
        assert_eq!(without_range("holiday 0m04.8s-1m12.5s (2)"), "holiday");
        // Only at the end, only in this app's spelling, and never the whole name.
        assert_eq!(without_range("holiday"), "holiday");
        assert_eq!(without_range("holiday (2)"), "holiday (2)");
        assert_eq!(without_range("holiday 0m00s-0m12s extended"), "holiday 0m00s-0m12s extended");
        assert_eq!(without_range("holiday 12s-20s"), "holiday 12s-20s");
        assert_eq!(without_range("holiday 0:00-0:12"), "holiday 0:00-0:12");
        assert_eq!(without_range("0m00s-0m12s"), "0m00s-0m12s");
    }

    #[test]
    fn an_export_of_an_export_replaces_the_range_rather_than_adding_one() {
        let dir = std::env::temp_dir().join("ud-export-restamp");
        let _ = std::fs::create_dir_all(&dir);
        let options = ExportOptions {
            container: "mp4".into(),
            ..ExportOptions::default()
        };
        let path = output_path(
            &PathBuf::from("C:\\films\\holiday 0m00s-0m12s.mp4"),
            &dir,
            &options,
            &[segment(0.0, 5.2)],
        )
        .unwrap();

        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("holiday 0m00s-0m05.2s"), "{name}");
        assert!(!name.contains("0m12s"), "{name}");
    }

    #[test]
    fn a_running_export_is_the_only_state_that_counts_as_work() {
        assert!(ExportStatus::Running.is_active());
        for status in [
            ExportStatus::Idle,
            ExportStatus::Completed,
            ExportStatus::Failed,
            ExportStatus::Canceled,
        ] {
            assert!(!status.is_active(), "{status:?}");
        }
    }
}
