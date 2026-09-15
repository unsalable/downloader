//! Provider architecture.
//!
//! Every source adapter implements the same three-method [`MediaProvider`]
//! interface. Adding a platform means adding a module here and a branch in
//! [`analyze`] -- nothing else in the app needs to change.
//!
//! The trait is used for static dispatch rather than behind `dyn`: its methods
//! are async, the set of providers is closed and known at compile time, and
//! calling them directly keeps the resolution path allocation-free while still
//! making the shared contract explicit.
//!
//! [`photos`] is the exception to the shape: it reads only the photo posts of
//! a few platforms, and answers "not one of mine" for everything else.

pub mod detect;
pub mod direct;
pub mod engine;
pub mod generic;
pub mod photos;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

use crate::error::{AppError, AppResult};
use crate::model::{FormatKind, MediaFormat, MediaKind, MediaMetadata, PlatformId};
use crate::settings::Settings;
use crate::{log_debug, log_warn, tools};

use direct::DirectProvider;
use engine::EngineProvider;
use generic::GenericProvider;

/// The contract every provider satisfies.
#[allow(async_fn_in_trait)]
pub trait MediaProvider {
    /// Stable identifier, surfaced in the developer panel.
    fn id(&self) -> &'static str;

    /// Whether this provider is willing to attempt the URL at all.
    fn can_handle(&self, url: &str) -> bool;

    /// Read whatever the source publishes: title, creator, thumbnail and the
    /// list of streams that can actually be fetched.
    async fn analyze(&self, url: &str, settings: &Settings) -> AppResult<MediaMetadata>;
}

impl MediaProvider for DirectProvider {
    fn id(&self) -> &'static str {
        DirectProvider::id(self)
    }
    fn can_handle(&self, url: &str) -> bool {
        DirectProvider::can_handle(self, url)
    }
    async fn analyze(&self, url: &str, settings: &Settings) -> AppResult<MediaMetadata> {
        DirectProvider::analyze(self, url, settings).await
    }
}

impl MediaProvider for EngineProvider {
    fn id(&self) -> &'static str {
        EngineProvider::id(self)
    }
    fn can_handle(&self, url: &str) -> bool {
        EngineProvider::can_handle(self, url)
    }
    async fn analyze(&self, url: &str, settings: &Settings) -> AppResult<MediaMetadata> {
        EngineProvider::analyze(self, url, settings).await
    }
}

impl MediaProvider for GenericProvider {
    fn id(&self) -> &'static str {
        GenericProvider::id(self)
    }
    fn can_handle(&self, url: &str) -> bool {
        GenericProvider::can_handle(self, url)
    }
    async fn analyze(&self, url: &str, settings: &Settings) -> AppResult<MediaMetadata> {
        GenericProvider::analyze(self, url, settings).await
    }
}

