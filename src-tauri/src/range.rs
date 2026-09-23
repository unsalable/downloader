//! Bringing a piece of a link into the editor without bringing the rest.
//!
//! A half-hour video the user wants ten seconds of is twenty-eight megabytes
//! against five hundred kilobytes, and several minutes against several seconds.
//! The engine can be asked for the piece -- `--download-sections "*START-END"`
//! -- and this is where that is decided, carried out and reported.
//!
//! It is shaped like [`crate::export`] and deliberately not like
//! [`crate::queue`]. The editor fetches one thing at a time, from a screen the
//! user is looking at, and what it wants back is a file it then opens. A queued
//! download is the opposite in every one of those: it is one of many, it
//! outlives the screen that started it, and it drags a request, a history row,
//! a retry count and a position in a list along with it. None of that belongs
//! to a file somebody is about to trim.
//!
//! Three things about a ranged fetch are worth knowing before reading the code.
//! FFmpeg does the transfer whatever `--downloader` says, which makes it a
//! grandchild of this process and is why [`crate::process::spawn_tree`] exists.
//! Because FFmpeg does it, the engine's progress template yields exactly one
//! line, after the fact, with the download already complete -- so a ranged
//! fetch publishes no percentage at all rather than a bar that waits at zero
//! and jumps to the end. And the session a link may have been analysed with
//! does not reach that FFmpeg: measured, `--cookies` alongside
//! `--download-sections` passes on the user agent and the accept headers and
//! nothing else, so a host that gates the media request itself on a cookie can
//! serve the whole video and refuse a piece of it.
//!
//! Not yet, and worth writing down: FFmpeg could be pointed straight at
//! [`crate::model::MediaFormat::url`] with the headers stored beside it, both
//! of which this app already parses and keeps. Measured, that is 524 KB in
//! 0.42 s against roughly 20 s through the engine. What it costs is owning the
//! signed URL's expiry, the 403 that a stale one answers with and the retry
//! after it, and the pairing of a video stream with its audio by hand -- all of
//! which the engine does today and gets right. It is worth having for
//! [`crate::model::PlatformId::Direct`], where there is one stream and no
//! signature; it is not worth having as the first version of this.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use crate::downloader::control::TaskControl;
use crate::downloader::engine_dl::{self, EngineDownload};
use crate::downloader::http::ProgressSample;
use crate::downloader::plan::{self, DownloadPlan, KeptRange};
use crate::error::{AppError, AppResult};
use crate::model::{
    DownloadMode, FetchState, FetchStatus, MediaMetadata, QualityPreference, RangeFetchRequest,
    WatermarkPreference,
};
use crate::settings::Settings;
use crate::{export, ffmpeg, filename, log_info, log_warn, paths, providers, tools, util};

pub const EVENT_CHANGED: &str = "editor://fetch";

/// The shortest range worth asking a source for. Below this the two marks are
/// one mark that was dragged by accident, and the answer -- a file with no
/// frames in it, or on some sources the whole video -- is worse than a refusal.
const MIN_RANGE_SEC: f64 = 0.25;

/// How long a fetched name may be before the timestamps are all that is left
/// of it. The same budget the export uses.
const MAX_STEM_CHARS: usize = 120;

pub struct RangeFetchManager {
    app: AppHandle,
    settings: Arc<Mutex<Settings>>,
    state: Mutex<FetchState>,
    /// The fetch that is running, so a later press can interrupt it.
    control: Mutex<Option<Arc<TaskControl>>>,
}

/// Everything one fetch knows about itself once the link has been read.
struct Job {
    metadata: MediaMetadata,
    plan: DownloadPlan,
    /// The piece that will actually be asked for. `None` is the whole video,
    /// whether that is what was wanted or all that is on offer.
    section: Option<KeptRange>,
    /// True when marks were given and had to be dropped, which is the one
    /// outcome here that is neither what was asked for nor a failure.
    whole_instead: bool,
    exact: bool,
}

