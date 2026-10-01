//! Download orchestration.
//!
//! Takes a request, resolves it into concrete streams, fetches them, and does
//! whatever post-processing the chosen output needs. Every intermediate file
//! lives in the app's temp directory; the user's download folder only ever
//! receives a finished file.
//!
//! Stream URLs are resolved here rather than reused from the analyze step: they
//! are signed and expire, so a download that was queued an hour ago still has
//! to ask for fresh ones.

pub mod control;
pub mod engine_dl;
pub mod http;
pub mod plan;
pub mod speed;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::model::{
    DownloadProgress, DownloadRequest, DownloadStage, MediaFormat, MediaMetadata,
    MusicTags, PlatformId, SourceContext,
};
use crate::settings::Settings;
use crate::{ffmpeg, filename, log_debug, log_info, paths, providers};

use control::TaskControl;
use plan::DownloadPlan;

pub struct DownloadOutcome {
    pub output_path: PathBuf,
    pub file_size: u64,
    pub label: String,
    pub quality_label: String,
    pub container: String,
    pub metadata: MediaMetadata,
}

/// Called on every progress tick. The queue turns these into IPC events.
pub type UpdateSink<'a> = &'a mut (dyn FnMut(DownloadProgress) + Send);

/// Aggregates per-stage byte counts into one overall figure, so a two-stream
/// download shows a single monotonic progress bar rather than restarting at 0%
/// when the audio stage begins.
struct Aggregator {
    completed_bytes: u64,
    total_estimate: Option<u64>,
    stage: DownloadStage,
    stage_index: u32,
    stage_count: u32,
}

impl Aggregator {
    fn new(total_estimate: Option<u64>, stage_count: u32) -> Self {
        Self {
            completed_bytes: 0,
            total_estimate,
            stage: DownloadStage::Resolving,
            stage_index: 1,
            stage_count,
        }
    }

    fn enter(&mut self, stage: DownloadStage, index: u32) {
        self.stage = stage;
        self.stage_index = index.min(self.stage_count);
    }

    fn sample(&mut self, sample: &http::ProgressSample) -> DownloadProgress {
        let received = self.completed_bytes + sample.received;

        // Prefer the plan's estimate: it covers every stream, where the current
        // response only knows about its own.
        let total = match (self.total_estimate, sample.total) {
            (Some(estimate), Some(current)) => {
                Some(estimate.max(self.completed_bytes + current))
            }
            (Some(estimate), None) => Some(estimate),
            (None, Some(current)) => Some(self.completed_bytes + current),
            (None, None) => None,
        };

        let percent = total.filter(|t| *t > 0).map(|t| (received as f64 / t as f64 * 100.0).clamp(0.0, 100.0));

        DownloadProgress {
            received_bytes: received,
            total_bytes: total,
            percent,
            speed_bps: sample.speed_bps,
            eta_sec: sample.eta_sec,
            stage: self.stage,
            stage_index: self.stage_index,
            stage_count: self.stage_count,
            resumable: sample.resumable,
        }
    }

    fn finish_stage(&mut self, bytes: u64) {
        self.completed_bytes += bytes;
    }

    /// Progress for a processing stage, where the unit of work is time rather
    /// than bytes.
    fn processing(&mut self, stage: DownloadStage, percent: Option<f64>) -> DownloadProgress {
        self.stage = stage;
        DownloadProgress {
            received_bytes: self.completed_bytes,
            total_bytes: self.total_estimate,
            percent,
            speed_bps: 0.0,
            eta_sec: None,
            stage,
            stage_index: self.stage_count,
            stage_count: self.stage_count,
            resumable: false,
        }
    }
}

