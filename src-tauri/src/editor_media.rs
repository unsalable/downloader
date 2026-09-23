//! What the editor's timeline is drawn from: the peaks of the audio and a
//! strip of frames.
//!
//! Both are read out of the file by FFmpeg rather than by the window. A
//! `<video>` element cannot be asked for a frame it is not currently showing,
//! and it cannot be asked for the audio at all, so a timeline built from what
//! the webview can reach would be blank for exactly the files the editor earns
//! its place on.
//!
//! What this module owns is the two passes, the cache they land in and the
//! order they run in. It owns none of the editing: nothing here knows where a
//! cut falls, and a strip describes the file as it sits on disk rather than the
//! edit in front of the user. It is also not a queue. One file is open at a
//! time and a zoom supersedes the zoom before it, so there is one running
//! request and a short list of what the view currently on screen still wants.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::downloader::control::TaskControl;
use crate::error::{AppError, AppResult};
use crate::model::{
    FilmstripData, MediaProbe, TimelineKind, TimelineRequest, TimelineState, WaveformData,
};
use crate::settings::Settings;
use crate::{cache, converter, log_warn, paths, process, tools, util};

pub const EVENT_CHANGED: &str = "editor://timeline";

/// How much of the raw sample stream is taken at once. The figure is not
/// critical; what matters is that a read can end in the middle of a sample,
/// which is why [`Peaks`] carries a byte over.
const READ_BYTES: usize = 64 * 1024;

/// The byte a bucket with nothing in it is drawn at: the middle of the range,
/// which is where a silent sample sits.
const SILENCE: u8 = 128;

/// Cells to a sprite. Measured on a four-minute file: forty cells as five
/// sprites cost 637 ms and 56,593 bytes against 621 ms and 55,249 bytes in one
/// sprite -- three percent of the time and two of the bytes, in exchange for
/// the strip filling from the left rather than appearing all at once.
const CELLS_PER_CHUNK: u32 = 8;

/// The shortest step between cells that is worth decoding only keyframes for.
///
/// `-skip_frame nokey` is between two and four times faster -- measured, a
/// four-minute 1080p file tiles in 621 ms against 2023 ms -- but it can only
/// ever show the keyframes there are. Measured on a file with a keyframe every
/// two seconds, forty cells over twenty seconds came back as nine distinct
/// pictures out of forty; the same forty cells over four minutes came back as
/// forty. Below this step the file is short enough that decoding all of it is
/// cheap anyway: the same twenty-second file decodes in full in 109 ms.
const KEYFRAME_STEP_SEC: f64 = 5.0;

/// Ceilings on what the interface may ask for. Neither is a policy about what
/// is useful; they are there so a number that arrived wrong cannot turn into
/// an allocation or a command line nobody intended.
const MAX_BUCKETS: u32 = 16_384;
const MAX_CELLS: u32 = 400;
const MIN_CELL_HEIGHT: u32 = 8;
const MAX_CELL_HEIGHT: u32 = 320;
const MAX_FRAME_HEIGHT: u32 = 2160;

/// What the timeline asks for when the interface names no cell height.
const DEFAULT_CELL_HEIGHT: u32 = 68;

/// Square the pixels before anything is measured off the frame.
///
/// The same expression the export uses, and for the same reason: a source
/// stored 720x576 with a 64:45 pixel is a 16:9 picture, and without this its
/// cells come out 86 by 68 -- 1.26:1, where the picture is 1.78:1. Measured,
/// the normaliser puts them back at 120 by 68 and costs nothing at all: 642 ms
/// against 635 ms over four minutes. A source whose pixel shape is unstated
/// passes through it unchanged, because the filter reads an absent sample
/// aspect as 1:1.
const SQUARE_PIXELS: &str = "scale=w='trunc(iw*max(1,sar)/2)*2':h='trunc(ih/min(1,sar)/2)*2',setsar=1";

/// Fill the end of a short strip with the last real frame rather than black.
///
/// Costs nothing and removes the worst-looking failure this pass has: measured
/// on a 26-second container holding 20 seconds of picture, the same tiling
/// without this ends in eleven pure black cells.
const TAIL_PAD: &str = "tpad=stop=-1:stop_mode=clone";

// -- the manager ------------------------------------------------------------

pub struct TimelineManager {
    app: AppHandle,
    settings: Arc<Mutex<Settings>>,
    state: Mutex<TimelineState>,
    inner: Mutex<Inner>,
}

/// What the view currently on screen still wants drawn, and what is drawing it.
#[derive(Default)]
struct Inner {
    /// Requests for the current token, in the order they arrived, run one after
    /// another. Running them together is not worth it -- measured, 2907 ms
    /// against 2995 ms, three percent for twice the peak memory -- and the
    /// order is worth a great deal: the interface asks for the waveform first,
    /// and on a file whose picture will not decode the marks are still
    /// meaningful against a waveform and meaningless against nothing.
    queue: VecDeque<TimelineRequest>,
    path: Option<String>,
    token: u64,
    running: bool,
    control: Option<Arc<TaskControl>>,
}

impl TimelineManager {
    pub fn new(app: AppHandle, settings: Arc<Mutex<Settings>>) -> Arc<Self> {
        Arc::new(Self {
            app,
            settings,
            state: Mutex::new(TimelineState::default()),
            inner: Mutex::new(Inner::default()),
        })
    }

