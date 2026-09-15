//! Turning a user's choice into a concrete set of streams.
//!
//! This is where "Best quality" stops being a word and becomes specific
//! formats. It is pure: metadata and a request go in, a plan comes out, and no
//! network or filesystem is involved -- which is what makes it straightforward
//! to test the selection rules directly.

use crate::error::{AppError, AppResult};
use crate::model::{
    DownloadMode, FormatKind, MediaFormat, MediaMetadata, QualityPreference, WatermarkPreference,
    WatermarkSupport,
};

#[derive(Debug, Clone)]
pub struct DownloadPlan {
    pub video: Option<MediaFormat>,
    pub audio: Option<MediaFormat>,
    pub image: Option<MediaFormat>,
    /// Two streams that FFmpeg has to interleave into one file.
    pub needs_merge: bool,
    /// At least one chosen stream is segmented, so the engine downloads it.
    pub needs_engine: bool,
    /// Format expression handed to the engine when it does the downloading.
    pub engine_selector: Option<String>,
    /// Extension of the finished file.
    pub container: String,
    /// Re-encode or repackage requested by the user, e.g. m4a -> mp3.
    pub convert_to: Option<String>,
    /// Short label for the queue card: "1080p - MP4".
    pub label: String,
    pub quality_label: String,
    pub estimated_bytes: Option<u64>,
}

impl DownloadPlan {
    /// The stream that carries the bulk of the bytes, used for stage naming.
    pub fn primary(&self) -> Option<&MediaFormat> {
        self.video.as_ref().or(self.image.as_ref()).or(self.audio.as_ref())
    }

    /// Format expression that describes this plan to the engine.
    ///
    /// [`Self::engine_selector`] is only set when the engine was always going
    /// to do the downloading. This rebuilds the same expression from the chosen
    /// streams, for when the native transfer has to hand the work over.
    pub fn selector_for_engine(&self) -> String {
        if let Some(selector) = &self.engine_selector {
            return selector.clone();
        }
        match (&self.video, &self.audio, &self.image) {
            (Some(video), Some(audio), _) => format!("{}+{}", video.id, audio.id),
            (Some(video), None, _) => video.id.clone(),
            (None, Some(audio), _) => audio.id.clone(),
            (None, None, Some(image)) => image.id.clone(),
            (None, None, None) => "best".to_string(),
        }
    }

    /// Number of steps the progress readout will walk through.
    ///
    /// When the engine performs the download it fetches every stream and does
    /// any merge inside a single opaque step, so counting the streams
    /// separately would leave the UI stuck on "1 of 3" for the whole transfer.
    pub fn stage_count(&self) -> u32 {
        if self.needs_engine {
            return 1 + u32::from(self.convert_to.is_some());
        }
        let streams = u32::from(self.video.is_some())
            + u32::from(self.audio.is_some())
            + u32::from(self.image.is_some());
        streams.max(1) + u32::from(self.needs_merge || self.convert_to.is_some())
    }
}

/// Whether `container` can carry `codec` as a plain stream copy.
///
/// Only the two strict containers are described. Matroska takes anything, and
/// an unrecognised container is left to FFmpeg to accept or refuse -- claiming
/// to know better would only block a merge that would have worked.
pub fn container_holds_video(container: &str, codec: &str) -> bool {
    let codec = codec.trim().to_ascii_lowercase();
    if codec.is_empty() {
        return true;
    }
    let starts_with_any = |families: &[&str]| families.iter().any(|f| codec.starts_with(f));
    match container {
        "webm" => starts_with_any(&["vp8", "vp08", "vp9", "vp09", "av1", "av01"]),
        "mp4" | "m4v" | "mov" => {
            starts_with_any(&["avc", "h264", "h.264", "hev", "hvc", "h265", "av1", "av01", "mp4v"])
        }
        _ => true,
    }
}

pub fn container_holds_audio(container: &str, codec: &str) -> bool {
    let codec = codec.trim().to_ascii_lowercase();
    if codec.is_empty() {
        return true;
    }
    let starts_with_any = |families: &[&str]| families.iter().any(|f| codec.starts_with(f));
    match container {
        "webm" => starts_with_any(&["opus", "vorbis"]),
        "mp4" | "m4v" | "mov" => {
            starts_with_any(&["mp4a", "aac", "alac", "mp3", "ac-3", "ac3", "ec-3"])
        }
        _ => true,
    }
}