pub async fn execute(
    task_id: &str,
    request: &DownloadRequest,
    settings: &Settings,
    control: Arc<TaskControl>,
    on_update: UpdateSink<'_>,
) -> AppResult<DownloadOutcome> {
    let temp_dir = paths::temp_dir()?;

    on_update(DownloadProgress {
        stage: DownloadStage::Resolving,
        ..Default::default()
    });

    // Fresh metadata means fresh (unexpired) stream URLs. An analysis the user
    // made moments ago is fresh enough, and repeating it is the costliest step.
    //
    // Not for a link the browser handed over, though. That one is read with
    // the page and headers it was playing under, and the kept analyses are
    // keyed on the address alone: one made without those headers is no answer
    // for this request, and one made with them is no answer for the next
    // request of the same address that has none.
    let metadata = if let Some(source) = request.source.as_ref() {
        providers::analyze_with_source(&request.url, settings, Some(source)).await?
    } else {
        match providers::recent_analysis(&request.url, settings) {
            Some(metadata) => {
                log_debug!("downloader", "task {task_id}: reusing the analysis made moments ago");
                metadata
            }
            None => {
                let metadata = providers::analyze(&request.url, settings).await?;
                // The other items of a gallery are queued behind this one and
                // can share it, rather than each asking the platform again.
                providers::remember_analysis(&request.url, settings, &metadata);
                metadata
            }
        }
    };

    let item = match providers::select_entry(metadata, request.entry) {
        Ok(item) => providers::prepare(item, settings).await,
        Err(err) => Err(err),
    };
    let result = match item {
        Ok(item) => download_analyzed(task_id, request, settings, control, on_update, &temp_dir, item).await,
        Err(err) => Err(err),
    };
    // The streams it named may be what failed; a retry has to look again. A
    // pause is not a failure, and resuming may still use it.
    if matches!(&result, Err(err) if !matches!(err, AppError::Canceled)) {
        providers::forget_analysis(&request.url);
    }
    result
}