impl RangeFetchManager {
    pub fn new(app: AppHandle, settings: Arc<Mutex<Settings>>) -> Arc<Self> {
        Arc::new(Self {
            app,
            settings,
            state: Mutex::new(FetchState::default()),
            control: Mutex::new(None),
        })
    }

    fn settings(&self) -> Settings {
        self.settings
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn state(&self) -> FetchState {
        self.state
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// How many fetches are under way: one or none, counted the same way as
    /// [`crate::export::ExportManager::active_count`] and for the same reader.
    pub fn active_count(&self) -> u32 {
        u32::from(self.state().status.is_active())
    }

    /// Replace the published state and tell the interface about it. Every
    /// change goes through here, so the screen cannot drift from the truth.
    fn publish(&self, update: impl FnOnce(&mut FetchState)) {
        let next = {
            let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
            update(&mut guard);
            guard.clone()
        };
        let _ = self.app.emit(EVENT_CHANGED, &next);
    }

    /// Interrupt the running fetch, if there is one. The task itself publishes
    /// the result: the engine and the FFmpeg under it have to be given the
    /// chance to die first.
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

    /// Begin a fetch, replacing whatever was running.
    ///
    /// Only what can be settled without touching the network is settled here.
    /// Reading the link is most of the wait on a source that hides its streams
    /// behind a challenge, and holding the command open for it would leave the
    /// button pressed and the screen saying nothing for half a minute.
    pub async fn start(self: &Arc<Self>, request: RangeFetchRequest) -> AppResult<()> {
        let url = request.url.trim().to_owned();
        if url.is_empty() {
            return Err(AppError::InvalidUrl("no link was given".into()));
        }
        // Nothing below works without it, and this is a far clearer way to
        // learn that than a failure to spawn a process.
        tools::require_engine()?;

        // A press while a fetch is running means "this one instead", so the old
        // one is interrupted before the new state is published over it.
        self.cancel();

        let control = Arc::new(TaskControl::new());
        *self.control.lock().unwrap_or_else(|err| err.into_inner()) = Some(Arc::clone(&control));

        self.publish(|state| {
            *state = FetchState {
                status: FetchStatus::Resolving,
                percent: None,
                received_bytes: 0,
                title: None,
                output_path: None,
                error: None,
            };
        });

        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let outcome = manager.execute(&request, &url, Arc::clone(&control)).await;

            // Only if this fetch is still the one the screen is waiting for. A
            // press that replaced it has already put its own control here, and
            // both clearing that and publishing this one's result over it would
            // leave the new fetch unstoppable and reported as the old one.
            if !manager.retire(&control) {
                return;
            }

            match outcome {
                Ok(path) => {
                    log_info!("range", "fetched {}", path.display());
                    manager.publish(|state| {
                        state.status = FetchStatus::Completed;
                        state.percent = None;
                        state.output_path = Some(path.to_string_lossy().into_owned());
                        state.error = None;
                    });
                }
                Err(AppError::Canceled) => {
                    manager.publish(|state| {
                        state.status = FetchStatus::Canceled;
                        state.percent = None;
                        state.output_path = None;
                        state.error = None;
                    });
                }
                Err(err) => {
                    // A stream address that has expired underneath a kept
                    // analysis looks exactly like a link that has gone; asking
                    // again is what tells them apart, and that only happens if
                    // what was kept is dropped first.
                    providers::forget_analysis(&url);
                    log_warn!("range", "the fetch failed: {err}");
                    manager.publish(|state| {
                        state.status = FetchStatus::Failed;
                        state.percent = None;
                        state.output_path = None;
                        state.error = Some(err.to_info());
                    });
                }
            }
        });

        Ok(())
    }