    pub fn state(&self) -> TimelineState {
        self.state
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Replace the published state and tell the interface about it. Every
    /// change goes through here or through [`Self::publish_for`], so the screen
    /// cannot drift from the truth.
    fn publish(&self, update: impl FnOnce(&mut TimelineState)) {
        let next = {
            let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
            update(&mut guard);
            guard.clone()
        };
        let _ = self.app.emit(EVENT_CHANGED, &next);
    }

    /// Publish, unless the screen has moved on.
    ///
    /// This is the whole point of the token. A strip of a four-minute file
    /// takes most of a second; a zoom during that second asks for a different
    /// window, and the older answer arriving afterwards would paint the wrong
    /// picture over the right one and look exactly like a bug in the zoom.
    fn publish_for(&self, token: u64, update: impl FnOnce(&mut TimelineState)) {
        let next = {
            let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
            if guard.token != token {
                return;
            }
            update(&mut guard);
            guard.clone()
        };
        let _ = self.app.emit(EVENT_CHANGED, &next);
    }

    /// Ask for one thing to be drawn.
    ///
    /// A request naming the token and file already on screen joins the list for
    /// that view. Anything else is a new view, and a new view supersedes the
    /// old one outright: whatever is running is drawing a window nobody is
    /// looking at any more.
    pub fn request(self: &Arc<Self>, request: TimelineRequest) {
        let start = {
            // The published state is changed while this lock is held, so that a
            // request arriving as the worker runs dry cannot have its "working"
            // overwritten by the worker's "finished".
            let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());

            let superseded = inner.token != request.token
                || inner.path.as_deref() != Some(request.path.as_str());
            if superseded {
                if let Some(control) = inner.control.take() {
                    control.cancel();
                }
                inner.queue.clear();
                inner.token = request.token;
                inner.path = Some(request.path.clone());

                let path = request.path.clone();
                let token = request.token;
                self.publish(|state| {
                    *state = TimelineState {
                        path: Some(path),
                        token,
                        working: true,
                        waveform: None,
                        filmstrip: None,
                        error: None,
                    };
                });
            } else {
                self.publish(|state| state.working = true);
            }

            inner.queue.push_back(request);
            let start = !inner.running;
            inner.running = true;
            start
        };

        if start {
            let manager = Arc::clone(self);
            tauri::async_runtime::spawn(manager.work());
        }
    }

    /// Stop drawing and forget what was still to be drawn. The interface calls
    /// this when the editor closes the file, which is the one moment when the
    /// answer to every outstanding question has become "nobody asked".
    pub fn cancel(&self) {
        let control = {
            let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
            inner.queue.clear();
            inner.control.take()
        };
        if let Some(control) = control {
            control.cancel();
        }
    }

    pub fn shutdown(&self) {
        self.cancel();
    }

    async fn work(self: Arc<Self>) {
        loop {
            let (request, control) = {
                let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
                let Some(request) = inner.queue.pop_front() else {
                    inner.running = false;
                    inner.control = None;
                    self.publish(|state| state.working = false);
                    break;
                };
                let control = Arc::new(TaskControl::new());
                inner.control = Some(Arc::clone(&control));
                (request, control)
            };

            let token = request.token;
            match self.draw(&request, &control).await {
                Ok(()) => {}
                // Superseded, or the editor closed the file. Neither is
                // something the user did wrong, and neither has a result.
                Err(AppError::Canceled) => {}
                Err(err) => {
                    log_warn!("editor", "the timeline could not be drawn: {err}");
                    let info = err.to_info();
                    self.publish_for(token, move |state| state.error = Some(info));
                }
            }
        }
    }

    async fn draw(&self, request: &TimelineRequest, control: &TaskControl) -> AppResult<()> {
        // Nothing below works without it, and this is a far clearer way to
        // learn that than a failure to spawn a process.
        tools::require_ffmpeg()?;
        let probe = converter::probe(&request.path).await?;

        match request.kind {
            TimelineKind::Waveform => {
                let data = self.waveform(request, control, &probe).await?;
                self.publish_for(request.token, move |state| state.waveform = Some(data));
                Ok(())
            }
            TimelineKind::Filmstrip => self.filmstrip(request, control, &probe).await,
        }
    }

    // -- the audio ----------------------------------------------------------

    /// The lowest and highest sample of every bucket across a stretch of file.
    async fn waveform(
        &self,
        request: &TimelineRequest,
        control: &TaskControl,
        probe: &MediaProbe,
    ) -> AppResult<WaveformData> {
        let file = Path::new(&request.path);
        let duration = probe
            .duration_sec
            .filter(|value| *value > 0.0)
            .ok_or_else(|| AppError::Other("the length of this file could not be read".into()))?;
        // The container's length rather than the audio stream's, so that the
        // peaks and the strip are laid out against the same clock. A file whose
        // sound stops before its picture does is drawn flat at the end, which
        // is what is actually there.
        let (start_sec, length_sec, seeking) = window(request, duration);

        // A flat line rather than a failure. A film with no sound is an
        // ordinary thing to cut, and the track beneath the strip should say so
        // instead of going missing. It also has to be caught here rather than
        // left to FFmpeg: measured, asking a file with no audio stream for raw
        // samples exits -22 with "Output file does not contain any stream",
        // which would reach the screen as an error about a file that is fine.
        if !probe.has_audio {
            let buckets = request.count.clamp(1, MAX_BUCKETS);
            return Ok(WaveformData {
                buckets,
                start_sec,
                length_sec,
                peaks: cache::base64_encode(&vec![SILENCE; buckets as usize * 2]),
            });
        }

        let rate = sample_rate(&request.path).await?;
        let total = length_sec * f64::from(rate);
        // Never more buckets than there are samples to fill them. Past that
        // point the extra buckets cannot receive anything, and a row of empty
        // ones between the full ones would draw a comb rather than a waveform.
        let buckets = request
            .count
            .clamp(1, MAX_BUCKETS)
            .min(total.max(1.0) as u32);

        let stored = artifact_path(
            file,
            "waveform",
            &format!("{buckets}|{start_sec:.3}|{length_sec:.3}"),
            "bin",
        );
        if let Some(path) = &stored {
            if let Ok(bytes) = std::fs::read(path) {
                // A truncated file is a crash caught mid-write, not a cache
                // entry, and it would draw a waveform with its end missing.
                if bytes.len() == buckets as usize * 2 {
                    cache::touch(path);
                    return Ok(WaveformData {
                        buckets,
                        start_sec,
                        length_sec,
                        peaks: cache::base64_encode(&bytes),
                    });
                }
            }
        }

        let args = waveform_args(file, seeking.then_some((start_sec, length_sec)));
        let mut peaks = Peaks::new(total, buckets as usize);
        stream_stdout(&args, control, &mut |bytes| peaks.feed(bytes)).await?;
        let bytes = peaks.finish();

        if let Some(path) = &stored {
            if std::fs::write(path, &bytes).is_ok() {
                cache::enforce_limit(self.cache_limit_mb());
            }
        }

        Ok(WaveformData {
            buckets,
            start_sec,
            length_sec,
            peaks: cache::base64_encode(&bytes),
        })
    }