async fn download_analyzed(
    task_id: &str,
    request: &DownloadRequest,
    settings: &Settings,
    control: Arc<TaskControl>,
    on_update: UpdateSink<'_>,
    temp_dir: &Path,
    metadata: MediaMetadata,
) -> AppResult<DownloadOutcome> {
    if control.interrupted() {
        return Err(AppError::Canceled);
    }

    // Before anything is named after it: the file, the finished task and the
    // history entry all take their title from here.
    let metadata = with_page_title(metadata, request);
    let headers = request
        .source
        .as_ref()
        .map(SourceContext::headers)
        .unwrap_or_default();

    let plan = plan::for_request(&metadata, request)?;

    // A merge is the one step with a hard external dependency. Failing here,
    // before any bytes move, is much better than after a 2 GB download.
    if plan.needs_merge && !plan.needs_engine && crate::tools::ffmpeg_path().is_none() {
        return Err(AppError::FfmpegMissing);
    }
    if plan.convert_to.is_some() && crate::tools::ffmpeg_path().is_none() {
        return Err(AppError::FfmpegMissing);
    }

    let output_dir = PathBuf::from(
        request
            .output_dir
            .clone()
            .unwrap_or_else(|| settings.download_dir.clone()),
    );
    std::fs::create_dir_all(&output_dir)?;

    let final_path = target_path(&output_dir, settings, &metadata, &plan, request.entry.is_some());
    log_debug!(
        "downloader",
        "task {task_id} -> {} (merge={}, engine={})",
        final_path.display(),
        plan.needs_merge,
        plan.needs_engine
    );

    let mut aggregate = Aggregator::new(plan.estimated_bytes, plan.stage_count());

    let produced = if plan.needs_engine {
        run_via_engine(task_id, &plan, &metadata, &headers, settings, &control, &mut aggregate, on_update, temp_dir)
            .await?
    } else {
        match run_natively(task_id, &plan, &metadata, settings, &control, &mut aggregate, on_update, temp_dir).await {
            Ok(path) => path,
            // A CDN that refuses a plain ranged GET is not necessarily refusing
            // us: several hosts only serve a stream to the session that
            // resolved it. The engine holds that session, so the transfer is
            // handed over to it rather than reported as a dead end.
            Err(AppError::Forbidden { status, detail }) if engine_can_take_over(&metadata, &plan) => {
                log_info!(
                    "downloader",
                    "task {task_id}: the host refused the direct transfer with HTTP {status}; retrying through the engine"
                );
                cleanup_task_files(task_id);
                aggregate = Aggregator::new(plan.estimated_bytes, 1 + u32::from(plan.convert_to.is_some()));
                run_via_engine(task_id, &plan, &metadata, &headers, settings, &control, &mut aggregate, on_update, temp_dir)
                    .await
                    // The engine failing here is the same refusal seen from
                    // another angle; the access error is the one that explains
                    // it to the user.
                    .map_err(|err| match err {
                        AppError::Canceled | AppError::DiskFull | AppError::Permission(_) => err,
                        _ => AppError::Forbidden { status, detail },
                    })?
            }
            Err(err) => return Err(err),
        }
    };

    // A song shared from a music service is named and tagged as that song,
    // not as the upload its sound came from.
    let produced = match metadata.music.as_ref() {
        Some(song) if plan.video.is_none() && plan.image.is_none() => {
            on_update(aggregate.processing(DownloadStage::Finalizing, Some(98.0)));
            tag_song(task_id, &produced, song, settings, &control, temp_dir).await?
        }
        _ => produced,
    };

    // Move into place last, so the user's folder never holds a partial file.
    on_update(aggregate.processing(DownloadStage::Finalizing, Some(99.0)));
    let final_path = finalize(&produced, &final_path)?;
    let file_size = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);

    let mut done = aggregate.processing(DownloadStage::Done, Some(100.0));
    done.received_bytes = file_size;
    done.total_bytes = Some(file_size);
    on_update(done);

    log_info!(
        "downloader",
        "task {task_id} completed: {} ({} bytes)",
        final_path.display(),
        file_size
    );

    Ok(DownloadOutcome {
        output_path: final_path,
        file_size,
        label: plan.label.clone(),
        quality_label: plan.quality_label.clone(),
        container: plan.container.clone(),
        metadata,
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_natively(
    task_id: &str,
    plan: &DownloadPlan,
    metadata: &MediaMetadata,
    settings: &Settings,
    control: &Arc<TaskControl>,
    aggregate: &mut Aggregator,
    on_update: UpdateSink<'_>,
    temp_dir: &Path,
) -> AppResult<PathBuf> {
    let client = crate::net::client(settings)?;
    let mut stage_index = 1u32;

    let mut video_path: Option<PathBuf> = None;
    let mut audio_path: Option<PathBuf> = None;

    if let Some(format) = plan.video.as_ref().or(plan.image.as_ref()) {
        let stage = if plan.image.is_some() {
            DownloadStage::Image
        } else {
            DownloadStage::Video
        };
        aggregate.enter(stage, stage_index);
        let path = ffmpeg::intermediate_path(temp_dir, task_id, "v", &format.container);
        let written =
            fetch_format(&client, format, metadata.stream_page(), &path, settings, control, aggregate, on_update)
                .await?;
        aggregate.finish_stage(written);
        video_path = Some(path);
        stage_index += 1;
    }

    if let Some(format) = plan.audio.as_ref() {
        aggregate.enter(DownloadStage::Audio, stage_index);
        let path = ffmpeg::intermediate_path(temp_dir, task_id, "a", &format.container);
        let written =
            fetch_format(&client, format, metadata.stream_page(), &path, settings, control, aggregate, on_update)
                .await?;
        aggregate.finish_stage(written);
        audio_path = Some(path);
    }

    let merged = match (video_path.as_ref(), audio_path.as_ref()) {
        (Some(video), Some(audio)) => {
            on_update(aggregate.processing(DownloadStage::Merging, None));
            let output = ffmpeg::intermediate_path(temp_dir, task_id, "m", &plan.container);
            let mut sink = |progress: ffmpeg::FfmpegProgress| {
                on_update(aggregate.processing(DownloadStage::Merging, progress.percent));
            };
            ffmpeg::merge(
                video,
                audio,
                &output,
                plan.video.as_ref().and_then(|format| format.vcodec.as_deref()),
                plan.audio.as_ref().and_then(|format| format.acodec.as_deref()),
                metadata.duration_sec,
                Arc::clone(control),
                &mut sink,
            )
            .await?;
            let _ = std::fs::remove_file(video);
            let _ = std::fs::remove_file(audio);
            output
        }
        (Some(single), None) => single.clone(),
        (None, Some(single)) => single.clone(),
        (None, None) => return Err(AppError::Unsupported("nothing to download".into())),
    };

    let Some(target_container) = plan.convert_to.as_ref() else {
        return Ok(merged);
    };

    on_update(aggregate.processing(DownloadStage::Converting, None));
    let converted = ffmpeg::intermediate_path(temp_dir, task_id, "c", target_container);
    let mut sink = |progress: ffmpeg::FfmpegProgress| {
        on_update(aggregate.processing(DownloadStage::Converting, progress.percent));
    };
    ffmpeg::convert(
        &merged,
        &converted,
        metadata.duration_sec,
        plan.audio.as_ref().and_then(|f| f.abr),
        settings.hardware_acceleration,
        Arc::clone(control),
        &mut sink,
    )
    .await?;
    let _ = std::fs::remove_file(&merged);
    Ok(converted)
}

#[allow(clippy::too_many_arguments)]
async fn run_via_engine(
    task_id: &str,
    plan: &DownloadPlan,
    metadata: &MediaMetadata,
    headers: &[(String, String)],
    settings: &Settings,
    control: &Arc<TaskControl>,
    aggregate: &mut Aggregator,
    on_update: UpdateSink<'_>,
    temp_dir: &Path,
) -> AppResult<PathBuf> {
    aggregate.enter(DownloadStage::Video, 1);

    let target = engine_target(plan, metadata, headers);
    let staged = ffmpeg::intermediate_path(temp_dir, task_id, "e", &plan.container);

    let mut sink = |sample: http::ProgressSample| {
        let progress = aggregate.sample(&sample);
        on_update(progress);
    };

    engine_dl::run(
        engine_dl::EngineDownload {
            url: &target.url,
            format_selector: &target.selector,
            target: &staged,
            merge_container: target.merge_container.as_deref(),
            // A queued download is always the whole thing. Fetching a piece of
            // a link is the editor's, and goes through its own manager.
            section: None,
            force_keyframes: false,
            headers: &target.headers,
        },
        settings,
        Arc::clone(control),
        &mut sink,
    )
    .await?;

    let produced = engine_dl::resolve_output(&staged)?;

    let Some(target_container) = plan.convert_to.as_ref() else {
        return Ok(produced);
    };

    on_update(aggregate.processing(DownloadStage::Converting, None));
    let converted = ffmpeg::intermediate_path(temp_dir, task_id, "c", target_container);
    let mut convert_sink = |progress: ffmpeg::FfmpegProgress| {
        on_update(aggregate.processing(DownloadStage::Converting, progress.percent));
    };
    ffmpeg::convert(
        &produced,
        &converted,
        metadata.duration_sec,
        plan.audio.as_ref().and_then(|f| f.abr),
        settings.hardware_acceleration,
        Arc::clone(control),
        &mut convert_sink,
    )
    .await?;
    let _ = std::fs::remove_file(&produced);
    Ok(converted)
}

/// What the engine is pointed at, and what it is asked to pick there.
#[derive(Debug, PartialEq)]
pub(crate) struct EngineTarget {
    pub(crate) url: String,
    pub(crate) selector: String,
    pub(crate) merge_container: Option<String>,
    pub(crate) headers: Vec<(String, String)>,
}

pub(crate) fn engine_target(
    plan: &DownloadPlan,
    metadata: &MediaMetadata,
    headers: &[(String, String)],
) -> EngineTarget {
    // The page reader's streams are addresses it found in a page's markup, and
    // `generic-0` is a name only this app gives them: asked for that format on
    // the page, the engine has none by that name. The stream is the address
    // itself -- a manifest, which the engine reads as readily as a page -- so
    // that is what it is pointed at, to take the best of what it lists.
    let found_on_page = (metadata.provider_id == providers::generic::PROVIDER_ID)
        .then(|| plan.primary())
        .flatten()
        .and_then(|format| Some((format, format.url.as_deref()?)));

    if let Some((_, url)) = found_on_page {
        let mut headers = headers.to_vec();
        // A manifest a page published is often served only to that page.
        if !headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("referer")) {
            if let Some(referer) = referer_for(&metadata.canonical_url) {
                headers.push(("Referer".to_string(), referer));
            }
        }
        // Asked of the plan rather than of the stream. A page's player stream
        // carries picture and sound together, and in Audio mode the plan takes
        // only the sound out of it: its container is then m4a or mp3, which
        // the engine refuses to merge into before it fetches a byte. The sound
        // alone is taken instead, and the plan's own conversion turns it into
        // the file that was asked for.
        let audio_only = plan.video.is_none() && plan.image.is_none();
        return EngineTarget {
            url: url.to_string(),
            selector: if audio_only { "ba/b" } else { "bv*+ba/b" }.to_string(),
            // What it picks may arrive as two streams, and they are put
            // together in the container the plan promised -- when that is one
            // the engine merges into at all. Otherwise it chooses its own
            // rather than refusing to start.
            merge_container: (!audio_only && engine_merges_into(&plan.container))
                .then(|| plan.container.clone()),
            headers,
        };
    }

    EngineTarget {
        url: metadata.stream_page().to_string(),
        selector: plan.selector_for_engine(),
        merge_container: plan.needs_merge.then(|| plan.container.clone()),
        headers: headers.to_vec(),
    }
}

/// Whether `--merge-output-format` takes `container`. The engine's own list,
/// and it refuses anything else while reading its options.
fn engine_merges_into(container: &str) -> bool {
    matches!(container, "avi" | "flv" | "mkv" | "mov" | "mp4" | "webm")
}

/// The title of the tab a handed-over link was playing in, for a result with
/// nothing better to be called.
///
/// A page's player fetches addresses like `master.m3u8` and `index.mp4`, and
/// read on its own that is all a web page or a bare file can be named after
/// -- a film saved as "master.mp4". The tab the user pressed Download in knew
/// what it was showing. Only for those two kinds of result: a platform the app
/// knows names its own media better than a tab title does.
fn with_page_title(mut metadata: MediaMetadata, request: &DownloadRequest) -> MediaMetadata {
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());
    if let (Some(_), Some(title)) = (request.source.as_ref(), title) {
        if matches!(metadata.platform, PlatformId::Generic | PlatformId::Direct) {
            metadata.title = title.to_string();
        }
    }
    metadata
}