    /// Give up ownership of the published state, and say whether this fetch
    /// still had it.
    fn retire(&self, control: &Arc<TaskControl>) -> bool {
        let mut guard = self.control.lock().unwrap_or_else(|err| err.into_inner());
        let ours = guard
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, control));
        if ours {
            *guard = None;
        }
        ours
    }

    async fn execute(
        self: &Arc<Self>,
        request: &RangeFetchRequest,
        url: &str,
        control: Arc<TaskControl>,
    ) -> AppResult<PathBuf> {
        let settings = self.settings();
        let job = self.resolve(request, url, &settings).await?;
        // Reading the link is the one stretch of this that cannot be
        // interrupted: the provider layer takes no control flag, and a run that
        // has already reached the network is going to finish reaching it. So
        // Cancel during Resolving lands here, which is late but never wrong --
        // nothing has been written by this point.
        if control.interrupted() {
            return Err(AppError::Canceled);
        }

        // A section is fetched by an FFmpeg the engine starts, and the engine
        // aborts outright when there is none. Asked here as well as inside the
        // engine runner so that the answer is this app's card rather than a
        // sentence from a tool the user did not know was involved.
        if job.section.is_some() {
            tools::require_ffmpeg()?;
        }

        let ranged = job.section.is_some();
        self.publish(|state| {
            state.status = FetchStatus::Fetching;
            state.title = Some(published_title(&job.metadata.title, job.whole_instead));
            // A ranged fetch has no percentage to give, and the one it could
            // invent would sit at zero and then jump to a hundred. Absent is
            // not zero, and the interface draws it as the indeterminate thing
            // it is.
            state.percent = (!ranged).then_some(0.0);
        });

        let scratch = paths::temp_dir()?;
        let id = util::new_id("fetch");
        let staged = ffmpeg::intermediate_path(&scratch, &id, "fetch", &job.plan.container);
        let selector = job.plan.selector_for_engine();

        let manager = Arc::clone(self);
        let mut sink = move |sample: ProgressSample| manager.on_progress(&sample, ranged);

        let outcome = engine_dl::run(
            EngineDownload {
                url: &job.metadata.canonical_url,
                format_selector: &selector,
                target: &staged,
                merge_container: job
                    .plan
                    .needs_merge
                    .then_some(job.plan.container.as_str()),
                section: job.section,
                force_keyframes: job.exact,
            },
            &settings,
            control,
            &mut sink,
        )
        .await;

        if let Err(err) = outcome {
            discard_staged(&staged);
            return Err(explain_refusal(err, ranged));
        }

        let produced = engine_dl::resolve_output(&staged)?;
        let destination = destination_dir(request.output_dir.as_deref())?;
        let target = destination.join(fetched_name(&job, &produced));
        let landed = move_into_place(&produced, &target)?;
        Ok(landed)
    }

    /// Read the link, choose the streams, and settle what can be asked for.
    async fn resolve(
        &self,
        request: &RangeFetchRequest,
        url: &str,
        settings: &Settings,
    ) -> AppResult<Job> {
        // The same kept analysis the download path reuses, and for the same
        // reason: pressing this button follows a Check by seconds, and reading
        // a YouTube link again means solving the player's JavaScript challenge
        // a second time for an answer that has not changed.
        let metadata = match providers::recent_analysis(url, settings) {
            Some(known) => known,
            None => {
                let found = providers::analyze(url, settings).await?;
                providers::remember_analysis(url, settings, &found);
                found
            }
        };

        let quality = match request.max_height {
            Some(height) if height > 0 => QualityPreference::MaxHeight { height },
            _ => QualityPreference::Best,
        };
        let mut plan = plan::build(
            &metadata,
            DownloadMode::Video,
            quality,
            None,
            None,
            None,
            WatermarkPreference::Any,
        )?;

        let asked = asked_range(request, metadata.duration_sec)?;
        let section = asked.filter(|_| plan.supports_range());
        let whole_instead = asked.is_some() && section.is_none();
        if whole_instead {
            // Not a failure. The user gets the file they can edit, by the only
            // route this source leaves open, and the state says which route
            // that was rather than letting the size of the download say it.
            log_info!(
                "range",
                "{} cannot hand over a range of these streams; fetching all of it",
                metadata.platform_label
            );
        }
        if let Some(range) = section {
            plan.keep_range(range, metadata.duration_sec);
        }

        Ok(Job {
            metadata,
            plan,
            section,
            whole_instead,
            exact: request.exact,
        })
    }

    fn on_progress(&self, sample: &ProgressSample, ranged: bool) {
        self.publish(|state| {
            // A late sample from a fetch that has already finished must not put
            // a running bar back on a finished screen.
            if state.status != FetchStatus::Fetching {
                return;
            }
            state.received_bytes = sample.received;
            state.percent = if ranged { None } else { sample.percent };
        });
    }
}