    // -- the picture --------------------------------------------------------

    /// A strip of frames, published a sprite at a time as they land.
    async fn filmstrip(
        &self,
        request: &TimelineRequest,
        control: &TaskControl,
        probe: &MediaProbe,
    ) -> AppResult<()> {
        let file = Path::new(&request.path);
        if !probe.has_video {
            return Err(AppError::Other(
                "this file has no picture to lay a strip out from".into(),
            ));
        }

        // The picture's own length, never the container's. Measured on a
        // 26-second container holding 20 seconds of picture, laying forty cells
        // out over 26 seconds emitted 31 frames and left the last nine cells
        // black; the tail padding below hides the black, but the strip still
        // spends its last quarter repeating one frame and lying about where the
        // film has got to.
        let picture = probe
            .video_duration_sec
            .or(probe.duration_sec)
            .filter(|value| *value > 0.0)
            .ok_or_else(|| AppError::Other("the length of this file could not be read".into()))?;
        let (start_sec, length_sec, seeking) = window(request, picture);

        let cells = request.count.clamp(1, MAX_CELLS);
        let cell_height = request
            .cell_height
            .unwrap_or(DEFAULT_CELL_HEIGHT)
            .clamp(MIN_CELL_HEIGHT, MAX_CELL_HEIGHT);
        let (chunk_frames, chunks) = chunk_plan(cells);
        // Only when there is genuinely a keyframe to spare per cell. See
        // [`KEYFRAME_STEP_SEC`].
        let keyframes_only = length_sec / f64::from(cells) >= KEYFRAME_STEP_SEC;

        let mut data = FilmstripData {
            // The total the strip will have, settled now and never changed.
            // The interface maps a moment to a cell through this figure, so a
            // count that grew with each sprite would move every cell it had
            // already drawn.
            frames: cells,
            chunk_frames,
            cell_width: 0,
            cell_height,
            start_sec,
            length_sec,
            chunks: Vec::new(),
        };

        let stored: Option<Vec<PathBuf>> = (0..chunks)
            .map(|index| {
                artifact_path(
                    file,
                    "filmstrip",
                    &format!("{cells}|{cell_height}|{start_sec:.3}|{length_sec:.3}|{index}"),
                    "jpg",
                )
            })
            .collect();

        if let Some(paths) = &stored {
            if paths.iter().all(|path| path.is_file()) {
                for path in paths {
                    let Ok(bytes) = std::fs::read(path) else {
                        // One sprite short is not a strip. Fall through and
                        // draw the whole thing again.
                        data.chunks.clear();
                        break;
                    };
                    cache::touch(path);
                    measure_cells(&mut data, &bytes);
                    data.chunks.push(cache::encode_data_url("image/jpeg", &bytes));
                }
                if data.chunks.len() == paths.len() {
                    self.publish_for(request.token, move |state| state.filmstrip = Some(data));
                    return Ok(());
                }
                data.cell_width = 0;
            }
        }

        // FFmpeg numbers its own output files, so the sprites are written under
        // a scratch name and moved into the cache one at a time. Writing them
        // there directly would leave a half-written sprite behind under a name
        // the next run would trust.
        let scratch = paths::temp_dir()?;
        let stem = util::new_id("strip");
        let pattern = scratch.join(format!("{stem}.%d.jpg"));
        let args = filmstrip_args(
            file,
            &pattern,
            seeking.then_some((start_sec, length_sec)),
            length_sec,
            cells,
            cell_height,
            chunk_frames,
            chunks,
            keyframes_only,
        );

        let mut taken = 0u32;
        let outcome = {
            let mut tick = || {
                self.take_sprites(
                    &scratch,
                    &stem,
                    chunks,
                    false,
                    &mut taken,
                    &mut data,
                    request.token,
                    stored.as_deref(),
                );
            };
            run_for_files(&args, control, &mut tick).await
        };

        if outcome.is_ok() {
            self.take_sprites(
                &scratch,
                &stem,
                chunks,
                true,
                &mut taken,
                &mut data,
                request.token,
                stored.as_deref(),
            );
            if stored.is_some() {
                cache::enforce_limit(self.cache_limit_mb());
            }
        }

        // Whatever a cancelled or failed run left behind is not a strip, and
        // the sweep at startup would not reach it until tomorrow.
        for index in 1..=chunks {
            let _ = std::fs::remove_file(scratch.join(format!("{stem}.{index}.jpg")));
        }
        outcome
    }

