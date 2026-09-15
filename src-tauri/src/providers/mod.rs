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

pub mod detect;
pub mod direct;
pub mod engine;
pub mod generic;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

use crate::error::{AppError, AppResult};
use crate::model::{MediaMetadata, PlatformId};
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
///   2. the engine, which covers the named platforms and many more
///   3. the generic page reader
///   4. unsupported
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

    let engine_available = tools::engine_path().is_some();
    let mut engine_error: Option<AppError> = None;

    if engine_available {
        let engine = EngineProvider;
        if MediaProvider::can_handle(&engine, url) {
            log_debug!("providers", "trying engine for {}", info.host);
            match MediaProvider::analyze(&engine, url, settings).await {
                Ok(metadata) => return Ok(metadata),
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

/// Expand a gallery, carousel or album into one URL per item.
pub async fn expand_entries(url: &str, settings: &Settings) -> AppResult<Vec<String>> {
    if tools::engine_path().is_none() {
        return Ok(vec![url.to_string()]);
    }
    EngineProvider.expand_entries(url, settings).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MediaKind, WatermarkSupport};

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
        }
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