/// The container that can hold this codec pair without re-encoding either of
/// them.
///
/// Both codecs have to be considered together. WebM accepts only VP8/VP9/AV1
/// video alongside Opus or Vorbis audio, so a VP9 video merged with an AAC
/// track -- an everyday pairing -- is not a WebM file, and FFmpeg refuses that
/// merge outright rather than converting. Matroska is the honest answer
/// whenever the two streams share no container: it takes any pair, still as a
/// stream copy.
fn default_container(video: Option<&MediaFormat>, audio: Option<&MediaFormat>) -> String {
    match (video, audio) {
        (Some(v), audio) => {
            let vcodec = v.vcodec.as_deref().unwrap_or("").to_ascii_lowercase();
            // Whatever audio actually ends up in the file: the separate stream
            // when one is being merged in, otherwise the video's own track.
            let acodec = audio
                .and_then(|a| a.acodec.as_deref())
                .or(v.acodec.as_deref())
                .unwrap_or("")
                .to_ascii_lowercase();

            // Nothing is being merged, so the stream arrives in the container
            // the source already chose for it. Renaming it would only add a
            // pointless remux pass.
            if audio.is_none() && !v.container.is_empty() {
                return v.container.clone();
            }

            let fits = |container: &str| {
                container_holds_video(container, &vcodec) && container_holds_audio(container, &acodec)
            };

            // An unreported video codec cannot be claimed as WebM-compatible;
            // MP4 is the safer guess for an unknown pairing.
            if !vcodec.is_empty() && fits("webm") {
                "webm".to_string()
            } else if fits("mp4") {
                "mp4".to_string()
            } else {
                "mkv".to_string()
            }
        }
        (None, Some(a)) => a.container.clone(),
        (None, None) => "bin".to_string(),
    }
}

fn watermark_allows(format: &MediaFormat, preference: WatermarkPreference) -> bool {
    match preference {
        WatermarkPreference::Any => true,
        // `None` means the source did not say. Excluding those would reject
        // every platform that simply does not watermark, so unknown passes.
        WatermarkPreference::CleanOnly => format.watermarked != Some(true),
    }
}

pub fn build(
    metadata: &MediaMetadata,
    mode: DownloadMode,
    quality: QualityPreference,
    video_format_id: Option<&str>,
    audio_format_id: Option<&str>,
    requested_container: Option<&str>,
    watermark: WatermarkPreference,
) -> AppResult<DownloadPlan> {
    if watermark == WatermarkPreference::CleanOnly
        && metadata.watermark_support == WatermarkSupport::WatermarkedOnly
    {
        return Err(AppError::Unsupported(
            "this source only publishes a watermarked rendition".into(),
        ));
    }

    let allowed: Vec<&MediaFormat> = metadata
        .formats
        .iter()
        .filter(|format| watermark_allows(format, watermark))
        .collect();

    if allowed.is_empty() {
        return Err(AppError::Unsupported(
            "no stream matches the selected options".into(),
        ));
    }

    // The options were chosen for one piece of media and may be applied to
    // another: every item of a gallery is queued with the choices made for the
    // first. A video among photos is still downloaded as a video, and a
    // container or stream picked for one kind means nothing for the other.
    let effective = effective_mode(mode, &allowed);
    let (requested_container, video_format_id, audio_format_id) = if effective == mode {
        (requested_container, video_format_id, audio_format_id)
    } else {
        (None, None, None)
    };

    match effective {
        DownloadMode::Image => build_image(&allowed, requested_container),
        DownloadMode::Audio => build_audio(&allowed, quality, audio_format_id, requested_container),
        DownloadMode::Video => build_video(
            &allowed,
            quality,
            video_format_id,
            audio_format_id,
            requested_container,
        ),
    }
}

/// The requested mode when the media offers it, otherwise what the media is:
/// a video first, then a picture, then sound.
fn effective_mode(requested: DownloadMode, allowed: &[&MediaFormat]) -> DownloadMode {
    let has_video = allowed.iter().any(|format| format.has_video);
    let has_audio = allowed.iter().any(|format| format.has_audio);
    let has_image = allowed.iter().any(|format| format.kind == FormatKind::Image);

    let offered = match requested {
        DownloadMode::Video => has_video,
        DownloadMode::Audio => has_audio,
        DownloadMode::Image => has_image,
    };
    if offered {
        requested
    } else if has_video {
        DownloadMode::Video
    } else if has_image {
        DownloadMode::Image
    } else if has_audio {
        DownloadMode::Audio
    } else {
        requested
    }
}

fn build_image(allowed: &[&MediaFormat], requested_container: Option<&str>) -> AppResult<DownloadPlan> {
    let image = allowed
        .iter()
        .filter(|format| format.kind == FormatKind::Image)
        .max_by_key(|format| format.pixels())
        .copied()
        .ok_or_else(|| AppError::Unsupported("no image stream was offered".into()))?;

    let source_container = image.container.clone();
    let container = requested_container.unwrap_or(&source_container).to_string();
    let convert_to = (container != source_container).then(|| container.clone());

    Ok(DownloadPlan {
        quality_label: image.quality_label.clone(),
        label: format!("{} - {}", image.quality_label, container.to_uppercase()),
        estimated_bytes: image.best_known_size(),
        needs_engine: image.needs_engine_download,
        engine_selector: image.needs_engine_download.then(|| image.id.clone()),
        image: Some(image.clone()),
        video: None,
        audio: None,
        needs_merge: false,
        container,
        convert_to,
    })
}