/// Whether the engine is in a position to retry a transfer the direct fetcher
/// was refused. Its format ids only mean something for metadata it produced
/// itself, so a direct or generic read cannot be handed over.
fn engine_can_take_over(metadata: &MediaMetadata, plan: &DownloadPlan) -> bool {
    !plan.needs_engine
        && metadata.provider_id == crate::providers::engine::PROVIDER_ID
        && crate::tools::engine_path().is_some()
}

/// The page the stream belongs to, as a `Referer`.
///
/// Media CDNs commonly serve a stream only to requests that name the page it is
/// embedded in. Providers that report their own headers already carry one; this
/// fills the gap for the ones that do not.
fn referer_for(page_url: &str) -> Option<String> {
    let rest = page_url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| {
        let scheme = page_url.split_once("://").map(|(s, _)| s).unwrap_or("https");
        format!("{scheme}://{host}/")
    })
}

#[allow(clippy::too_many_arguments)]
async fn fetch_format(
    client: &reqwest::Client,
    format: &MediaFormat,
    page_url: &str,
    target: &Path,
    settings: &Settings,
    control: &Arc<TaskControl>,
    aggregate: &mut Aggregator,
    on_update: UpdateSink<'_>,
) -> AppResult<u64> {
    let url = format
        .url
        .as_deref()
        .ok_or_else(|| AppError::Unsupported("the selected stream has no address".into()))?;

    let mut headers = format.http_headers.clone();
    if !headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("referer")) {
        if let Some(referer) = referer_for(page_url) {
            headers.push(("Referer".to_string(), referer));
        }
    }

    let mut sink = |sample: http::ProgressSample| {
        let progress = aggregate.sample(&sample);
        on_update(progress);
    };

    http::fetch(
        client,
        http::FetchOptions {
            url,
            headers: &headers,
            target,
            low_resource: settings.low_resource_mode,
        },
        Arc::clone(control),
        &mut sink,
    )
    .await
}