/// Resolution order, matching the documented provider priority:
///   1. a direct media file, which needs no extraction at all
///   2. a photo post the engine cannot read (X, Reddit, TikTok photo mode)
///   3. the engine, which covers the named platforms and many more
///   4. the generic page reader
///   5. unsupported
pub async fn analyze(url: &str, settings: &Settings) -> AppResult<MediaMetadata> {
    let info = detect::classify(url)
        .ok_or_else(|| AppError::InvalidUrl(format!("not an http(s) address: {url}")))?;

    let direct = DirectProvider;
    if MediaProvider::can_handle(&direct, url) {
        log_debug!("providers", "trying direct for {}", info.host);
        match MediaProvider::analyze(&direct, url, settings).await {
            Ok(metadata) => return Ok(metadata),
            Err(err) => {
                // A file that 404s is a real answer; only fall through when the
                // direct read was inconclusive.
                if matches!(err, AppError::NotFound { .. } | AppError::Forbidden { .. }) {
                    return Err(err);
                }
                log_warn!("providers", "direct provider failed, falling through: {err}");
            }
        }
    }

    // One HTTP request each, so they go ahead of starting the engine: a photo
    // post answered here never costs a process launch. Anything they do not
    // recognise as a photo post, or fail to read, is the engine's to try.
    let mut mixed = None;
    match photos::analyze(url, info.platform, settings).await {
        Ok(photos::Reading::Photos(metadata)) => return Ok(*metadata),
        Ok(photos::Reading::WithVideos(post)) => mixed = Some(post),
        Ok(photos::Reading::Elsewhere) => {}
        Err(err) => log_warn!("providers", "photo reader failed for {}: {err}", info.host),
    }

    let engine_available = tools::engine_path().is_some();
    let mut engine_error: Option<AppError> = None;

    if engine_available {
        let engine = EngineProvider;
        if MediaProvider::can_handle(&engine, url) {
            log_debug!("providers", "trying engine for {}", info.host);
            match MediaProvider::analyze(&engine, url, settings).await {
                Ok(metadata) => return Ok(photos::complete(metadata, mixed, settings).await),
                Err(AppError::Unsupported(detail)) => {
                    log_debug!("providers", "engine does not know {}: {detail}", info.host);
                    engine_error = Some(AppError::Unsupported(detail));
                }
                Err(err) => return Err(err),
            }
        }
    } else if info.platform != PlatformId::Generic {
        // A known platform with no engine installed has no other route.
        return Err(AppError::EngineMissing);
    }

    let generic = GenericProvider;
    if MediaProvider::can_handle(&generic, url) {
        log_debug!("providers", "trying generic for {}", info.host);
        match MediaProvider::analyze(&generic, url, settings).await {
            Ok(metadata) => return Ok(metadata),
            Err(err) => {
                log_debug!("providers", "generic provider failed: {err}");
                if !engine_available {
                    return Err(AppError::EngineMissing);
                }
                return Err(engine_error.unwrap_or(err));
            }
        }
    }

    Err(engine_error.unwrap_or_else(|| {
        AppError::Unsupported(format!("no provider could read {}", info.host))
    }))
}

// -- recent analyses -----------------------------------------------------------
//
// Pressing Download usually follows an analysis by seconds, and the download
// used to start by running that same analysis again. For the engine that means
// starting Python and, for YouTube, solving the player's JavaScript challenge
// -- the most expensive thing the app does, and on a phone enough on its own to
// heat it. An analysis the user has just seen is fresh enough to download from:
// stream URLs stay valid for hours, and a download that fails with a reused
// analysis forgets it, so the retry resolves the link anew.

/// How long an analysis may be reused. Well inside the lifetime of the signed
/// stream URLs it carries.
const RECENT_TTL: Duration = Duration::from_secs(5 * 60);
const RECENT_LIMIT: usize = 8;

struct RecentAnalysis {
    at: Instant,
    /// The address typed, and the ones the source reported for itself. A
    /// download is requested by the canonical one.
    urls: Vec<String>,
    /// Stream URLs can be tied to the network path that resolved them.
    proxy: Option<String>,
    metadata: MediaMetadata,
}

static RECENT: Lazy<Mutex<Vec<RecentAnalysis>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Keep an analysis the interface is showing, for the download that follows.
pub fn remember_analysis(input_url: &str, settings: &Settings, metadata: &MediaMetadata) {
    let mut urls = vec![input_url.to_string()];
    for url in [&metadata.url, &metadata.canonical_url] {
        if !url.is_empty() && !urls.contains(url) {
            urls.push(url.clone());
        }
    }

    let mut recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    recent.retain(|entry| entry.at.elapsed() < RECENT_TTL && !entry.urls.iter().any(|u| urls.contains(u)));
    if recent.len() >= RECENT_LIMIT {
        recent.remove(0);
    }
    recent.push(RecentAnalysis {
        at: Instant::now(),
        urls,
        proxy: settings.proxy_url.clone(),
        metadata: metadata.clone(),
    });
}

/// An analysis of `url` made moments ago under the same network settings.
pub fn recent_analysis(url: &str, settings: &Settings) -> Option<MediaMetadata> {
    let recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    recent
        .iter()
        .rev()
        .find(|entry| {
            entry.at.elapsed() < RECENT_TTL
                && entry.proxy == settings.proxy_url
                && entry.urls.iter().any(|known| known == url)
        })
        .map(|entry| entry.metadata.clone())
}

/// Drop any kept analysis of `url`, so the next attempt resolves it afresh.
pub fn forget_analysis(url: &str) {
    RECENT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|entry| !entry.urls.iter().any(|known| known == url));
}