// -- what a fetch is allowed to be ------------------------------------------

/// The marks as they can actually be fetched, or nothing when none were given.
///
/// `Err` is for marks that say something impossible. Treating those as "no
/// range" instead would answer a mis-dragged handle by quietly downloading the
/// entire film, which is the most expensive thing this screen can do and the
/// last thing anyone meant by it.
fn asked_range(request: &RangeFetchRequest, duration: Option<f64>) -> AppResult<Option<KeptRange>> {
    let (Some(start), Some(end)) = (request.start_sec, request.end_sec) else {
        // Neither mark alone means anything; the model says so and this is
        // where that is honoured.
        return Ok(None);
    };
    if !start.is_finite() || !end.is_finite() {
        return Err(AppError::Other("one of the marks is not a time".into()));
    }

    let duration = duration.filter(|value| value.is_finite() && *value > 0.0);
    let start = start.max(0.0);
    if duration.is_some_and(|duration| start >= duration) {
        return Err(AppError::Other(
            "the range starts after the video ends".into(),
        ));
    }
    let end = match duration {
        Some(duration) => end.min(duration),
        None => end,
    };
    // Catches both the range with no length and the range the wrong way round.
    if end - start < MIN_RANGE_SEC {
        return Err(AppError::Other(
            "that range is too short to hold anything".into(),
        ));
    }

    Ok(Some(KeptRange {
        start_sec: start,
        end_sec: end,
    }))
}

/// What the state publishes as the title.
///
/// Two things have to fit in one string, because [`FetchState`] carries one and
/// its shape is fixed. Ordinarily it is the media's own title. When marks were
/// given and the chosen streams cannot be cut into, the whole video comes down
/// instead -- a far longer and far larger download than the one the button
/// promised -- and learning that from the size of the file afterwards would be
/// learning it too late to stop it.
fn published_title(title: &str, whole_instead: bool) -> String {
    let title = match title.trim() {
        "" => "This video",
        named => named,
    };
    if whole_instead {
        format!("{title} (all of it: this source will not hand over a range)")
    } else {
        title.to_owned()
    }
}

/// A refusal of the piece rather than of the link.
///
/// The analysis has already succeeded by this point, so the link is one this
/// machine can reach and, where a session was needed, one the session opened.
/// A refusal arriving now is the media request being turned away -- and that
/// request was made by an FFmpeg the session never reached. Reported as itself,
/// it would send the user back to Settings to reconnect a browser that is
/// already connected and cannot help.
fn explain_refusal(err: AppError, ranged: bool) -> AppError {
    if !ranged {
        return err;
    }
    match err {
        AppError::Forbidden { detail, .. } | AppError::MembershipRequired { detail } => {
            AppError::RangeUnavailable(detail)
        }
        other => other,
    }
}

