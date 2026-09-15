//! Direct media files.
//!
//! When a URL already points at a media file there is nothing to extract, so
//! this provider asks the server what it is holding and hands back a single
//! format. It runs before the engine because a one-request HEAD is far cheaper
//! than starting an extractor process.

use crate::error::{AppError, AppResult};
use crate::model::{FormatKind, MediaFormat, MediaKind, MediaMetadata, PlatformId, WatermarkSupport};
use crate::providers::detect;
use crate::settings::Settings;

pub const PROVIDER_ID: &str = "direct";

pub struct DirectProvider;

impl DirectProvider {
    pub fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    /// Only claims URLs that end in a media extension on a host with no
    /// dedicated platform, so a Reddit page ending in `.mp4` still goes to the
    /// engine, which knows how to read the post around it.
    pub fn can_handle(&self, url: &str) -> bool {
        detect::classify(url)
            .is_some_and(|info| info.direct_extension.is_some() && info.platform == PlatformId::Generic)
    }

    pub async fn analyze(&self, url: &str, settings: &Settings) -> AppResult<MediaMetadata> {
        let info = detect::classify(url)
            .ok_or_else(|| AppError::InvalidUrl("the address could not be parsed".into()))?;
        let extension = info
            .direct_extension
            .clone()
            .ok_or_else(|| AppError::Unsupported("not a direct media file".into()))?;

        let client = crate::net::client(settings)?;

        // Some CDNs refuse HEAD. A ranged GET for a single byte gets the same
        // headers and is universally supported.
        let response = client
            .get(url)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() && status.as_u16() != 206 {
            return Err(AppError::from_status(status.as_u16(), "the file could not be read"));
        }

        let headers = response.headers();
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();

        // With a ranged request the length arrives in Content-Range, not
        // Content-Length (which describes the single returned byte).
        let total = headers
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit('/').next()?.parse::<u64>().ok())
            .or_else(|| {
                if status.as_u16() == 206 {
                    None
                } else {
                    response.content_length()
                }
            });

        let resumable = status.as_u16() == 206
            || headers
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("bytes"));

        let (kind, media_kind) = if content_type.starts_with("image/")
            || detect::is_image_extension(&extension)
        {
            (FormatKind::Image, MediaKind::Image)
        } else if content_type.starts_with("audio/") || detect::is_audio_extension(&extension) {
            (FormatKind::Audio, MediaKind::Audio)
        } else {
            (FormatKind::Muxed, MediaKind::Video)
        };

        let file_name = info
            .path
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .rsplit('/')
            .next()
            .filter(|value| !value.is_empty())
            .map(|value| urlencoding::decode(value).map(|d| d.into_owned()).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "download".to_string());

        let title = file_name
            .rsplit_once('.')
            .map(|(stem, _)| stem.to_string())
            .unwrap_or(file_name);

        let quality_label = match kind {
            FormatKind::Image => "Original".to_string(),
            FormatKind::Audio => "Original audio".to_string(),
            _ => "Original".to_string(),
        };

        let format = MediaFormat {
            id: "direct".to_string(),
            kind,
            container: extension,
            protocol: "https".to_string(),
            has_video: matches!(kind, FormatKind::Muxed),
            has_audio: matches!(kind, FormatKind::Muxed | FormatKind::Audio),
            width: None,
            height: None,
            fps: None,
            vcodec: None,
            acodec: None,
            tbr: None,
            vbr: None,
            abr: None,
            filesize: total,
            filesize_approx: None,
            quality_label,
            watermarked: None,
            note: (!resumable).then(|| "no-resume".to_string()),
            needs_engine_download: false,
            url: Some(url.to_string()),
            http_headers: Vec::new(),
        };

        Ok(MediaMetadata {
            url: url.to_string(),
            canonical_url: url.to_string(),
            platform: PlatformId::Direct,
            platform_label: PlatformId::Direct.label().to_string(),
            provider_id: PROVIDER_ID.to_string(),
            media_kind,
            title,
            creator: Some(info.host),
            description: None,
            thumbnail_url: matches!(kind, FormatKind::Image).then(|| url.to_string()),
            duration_sec: None,
            view_count: None,
            like_count: None,
            upload_date: None,
            is_live: false,
            formats: vec![format],
            entry_count: None,
            watermark_support: WatermarkSupport::NotApplicable,
            warnings: Vec::new(),
            entries: Vec::new(),
        })
    }
}