// -- galleries -----------------------------------------------------------------

/// Metadata for a carousel, gallery or album, from its items in order.
///
/// The link as a whole previews and downloads as its first item, so that item
/// supplies the streams; the post supplies the title. Items are named after
/// the post with their position -- the source's own per-item titles are
/// usually all the same, and files named that way would only be told apart by
/// the order they happened to finish in. A single item is simply that item.
pub fn gallery(
    title: Option<String>,
    canonical_url: Option<String>,
    mut items: Vec<MediaMetadata>,
) -> Option<MediaMetadata> {
    match items.len() {
        0 => return None,
        1 => return items.pop(),
        _ => {}
    }

    let title = title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| items[0].title.clone());
    for (index, item) in items.iter_mut().enumerate() {
        item.title = format!("{title} ({})", index + 1);
    }

    let mut post = items[0].clone();
    post.title = title;
    if let Some(url) = canonical_url {
        post.canonical_url = url;
    }
    post.media_kind = MediaKind::Gallery;
    post.entry_count = Some(items.len() as u32);
    post.entries = items;
    Some(post)
}

/// The item a download names: one entry of a gallery, or the link itself.
pub fn select_entry(metadata: MediaMetadata, entry: Option<u32>) -> AppResult<MediaMetadata> {
    let Some(position) = entry else {
        return Ok(metadata);
    };
    if metadata.entries.is_empty() && position == 1 {
        return Ok(metadata);
    }

    let count = metadata.entries.len();
    let index = (position as usize).checked_sub(1);
    let MediaMetadata { entries, .. } = metadata;
    index
        .and_then(|index| entries.into_iter().nth(index))
        .ok_or_else(|| AppError::NotFound {
            status: 404,
            detail: format!("item {position} is no longer part of this post, which now has {count}"),
        })
}