fn build_audio(
    allowed: &[&MediaFormat],
    quality: QualityPreference,
    audio_format_id: Option<&str>,
    requested_container: Option<&str>,
) -> AppResult<DownloadPlan> {
    let audio_only: Vec<&MediaFormat> = allowed
        .iter()
        .filter(|format| format.kind == FormatKind::Audio)
        .copied()
        .collect();

    // Falling back to a muxed stream means downloading video bytes we throw
    // away, so it is only done when the source has no audio-only rendition.
    let (chosen, from_muxed) = if let Some(id) = audio_format_id {
        let format = allowed
            .iter()
            .find(|format| format.id == id)
            .copied()
            .ok_or_else(|| AppError::Unsupported("the chosen audio stream is unavailable".into()))?;
        (format, format.has_video)
    } else if !audio_only.is_empty() {
        (pick_audio(&audio_only, quality), false)
    } else {
        let muxed = allowed
            .iter()
            .filter(|format| format.has_audio)
            .max_by(|a, b| compare_by_bitrate(a, b))
            .copied()
            .ok_or_else(|| AppError::Unsupported("no audio stream was offered".into()))?;
        (muxed, true)
    };

    let source_container = chosen.container.clone();
    // Extracting audio out of a video container always needs a rewrite.
    let container = requested_container
        .map(str::to_string)
        .unwrap_or_else(|| if from_muxed { "m4a".to_string() } else { source_container.clone() });
    let convert_to = (from_muxed || container != source_container).then(|| container.clone());

    Ok(DownloadPlan {
        quality_label: chosen.quality_label.clone(),
        label: format!("{} - {}", chosen.quality_label, container.to_uppercase()),
        estimated_bytes: chosen.best_known_size(),
        needs_engine: chosen.needs_engine_download,
        engine_selector: chosen.needs_engine_download.then(|| chosen.id.clone()),
        audio: Some(chosen.clone()),
        video: None,
        image: None,
        needs_merge: false,
        container,
        convert_to,
    })
}

fn build_video(
    allowed: &[&MediaFormat],
    quality: QualityPreference,
    video_format_id: Option<&str>,
    audio_format_id: Option<&str>,
    requested_container: Option<&str>,
) -> AppResult<DownloadPlan> {
    // An explicit pair from the advanced selector overrides every rule below.
    if video_format_id.is_some() || audio_format_id.is_some() {
        return build_explicit(allowed, video_format_id, audio_format_id, requested_container);
    }

    let muxed: Vec<&MediaFormat> = allowed
        .iter()
        .filter(|format| format.kind == FormatKind::Muxed)
        .copied()
        .collect();
    let video_only: Vec<&MediaFormat> = allowed
        .iter()
        .filter(|format| format.kind == FormatKind::Video)
        .copied()
        .collect();
    let audio_only: Vec<&MediaFormat> = allowed
        .iter()
        .filter(|format| format.kind == FormatKind::Audio)
        .copied()
        .collect();

    let ceiling = match quality {
        QualityPreference::MaxHeight { height } => Some(height),
        _ => None,
    };

    let best_muxed = pick_video(&muxed, ceiling);
    let best_split = pick_video(&video_only, ceiling);

    let chosen_split = match quality {
        // "Auto" avoids a merge when a single-stream rendition is close enough,
        // because merging needs FFmpeg and doubles the work.
        QualityPreference::Auto => match (best_muxed, best_split) {
            (Some(m), Some(s)) if m.pixels() * 2 >= s.pixels() => None,
            (Some(_), Some(s)) => Some(s),
            (None, Some(s)) => Some(s),
            _ => None,
        },
        // "Best" takes the highest resolution available, merging if that is
        // what it takes.
        _ => match (best_muxed, best_split) {
            (Some(m), Some(s)) if s.pixels() > m.pixels() => Some(s),
            (None, Some(s)) => Some(s),
            _ => None,
        },
    };

    if let Some(video) = chosen_split {
        let audio = pick_audio_opt(&audio_only, QualityPreference::Best);
        return Ok(assemble(video, audio, requested_container));
    }

    if let Some(video) = best_muxed {
        return Ok(assemble(video, None, requested_container));
    }

    // No video at all: fall back to whatever the source does offer rather than
    // failing outright, so an audio-only post still downloads.
    if let Some(video) = pick_video(&video_only, None) {
        let audio = pick_audio_opt(&audio_only, QualityPreference::Best);
        return Ok(assemble(video, audio, requested_container));
    }
    if !audio_only.is_empty() {
        return build_audio(allowed, quality, None, requested_container);
    }
    if allowed.iter().any(|format| format.kind == FormatKind::Image) {
        return build_image(allowed, requested_container);
    }

    Err(AppError::Unsupported(
        "no video stream matched the selected quality".into(),
    ))
}