    /// Move every sprite FFmpeg has finished with into the cache and publish it.
    ///
    /// A sprite is finished once the next one has been opened, because the
    /// muxer writes them strictly in order; the last one is finished only when
    /// the process is. Publishing one that is still being written would hand
    /// the interface half a picture, and keeping it would leave half a picture
    /// in the cache for good.
    #[allow(clippy::too_many_arguments)]
    fn take_sprites(
        &self,
        scratch: &Path,
        stem: &str,
        total: u32,
        finished: bool,
        taken: &mut u32,
        data: &mut FilmstripData,
        token: u64,
        stored: Option<&[PathBuf]>,
    ) {
        while *taken < total {
            let written = scratch.join(format!("{stem}.{}.jpg", *taken + 1));
            if !written.is_file() {
                break;
            }
            let next = scratch.join(format!("{stem}.{}.jpg", *taken + 2));
            if !finished && !next.is_file() {
                break;
            }
            let Ok(bytes) = std::fs::read(&written) else {
                break;
            };

            measure_cells(data, &bytes);
            data.chunks.push(cache::encode_data_url("image/jpeg", &bytes));

            match stored.and_then(|paths| paths.get(*taken as usize)) {
                // Across directories a rename can fail where a copy succeeds,
                // and the scratch directory and the cache are only on the same
                // volume by convention.
                Some(path) => {
                    if std::fs::rename(&written, path).is_err() {
                        let _ = std::fs::copy(&written, path);
                        let _ = std::fs::remove_file(&written);
                    }
                }
                None => {
                    let _ = std::fs::remove_file(&written);
                }
            }
            *taken += 1;

            let snapshot = data.clone();
            self.publish_for(token, move |state| state.filmstrip = Some(snapshot));
        }
    }

    fn cache_limit_mb(&self) -> u64 {
        self.settings
            .lock()
            .map(|guard| guard.cache_limit_mb)
            .unwrap_or_else(|_| Settings::default().cache_limit_mb)
    }
}

// -- one frame --------------------------------------------------------------

/// One frame of a file, as a data URI.
///
/// For the files the window will not decode at all: the strip, the peaks and
/// the marks all still work, so what is lost is the moving picture rather than
/// the screen. Nothing is cached -- the rail asks once per clip and keeps what
/// it is given, and a tenth of a second that happens once does not earn a file
/// on somebody's disk.
pub async fn frame_at(path: &str, seconds: f64, height: u32) -> AppResult<String> {
    tools::require_ffmpeg()?;
    let file = Path::new(path);
    if !file.is_file() {
        return Err(AppError::Io(format!("{path} is not a file")));
    }

    let args = frame_args(
        file,
        seconds.max(0.0),
        height.clamp(MIN_CELL_HEIGHT, MAX_FRAME_HEIGHT),
    );
    let mut jpeg: Vec<u8> = Vec::new();
    // Nothing cancels a single frame: it is one process, it is over in about a
    // tenth of a second, and the caller has nowhere to put a cancellation.
    let control = TaskControl::new();
    let outcome = stream_stdout(&args, &control, &mut |bytes| jpeg.extend_from_slice(bytes)).await;

    if jpeg.is_empty() {
        // FFmpeg exits happily having written nothing when the seek lands past
        // the last frame, which is the ordinary way to ask for a moment that is
        // not in the file. It is worth saying so rather than passing on
        // whatever the process last complained about.
        return Err(AppError::Other(
            "there is no frame at that point in the file".into(),
        ));
    }
    outcome?;
    Ok(cache::encode_data_url("image/jpeg", &jpeg))
}

// -- what gets run ----------------------------------------------------------

/// Raw signed 16-bit samples, one channel, at whatever rate the file holds.
///
/// There is deliberately no `-ar`. Resampling saves nothing -- measured, an
/// hour of audio reads in 1607 ms at its own rate against 1573 ms resampled to
/// 4 kHz, and four minutes in 73 ms against 69 ms -- and it costs the peaks
/// themselves: a one-millisecond click that reads 32000 at 48 kHz reads 3258
/// through the resampler's low-pass, which is nine tenths of the transient
/// gone from the picture the user is cutting against.
fn waveform_args(input: &Path, window: Option<(f64, f64)>) -> Vec<String> {
    let mut args: Vec<String> = vec!["-v".into(), "error".into()];
    if let Some((start, length)) = window {
        // Both before `-i`, so FFmpeg seeks to the window instead of decoding
        // everything ahead of it: measured, twenty seconds taken from half an
        // hour in costs 76 ms against 1607 ms for the whole file.
        args.extend([
            "-ss".into(),
            format!("{start:.3}"),
            "-t".into(),
            format!("{length:.3}"),
        ]);
    }
    args.extend([
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-vn".into(),
        "-ac".into(),
        "1".into(),
        "-f".into(),
        "s16le".into(),
        "-".into(),
    ]);
    args
}

/// One tiled sprite per chunk rather than one file per cell.
///
/// Measured on a four-minute 1080p file: forty cells as tiles in 621 ms, the
/// same forty as separate seeked JPEGs in 10,028 ms. The seeks are the cost,
/// and tiling has none.
#[allow(clippy::too_many_arguments)]
fn filmstrip_args(
    input: &Path,
    output: &Path,
    window: Option<(f64, f64)>,
    length_sec: f64,
    cells: u32,
    cell_height: u32,
    chunk_frames: u32,
    chunks: u32,
    keyframes_only: bool,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["-v".into(), "error".into(), "-y".into()];
    if keyframes_only {
        args.extend(["-skip_frame".into(), "nokey".into()]);
    }
    args.push("-an".into());
    if let Some((start, length)) = window {
        args.extend([
            "-ss".into(),
            format!("{start:.3}"),
            "-t".into(),
            format!("{length:.3}"),
        ]);
    }
    args.extend(["-i".into(), input.to_string_lossy().into_owned()]);
    args.extend([
        "-vf".into(),
        format!(
            // The height alone, never a width. A 1080x1920 portrait then gives
            // 50 by 90 cells and a 45 KB sprite where a fixed 160 by 90 gives
            // 219 KB -- and, more to the point, cells the shape of the frame
            // rather than cells the shape of the strip.
            "{SQUARE_PIXELS},fps={cells}/{length_sec:.3},scale=-2:{cell_height},{TAIL_PAD},tile={chunk_frames}x1"
        ),
        // The tail padding never ends on its own, so this is what stops the
        // pass rather than the end of the film.
        "-frames:v".into(),
        chunks.to_string(),
        // WebP at the same cell size is 14 KB against 45, but the picture went
        // with the bytes: 0.820 structural similarity against 0.965.
        "-q:v".into(),
        "4".into(),
        output.to_string_lossy().into_owned(),
    ]);
    args
}

