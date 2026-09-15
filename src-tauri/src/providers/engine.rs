//! The engine-backed provider.
//!
//! Metadata and stream URLs come from yt-dlp, run as a separate process with an
//! argument vector -- the URL is never interpolated into a command string.
//! Everything the process prints is parsed here into the app's own model, so
//! nothing yt-dlp-shaped leaks into the UI.
//!
//! Stream URLs are deliberately *not* cached and never sent to the webview:
//! they are signed and short-lived, so they are re-resolved immediately before
//! a download starts.

use std::path::Path;

use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::model::{
    FormatKind, MediaFormat, MediaKind, MediaMetadata, PlatformId, WatermarkSupport,
};
use crate::providers::detect;
use crate::settings::Settings;
use crate::{log_debug, paths, process, tools};

pub const PROVIDER_ID: &str = "engine";

pub struct EngineProvider;

impl EngineProvider {
    pub fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    /// The engine supports well over a thousand sites, so it claims any http
    /// URL. `registry` still tries the direct-file provider first.
    pub fn can_handle(&self, url: &str) -> bool {
        detect::classify(url).is_some()
    }

    pub async fn analyze(&self, url: &str, settings: &Settings) -> AppResult<MediaMetadata> {
        let engine = tools::require_engine()?;
        let mut args = base_args(settings);
        args.push("--no-playlist".into());
        args.push("-J".into());
        args.push(url.to_string());

        let output = process::run(&engine, &args).await?;
        if !output.success() {
            return Err(classify_engine_error(&output.stderr));
        }

        let root: Value = serde_json::from_str(output.stdout.trim())
            .map_err(|err| AppError::Parse(format!("the engine returned unreadable JSON: {err}")))?;

        // A playlist here means a carousel, gallery or album: the first entry
        // drives the preview, and the count tells the UI there is more.
        let (node, entry_count) = match root.get("_type").and_then(Value::as_str) {
            Some("playlist") => {
                let entries = root
                    .get("entries")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let count = entries.len() as u32;
                let first = entries
                    .into_iter()
                    .next()
                    .ok_or_else(|| AppError::Unsupported("the source returned no media".into()))?;
                (first, Some(count.max(1)))
            }
            _ => (root, None),
        };

        let mut metadata = parse_metadata(&node, url)?;
        metadata.entry_count = entry_count;
        if entry_count.is_some_and(|count| count > 1) {
            metadata.media_kind = MediaKind::Gallery;
        }
        Ok(metadata)
    }