fn build_explicit(
    allowed: &[&MediaFormat],
    video_format_id: Option<&str>,
    audio_format_id: Option<&str>,
    requested_container: Option<&str>,
) -> AppResult<DownloadPlan> {
    let find = |id: &str| {
        allowed
            .iter()
            .find(|format| format.id == id)
            .copied()
            .ok_or_else(|| AppError::Unsupported("the chosen stream is unavailable".into()))
    };

    let video = video_format_id.map(find).transpose()?;
    let audio = audio_format_id.map(find).transpose()?;

    match (video, audio) {
        (Some(video), audio) => {
            // A muxed video stream plus a separate audio track would give two
            // audio tracks in the output; drop the redundant one.
            let audio = audio.filter(|_| !video.has_audio);
            Ok(assemble(video, audio, requested_container))
        }
        (None, Some(audio)) => {
            let container = requested_container.unwrap_or(&audio.container).to_string();
            let convert_to = (container != audio.container).then(|| container.clone());
            Ok(DownloadPlan {
                quality_label: audio.quality_label.clone(),
                label: format!("{} - {}", audio.quality_label, container.to_uppercase()),
                estimated_bytes: audio.best_known_size(),
                needs_engine: audio.needs_engine_download,
                engine_selector: audio.needs_engine_download.then(|| audio.id.clone()),
                audio: Some(audio.clone()),
                video: None,
                image: None,
                needs_merge: false,
                container,
                convert_to,
            })
        }
        (None, None) => Err(AppError::Unsupported("no stream was selected".into())),
    }
}

fn assemble(
    video: &MediaFormat,
    audio: Option<&MediaFormat>,
    requested_container: Option<&str>,
) -> DownloadPlan {
    let needs_merge = audio.is_some();
    let source_container = default_container(Some(video), audio);
    let container = requested_container.unwrap_or(&source_container).to_string();

    // A merge already rewrites the container, so a separate conversion pass is
    // only needed when no merge is happening.
    let convert_to = (!needs_merge && container != video.container).then(|| container.clone());

    let needs_engine =
        video.needs_engine_download || audio.is_some_and(|format| format.needs_engine_download);

    let engine_selector = needs_engine.then(|| match audio {
        Some(audio) => format!("{}+{}", video.id, audio.id),
        None => video.id.clone(),
    });

    let estimated_bytes = match (video.best_known_size(), audio.and_then(|a| a.best_known_size())) {
        (Some(v), Some(a)) => Some(v + a),
        (Some(v), None) => Some(v),
        (None, _) => None,
    };

    DownloadPlan {
        quality_label: video.quality_label.clone(),
        label: format!("{} - {}", video.quality_label, container.to_uppercase()),
        estimated_bytes,
        video: Some(video.clone()),
        audio: audio.cloned(),
        image: None,
        needs_merge,
        needs_engine,
        engine_selector,
        container,
        convert_to,
    }
}

/// How widely a video codec plays back without extra components installed.
/// Higher is safer. This only ever breaks a tie *at the same resolution* --
/// resolution is compared first, so asking for the best quality still reaches
/// 4K on sources that only publish it as VP9 or AV1.
fn playback_compatibility(format: &MediaFormat) -> u8 {
    let codec = format.vcodec.as_deref().unwrap_or("").to_ascii_lowercase();
    if codec.starts_with("avc") || codec.starts_with("h264") {
        3
    } else if codec.starts_with("vp9") || codec.starts_with("vp09") || codec.starts_with("vp8") {
        2
    } else if codec.starts_with("av01") || codec.starts_with("av1") {
        1
    } else {
        0
    }
}

fn pick_video<'a>(formats: &[&'a MediaFormat], ceiling: Option<u32>) -> Option<&'a MediaFormat> {
    let within: Vec<&&MediaFormat> = formats
        .iter()
        .filter(|format| match (ceiling, format.height) {
            (Some(limit), Some(height)) => height <= limit,
            (Some(_), None) => false,
            (None, _) => true,
        })
        .collect();

    // Nothing at or below the ceiling: take the smallest available instead of
    // silently jumping to something larger than the user asked for.
    if within.is_empty() {
        return formats
            .iter()
            .min_by_key(|format| format.pixels())
            .copied();
    }

    within
        .into_iter()
        .max_by(|a, b| {
            a.pixels()
                .cmp(&b.pixels())
                // Comparing bitrate across codecs is meaningless -- AV1 at half
                // the bitrate looks the same -- so compatibility is settled
                // first, and bitrate only separates two streams of the same
                // codec and resolution.
                .then_with(|| playback_compatibility(a).cmp(&playback_compatibility(b)))
                .then_with(|| compare_by_bitrate(a, b))
        })
        .copied()
}

fn pick_audio<'a>(formats: &[&'a MediaFormat], quality: QualityPreference) -> &'a MediaFormat {
    if let QualityPreference::AudioBitrate { kbps } = quality {
        let target = f64::from(kbps);
        if let Some(best) = formats
            .iter()
            .filter(|format| format.abr.is_some_and(|abr| abr <= target * 1.05))
            .max_by(|a, b| compare_by_bitrate(a, b))
        {
            return best;
        }
        // Nothing at or under the requested bitrate: the closest one above it
        // is a better answer than nothing.
        if let Some(closest) = formats.iter().min_by(|a, b| {
            let da = (a.abr.unwrap_or(0.0) - target).abs();
            let db = (b.abr.unwrap_or(0.0) - target).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        }) {
            return closest;
        }
    }

    formats
        .iter()
        .max_by(|a, b| compare_by_bitrate(a, b))
        .copied()
        .unwrap_or(formats[0])
}