/// Write a song's tags and cover into the file that was downloaded for it.
///
/// Worth doing, not worth failing over: a file whose tags could not be written
/// still holds the song, so anything short of a cancel keeps it as it is.
async fn tag_song(
    task_id: &str,
    produced: &Path,
    song: &MusicTags,
    settings: &Settings,
    control: &Arc<TaskControl>,
    temp_dir: &Path,
) -> AppResult<PathBuf> {
    if crate::tools::ffmpeg_path().is_none() {
        return Ok(produced.to_path_buf());
    }
    let container = produced
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let cover = match song.cover_url.as_deref().filter(|_| ffmpeg::holds_cover(&container)) {
        Some(url) => fetch_cover(url, &temp_dir.join(format!("{task_id}.cover")), settings).await,
        None => None,
    };

    let tagged = ffmpeg::intermediate_path(temp_dir, task_id, "t", &container);
    let args = ffmpeg::tag_args(
        produced,
        cover.as_ref().map(|(path, art)| (path.as_path(), *art)),
        &tagged,
        &song_tags(song),
    );
    let mut quiet = |_: ffmpeg::FfmpegProgress| {};
    let result = ffmpeg::run_with_progress(&args, None, Arc::clone(control), &mut quiet).await;
    if let Some((path, _)) = &cover {
        let _ = std::fs::remove_file(path);
    }

    match result {
        Ok(()) => {
            let _ = std::fs::remove_file(produced);
            Ok(tagged)
        }
        Err(AppError::Canceled) => {
            let _ = std::fs::remove_file(&tagged);
            Err(AppError::Canceled)
        }
        Err(err) => {
            log_info!("downloader", "task {task_id}: the song's tags could not be written: {err}");
            let _ = std::fs::remove_file(&tagged);
            Ok(produced.to_path_buf())
        }
    }
}