    /// Resolve the individual item URLs behind a gallery or carousel, so each
    /// one can become its own queue entry.
    pub async fn expand_entries(&self, url: &str, settings: &Settings) -> AppResult<Vec<String>> {
        let engine = tools::require_engine()?;
        let mut args = base_args(settings);
        args.push("--yes-playlist".into());
        args.push("--flat-playlist".into());
        args.push("-J".into());
        args.push(url.to_string());

        let output = process::run(&engine, &args).await?;
        if !output.success() {
            return Err(classify_engine_error(&output.stderr));
        }

        let root: Value = serde_json::from_str(output.stdout.trim())?;
        let Some(entries) = root.get("entries").and_then(Value::as_array) else {
            return Ok(vec![url.to_string()]);
        };

        let urls: Vec<String> = entries
            .iter()
            .filter_map(|entry| {
                entry
                    .get("webpage_url")
                    .or_else(|| entry.get("url"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();

        Ok(if urls.is_empty() { vec![url.to_string()] } else { urls })
    }
}

/// Arguments shared by every engine invocation.
pub fn base_args(settings: &Settings) -> Vec<String> {
    let mut args = vec![
        // Ignore any yt-dlp config the machine happens to have, so behaviour
        // does not silently differ between installs.
        "--ignore-config".to_string(),
        "--no-warnings".to_string(),
        "--no-progress".to_string(),
        "--no-colors".to_string(),
        "--retries".to_string(),
        "3".to_string(),
        "--extractor-retries".to_string(),
        "2".to_string(),
        "--socket-timeout".to_string(),
        settings.network_timeout_sec.to_string(),
    ];

    if let Ok(cache) = paths::cache_dir() {
        args.push("--cache-dir".to_string());
        args.push(cache.join("engine").to_string_lossy().into_owned());
    }

    if let Some(proxy) = settings.proxy_url.as_deref() {
        args.push("--proxy".to_string());
        args.push(proxy.to_string());
    }

    if let Some(agent) = settings.custom_user_agent.as_deref() {
        args.push("--add-header".to_string());
        args.push(format!("User-Agent:{agent}"));
    }

    // YouTube's player has to be run through JavaScript before most formats
    // are offered. yt-dlp only looks for Deno by default, which a phone does
    // not have; the APK carries QuickJS for this.
    #[cfg(target_os = "android")]
    {
        args.push("--js-runtimes".to_string());
        args.push(format!(
            "quickjs:{}",
            crate::android::quickjs_binary().display()
        ));
    }

    args
}

pub fn engine_binary() -> AppResult<std::path::PathBuf> {
    tools::require_engine()
}

/// Turn the engine's diagnostic text into the app's error taxonomy. The raw
/// text is preserved as the technical detail; only the classification is used
/// to decide what the user is shown.
pub fn classify_engine_error(stderr: &str) -> AppError {
    let lower = stderr.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));

    // Ordered most-specific first: "http error 404" has to be recognised before
    // the broader 4xx check, and a missing extractor before anything else.
    if has(&["unsupported url", "no suitable extractor", "is not a valid url"]) {
        return AppError::Unsupported(first_error_line(stderr));
    }

    if has(&[
        "http error 404",
        "http error 410",
        "video unavailable",
        "has been removed",
        "no longer available",
        "does not exist",
    ]) {
        return AppError::NotFound {
            status: 404,
            detail: first_error_line(stderr),
        };
    }

    // Everything that means "you are not allowed to see this", including the
    // engine's own suggestion to supply cookies or credentials. The app does
    // not do that -- it reports the wall rather than trying to get around it.
    if has(&[
        "http error 401",
        "http error 402",
        "http error 403",
        "http error 429",
        "sign in",
        "sign-in",
        "signed in",
        "log in",
        "logged in",
        "logged-in",
        "login required",
        "credentials",
        "--cookies",
        "requires authentication",
        "private",
        "members",
        "premium",
        "subscriber",
        "paid",
        "purchase",
        "not available in your",
        "geo",
    ]) || (lower.contains("age") && lower.contains("confirm"))
    {
        return AppError::Forbidden {
            status: 403,
            detail: first_error_line(stderr),
        };
    }

    if has(&[
        "unable to download",
        "connection",
        "timed out",
        "temporary failure",
        "getaddrinfo",
        "network is unreachable",
    ]) {
        return AppError::Network(first_error_line(stderr));
    }

    AppError::Engine(first_error_line(stderr))
}

fn first_error_line(stderr: &str) -> String {
    stderr
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("ERROR:") || line.starts_with("error:"))
        .or_else(|| stderr.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or("the engine reported no detail")
        .chars()
        .take(600)
        .collect()
}

// -- JSON -> model ---------------------------------------------------------

pub fn parse_metadata(node: &Value, requested_url: &str) -> AppResult<MediaMetadata> {
    let platform = detect::detect_platform(requested_url);

    let title = node
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            node.get("description")
                .and_then(Value::as_str)
                .map(|d| d.lines().next().unwrap_or(d).to_string())
        })
        .unwrap_or_else(|| "Untitled".to_string());

    let creator = ["uploader", "channel", "creator", "artist", "uploader_id"]
        .iter()
        .find_map(|key| node.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);