fn pick_audio_opt<'a>(
    formats: &[&'a MediaFormat],
    quality: QualityPreference,
) -> Option<&'a MediaFormat> {
    (!formats.is_empty()).then(|| pick_audio(formats, quality))
}

/// Highest useful bitrate a format reports, in kbps.
fn bitrate_of(format: &MediaFormat) -> f64 {
    format.abr.or(format.tbr).or(format.vbr).unwrap_or(0.0)
}

fn compare_by_bitrate(a: &MediaFormat, b: &MediaFormat) -> std::cmp::Ordering {
    bitrate_of(a)
        .partial_cmp(&bitrate_of(b))
        .unwrap_or(std::cmp::Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MediaKind, PlatformId};

    fn format(
        id: &str,
        kind: FormatKind,
        height: Option<u32>,
        abr: Option<f64>,
        container: &str,
    ) -> MediaFormat {
        MediaFormat {
            id: id.to_string(),
            kind,
            container: container.to_string(),
            protocol: "https".to_string(),
            has_video: matches!(kind, FormatKind::Video | FormatKind::Muxed),
            has_audio: matches!(kind, FormatKind::Audio | FormatKind::Muxed),
            width: height.map(|h| h * 16 / 9),
            height,
            fps: Some(30.0),
            vcodec: matches!(kind, FormatKind::Video | FormatKind::Muxed)
                .then(|| "avc1.640028".to_string()),
            acodec: matches!(kind, FormatKind::Audio | FormatKind::Muxed)
                .then(|| "mp4a.40.2".to_string()),
            tbr: Some(1000.0),
            vbr: None,
            abr,
            filesize: Some(1_000_000),
            filesize_approx: None,
            quality_label: height
                .map(|h| format!("{h}p"))
                .or_else(|| abr.map(|a| format!("{a} kbps")))
                .unwrap_or_else(|| "Original".to_string()),
            watermarked: None,
            note: None,
            needs_engine_download: false,
            url: Some(format!("https://cdn.test/{id}")),
            http_headers: Vec::new(),
        }
    }

    fn metadata(formats: Vec<MediaFormat>) -> MediaMetadata {
        MediaMetadata {
            url: "https://example.test/x".into(),
            canonical_url: "https://example.test/x".into(),
            platform: PlatformId::Youtube,
            platform_label: "YouTube".into(),
            provider_id: "engine".into(),
            media_kind: MediaKind::Video,
            title: "t".into(),
            creator: None,
            description: None,
            thumbnail_url: None,
            duration_sec: None,
            view_count: None,
            like_count: None,
            upload_date: None,
            is_live: false,
            formats,
            entry_count: None,
            watermark_support: WatermarkSupport::NotApplicable,
            warnings: Vec::new(),
            entries: Vec::new(),
        }
    }

    fn plan(meta: &MediaMetadata, quality: QualityPreference) -> DownloadPlan {
        build(
            meta,
            DownloadMode::Video,
            quality,
            None,
            None,
            None,
            WatermarkPreference::Any,
        )
        .unwrap()
    }

    #[test]
    fn best_merges_when_the_split_stream_is_higher_resolution() {
        let meta = metadata(vec![
            format("18", FormatKind::Muxed, Some(360), Some(96.0), "mp4"),
            format("137", FormatKind::Video, Some(1080), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = plan(&meta, QualityPreference::Best);
        assert!(result.needs_merge);
        assert_eq!(result.video.as_ref().unwrap().id, "137");
        assert_eq!(result.audio.as_ref().unwrap().id, "140");
        assert_eq!(result.container, "mp4");
        assert_eq!(result.quality_label, "1080p");
    }

    #[test]
    fn auto_prefers_a_single_stream_when_it_is_close_enough() {
        let meta = metadata(vec![
            format("22", FormatKind::Muxed, Some(1080), Some(96.0), "mp4"),
            format("400", FormatKind::Video, Some(1440), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = plan(&meta, QualityPreference::Auto);
        assert!(!result.needs_merge);
        assert_eq!(result.video.as_ref().unwrap().id, "22");
    }

    #[test]
    fn auto_merges_when_the_single_stream_is_a_full_step_behind() {
        // The common case: a site caps its muxed rendition at 720p while
        // offering 1080p as separate streams. Auto should take the 1080p.
        let meta = metadata(vec![
            format("22", FormatKind::Muxed, Some(720), Some(96.0), "mp4"),
            format("137", FormatKind::Video, Some(1080), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = plan(&meta, QualityPreference::Auto);
        assert!(result.needs_merge);
        assert_eq!(result.video.as_ref().unwrap().id, "137");
    }

    #[test]
    fn auto_still_merges_when_the_single_stream_is_far_worse() {
        let meta = metadata(vec![
            format("18", FormatKind::Muxed, Some(240), Some(64.0), "mp4"),
            format("313", FormatKind::Video, Some(2160), None, "webm"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = plan(&meta, QualityPreference::Auto);
        assert!(result.needs_merge);
        assert_eq!(result.video.as_ref().unwrap().id, "313");
    }

    #[test]
    fn at_equal_resolution_the_more_compatible_codec_wins() {
        let mut av1 = format("av1-480", FormatKind::Video, Some(480), None, "mp4");
        av1.vcodec = Some("av01.0.05M.08".into());
        av1.tbr = Some(400.0);
        let mut h264 = format("h264-480", FormatKind::Video, Some(480), None, "mp4");
        h264.vcodec = Some("avc1.4d401e".into());
        h264.tbr = Some(300.0);

        let meta = metadata(vec![av1, h264, format("140", FormatKind::Audio, None, Some(128.0), "m4a")]);
        let result = plan(&meta, QualityPreference::MaxHeight { height: 480 });
        assert_eq!(
            result.video.as_ref().unwrap().id,
            "h264-480",
            "H.264 should win a tie against AV1 at the same resolution"
        );
    }

    #[test]
    fn compatibility_never_costs_resolution() {
        let mut av1_4k = format("av1-2160", FormatKind::Video, Some(2160), None, "webm");
        av1_4k.vcodec = Some("av01.0.12M.08".into());
        let mut h264_1080 = format("h264-1080", FormatKind::Video, Some(1080), None, "mp4");
        h264_1080.vcodec = Some("avc1.640028".into());

        let meta = metadata(vec![
            av1_4k,
            h264_1080,
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = plan(&meta, QualityPreference::Best);
        assert_eq!(
            result.video.as_ref().unwrap().id,
            "av1-2160",
            "best quality must still reach 4K even when only AV1 offers it"
        );
    }

    #[test]
    fn a_height_ceiling_is_respected() {
        let meta = metadata(vec![
            format("137", FormatKind::Video, Some(1080), None, "mp4"),
            format("136", FormatKind::Video, Some(720), None, "mp4"),
            format("135", FormatKind::Video, Some(480), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = plan(&meta, QualityPreference::MaxHeight { height: 720 });
        assert_eq!(result.video.as_ref().unwrap().height, Some(720));
    }

    #[test]
    fn a_ceiling_below_everything_falls_back_to_the_smallest() {
        let meta = metadata(vec![
            format("137", FormatKind::Video, Some(1080), None, "mp4"),
            format("136", FormatKind::Video, Some(720), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = plan(&meta, QualityPreference::MaxHeight { height: 144 });
        assert_eq!(result.video.as_ref().unwrap().height, Some(720));
    }

    #[test]
    fn vp9_with_opus_lands_in_webm_and_h264_with_aac_in_mp4() {
        let mut vp9 = format("313", FormatKind::Video, Some(2160), None, "webm");
        vp9.vcodec = Some("vp9".into());
        let mut opus = format("251", FormatKind::Audio, None, Some(160.0), "webm");
        opus.acodec = Some("opus".into());
        let meta = metadata(vec![vp9, opus]);
        assert_eq!(plan(&meta, QualityPreference::Best).container, "webm");

        let meta = metadata(vec![
            format("137", FormatKind::Video, Some(1080), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        assert_eq!(plan(&meta, QualityPreference::Best).container, "mp4");
    }

    #[test]
    fn a_codec_pair_no_container_holds_falls_back_to_matroska() {
        // WebM cannot hold AAC and MP4 cannot hold VP9, so neither stream would
        // survive a copy into the other's container. This is the pairing that
        // made FFmpeg refuse the merge with "Only VP8 or VP9 or AV1 video and
        // Vorbis or Opus audio ... are supported for WebM".
        let mut vp9 = format("313", FormatKind::Video, Some(2160), None, "webm");
        vp9.vcodec = Some("vp9".into());
        let meta = metadata(vec![vp9, format("140", FormatKind::Audio, None, Some(128.0), "m4a")]);
        assert_eq!(plan(&meta, QualityPreference::Best).container, "mkv");

        let mut h264 = format("137", FormatKind::Video, Some(1080), None, "mp4");
        h264.vcodec = Some("avc1.640028".into());
        let mut opus = format("251", FormatKind::Audio, None, Some(160.0), "webm");
        opus.acodec = Some("opus".into());
        let meta = metadata(vec![h264, opus]);
        assert_eq!(plan(&meta, QualityPreference::Best).container, "mkv");
    }

    #[test]
    fn av1_pairs_with_either_container_depending_on_the_audio() {
        let mut av1 = format("399", FormatKind::Video, Some(1080), None, "mp4");
        av1.vcodec = Some("av01.0.08M.08".into());

        let mut opus = format("251", FormatKind::Audio, None, Some(160.0), "webm");
        opus.acodec = Some("opus".into());
        let meta = metadata(vec![av1.clone(), opus]);
        assert_eq!(plan(&meta, QualityPreference::Best).container, "webm");

        let meta = metadata(vec![av1, format("140", FormatKind::Audio, None, Some(128.0), "m4a")]);
        assert_eq!(plan(&meta, QualityPreference::Best).container, "mp4");
    }

    #[test]
    fn a_single_stream_keeps_the_container_it_arrived_in() {
        // No merge means no remux, so the source's own container is the answer
        // and `convert_to` stays empty.
        let mut muxed = format("18", FormatKind::Muxed, Some(360), Some(96.0), "webm");
        muxed.vcodec = Some("vp9".into());
        muxed.acodec = Some("opus".into());
        let result = plan(&metadata(vec![muxed]), QualityPreference::Best);
        assert_eq!(result.container, "webm");
        assert!(result.convert_to.is_none());
        assert!(!result.needs_merge);
    }

    #[test]
    fn audio_mode_prefers_an_audio_only_stream() {
        let meta = metadata(vec![
            format("18", FormatKind::Muxed, Some(360), Some(96.0), "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = build(
            &meta,
            DownloadMode::Audio,
            QualityPreference::Best,
            None,
            None,
            None,
            WatermarkPreference::Any,
        )
        .unwrap();
        assert_eq!(result.audio.as_ref().unwrap().id, "140");
        assert!(result.video.is_none());
        assert!(result.convert_to.is_none());
    }

    #[test]
    fn audio_mode_extracts_from_video_when_that_is_all_there_is() {
        let meta = metadata(vec![format("18", FormatKind::Muxed, Some(360), Some(96.0), "mp4")]);
        let result = build(
            &meta,
            DownloadMode::Audio,
            QualityPreference::Best,
            None,
            None,
            None,
            WatermarkPreference::Any,
        )
        .unwrap();
        assert_eq!(result.convert_to.as_deref(), Some("m4a"));
    }

    #[test]
    fn a_requested_audio_bitrate_picks_the_closest_at_or_below_it() {
        let meta = metadata(vec![
            format("a", FormatKind::Audio, None, Some(320.0), "m4a"),
            format("b", FormatKind::Audio, None, Some(192.0), "m4a"),
            format("c", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = build(
            &meta,
            DownloadMode::Audio,
            QualityPreference::AudioBitrate { kbps: 192 },
            None,
            None,
            None,
            WatermarkPreference::Any,
        )
        .unwrap();
        assert_eq!(result.audio.as_ref().unwrap().id, "b");
    }

    #[test]
    fn a_requested_container_triggers_conversion() {
        let meta = metadata(vec![format("140", FormatKind::Audio, None, Some(128.0), "m4a")]);
        let result = build(
            &meta,
            DownloadMode::Audio,
            QualityPreference::Best,
            None,
            None,
            Some("mp3"),
            WatermarkPreference::Any,
        )
        .unwrap();
        assert_eq!(result.container, "mp3");
        assert_eq!(result.convert_to.as_deref(), Some("mp3"));
    }

    #[test]
    fn watermark_only_sources_are_refused_when_clean_is_required() {
        let mut meta = metadata(vec![format("d", FormatKind::Muxed, Some(720), Some(128.0), "mp4")]);
        meta.watermark_support = WatermarkSupport::WatermarkedOnly;
        let err = build(
            &meta,
            DownloadMode::Video,
            QualityPreference::Best,
            None,
            None,
            None,
            WatermarkPreference::CleanOnly,
        )
        .unwrap_err();
        assert_eq!(err.code(), "unsupported");
    }

    #[test]
    fn clean_only_skips_the_stamped_rendition() {
        let mut stamped = format("download_addr", FormatKind::Muxed, Some(1080), Some(128.0), "mp4");
        stamped.watermarked = Some(true);
        let mut clean = format("play_addr", FormatKind::Muxed, Some(720), Some(128.0), "mp4");
        clean.watermarked = Some(false);

        let mut meta = metadata(vec![stamped, clean]);
        meta.watermark_support = WatermarkSupport::CleanAvailable;

        let result = build(
            &meta,
            DownloadMode::Video,
            QualityPreference::Best,
            None,
            None,
            None,
            WatermarkPreference::CleanOnly,
        )
        .unwrap();
        assert_eq!(result.video.as_ref().unwrap().id, "play_addr");
    }

    #[test]
    fn an_explicit_pair_is_used_verbatim() {
        let meta = metadata(vec![
            format("137", FormatKind::Video, Some(1080), None, "mp4"),
            format("136", FormatKind::Video, Some(720), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = build(
            &meta,
            DownloadMode::Video,
            QualityPreference::Best,
            Some("136"),
            Some("140"),
            None,
            WatermarkPreference::Any,
        )
        .unwrap();
        assert_eq!(result.video.as_ref().unwrap().id, "136");
        assert!(result.needs_merge);
    }

    #[test]
    fn a_muxed_choice_does_not_pick_up_a_second_audio_track() {
        let meta = metadata(vec![
            format("18", FormatKind::Muxed, Some(360), Some(96.0), "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = build(
            &meta,
            DownloadMode::Video,
            QualityPreference::Best,
            Some("18"),
            Some("140"),
            None,
            WatermarkPreference::Any,
        )
        .unwrap();
        assert!(!result.needs_merge);
        assert!(result.audio.is_none());
    }

    #[test]
    fn an_unknown_format_id_is_an_error() {
        let meta = metadata(vec![format("137", FormatKind::Video, Some(1080), None, "mp4")]);
        assert!(build(
            &meta,
            DownloadMode::Video,
            QualityPreference::Best,
            Some("nope"),
            None,
            None,
            WatermarkPreference::Any,
        )
        .is_err());
    }

    #[test]
    fn a_segmented_stream_is_routed_to_the_engine() {
        let mut video = format("hls-1080", FormatKind::Muxed, Some(1080), Some(128.0), "mp4");
        video.needs_engine_download = true;
        video.protocol = "m3u8_native".into();
        let meta = metadata(vec![video]);
        let result = plan(&meta, QualityPreference::Best);
        assert!(result.needs_engine);
        assert_eq!(result.engine_selector.as_deref(), Some("hls-1080"));
    }

    #[test]
    fn an_engine_download_counts_as_one_step() {
        let mut video = format("hls-1080", FormatKind::Muxed, Some(1080), Some(128.0), "mp4");
        video.needs_engine_download = true;
        video.protocol = "m3u8_native".into();
        let meta = metadata(vec![video]);
        // The engine fetches and merges opaquely, so there is one step to show.
        assert_eq!(plan(&meta, QualityPreference::Best).stage_count(), 1);
    }

    #[test]
    fn stage_count_covers_every_step() {
        let meta = metadata(vec![
            format("137", FormatKind::Video, Some(1080), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        // Two downloads plus a merge.
        assert_eq!(plan(&meta, QualityPreference::Best).stage_count(), 3);

        let meta = metadata(vec![format("18", FormatKind::Muxed, Some(360), Some(96.0), "mp4")]);
        assert_eq!(plan(&meta, QualityPreference::Best).stage_count(), 1);
    }

    #[test]
    fn video_mode_degrades_to_audio_when_no_video_exists() {
        let meta = metadata(vec![format("140", FormatKind::Audio, None, Some(128.0), "m4a")]);
        let result = plan(&meta, QualityPreference::Best);
        assert!(result.video.is_none());
        assert_eq!(result.audio.as_ref().unwrap().id, "140");
    }

    fn image(id: &str, width: u32, height: u32, container: &str) -> MediaFormat {
        let mut picture = format(id, FormatKind::Image, None, None, container);
        picture.width = Some(width);
        picture.height = Some(height);
        picture.quality_label = format!("{width}x{height}");
        picture
    }

    fn build_with(
        meta: &MediaMetadata,
        mode: DownloadMode,
        container: Option<&str>,
    ) -> DownloadPlan {
        build(
            meta,
            mode,
            QualityPreference::Best,
            None,
            None,
            container,
            WatermarkPreference::Any,
        )
        .unwrap()
    }

    #[test]
    fn image_mode_takes_the_largest_picture() {
        let meta = metadata(vec![image("small", 320, 400, "jpg"), image("full", 1440, 1800, "jpg")]);
        let result = build_with(&meta, DownloadMode::Image, None);
        assert_eq!(result.image.as_ref().unwrap().id, "full");
        assert_eq!(result.container, "jpg");
        assert!(result.convert_to.is_none());
    }

    #[test]
    fn a_video_among_photos_is_still_downloaded_as_a_video() {
        // Every item of a gallery is queued with the options chosen for the
        // first, which here was a photo.
        let meta = metadata(vec![
            format("137", FormatKind::Video, Some(1080), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = build_with(&meta, DownloadMode::Image, Some("png"));
        assert_eq!(result.video.as_ref().unwrap().id, "137");
        assert_eq!(result.container, "mp4", "a picture format must not reach a video");
        assert!(result.image.is_none());
    }

    #[test]
    fn a_photo_is_downloaded_as_a_photo_whatever_mode_was_chosen() {
        let meta = metadata(vec![image("full", 1080, 1350, "jpg")]);
        for mode in [DownloadMode::Video, DownloadMode::Audio] {
            let result = build_with(&meta, mode, Some("mp3"));
            assert_eq!(result.image.as_ref().unwrap().id, "full");
            assert_eq!(result.container, "jpg");
            assert!(result.convert_to.is_none());
        }
    }

    #[test]
    fn audio_mode_on_a_photo_post_with_a_soundtrack_takes_the_sound() {
        let meta = metadata(vec![
            image("full", 1080, 1350, "jpg"),
            format("audio", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        let result = build_with(&meta, DownloadMode::Audio, None);
        assert_eq!(result.audio.as_ref().unwrap().id, "audio");

        let result = build_with(&meta, DownloadMode::Video, None);
        assert_eq!(result.image.as_ref().unwrap().id, "full");
    }

    #[test]
    fn a_requested_picture_format_converts_the_image() {
        let meta = metadata(vec![image("full", 1080, 1350, "jpg")]);
        let result = build_with(&meta, DownloadMode::Image, Some("png"));
        assert_eq!(result.container, "png");
        assert_eq!(result.convert_to.as_deref(), Some("png"));
    }

    #[test]
    fn estimated_size_adds_both_streams() {
        let meta = metadata(vec![
            format("137", FormatKind::Video, Some(1080), None, "mp4"),
            format("140", FormatKind::Audio, None, Some(128.0), "m4a"),
        ]);
        assert_eq!(plan(&meta, QualityPreference::Best).estimated_bytes, Some(2_000_000));
    }
}