/// One frame, written to the pipe rather than to a file.
fn frame_args(input: &Path, seconds: f64, height: u32) -> Vec<String> {
    vec![
        "-v".into(),
        "error".into(),
        // Before `-i`, which is the whole of it: measured on a four-minute
        // 1080p file, 101 ms here against 1217 ms for the same seek placed
        // after the input, for a byte-identical frame.
        "-ss".into(),
        format!("{seconds:.3}"),
        "-an".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-frames:v".into(),
        "1".into(),
        "-vf".into(),
        format!("{SQUARE_PIXELS},scale=-2:{height}"),
        "-q:v".into(),
        "4".into(),
        // Said outright, although it is what the encoder would pick anyway.
        // Measured: without it this pass writes a byte-identical frame and
        // then fails strict compliance over the range it chose, so a seek past
        // the last frame came back as exit -22 with "Non full-range YUV is
        // non-standard" -- an account of the colour of a frame it never wrote,
        // in place of the plain fact that there is nothing at that point.
        "-pix_fmt".into(),
        "yuvj420p".into(),
        "-f".into(),
        "image2pipe".into(),
        "-c:v".into(),
        "mjpeg".into(),
        "-".into(),
    ]
}

/// The rate FFmpeg will decode this file's audio at.
///
/// Asked for on its own although the probe before it has already read the same
/// JSON: `MediaProbe` does not carry a sample rate, and the peaks cannot be
/// laid out in time without one, since the only thing that says where a sample
/// falls is how many of them there are to the second. Measured at 56 ms,
/// against 73 ms for the shortest pass it precedes.
async fn sample_rate(path: &str) -> AppResult<u32> {
    let binary = converter::ffprobe_path()
        .ok_or_else(|| AppError::Other("ffprobe is needed to read the audio".into()))?;
    let args: Vec<String> = vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "a:0".into(),
        "-show_entries".into(),
        "stream=sample_rate".into(),
        "-of".into(),
        "csv=p=0".into(),
        path.to_owned(),
    ];

    let output = process::run(&binary, &args).await?;
    output
        .stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .find(|rate| *rate > 0)
        .ok_or_else(|| AppError::Other("the audio in this file could not be measured".into()))
}

/// Run FFmpeg and hand every byte it writes to its pipe straight to `sink`.
///
/// Deliberately not [`crate::process::run`], which returns the output as a
/// lossy string: every byte above 0x7F would come back as a replacement
/// character, which for raw samples means most of them.
async fn stream_stdout(
    args: &[String],
    control: &TaskControl,
    sink: &mut (dyn FnMut(&[u8]) + Send),
) -> AppResult<()> {
    let binary = tools::require_ffmpeg()?;
    let mut child = process::command(&binary)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| AppError::Other(format!("FFmpeg could not be started: {err}")))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (Some(mut stdout), Some(stderr)) = (stdout, stderr) else {
        return Err(AppError::Other("FFmpeg produced no output stream".into()));
    };
    // Drained in its own task rather than read afterwards: a pass that has
    // something to say fills the pipe and then stops dead waiting for somebody
    // to empty it, and this one is reading the other pipe.
    let diagnostics = tauri::async_runtime::spawn(collect(stderr));

    let mut buffer = vec![0u8; READ_BYTES];
    let mut poll = tokio::time::interval(Duration::from_millis(100));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let interrupted = loop {
        tokio::select! {
            read = stdout.read(&mut buffer) => match read {
                Ok(0) => break false,
                Ok(count) => sink(&buffer[..count]),
                Err(_) => break false,
            },
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
    finish(child, diagnostics).await
}

/// Run FFmpeg for the files it writes rather than for its output, looking in on
/// it as it goes so that a pass writing several of them can publish each one as
/// it lands.
async fn run_for_files(
    args: &[String],
    control: &TaskControl,
    tick: &mut (dyn FnMut() + Send),
) -> AppResult<()> {
    let binary = tools::require_ffmpeg()?;
    let mut child = process::command(&binary)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| AppError::Other(format!("FFmpeg could not be started: {err}")))?;

    let Some(stderr) = child.stderr.take() else {
        return Err(AppError::Other("FFmpeg produced no output stream".into()));
    };
    let diagnostics = tauri::async_runtime::spawn(collect(stderr));

    let mut poll = tokio::time::interval(Duration::from_millis(120));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = child.wait() => break,
            _ = poll.tick() => {
                if control.interrupted() {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(AppError::Canceled);
                }
                tick();
            }
        }
    }
    finish(child, diagnostics).await
}

/// Everything FFmpeg had to say, capped so that a file producing a warning per
/// frame cannot grow this without bound.
async fn collect(stderr: tokio::process::ChildStderr) -> String {
    let mut text = String::new();
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if !line.trim().is_empty() && text.len() < 4096 {
            text.push_str(line.trim());
            text.push('\n');
        }
    }
    text
}

async fn finish(
    mut child: tokio::process::Child,
    diagnostics: tauri::async_runtime::JoinHandle<String>,
) -> AppResult<()> {
    let status = child
        .wait()
        .await
        .map_err(|err| AppError::Other(format!("FFmpeg did not exit cleanly: {err}")))?;
    let trouble = diagnostics.await.unwrap_or_default();

    if !status.success() {
        return Err(AppError::Other(format!(
            "FFmpeg exited with {}: {}",
            status.code().unwrap_or(-1),
            trouble.lines().next().unwrap_or("no detail").trim()
        )));
    }
    Ok(())
}

// -- the peaks --------------------------------------------------------------