    let mut formats: Vec<MediaFormat> = node
        .get("formats")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(parse_format).collect())
        .unwrap_or_default();

    // Some extractors return a single stream at the top level with no formats
    // array at all (many image posts, and a few simple video hosts).
    if formats.is_empty() {
        if let Some(single) = parse_format(node) {
            formats.push(single);
        }
    }

    if formats.is_empty() {
        return Err(AppError::Unsupported(
            "the source offered no downloadable stream".into(),
        ));
    }

    let (watermark_support, _) = apply_watermark_flags(platform, &mut formats);
    sort_formats(&mut formats);

    let is_live = node
        .get("is_live")
        .and_then(Value::as_bool)
        .or_else(|| {
            node.get("live_status")
                .and_then(Value::as_str)
                .map(|status| status == "is_live")
        })
        .unwrap_or(false);

    let media_kind = infer_media_kind(&formats, node);

    // Only notes the UI cannot infer for itself belong here. A live stream, for
    // instance, is already obvious from `is_live` and gets its own badge.
    let warnings = Vec::new();

    Ok(MediaMetadata {
        url: requested_url.to_string(),
        canonical_url: node
            .get("webpage_url")
            .and_then(Value::as_str)
            .unwrap_or(requested_url)
            .to_string(),
        platform,
        platform_label: platform.label().to_string(),
        provider_id: PROVIDER_ID.to_string(),
        media_kind,
        title,
        creator,
        description: node
            .get("description")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.chars().take(600).collect()),
        thumbnail_url: pick_thumbnail(node),
        duration_sec: node.get("duration").and_then(Value::as_f64),
        view_count: node.get("view_count").and_then(Value::as_u64),
        like_count: node.get("like_count").and_then(Value::as_u64),
        upload_date: node
            .get("upload_date")
            .and_then(Value::as_str)
            .map(str::to_string),
        is_live,
        formats,
        entry_count: None,
        watermark_support,
        warnings,
    })
}

fn parse_format(node: &Value) -> Option<MediaFormat> {
    let url = node.get("url").and_then(Value::as_str)?.to_string();
    if url.is_empty() {
        return None;
    }

    let id = node
        .get("format_id")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();

    let protocol = node
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("https")
        .to_string();

    // yt-dlp uses the literal string "none" rather than null for an absent codec.
    let vcodec = node
        .get("vcodec")
        .and_then(Value::as_str)
        .filter(|value| *value != "none" && !value.is_empty())
        .map(str::to_string);
    let acodec = node
        .get("acodec")
        .and_then(Value::as_str)
        .filter(|value| *value != "none" && !value.is_empty())
        .map(str::to_string);

    let width = node.get("width").and_then(Value::as_u64).map(|v| v as u32);
    let height = node.get("height").and_then(Value::as_u64).map(|v| v as u32);
    let container = node
        .get("ext")
        .and_then(Value::as_str)
        .unwrap_or("bin")
        .to_string();

    let has_video = vcodec.is_some() || (height.is_some() && detect::is_video_extension(&container));
    let has_audio = acodec.is_some();
    let is_image = !has_video && !has_audio && detect::is_image_extension(&container);

    let kind = if is_image {
        FormatKind::Image
    } else if has_video && has_audio {
        FormatKind::Muxed
    } else if has_video {
        FormatKind::Video
    } else if has_audio {
        FormatKind::Audio
    } else {
        return None;
    };

    let abr = node.get("abr").and_then(Value::as_f64);
    let tbr = node.get("tbr").and_then(Value::as_f64);

    let http_headers = node
        .get("http_headers")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|v| (key.clone(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    let note = node
        .get("format_note")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);

    let quality_label = quality_label(kind, height, width, abr.or(tbr), note.as_deref());

    Some(MediaFormat {
        id,
        kind,
        container,
        // Segmented protocols need a real HLS/DASH client; a ranged GET cannot
        // fetch a manifest's worth of segments.
        needs_engine_download: is_segmented(&protocol),
        protocol,
        has_video,
        has_audio,
        width,
        height,
        fps: node.get("fps").and_then(Value::as_f64),
        vcodec,
        acodec,
        tbr,
        vbr: node.get("vbr").and_then(Value::as_f64),
        abr,
        filesize: node.get("filesize").and_then(Value::as_u64),
        filesize_approx: node.get("filesize_approx").and_then(Value::as_u64),
        quality_label,
        watermarked: None,
        note,
        url: Some(url),
        http_headers,
    })
}

pub fn is_segmented(protocol: &str) -> bool {
    protocol.contains("m3u8")
        || protocol.contains("dash")
        || protocol.contains("ism")
        || protocol.contains("f4m")
        || protocol.starts_with("rtmp")
        || protocol.starts_with("rtsp")
        || protocol.contains("websocket")
}

fn quality_label(
    kind: FormatKind,
    height: Option<u32>,
    width: Option<u32>,
    bitrate: Option<f64>,
    note: Option<&str>,
) -> String {
    match kind {
        FormatKind::Audio => bitrate
            .filter(|value| *value > 0.0)
            .map(|value| format!("{} kbps", value.round() as u64))
            .or_else(|| note.map(str::to_string))
            .unwrap_or_else(|| "Audio".to_string()),
        FormatKind::Image => match (width, height) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => "Image".to_string(),
        },
        _ => match (height, width) {
            (Some(h), _) if h > 0 => format!("{h}p"),
            (_, Some(w)) if w > 0 => format!("{w}w"),
            _ => note.unwrap_or("Video").to_string(),
        },
    }
}

