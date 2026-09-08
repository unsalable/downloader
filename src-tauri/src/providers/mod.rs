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

/// Expand a gallery, carousel or album into one URL per item.
pub async fn expand_entries(url: &str, settings: &Settings) -> AppResult<Vec<String>> {
    if tools::engine_path().is_none() {
        return Ok(vec![url.to_string()]);
    }
    EngineProvider.expand_entries(url, settings).await
}