/// Take away what a fetch that did not finish left in scratch.
///
/// Everything beginning with the staged name goes, not the staged name alone:
/// the engine appends the container it settled on when that is not the one it
/// was asked for, and leaves a `.part` beside it while it works. The name is an
/// id this run made in the app's own temporary folder, so nothing else can
/// begin with it. The startup sweep would get there in a day; a cancelled fetch
/// of a long film is worth more than that.
fn discard_staged(staged: &Path) {
    let (Some(dir), Some(name)) = (staged.parent(), staged.file_name()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let name = name.to_string_lossy().into_owned();
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Where the fetched file is written.
///
/// The app's own temporary folder unless the user said otherwise, which is what
/// the editor wants: the file is a working copy of somebody else's video, and
/// leaving it in Downloads would be leaving a file nobody asked to keep.
fn destination_dir(requested: Option<&str>) -> AppResult<PathBuf> {
    let Some(dir) = requested.map(str::trim).filter(|dir| !dir.is_empty()) else {
        return paths::temp_dir();
    };
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        std::fs::create_dir_all(&dir).map_err(|err| {
            AppError::Permission(format!("{} could not be created: {err}", dir.display()))
        })?;
    }
    Ok(dir)
}

/// What the fetched file is called.
///
/// The extension comes from the file the engine actually wrote rather than from
/// the plan, because a merge or an extraction can change it. The marks go in
/// the name for the same reason they go in an export's, and in the same
/// spelling: two pieces of one video are otherwise told apart only by the "(2)"
/// that a collision adds, which says nothing about which piece is which.
fn fetched_name(job: &Job, produced: &Path) -> String {
    let title = match job.metadata.title.trim() {
        "" => "clip",
        named => named,
    };
    let stem = match job.section {
        Some(range) => format!(
            "{} {}-{}",
            filename::truncate_stem(title, MAX_STEM_CHARS),
            export::stamp(range.start_sec),
            export::stamp(range.end_sec)
        ),
        None => filename::truncate_stem(title, MAX_STEM_CHARS),
    };
    let extension = produced
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or(&job.plan.container);
    format!("{}.{extension}", filename::sanitize_component(&stem))
}

/// Move the finished file out of scratch, without overwriting anything.
///
/// A rename is the whole operation when both sides are on one volume, which
/// they are whenever the editor keeps the default. A chosen folder on another
/// drive is not, and there a rename fails with a message about the file being
/// on a different device -- so the copy is the fallback rather than the rule.
fn move_into_place(produced: &Path, target: &Path) -> AppResult<PathBuf> {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(AppError::Io("the fetch has nowhere to be written".into()));
    };
    let stem = Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("clip");
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let unique = filename::unique_path(dir, stem, extension);

    if std::fs::rename(produced, &unique).is_ok() {
        return Ok(unique);
    }
    std::fs::copy(produced, &unique)?;
    let _ = std::fs::remove_file(produced);
    Ok(unique)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        FormatKind, MediaFormat, MediaKind, PlatformId, WatermarkSupport,
    };

    fn request(start: Option<f64>, end: Option<f64>) -> RangeFetchRequest {
        RangeFetchRequest {
            url: "https://example.test/x".into(),
            start_sec: start,
            end_sec: end,
            max_height: None,
            exact: false,
            output_dir: None,
        }
    }

    fn format(id: &str, kind: FormatKind, protocol: &str) -> MediaFormat {
        MediaFormat {
            id: id.into(),
            kind,
            container: "mp4".into(),
            protocol: protocol.into(),
            has_video: matches!(kind, FormatKind::Video | FormatKind::Muxed),
            has_audio: matches!(kind, FormatKind::Audio | FormatKind::Muxed),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30.0),
            vcodec: Some("avc1.640028".into()),
            acodec: Some("mp4a.40.2".into()),
            tbr: Some(1000.0),
            vbr: None,
            abr: Some(128.0),
            filesize: Some(28_000_000),
            filesize_approx: None,
            quality_label: "1080p".into(),
            watermarked: None,
            note: None,
            needs_engine_download: false,
            url: Some("https://cdn.test/v".into()),
            http_headers: Vec::new(),
        }
    }

    fn metadata(title: &str) -> MediaMetadata {
        MediaMetadata {
            url: "https://example.test/x".into(),
            canonical_url: "https://example.test/x".into(),
            platform: PlatformId::Youtube,
            platform_label: "YouTube".into(),
            provider_id: "engine".into(),
            media_kind: MediaKind::Video,
            title: title.into(),
            creator: None,
            description: None,
            thumbnail_url: None,
            duration_sec: Some(635.0),
            view_count: None,
            like_count: None,
            upload_date: None,
            is_live: false,
            formats: vec![format("18", FormatKind::Muxed, "https")],
            entry_count: None,
            watermark_support: WatermarkSupport::NotApplicable,
            range_fetchable: true,
            warnings: Vec::new(),
            entries: Vec::new(),
        }
    }

    fn job(title: &str, section: Option<KeptRange>) -> Job {
        let meta = metadata(title);
        let plan = plan::build(
            &meta,
            DownloadMode::Video,
            QualityPreference::Best,
            None,
            None,
            None,
            WatermarkPreference::Any,
        )
        .unwrap();
        Job {
            metadata: meta,
            plan,
            section,
            whole_instead: false,
            exact: false,
        }
    }

    #[test]
    fn reading_the_link_counts_as_work_as_much_as_fetching_it() {
        assert!(FetchStatus::Resolving.is_active());
        assert!(FetchStatus::Fetching.is_active());
        for status in [
            FetchStatus::Idle,
            FetchStatus::Completed,
            FetchStatus::Failed,
            FetchStatus::Canceled,
        ] {
            assert!(!status.is_active(), "{status:?}");
        }
    }

    #[test]
    fn both_marks_or_neither() {
        assert!(asked_range(&request(None, None), Some(635.0)).unwrap().is_none());
        assert!(asked_range(&request(Some(10.0), None), Some(635.0)).unwrap().is_none());
        assert!(asked_range(&request(None, Some(20.0)), Some(635.0)).unwrap().is_none());
    }

    #[test]
    fn a_pair_of_marks_becomes_the_range_between_them() {
        let range = asked_range(&request(Some(30.0), Some(40.0)), Some(635.0))
            .unwrap()
            .unwrap();
        assert_eq!((range.start_sec, range.end_sec), (30.0, 40.0));
        assert_eq!(range.length(), 10.0);
    }

    #[test]
    fn a_range_that_says_nothing_is_refused_rather_than_widened() {
        // Every one of these would otherwise be answered by downloading the
        // whole video, which is the one outcome nobody dragging a handle meant.
        for (start, end) in [(10.0, 10.0), (10.0, 10.1), (40.0, 30.0)] {
            assert!(
                asked_range(&request(Some(start), Some(end)), Some(635.0)).is_err(),
                "{start}-{end}"
            );
        }
        assert!(asked_range(&request(Some(f64::NAN), Some(30.0)), Some(635.0)).is_err());
        assert!(asked_range(&request(Some(700.0), Some(710.0)), Some(635.0)).is_err());
    }

    #[test]
    fn a_mark_past_the_end_is_brought_back_to_it() {
        let range = asked_range(&request(Some(600.0), Some(900.0)), Some(635.0))
            .unwrap()
            .unwrap();
        assert_eq!(range.end_sec, 635.0);

        // Nothing to clamp against leaves the mark as it was asked for.
        let range = asked_range(&request(Some(600.0), Some(900.0)), None)
            .unwrap()
            .unwrap();
        assert_eq!(range.end_sec, 900.0);
    }

    #[test]
    fn a_negative_start_becomes_the_beginning() {
        let range = asked_range(&request(Some(-5.0), Some(20.0)), Some(635.0))
            .unwrap()
            .unwrap();
        assert_eq!(range.start_sec, 0.0);
    }

    #[test]
    fn the_title_says_when_the_whole_video_is_coming_instead() {
        assert_eq!(published_title("Holiday", false), "Holiday");
        assert_eq!(
            published_title("Holiday", true),
            "Holiday (all of it: this source will not hand over a range)"
        );
        assert_eq!(published_title("   ", false), "This video");
    }

    #[test]
    fn a_refused_piece_is_not_reported_as_a_refused_link() {
        let refusal = AppError::Forbidden {
            status: 403,
            detail: "ERROR: unable to open input: Server returned 403 Forbidden".into(),
        };
        assert_eq!(explain_refusal(refusal, true).code(), "rangeUnavailable");

        // The same refusal of the whole video is exactly what it says it is.
        let refusal = AppError::Forbidden {
            status: 403,
            detail: "ERROR: Sign in to confirm your age".into(),
        };
        assert_eq!(explain_refusal(refusal, false).code(), "forbidden");

        // Nothing else is reinterpreted: a link that has gone has gone,
        // whichever part of it was wanted.
        let gone = AppError::NotFound {
            status: 404,
            detail: "ERROR: Video unavailable".into(),
        };
        assert_eq!(explain_refusal(gone, true).code(), "notFound");
    }

    #[test]
    fn a_fetched_piece_is_named_by_the_marks_it_was_taken_from() {
        let job = job(
            "holiday",
            Some(KeptRange {
                start_sec: 4.8,
                end_sec: 72.5,
            }),
        );
        assert_eq!(
            fetched_name(&job, Path::new("C:\\temp\\fetch-1.fetch.mp4")),
            "holiday 0m04.8s-1m12.5s.mp4"
        );
    }

    #[test]
    fn a_whole_fetch_is_named_after_the_video_alone() {
        let job = job("holiday", None);
        assert_eq!(
            fetched_name(&job, Path::new("C:\\temp\\fetch-1.fetch.mp4")),
            "holiday.mp4"
        );
    }

    #[test]
    fn the_extension_is_the_one_the_engine_actually_wrote() {
        // A merge into Matroska leaves a sibling with a different extension,
        // and naming the result after the plan would produce an .mp4 holding
        // an .mkv.
        let job = job("holiday", None);
        assert_eq!(
            fetched_name(&job, Path::new("C:\\temp\\fetch-1.fetch.mkv")),
            "holiday.mkv"
        );
    }

    #[test]
    fn a_name_a_filesystem_would_refuse_never_reaches_it() {
        let job = job("4/5: why? <best>", None);
        let name = fetched_name(&job, Path::new("C:\\temp\\fetch-1.fetch.mp4"));
        assert_eq!(filename::sanitize_component(&name), name);
        assert!(!name.contains('/'), "{name}");
        assert!(!name.contains(':'), "{name}");
    }

    #[test]
    fn an_untitled_video_still_gets_a_name() {
        let job = job("  ", None);
        assert_eq!(
            fetched_name(&job, Path::new("C:\\temp\\fetch-1.fetch.mp4")),
            "clip.mp4"
        );
    }

    #[test]
    fn a_fetch_that_gave_up_leaves_nothing_of_itself_behind() {
        let scratch = std::env::temp_dir().join("ud-range-scratch");
        let _ = std::fs::create_dir_all(&scratch);
        let staged = scratch.join("fetch-abc.fetch.mp4");

        // Every spelling one interrupted fetch can leave: the name as asked
        // for, the name with the container the engine settled on, and the
        // partial file beside either.
        let strays = [
            scratch.join("fetch-abc.fetch.mp4"),
            scratch.join("fetch-abc.fetch.mp4.part"),
            scratch.join("fetch-abc.fetch.mp4.webm"),
            scratch.join("fetch-abc.fetch.mp4.webm.part"),
        ];
        let someone_else = scratch.join("fetch-xyz.fetch.mp4");
        for path in strays.iter().chain([&someone_else]) {
            std::fs::write(path, b"half a film").unwrap();
        }

        discard_staged(&staged);

        for path in &strays {
            assert!(!path.exists(), "{}", path.display());
        }
        assert!(
            someone_else.exists(),
            "another fetch's file is not this one's to remove"
        );
        let _ = std::fs::remove_file(&someone_else);
    }

    #[test]
    fn a_fetch_never_writes_over_a_file_that_is_already_there() {
        let dir = std::env::temp_dir().join("ud-range-move");
        let _ = std::fs::create_dir_all(&dir);
        let occupied = dir.join("taken.mp4");
        std::fs::write(&occupied, b"an earlier fetch").unwrap();

        let produced = dir.join("staged.tmp");
        std::fs::write(&produced, b"this one").unwrap();

        let landed = move_into_place(&produced, &occupied).unwrap();
        assert_ne!(landed, occupied);
        assert_eq!(std::fs::read(&occupied).unwrap(), b"an earlier fetch");
        assert_eq!(std::fs::read(&landed).unwrap(), b"this one");
        assert!(!produced.exists(), "the staged copy is not left behind");

        let _ = std::fs::remove_file(&occupied);
        let _ = std::fs::remove_file(&landed);
    }
}