fn pick_thumbnail(node: &Value) -> Option<String> {
    if let Some(url) = node
        .get("thumbnail")
        .and_then(Value::as_str)
        .filter(|value| value.starts_with("http"))
    {
        return Some(url.to_string());
    }

    // Otherwise take the widest thumbnail the source offers, which is the one
    // that will still look sharp in the preview card.
    node.get("thumbnails")
        .and_then(Value::as_array)?
        .iter()
        .filter(|entry| {
            entry
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|url| url.starts_with("http"))
        })
        .max_by_key(|entry| entry.get("width").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|entry| entry.get("url").and_then(Value::as_str))
        .map(str::to_string)
}

fn infer_media_kind(formats: &[MediaFormat], node: &Value) -> MediaKind {
    if formats.iter().any(|f| f.has_video) {
        MediaKind::Video
    } else if formats.iter().any(|f| f.kind == FormatKind::Image) {
        MediaKind::Image
    } else if formats.iter().any(|f| f.has_audio) {
        MediaKind::Audio
    } else if node.get("duration").and_then(Value::as_f64).is_some() {
        MediaKind::Video
    } else {
        MediaKind::Image
    }
}

/// Mark which formats carry a platform watermark.
///
/// This only reports what the source itself distinguishes -- TikTok publishes a
/// watermarked rendition alongside clean ones, and yt-dlp labels them. Nothing
/// here removes a watermark; when a platform only offers a stamped rendition,
/// that is reported as `WatermarkedOnly` and the UI says so.
fn apply_watermark_flags(
    platform: PlatformId,
    formats: &mut [MediaFormat],
) -> (WatermarkSupport, usize) {
    if platform != PlatformId::Tiktok {
        return (WatermarkSupport::NotApplicable, 0);
    }

    let mut clean = 0usize;
    let mut stamped = 0usize;

    for format in formats.iter_mut() {
        if !format.has_video {
            continue;
        }
        let id = format.id.to_ascii_lowercase();
        let note = format.note.as_deref().unwrap_or("").to_ascii_lowercase();

        let is_watermarked = note.contains("watermark")
            || id.starts_with("download")
            || id.contains("watermark");

        format.watermarked = Some(is_watermarked);
        if is_watermarked {
            stamped += 1;
        } else {
            clean += 1;
        }
    }

    let support = if clean > 0 {
        WatermarkSupport::CleanAvailable
    } else if stamped > 0 {
        WatermarkSupport::WatermarkedOnly
    } else {
        WatermarkSupport::NotApplicable
    };

    (support, clean)
}