/// The tags a song is written with, under the names FFmpeg maps onto each
/// container's own.
fn song_tags(song: &MusicTags) -> Vec<(&'static str, String)> {
    let mut tags = vec![("title", song.title.clone()), ("artist", song.artist_line())];
    if let Some(album) = &song.album {
        tags.push(("album", album.clone()));
    }
    if let Some(artist) = &song.album_artist {
        tags.push(("album_artist", artist.clone()));
    }
    if let Some(number) = song.track_number {
        tags.push(("track", number.to_string()));
    }
    if let Some(date) = &song.release_date {
        tags.push(("date", date.clone()));
    }
    tags
}

/// Fetch a cover picture, and say whether it can go into a file as it is.
async fn fetch_cover(url: &str, stem: &Path, settings: &Settings) -> Option<(PathBuf, ffmpeg::CoverArt)> {
    let client = crate::net::client(settings).ok()?;
    let response = client.get(url).send().await.ok()?.error_for_status().ok()?;
    let bytes = response.bytes().await.ok()?;
    // A cover is a few hundred kilobytes; anything far larger is not one.
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return None;
    }
    let (extension, art) = if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ("jpg", ffmpeg::CoverArt::Copy)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        ("png", ffmpeg::CoverArt::Copy)
    } else {
        ("img", ffmpeg::CoverArt::Encode)
    };
    let path = stem.with_extension(extension);
    std::fs::write(&path, &bytes).ok()?;
    Some((path, art))
}

/// Where a finished download goes.
///
/// A song is named for itself -- who made it, then its name -- and one queued
/// from an album or playlist goes into a folder named after it, so the songs
/// of an album arrive together.
fn target_path(
    output_dir: &Path,
    settings: &Settings,
    metadata: &MediaMetadata,
    plan: &DownloadPlan,
    from_collection: bool,
) -> PathBuf {
    if let Some(song) = metadata.music.as_ref().filter(|_| plan.video.is_none()) {
        let folder = song
            .collection
            .as_deref()
            .filter(|_| from_collection)
            .map(filename::sanitize_component)
            .filter(|name| !name.is_empty());
        let dir = match folder {
            Some(name) => {
                let dir = output_dir.join(filename::truncate_stem(&name, 80));
                match std::fs::create_dir_all(&dir) {
                    Ok(()) => dir,
                    Err(_) => output_dir.to_path_buf(),
                }
            }
            None => output_dir.to_path_buf(),
        };
        let artists = song.artist_line();
        return filename::build_output_path(
            &dir,
            "{creator} - {title}",
            &filename::NameContext {
                title: &song.title,
                creator: (!artists.is_empty()).then_some(artists.as_str()),
                quality: &plan.quality_label,
                platform: metadata.platform.slug(),
                date: "",
                ext: &plan.container,
            },
        );
    }

    // An episode from a channel that licenses anime is filed the way media
    // servers expect a series: its own folder, a folder per season, and the
    // season and episode in the name.
    if let Some(episode) = crate::providers::anime::episode_of(metadata).filter(|_| plan.video.is_some()) {
        let series = filename::sanitize_component(&episode.series);
        if !series.is_empty() {
            let dir = output_dir
                .join(filename::truncate_stem(&series, 80))
                .join(format!("Season {:02}", episode.season));
            if std::fs::create_dir_all(&dir).is_ok() {
                let name = format!("{series} - S{:02}E{:02}", episode.season, episode.number);
                return filename::build_output_path(
                    &dir,
                    "{title}",
                    &filename::NameContext {
                        title: &name,
                        creator: None,
                        quality: &plan.quality_label,
                        platform: metadata.platform.slug(),
                        date: "",
                        ext: &plan.container,
                    },
                );
            }
        }
    }

    let date = metadata
        .upload_date
        .as_deref()
        .and_then(|raw| {
            (raw.len() == 8).then(|| format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8]))
        })
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    filename::build_output_path(
        output_dir,
        &settings.filename_template,
        &filename::NameContext {
            title: &metadata.title,
            creator: metadata.creator.as_deref(),
            quality: &plan.quality_label,
            platform: metadata.platform.slug(),
            date: &date,
            ext: &plan.container,
        },
    )
}