/// The lowest and highest sample of every bucket, built as the bytes arrive.
///
/// A whole file's samples are never held: an hour of 48 kHz mono is 345 MB of
/// raw audio, and all the timeline wants out of it is two bytes per bucket.
struct Peaks {
    /// Samples to a bucket, kept fractional. Rounded to a whole number it
    /// drifts by a bucket or more across four thousand of them, which draws a
    /// waveform that no longer lines up with the picture at the far end.
    per: f64,
    low: Vec<i16>,
    high: Vec<i16>,
    index: u64,
    /// The first byte of a sample whose second byte is in the next read. A
    /// 64 KiB read holds a whole number of samples, but nothing obliges FFmpeg
    /// to fill one, and a byte dropped here shifts every sample after it.
    carry: Option<u8>,
}

impl Peaks {
    fn new(total_samples: f64, buckets: usize) -> Self {
        let buckets = buckets.max(1);
        Self {
            per: (total_samples / buckets as f64).max(f64::MIN_POSITIVE),
            low: vec![i16::MAX; buckets],
            high: vec![i16::MIN; buckets],
            index: 0,
            carry: None,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        if let Some(first) = self.carry.take() {
            let Some((second, tail)) = rest.split_first() else {
                self.carry = Some(first);
                return;
            };
            self.record(i16::from_le_bytes([first, *second]));
            rest = tail;
        }

        let mut pairs = rest.chunks_exact(2);
        for pair in pairs.by_ref() {
            self.record(i16::from_le_bytes([pair[0], pair[1]]));
        }
        if let [odd] = pairs.remainder() {
            self.carry = Some(*odd);
        }
    }

    fn record(&mut self, sample: i16) {
        // A read longer than the window was asked for -- a seek that landed a
        // frame early, a container whose duration is a little optimistic --
        // belongs to the last bucket rather than past the end of the strip.
        let bucket = ((self.index as f64 / self.per) as usize).min(self.low.len() - 1);
        self.index += 1;
        if sample < self.low[bucket] {
            self.low[bucket] = sample;
        }
        if sample > self.high[bucket] {
            self.high[bucket] = sample;
        }
    }

    /// Two bytes a bucket, lowest then highest.
    fn finish(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.low.len() * 2);
        for (low, high) in self.low.into_iter().zip(self.high) {
            if low > high {
                // Nothing ever reached this bucket, so it still holds the two
                // ends of the range it started with rather than a peak. It is
                // drawn silent, which is what a file whose sound stops before
                // its picture does actually has there.
                out.push(SILENCE);
                out.push(SILENCE);
            } else {
                out.push(level(low));
                out.push(level(high));
            }
        }
        out
    }
}

/// A sample as the canvas wants it: one byte, with silence in the middle, so
/// that the two bytes of a bucket are an excursion either side of the line
/// rather than a pair of unsigned magnitudes.
fn level(sample: i16) -> u8 {
    ((i32::from(sample) + 32_768) >> 8) as u8
}

// -- odds and ends ----------------------------------------------------------

/// The stretch of the file a request is about, and whether FFmpeg has to be
/// told to seek for it.
///
/// A start without a length, or a length without a start, is not half a window.
/// The interface sends both or neither, and anything else is read as the whole
/// file rather than guessed at.
fn window(request: &TimelineRequest, whole: f64) -> (f64, f64, bool) {
    match (request.start_sec, request.length_sec) {
        (Some(start), Some(length))
            if start.is_finite() && start >= 0.0 && length.is_finite() && length > 0.0 =>
        {
            (start, length, true)
        }
        _ => (0.0, whole, false),
    }
}

/// How many sprites a strip of `cells` is cut into, and how many cells each one
/// holds. The last sprite is a full one whose spare cells repeat the last
/// frame; the interface never draws past `frames`, so they are never seen.
fn chunk_plan(cells: u32) -> (u32, u32) {
    let frames = CELLS_PER_CHUNK.min(cells).max(1);
    (frames, cells.div_ceil(frames))
}

/// Take the cell width from the sprite itself, once.
///
/// Not worked out in advance, because `scale=-2` does not round the way any
/// one rule predicts. Measured on this build: 641x360 into a 68-pixel cell
/// comes back 120 wide where the exact figure is 121.08, 1080x1920 into 70
/// comes back 40 where it is 39.38, and 720x576 into 68 comes back 86 where it
/// is exactly 85. A cell width one pixel out is a strip that shears a little
/// further along with every cell.
fn measure_cells(data: &mut FilmstripData, sprite: &[u8]) {
    if data.cell_width != 0 {
        return;
    }
    match jpeg_size(sprite) {
        Some((width, _)) => data.cell_width = width / data.chunk_frames.max(1),
        None => log_warn!("editor", "a sprite came back without a readable size"),
    }
}

/// The pixel size of a JPEG, read out of its frame header.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..2)? != [0xff, 0xd8] {
        return None;
    }

    let mut at = 2usize;
    while at + 3 < bytes.len() {
        if bytes[at] != 0xff {
            at += 1;
            continue;
        }
        let marker = bytes[at + 1];
        // Fill bytes and the markers that stand alone: neither carries a length
        // to step over.
        if marker == 0xff || marker == 0x01 || (0xd0..=0xd9).contains(&marker) {
            at += 1;
            continue;
        }
        // Every start-of-frame marker. The three exceptions are a Huffman
        // table, an arithmetic-coding table and a restart interval, which wear
        // numbers in the same range and describe no frame.
        if (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc {
            let height = u16::from_be_bytes([*bytes.get(at + 5)?, *bytes.get(at + 6)?]);
            let width = u16::from_be_bytes([*bytes.get(at + 7)?, *bytes.get(at + 8)?]);
            return (width > 0 && height > 0).then_some((u32::from(width), u32::from(height)));
        }
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(at + 2)?,
            *bytes.get(at + 3)?,
        ]));
        if length < 2 {
            return None;
        }
        at += 2 + length;
    }
    None
}