/// A picture as a downloadable format.
///
/// The container is read from the address, which is what the CDNs these come
/// from name their files by; JPEG is the answer when the address does not say.
pub fn image_format(
    id: impl Into<String>,
    url: impl Into<String>,
    width: Option<u32>,
    height: Option<u32>,
    http_headers: Vec<(String, String)>,
) -> MediaFormat {
    let url = url.into();
    let container = detect::classify(&url)
        .and_then(|info| info.direct_extension)
        .filter(|extension| detect::is_image_extension(extension))
        .map(|extension| if extension == "jpeg" { "jpg".to_string() } else { extension })
        .unwrap_or_else(|| "jpg".to_string());

    let quality_label = match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => format!("{w}x{h}"),
        _ => "Original".to_string(),
    };

    MediaFormat {
        id: id.into(),
        kind: FormatKind::Image,
        container,
        protocol: "https".to_string(),
        has_video: false,
        has_audio: false,
        width,
        height,
        fps: None,
        vcodec: None,
        acodec: None,
        tbr: None,
        vbr: None,
        abr: None,
        filesize: None,
        filesize_approx: None,
        quality_label,
        watermarked: None,
        note: None,
        needs_engine_download: false,
        url: Some(url),
        http_headers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WatermarkSupport;

    // The kept analyses are process-wide, so each test uses addresses of its own.
    fn metadata(url: &str, canonical: &str) -> MediaMetadata {
        MediaMetadata {
            url: url.into(),
            canonical_url: canonical.into(),
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
            formats: Vec::new(),
            entry_count: None,
            watermark_support: WatermarkSupport::NotApplicable,
            warnings: Vec::new(),
            entries: Vec::new(),
        }
    }

    fn photo(title: &str, file: &str) -> MediaMetadata {
        MediaMetadata {
            title: title.into(),
            media_kind: MediaKind::Image,
            thumbnail_url: Some(format!("https://cdn.test/{file}")),
            formats: vec![image_format(
                "image",
                format!("https://cdn.test/{file}"),
                Some(1080),
                Some(1350),
                Vec::new(),
            )],
            ..metadata("https://example.test/p/1", "https://example.test/p/1")
        }
    }

    #[test]
    fn a_gallery_previews_as_its_first_item_under_the_post_title() {
        let post = gallery(
            Some("Post by someone".into()),
            Some("https://example.test/p/1/".into()),
            vec![photo("Video by someone", "a.jpg"), photo("Video by someone", "b.jpg")],
        )
        .unwrap();

        assert_eq!(post.title, "Post by someone");
        assert_eq!(post.media_kind, MediaKind::Gallery);
        assert_eq!(post.entry_count, Some(2));
        assert_eq!(post.canonical_url, "https://example.test/p/1/");
        assert_eq!(post.formats[0].url.as_deref(), Some("https://cdn.test/a.jpg"));

        let titles: Vec<_> = post.entries.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["Post by someone (1)", "Post by someone (2)"]);
    }

    #[test]
    fn a_gallery_of_one_is_just_that_item() {
        let single = gallery(Some("Post".into()), None, vec![photo("Photo by x", "a.jpg")]).unwrap();
        assert_eq!(single.title, "Photo by x");
        assert!(single.entries.is_empty());
        assert_eq!(single.entry_count, None);
        assert!(gallery(Some("Post".into()), None, Vec::new()).is_none());
    }

    #[test]
    fn a_download_names_its_item_by_position() {
        let post = gallery(
            None,
            None,
            vec![photo("first", "a.jpg"), photo("second", "b.jpg"), photo("third", "c.jpg")],
        )
        .unwrap();

        let second = select_entry(post.clone(), Some(2)).unwrap();
        assert_eq!(second.formats[0].url.as_deref(), Some("https://cdn.test/b.jpg"));
        assert!(second.entries.is_empty());

        // The link itself is the first item.
        let whole = select_entry(post.clone(), None).unwrap();
        assert_eq!(whole.formats[0].url.as_deref(), Some("https://cdn.test/a.jpg"));

        assert_eq!(select_entry(post.clone(), Some(4)).unwrap_err().code(), "notFound");
        assert_eq!(select_entry(post, Some(0)).unwrap_err().code(), "notFound");
    }

    #[test]
    fn a_single_item_answers_to_position_one() {
        let single = photo("only", "a.jpg");
        assert!(select_entry(single.clone(), Some(1)).is_ok());
        assert!(select_entry(single, Some(2)).is_err());
    }

    #[test]
    fn an_image_format_takes_its_container_from_the_address() {
        let format = image_format("i", "https://cdn.test/x/photo.jpeg?sig=1", None, None, Vec::new());
        assert_eq!(format.container, "jpg");
        assert_eq!(format.kind, FormatKind::Image);
        assert_eq!(format.quality_label, "Original");

        let format = image_format("i", "https://cdn.test/x/pin.png", Some(1536), Some(1024), Vec::new());
        assert_eq!(format.container, "png");
        assert_eq!(format.quality_label, "1536x1024");

        // No extension, or one that is not a picture: JPEG.
        assert_eq!(image_format("i", "https://cdn.test/media/abc", None, None, Vec::new()).container, "jpg");
        assert_eq!(image_format("i", "https://cdn.test/a.mp4", None, None, Vec::new()).container, "jpg");
    }

    #[test]
    fn a_download_by_the_canonical_address_finds_the_analysis_of_the_typed_one() {
        let settings = Settings::default();
        let typed = "https://youtu.be/recent-a";
        let canonical = "https://www.youtube.com/watch?v=recent-a";
        remember_analysis(typed, &settings, &metadata(typed, canonical));

        assert!(recent_analysis(canonical, &settings).is_some());
        assert!(recent_analysis(typed, &settings).is_some());
        assert!(recent_analysis("https://youtu.be/someone-else", &settings).is_none());
    }

    #[test]
    fn a_forgotten_analysis_is_not_reused() {
        let settings = Settings::default();
        let url = "https://example.test/recent-b";
        remember_analysis(url, &settings, &metadata(url, url));
        forget_analysis(url);

        assert!(recent_analysis(url, &settings).is_none());
    }

    #[test]
    fn a_different_proxy_resolves_the_link_again() {
        let settings = Settings::default();
        let url = "https://example.test/recent-c";
        remember_analysis(url, &settings, &metadata(url, url));

        let proxied = Settings {
            proxy_url: Some("socks5://127.0.0.1:1080".into()),
            ..Settings::default()
        };
        assert!(recent_analysis(url, &proxied).is_none());
    }
}
