//! Cutting one range out of one local file.
//!
//! This is deliberately not built on [`crate::converter`], even though both
//! drive FFmpeg over a file the user picked. A conversion is a batch: several
//! files are staged, a format is chosen for all of them, and the results land
//! in a list that outlives the screen. A cut is the opposite -- one file, open
//! in front of the user, with two marks they are still moving. There is only
//! ever one of them, so there is no queue here: starting a cut while one is
//! running replaces it, which is what pressing the button again means.
//!
//! What is shared is everything underneath: [`crate::ffmpeg::trim`] builds the
//! arguments, [`crate::ffmpeg::run_with_progress`] runs them, [`TaskControl`]
//! interrupts them and [`crate::filename::unique_path`] keeps the result from
//! landing on a file that is already there.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use crate::downloader::control::TaskControl;
use crate::error::{AppError, AppResult};
use crate::ffmpeg::{self, TrimPrecision};
use crate::model::{TrimRequest, TrimState, TrimStatus};
use crate::settings::Settings;
use crate::{converter, filename, log_info, log_warn, tools};

pub const EVENT_CHANGED: &str = "trim://changed";

/// The shortest cut worth making. Below this the two marks are the same mark
/// the user has not finished dragging, and FFmpeg would be asked for a file
/// with no frames in it.
const MIN_DURATION_SEC: f64 = 0.05;

pub struct TrimManager {
    app: AppHandle,
    settings: Arc<Mutex<Settings>>,
    state: Mutex<TrimState>,
    /// The cut that is running, so a later press can interrupt it.
    control: Mutex<Option<Arc<TaskControl>>>,
}

impl TrimManager {
    pub fn new(app: AppHandle, settings: Arc<Mutex<Settings>>) -> Arc<Self> {
        Arc::new(Self {
            app,
            settings,
            state: Mutex::new(TrimState::default()),
            control: Mutex::new(None),
        })
    }

    fn settings(&self) -> Settings {
        self.settings
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn state(&self) -> TrimState {
        self.state
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Replace the published state and tell the interface about it. Every
    /// change goes through here, so the screen cannot drift from the truth.
    fn publish(&self, update: impl FnOnce(&mut TrimState)) {
        let next = {
            let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
            update(&mut guard);
            guard.clone()
        };
        let _ = self.app.emit(EVENT_CHANGED, &next);
    }

    /// Interrupt the running cut, if there is one. The task itself publishes
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

    /// Begin a cut, replacing whatever was running.
    ///
    /// The marks are checked against what is actually in the file rather than
    /// trusted: the interface clamps them to the duration it was shown, and
    /// that duration came from a probe of a file that may since have been
    /// replaced on disk.
    pub async fn start(self: &Arc<Self>, request: TrimRequest) -> AppResult<()> {
        // Nothing below works without it, and this is a far clearer way to
        // learn that than a failure to spawn a process.
        tools::require_ffmpeg()?;

        let probe = converter::probe(&request.input_path).await?;
        let start = request.start_sec.max(0.0);
        let end = match probe.duration_sec {
            Some(duration) => request.end_sec.min(duration),
            None => request.end_sec,
        };
        let duration = end - start;
        if !duration.is_finite() || duration < MIN_DURATION_SEC {
            return Err(AppError::Other(
                "the two marks are in the same place".into(),
            ));
        }

        let input = PathBuf::from(&request.input_path);
        let output = output_path(&input, &request, start, end)?;

        // A press while a cut is running means "this one instead", so the old
        // one is interrupted before the new state is published over it.
        self.cancel();

        let control = Arc::new(TaskControl::new());
        *self.control.lock().unwrap_or_else(|err| err.into_inner()) = Some(Arc::clone(&control));

        self.publish(|state| {
            *state = TrimState {
                status: TrimStatus::Running,
                percent: Some(0.0),
                output_path: None,
                error: None,
            };
        });

        let manager = Arc::clone(self);
        let precision = match request.precision {
            crate::model::TrimPrecision::Fast => TrimPrecision::Keyframe,
            crate::model::TrimPrecision::Exact => TrimPrecision::Exact,
        };
        let hardware = self.settings().hardware_acceleration;

        tauri::async_runtime::spawn(async move {
            let progress_of = Arc::clone(&manager);
            let mut on_progress = move |update: ffmpeg::FfmpegProgress| {
                progress_of.publish(|state| {
                    // A late tick from a cut that has already finished must not
                    // put a running bar back on a finished screen.
                    if state.status == TrimStatus::Running {
                        state.percent = update.percent;
                    }
                });
            };

            let outcome = ffmpeg::trim(
                &input,
                &output,
                start,
                duration,
                precision,
                hardware,
                Arc::clone(&control),
                &mut on_progress,
            )
            .await;

            *manager
                .control
                .lock()
                .unwrap_or_else(|err| err.into_inner()) = None;

            match outcome {
                Ok(()) => {
                    log_info!("trim", "wrote {}", output.display());
                    manager.publish(|state| {
                        *state = TrimState {
                            status: TrimStatus::Completed,
                            percent: Some(100.0),
                            output_path: Some(output.to_string_lossy().into_owned()),
                            error: None,
                        };
                    });
                }
                Err(AppError::Canceled) => {
                    // A half-written file is not a result, and leaving it
                    // beside the source would look like one.
                    let _ = std::fs::remove_file(&output);
                    manager.publish(|state| {
                        *state = TrimState {
                            status: TrimStatus::Canceled,
                            percent: None,
                            output_path: None,
                            error: None,
                        };
                    });
                }
                Err(err) => {
                    let _ = std::fs::remove_file(&output);
                    log_warn!("trim", "the cut failed: {err}");
                    manager.publish(|state| {
                        *state = TrimState {
                            status: TrimStatus::Failed,
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
}

/// Where the cut is written.
///
/// Beside the source unless the user said otherwise, keeping the source's
/// container -- a cut is not a conversion, and re-wrapping it would be one.
/// The range goes in the name because it is the only thing that distinguishes
/// two cuts of the same film, and it says so in digits rather than in a word
/// that would have to be translated.
fn output_path(
    input: &Path,
    request: &TrimRequest,
    start: f64,
    end: f64,
) -> AppResult<PathBuf> {
    let dir = match request.output_dir.as_deref() {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => input
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| AppError::Io("that file has nowhere beside it to write to".into()))?,
    };

    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("clip");
    let ext = input
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("mp4");

    let named = format!(
        "{} {}-{}",
        filename::truncate_stem(stem, 120),
        stamp(start),
        stamp(end)
    );
    Ok(filename::unique_path(
        &dir,
        &filename::sanitize_component(&named),
        ext,
    ))
}

/// A mark, as a file name can hold it: minutes and seconds, no colon -- which
/// Windows will not take in a name -- and no decimal point unless the tenth of
/// a second is what tells two cuts apart.
fn stamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let rest = seconds - (minutes as f64) * 60.0;
    if (rest - rest.round()).abs() < 0.05 {
        format!("{minutes}m{:02}s", rest.round() as u64)
    } else {
        format!("{minutes}m{rest:04.1}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