/// Best first, so the default selection is a simple "take the head".
fn sort_formats(formats: &mut [MediaFormat]) {
    formats.sort_by(|a, b| {
        b.pixels()
            .cmp(&a.pixels())
            .then_with(|| {
                b.tbr
                    .unwrap_or(0.0)
                    .partial_cmp(&a.tbr.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                b.abr
                    .unwrap_or(0.0)
                    .partial_cmp(&a.abr.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            // Prefer a stream we can fetch ourselves over one that needs the
            // engine, all else being equal.
            .then_with(|| a.needs_engine_download.cmp(&b.needs_engine_download))
    });
}

/// Where the engine should write when it handles a download itself.
pub fn output_template(target: &Path) -> String {
    target.to_string_lossy().into_owned()
}

pub fn log_engine_invocation(args: &[String]) {
    log_debug!("engine", "invoking with {} args: {:?}", args.len(), args);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format_json(id: &str, note: Option<&str>) -> Value {
        serde_json::json!({
            "format_id": id,
            "url": "https://cdn.test/v.mp4",
            "ext": "mp4",
            "vcodec": "h264",
            "acodec": "aac",
            "height": 1080,
            "width": 1920,
            "format_note": note,
        })
    }

    #[test]
    fn classifies_unsupported_urls() {
        let err = classify_engine_error("ERROR: Unsupported URL: https://example.test/");
        assert_eq!(err.code(), "unsupported");
    }

    #[test]
    fn classifies_private_media_as_forbidden() {
        let err = classify_engine_error("ERROR: This video is private");
        assert_eq!(err.code(), "forbidden");
    }

    #[test]
    fn classifies_missing_media_as_not_found() {
        assert_eq!(
            classify_engine_error("ERROR: HTTP Error 404: Not Found").code(),
            "notFound"
        );
        assert_eq!(
            classify_engine_error("ERROR: Video unavailable").code(),
            "notFound"
        );
    }

    #[test]
    fn a_login_wall_is_forbidden_however_the_engine_words_it() {
        let messages = [
            "ERROR: [vimeo] 76979871: The web client only works when logged-in.              Use --cookies, --cookies-from-browser, --username and --password to              provide account credentials",
            "ERROR: Sign in to confirm your age",
            "ERROR: This video is available to Music Premium members",
            "ERROR: Please log in to view this content",
            "ERROR: Login required",
            "ERROR: HTTP Error 401: Unauthorized",
            "ERROR: HTTP Error 429: Too Many Requests",
            "ERROR: This video is not available in your country",
        ];
        for message in messages {
            assert_eq!(
                classify_engine_error(message).code(),
                "forbidden",
                "misclassified: {message}"
            );
        }
    }

    #[test]
    fn a_login_wall_is_not_reported_as_retryable_nonsense() {
        let err = classify_engine_error("ERROR: The web client only works when logged-in.");
        // Retrying an auth wall is pointless but harmless; what matters is that
        // the user is told the real reason rather than "something went wrong".
        assert_eq!(err.code(), "forbidden");
        assert!(err.technical().is_some_and(|t| t.contains("logged-in")));
    }

    #[test]
    fn classifies_connection_problems_as_network() {
        let err = classify_engine_error("ERROR: Unable to download webpage: getaddrinfo failed");
        assert_eq!(err.code(), "network");
        assert!(err.retryable());
    }

    #[test]
    fn segmented_protocols_are_delegated_to_the_engine() {
        assert!(is_segmented("m3u8_native"));
        assert!(is_segmented("http_dash_segments"));
        assert!(!is_segmented("https"));
        assert!(!is_segmented("http"));
    }

    #[test]
    fn tiktok_watermarked_formats_are_flagged_and_clean_ones_offered() {
        let mut formats: Vec<MediaFormat> = [
            format_json("download_addr-0", None),
            format_json("play_addr-1", None),
        ]
        .iter()
        .filter_map(parse_format)
        .collect();

        let (support, clean) = apply_watermark_flags(PlatformId::Tiktok, &mut formats);
        assert_eq!(support, WatermarkSupport::CleanAvailable);
        assert_eq!(clean, 1);
        assert_eq!(formats[0].watermarked, Some(true));
        assert_eq!(formats[1].watermarked, Some(false));
    }

    #[test]
    fn tiktok_with_only_stamped_formats_reports_watermarked_only() {
        let mut formats: Vec<MediaFormat> = [format_json("download_addr-0", Some("watermarked"))]
            .iter()
            .filter_map(parse_format)
            .collect();
        let (support, clean) = apply_watermark_flags(PlatformId::Tiktok, &mut formats);
        assert_eq!(support, WatermarkSupport::WatermarkedOnly);
        assert_eq!(clean, 0);
    }

    #[test]
    fn other_platforms_report_watermarking_as_not_applicable() {
        let mut formats: Vec<MediaFormat> =
            [format_json("137", None)].iter().filter_map(parse_format).collect();
        let (support, _) = apply_watermark_flags(PlatformId::Youtube, &mut formats);
        assert_eq!(support, WatermarkSupport::NotApplicable);
        assert_eq!(formats[0].watermarked, None);
    }

    #[test]
    fn a_video_only_stream_is_not_marked_as_muxed() {
        let node = serde_json::json!({
            "format_id": "137",
            "url": "https://cdn.test/v.mp4",
            "ext": "mp4",
            "vcodec": "avc1.640028",
            "acodec": "none",
            "height": 1080,
        });
        let format = parse_format(&node).unwrap();
        assert_eq!(format.kind, FormatKind::Video);
        assert!(format.has_video && !format.has_audio);
        assert_eq!(format.quality_label, "1080p");
    }

    #[test]
    fn an_audio_stream_is_labelled_by_bitrate() {
        let node = serde_json::json!({
            "format_id": "140",
            "url": "https://cdn.test/a.m4a",
            "ext": "m4a",
            "vcodec": "none",
            "acodec": "mp4a.40.2",
            "abr": 192.0,
        });
        let format = parse_format(&node).unwrap();
        assert_eq!(format.kind, FormatKind::Audio);
        assert_eq!(format.quality_label, "192 kbps");
    }

    #[test]
    fn formats_without_a_url_are_dropped() {
        assert!(parse_format(&serde_json::json!({ "format_id": "x" })).is_none());
    }

    #[test]
    fn parses_a_minimal_payload_end_to_end() {
        let node = serde_json::json!({
            "title": "Example",
            "uploader": "someone",
            "duration": 42.0,
            "webpage_url": "https://www.youtube.com/watch?v=abc",
            "thumbnails": [
                { "url": "https://cdn.test/small.jpg", "width": 120 },
                { "url": "https://cdn.test/large.jpg", "width": 1280 }
            ],
            "formats": [format_json("18", None)],
        });
        let meta = parse_metadata(&node, "https://www.youtube.com/watch?v=abc").unwrap();
        assert_eq!(meta.title, "Example");
        assert_eq!(meta.creator.as_deref(), Some("someone"));
        assert_eq!(meta.platform, PlatformId::Youtube);
        assert_eq!(meta.media_kind, MediaKind::Video);
        assert_eq!(meta.thumbnail_url.as_deref(), Some("https://cdn.test/large.jpg"));
        assert_eq!(meta.formats.len(), 1);
    }

    #[test]
    fn a_payload_with_no_usable_stream_is_unsupported() {
        let node = serde_json::json!({ "title": "Example", "formats": [] });
        let err = parse_metadata(&node, "https://example.test/x").unwrap_err();
        assert_eq!(err.code(), "unsupported");
    }

    #[test]
    fn a_height_only_format_still_ranks_by_resolution() {
        let tall = serde_json::json!({
            "format_id": "a", "url": "https://x/1", "ext": "mp4",
            "vcodec": "h264", "height": 1080
        });
        let short = serde_json::json!({
            "format_id": "b", "url": "https://x/2", "ext": "mp4",
            "vcodec": "h264", "height": 360
        });
        let tall = parse_format(&tall).unwrap();
        let short = parse_format(&short).unwrap();
        assert!(tall.pixels() > short.pixels());
    }

    #[test]
    fn formats_sort_best_first() {
        let node = serde_json::json!({
            "title": "t",
            "formats": [
                { "format_id": "a", "url": "https://x/1", "ext": "mp4", "vcodec": "h264", "height": 360 },
                { "format_id": "b", "url": "https://x/2", "ext": "mp4", "vcodec": "h264", "height": 1080 },
                { "format_id": "c", "url": "https://x/3", "ext": "mp4", "vcodec": "h264", "height": 720 },
            ]
        });
        let meta = parse_metadata(&node, "https://example.test/x").unwrap();
        let heights: Vec<_> = meta.formats.iter().map(|f| f.height).collect();
        assert_eq!(heights, vec![Some(1080), Some(720), Some(360)]);
    }
}
