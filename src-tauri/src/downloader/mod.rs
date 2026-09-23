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
    DownloadProgress, DownloadRequest, DownloadStage, MediaFormat, MediaMetadata, PlatformId,
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
    let metadata = match providers::recent_analysis(&request.url, settings) {
        Some(metadata) => {
            log_debug!("downloader", "task {task_id}: reusing the analysis made moments ago");
            metadata
        }
        None => {
            let metadata = providers::analyze(&request.url, settings).await?;
            // The other items of a gallery are queued behind this one and can
            // share it, rather than each asking the platform again.
            providers::remember_analysis(&request.url, settings, &metadata);
            metadata
        }
    };

    let result = match providers::select_entry(metadata, request.entry) {
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

    let plan = plan::build(
        &metadata,
        request.mode,
        request.quality,
        request.video_format_id.as_deref(),
        request.audio_format_id.as_deref(),
        request.container.as_deref(),
        request.watermark,
    )?;

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

    let final_path = target_path(&output_dir, settings, &metadata, &plan);
    log_debug!(
        "downloader",
        "task {task_id} -> {} (merge={}, engine={})",
        final_path.display(),
        plan.needs_merge,
        plan.needs_engine
    );

    let mut aggregate = Aggregator::new(plan.estimated_bytes, plan.stage_count());

    let produced = if plan.needs_engine {
        run_via_engine(task_id, &plan, &metadata, settings, &control, &mut aggregate, on_update, temp_dir).await?
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
                run_via_engine(task_id, &plan, &metadata, settings, &control, &mut aggregate, on_update, temp_dir)
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
            fetch_format(&client, format, &metadata.canonical_url, &path, settings, control, aggregate, on_update)
                .await?;
        aggregate.finish_stage(written);
        video_path = Some(path);
        stage_index += 1;
    }

    if let Some(format) = plan.audio.as_ref() {
        aggregate.enter(DownloadStage::Audio, stage_index);
        let path = ffmpeg::intermediate_path(temp_dir, task_id, "a", &format.container);
        let written =
            fetch_format(&client, format, &metadata.canonical_url, &path, settings, control, aggregate, on_update)
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
    settings: &Settings,
    control: &Arc<TaskControl>,
    aggregate: &mut Aggregator,
    on_update: UpdateSink<'_>,
    temp_dir: &Path,
) -> AppResult<PathBuf> {
    aggregate.enter(DownloadStage::Video, 1);

    let selector = plan.selector_for_engine();
    let staged = ffmpeg::intermediate_path(temp_dir, task_id, "e", &plan.container);

    let mut sink = |sample: http::ProgressSample| {
        let progress = aggregate.sample(&sample);
        on_update(progress);
    };

    engine_dl::run(
        engine_dl::EngineDownload {
            url: &metadata.canonical_url,
            format_selector: &selector,
            target: &staged,
            merge_container: plan.needs_merge.then_some(plan.container.as_str()),
            // A queued download is always the whole thing. Fetching a piece of
            // a link is the editor's, and goes through its own manager.
            section: None,
            force_keyframes: false,
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

fn target_path(
    output_dir: &Path,
    settings: &Settings,
    metadata: &MediaMetadata,
    plan: &DownloadPlan,
) -> PathBuf {
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
}