/// Move the finished file out of temp. A rename across volumes fails on
/// Windows, so fall back to a copy when the download folder is on another disk.
fn finalize(produced: &Path, requested: &Path) -> AppResult<PathBuf> {
    let target = if requested.exists() {
        let stem = requested
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("download");
        let ext = requested.extension().and_then(|e| e.to_str()).unwrap_or("");
        filename::unique_path(requested.parent().unwrap_or(Path::new(".")), stem, ext)
    } else {
        requested.to_path_buf()
    };

    match std::fs::rename(produced, &target) {
        Ok(()) => Ok(target),
        Err(_) => {
            std::fs::copy(produced, &target)?;
            let _ = std::fs::remove_file(produced);
            Ok(target)
        }
    }
}

/// Remove any partial artifacts a cancelled task left behind.
pub fn cleanup_task_files(task_id: &str) {
    let Ok(dir) = paths::temp_dir() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(task_id) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Label used on the queue card before a plan exists.
pub fn provisional_label(request: &DownloadRequest) -> String {
    let quality = match request.quality {
        crate::model::QualityPreference::Best => "Best".to_string(),
        crate::model::QualityPreference::Auto => "Auto".to_string(),
        crate::model::QualityPreference::MaxHeight { height } => format!("{height}p"),
        crate::model::QualityPreference::AudioBitrate { kbps } => format!("{kbps} kbps"),
    };
    match request.container.as_deref() {
        Some(container) => format!("{quality} - {}", container.to_uppercase()),
        None => quality,
    }
}

pub fn platform_of(request: &DownloadRequest) -> PlatformId {
    request
        .platform
        .unwrap_or_else(|| providers::detect::detect_platform(&request.url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FormatKind;

    #[test]
    fn a_referer_is_the_origin_of_the_page_the_media_came_from() {
        assert_eq!(
            referer_for("https://www.tiktok.com/@someone/video/123?is_from_webapp=1").as_deref(),
            Some("https://www.tiktok.com/")
        );
        assert_eq!(
            referer_for("http://example.org/watch").as_deref(),
            Some("http://example.org/")
        );
    }

    #[test]
    fn an_address_with_no_host_yields_no_referer() {
        assert_eq!(referer_for("not-a-url"), None);
        assert_eq!(referer_for("https://"), None);
    }

    fn request(source: Option<SourceContext>, title: Option<&str>) -> DownloadRequest {
        DownloadRequest {
            url: "https://cdn.example/hls/master.m3u8".into(),
            mode: crate::model::DownloadMode::Video,
            quality: crate::model::QualityPreference::Best,
            video_format_id: None,
            audio_format_id: None,
            container: None,
            watermark: crate::model::WatermarkPreference::Any,
            output_dir: None,
            title: title.map(str::to_string),
            thumbnail_url: None,
            platform: None,
            entry: None,
            audio_language: None,
            source,
        }
    }

    fn handed_over() -> Option<SourceContext> {
        Some(SourceContext {
            page_url: Some("https://site.example/watch/5".into()),
            ..SourceContext::default()
        })
    }

    /// A stream the page reader found, as `generic::push_format` lists one.
    fn page_stream(kind: FormatKind) -> MediaMetadata {
        let mut format = providers::image_format("generic-0", "https://cdn.example/hls/master.m3u8", None, None, Vec::new());
        format.kind = kind;
        format.container = if kind == FormatKind::Audio { "mp3" } else { "mp4" }.into();
        format.protocol = "m3u8".into();
        format.has_video = kind == FormatKind::Muxed;
        format.has_audio = true;
        format.needs_engine_download = true;
        format.quality_label = "Original".into();

        let mut metadata = providers::tests_support::blank();
        metadata.provider_id = providers::generic::PROVIDER_ID.into();
        metadata.platform = PlatformId::Generic;
        metadata.canonical_url = "https://site.example/watch/5".into();
        metadata.media_kind = if kind == FormatKind::Audio {
            crate::model::MediaKind::Audio
        } else {
            crate::model::MediaKind::Video
        };
        metadata.formats = vec![format];
        metadata
    }

    #[test]
    fn a_stream_found_on_a_page_is_fetched_from_its_own_address() {
        let metadata = page_stream(FormatKind::Muxed);
        let plan = plan::for_request(&metadata, &request(None, None)).unwrap();
        assert!(plan.needs_engine);

        let target = engine_target(&plan, &metadata, &[]);
        assert_eq!(target.url, "https://cdn.example/hls/master.m3u8");
        assert_eq!(target.selector, "bv*+ba/b", "`generic-0` means nothing to the engine");
        assert_eq!(target.merge_container.as_deref(), Some("mp4"));
        assert_eq!(
            target.headers,
            vec![("Referer".to_string(), "https://site.example/".to_string())]
        );

        // The browser's own Referer is kept rather than replaced.
        let sent = vec![("Referer".to_string(), "https://player.example/".to_string())];
        assert_eq!(engine_target(&plan, &metadata, &sent).headers, sent);

        let metadata = page_stream(FormatKind::Audio);
        let plan = plan::for_request(
            &metadata,
            &DownloadRequest {
                mode: crate::model::DownloadMode::Audio,
                ..request(None, None)
            },
        )
        .unwrap();
        let target = engine_target(&plan, &metadata, &[]);
        assert_eq!(target.selector, "ba/b");
        assert_eq!(target.merge_container, None);
    }

    #[test]
    fn the_sound_of_a_page_stream_is_taken_without_a_merge() {
        // A player stream carries picture and sound together, and Audio mode
        // takes the sound out of it into m4a or mp3 -- neither of which the
        // engine merges into. It refuses `--merge-output-format m4a` outright.
        let metadata = page_stream(FormatKind::Muxed);
        for container in [None, Some("mp3")] {
            let plan = plan::for_request(
                &metadata,
                &DownloadRequest {
                    mode: crate::model::DownloadMode::Audio,
                    container: container.map(str::to_string),
                    ..request(None, None)
                },
            )
            .unwrap();
            assert!(plan.needs_engine);
            assert!(plan.video.is_none(), "the plan wants the sound only");

            let target = engine_target(&plan, &metadata, &[]);
            assert_eq!(target.url, "https://cdn.example/hls/master.m3u8");
            assert_eq!(target.selector, "ba/b");
            assert_eq!(target.merge_container, None);
            // The plan's own conversion is what makes the audio file.
            assert_eq!(plan.convert_to.as_deref(), Some(container.unwrap_or("m4a")));
        }

        // A picture in a container the engine cannot merge into is fetched
        // without asking it to.
        let mut metadata = page_stream(FormatKind::Muxed);
        metadata.formats[0].container = "3gp".into();
        let plan = plan::for_request(&metadata, &request(None, None)).unwrap();
        let target = engine_target(&plan, &metadata, &[]);
        assert_eq!(target.selector, "bv*+ba/b");
        assert_eq!(target.merge_container, None);
    }

    #[test]
    fn a_stream_the_engine_listed_is_still_asked_for_by_its_id_on_its_page() {
        let mut metadata = page_stream(FormatKind::Muxed);
        metadata.provider_id = providers::engine::PROVIDER_ID.into();
        metadata.formats[0].id = "hls-1080".into();
        let plan = plan::for_request(&metadata, &request(None, None)).unwrap();

        let headers = vec![("Origin".to_string(), "https://player.example".to_string())];
        let target = engine_target(&plan, &metadata, &headers);
        assert_eq!(target.url, "https://site.example/watch/5");
        assert_eq!(target.selector, "hls-1080");
        assert_eq!(target.headers, headers);
    }

    #[test]
    fn a_handed_over_stream_is_named_after_the_tab_it_played_in() {
        let mut metadata = providers::tests_support::blank();
        metadata.platform = PlatformId::Generic;
        metadata.title = "master".into();

        let titled = with_page_title(metadata.clone(), &request(handed_over(), Some("  Bölüm 5 ")));
        assert_eq!(titled.title, "Bölüm 5");

        // A link typed in keeps what the source called it, and so does a
        // handed-over one with no title of its own.
        assert_eq!(with_page_title(metadata.clone(), &request(None, Some("Bölüm 5"))).title, "master");
        assert_eq!(with_page_title(metadata.clone(), &request(handed_over(), Some("  "))).title, "master");

        // A platform the app knows names its media better than a tab does.
        metadata.platform = PlatformId::Youtube;
        assert_eq!(with_page_title(metadata, &request(handed_over(), Some("Bölüm 5"))).title, "master");
    }
}