/// Where a rendered artifact is kept.
///
/// Both the size and the modification time are in the key, because either on
/// its own can miss: a file re-encoded in place can keep its size, and a file
/// restored from a copy can keep its time. Getting this wrong does not mean a
/// slow redraw, it means the wrong picture for the file that is open.
///
/// `None` means the file's own metadata could not be read, and the artifact
/// simply goes uncached rather than being filed under a name that cannot be
/// checked against anything.
fn artifact_path(file: &Path, kind: &str, params: &str, extension: &str) -> Option<PathBuf> {
    let metadata = std::fs::metadata(file).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    let key = util::hash_key(&format!(
        "{}|{modified}|{}|{kind}|{params}",
        file.to_string_lossy(),
        metadata.len()
    ));
    Some(paths::editor_cache_dir().ok()?.join(format!("{key}.{extension}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: TimelineKind, count: u32) -> TimelineRequest {
        TimelineRequest {
            path: "in.mp4".into(),
            kind,
            start_sec: None,
            length_sec: None,
            count,
            cell_height: None,
            token: 7,
        }
    }

    fn joined(args: &[String]) -> String {
        args.join(" ")
    }

    // -- the waveform pass --------------------------------------------------

    #[test]
    fn the_whole_file_is_read_at_its_own_rate() {
        let args = waveform_args(Path::new("in.mp4"), None);
        assert_eq!(joined(&args), "-v error -i in.mp4 -vn -ac 1 -f s16le -");
    }

    #[test]
    fn a_window_seeks_before_the_input() {
        let args = waveform_args(Path::new("in.mp4"), Some((12.5, 20.0)));
        assert_eq!(
            joined(&args),
            "-v error -ss 12.500 -t 20.000 -i in.mp4 -vn -ac 1 -f s16le -"
        );
    }

    #[test]
    fn the_waveform_never_resamples() {
        let whole = waveform_args(Path::new("in.mp4"), None);
        let windowed = waveform_args(Path::new("in.mp4"), Some((1.0, 2.0)));
        assert!(!whole.iter().any(|arg| arg == "-ar"));
        assert!(!windowed.iter().any(|arg| arg == "-ar"));
    }

    // -- the filmstrip pass -------------------------------------------------

    fn strip(length: f64, cells: u32, keyframes_only: bool) -> Vec<String> {
        let (chunk_frames, chunks) = chunk_plan(cells);
        filmstrip_args(
            Path::new("in.mp4"),
            Path::new("out.%d.jpg"),
            None,
            length,
            cells,
            68,
            chunk_frames,
            chunks,
            keyframes_only,
        )
    }

    #[test]
    fn a_strip_tiles_in_chunks_of_eight() {
        let args = strip(240.0, 40, true);
        assert_eq!(
            joined(&args),
            "-v error -y -skip_frame nokey -an -i in.mp4 -vf \
             scale=w='trunc(iw*max(1,sar)/2)*2':h='trunc(ih/min(1,sar)/2)*2',setsar=1,\
             fps=40/240.000,scale=-2:68,tpad=stop=-1:stop_mode=clone,tile=8x1 \
             -frames:v 5 -q:v 4 out.%d.jpg"
        );
    }

    #[test]
    fn a_short_step_decodes_every_frame() {
        let args = strip(20.0, 40, false);
        assert!(!args.iter().any(|arg| arg == "-skip_frame"));
        assert!(!args.iter().any(|arg| arg == "nokey"));
    }

    #[test]
    fn a_windowed_strip_seeks_before_the_input() {
        let args = filmstrip_args(
            Path::new("in.mp4"),
            Path::new("out.%d.jpg"),
            Some((1800.0, 20.0)),
            20.0,
            40,
            68,
            8,
            5,
            false,
        );
        let at = args.iter().position(|arg| arg == "-ss").expect("a seek");
        let input = args.iter().position(|arg| arg == "-i").expect("an input");
        assert!(at < input);
        assert_eq!(args[at + 1], "1800.000");
        assert_eq!(args[args.iter().position(|arg| arg == "-t").unwrap() + 1], "20.000");
    }

    #[test]
    fn the_strip_scales_by_height_alone() {
        let graph = strip(240.0, 40, true)
            .iter()
            .find(|arg| arg.contains("tile="))
            .cloned()
            .expect("a filter graph");
        assert!(graph.contains("scale=-2:68"));
        assert!(!graph.contains("scale=120:68"));
    }

    #[test]
    fn the_tail_is_padded_with_the_last_frame_before_it_is_tiled() {
        let graph = strip(240.0, 40, true)
            .iter()
            .find(|arg| arg.contains("tile="))
            .cloned()
            .expect("a filter graph");
        let pad = graph.find(TAIL_PAD).expect("padding");
        let tile = graph.find("tile=").expect("a tile");
        assert!(pad < tile);
    }

    #[test]
    fn every_strip_squares_its_pixels_first() {
        let graph = strip(240.0, 40, true)
            .iter()
            .find(|arg| arg.contains("tile="))
            .cloned()
            .expect("a filter graph");
        assert!(graph.starts_with(SQUARE_PIXELS));
    }

    // -- one frame ----------------------------------------------------------

    #[test]
    fn a_single_frame_seeks_before_the_input_and_writes_to_the_pipe() {
        let args = frame_args(Path::new("in.mp4"), 120.0, 180);
        assert_eq!(
            joined(&args),
            "-v error -ss 120.000 -an -i in.mp4 -frames:v 1 -vf \
             scale=w='trunc(iw*max(1,sar)/2)*2':h='trunc(ih/min(1,sar)/2)*2',setsar=1,scale=-2:180 \
             -q:v 4 -pix_fmt yuvj420p -f image2pipe -c:v mjpeg -"
        );
    }

    // -- how a strip is cut up ----------------------------------------------

    #[test]
    fn chunks_cover_every_cell() {
        assert_eq!(chunk_plan(40), (8, 5));
        assert_eq!(chunk_plan(13), (8, 2));
        assert_eq!(chunk_plan(8), (8, 1));
        assert_eq!(chunk_plan(3), (3, 1));
        assert_eq!(chunk_plan(1), (1, 1));
    }

    // -- the window ---------------------------------------------------------

    #[test]
    fn a_request_without_marks_is_the_whole_file() {
        assert_eq!(
            window(&request(TimelineKind::Waveform, 4000), 26.0),
            (0.0, 26.0, false)
        );
    }

    #[test]
    fn half_a_window_is_not_a_window() {
        let mut only_start = request(TimelineKind::Waveform, 4000);
        only_start.start_sec = Some(4.0);
        assert_eq!(window(&only_start, 26.0), (0.0, 26.0, false));

        let mut no_length = request(TimelineKind::Waveform, 4000);
        no_length.start_sec = Some(4.0);
        no_length.length_sec = Some(0.0);
        assert_eq!(window(&no_length, 26.0), (0.0, 26.0, false));
    }

    #[test]
    fn a_window_with_both_marks_is_taken_as_asked() {
        let mut windowed = request(TimelineKind::Filmstrip, 40);
        windowed.start_sec = Some(1800.0);
        windowed.length_sec = Some(20.0);
        assert_eq!(window(&windowed, 3600.0), (1800.0, 20.0, true));
    }

    // -- the peaks ----------------------------------------------------------

    #[test]
    fn silence_is_the_middle_of_the_range() {
        assert_eq!(level(0), 128);
        assert_eq!(level(i16::MAX), 255);
        assert_eq!(level(i16::MIN), 0);
    }

    #[test]
    fn a_bucket_keeps_its_lowest_and_highest_sample() {
        // Four samples, two buckets.
        let mut peaks = Peaks::new(4.0, 2);
        let samples: [i16; 4] = [1000, -2000, 30000, -30000];
        let mut bytes = Vec::new();
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        peaks.feed(&bytes);
        assert_eq!(
            peaks.finish(),
            vec![level(-2000), level(1000), level(-30000), level(30000)]
        );
    }

    #[test]
    fn a_sample_split_across_two_reads_survives() {
        let samples: [i16; 4] = [1000, -2000, 30000, -30000];
        let mut bytes = Vec::new();
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        let mut whole = Peaks::new(4.0, 2);
        whole.feed(&bytes);
        let expected = whole.finish();

        // Every odd split, so that each one leaves a byte in hand across the
        // boundary rather than a whole sample.
        for cut in [1usize, 3, 5, 7] {
            let mut split = Peaks::new(4.0, 2);
            split.feed(&bytes[..cut]);
            split.feed(&bytes[cut..]);
            assert_eq!(split.finish(), expected, "split after {cut} bytes");
        }
    }

    #[test]
    fn a_bucket_that_received_nothing_is_silent() {
        // Two samples' worth of audio laid out over four buckets: the last two
        // are the file running out, not a quiet passage.
        let mut peaks = Peaks::new(4.0, 4);
        let mut bytes = Vec::new();
        for sample in [20000i16, -20000] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        peaks.feed(&bytes);
        let out = peaks.finish();
        assert_eq!(out[0], level(20000));
        assert_eq!(out[2], level(-20000));
        assert_eq!(&out[4..], &[SILENCE, SILENCE, SILENCE, SILENCE]);
    }

    #[test]
    fn a_read_longer_than_the_window_lands_in_the_last_bucket() {
        // The seek came back a sample early, which is ordinary.
        let mut peaks = Peaks::new(2.0, 2);
        let mut bytes = Vec::new();
        for sample in [100i16, 200, 32000] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        peaks.feed(&bytes);
        let out = peaks.finish();
        assert_eq!(out[2], level(200));
        assert_eq!(out[3], level(32000));
    }

    // -- reading a sprite ---------------------------------------------------

    #[test]
    fn a_sprite_is_measured_from_its_own_header() {
        // Start of image, an application segment to step over, then a baseline
        // frame header of 960 by 68.
        let mut jpeg: Vec<u8> = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&68u16.to_be_bytes());
        jpeg.extend_from_slice(&960u16.to_be_bytes());
        assert_eq!(jpeg_size(&jpeg), Some((960, 68)));
    }

    #[test]
    fn a_huffman_table_is_not_a_frame_header() {
        // 0xc4 sits in the same range as the frame markers and describes none.
        let mut jpeg: Vec<u8> = vec![0xff, 0xd8, 0xff, 0xc4, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend_from_slice(&[0xff, 0xc2, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&90u16.to_be_bytes());
        jpeg.extend_from_slice(&50u16.to_be_bytes());
        assert_eq!(jpeg_size(&jpeg), Some((50, 90)));
    }

    #[test]
    fn something_that_is_not_a_sprite_measures_nothing() {
        assert_eq!(jpeg_size(b""), None);
        assert_eq!(jpeg_size(b"\x89PNG\r\n\x1a\n"), None);
        assert_eq!(jpeg_size(&[0xff, 0xd8]), None);
    }

    #[test]
    fn cells_are_the_sprite_divided_by_what_it_holds() {
        let mut data = FilmstripData {
            frames: 40,
            chunk_frames: 8,
            cell_width: 0,
            cell_height: 68,
            start_sec: 0.0,
            length_sec: 240.0,
            chunks: Vec::new(),
        };
        let mut jpeg: Vec<u8> = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08];
        jpeg.extend_from_slice(&68u16.to_be_bytes());
        jpeg.extend_from_slice(&960u16.to_be_bytes());
        measure_cells(&mut data, &jpeg);
        assert_eq!(data.cell_width, 120);

        // Measured once. A later sprite must not move the cells already drawn.
        let mut narrower: Vec<u8> = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08];
        narrower.extend_from_slice(&68u16.to_be_bytes());
        narrower.extend_from_slice(&400u16.to_be_bytes());
        measure_cells(&mut data, &narrower);
        assert_eq!(data.cell_width, 120);
    }
}
