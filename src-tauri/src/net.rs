//! HTTP client construction.
//!
//! One pooled client is shared by every download so connections are reused
//! across a queue rather than renegotiated per file. It is rebuilt only when a
//! setting that affects transport (proxy, timeout, user agent) changes.

use std::sync::RwLock;
use std::time::Duration;

use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};

use crate::error::{AppError, AppResult};
use crate::settings::Settings;

/// Media CDNs routinely reject clients that do not look like a browser. This is
/// a plain identification header, not an attempt to defeat any access control.
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

struct ClientCell {
    client: reqwest::Client,
    signature: String,
}

static CLIENT: Lazy<RwLock<Option<ClientCell>>> = Lazy::new(|| RwLock::new(None));

/// Identifies the transport-affecting subset of settings, so the client is only
/// rebuilt when one of them actually changed.
fn signature(settings: &Settings) -> String {
    format!(
        "{}|{}|{}",
        settings.network_timeout_sec,
        settings.proxy_url.as_deref().unwrap_or(""),
        settings.custom_user_agent.as_deref().unwrap_or(""),
    )
}

pub fn client(settings: &Settings) -> AppResult<reqwest::Client> {
    let wanted = signature(settings);

    if let Ok(guard) = CLIENT.read() {
        if let Some(cell) = guard.as_ref() {
            if cell.signature == wanted {
                return Ok(cell.client.clone());
            }
        }
    }

    let built = build(settings)?;
    if let Ok(mut guard) = CLIENT.write() {
        *guard = Some(ClientCell {
            client: built.clone(),
            signature: wanted,
        });
    }
    Ok(built)
}

fn build(settings: &Settings) -> AppResult<reqwest::Client> {
    let user_agent = settings
        .custom_user_agent
        .clone()
        .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&user_agent)
            .map_err(|_| AppError::Other("the custom user agent contains invalid characters".into()))?,
    );
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));

    let mut builder = reqwest::Client::builder()
        .default_headers(headers)
        // The overall timeout is deliberately not set: a large download can
        // legitimately run for an hour. Connect and read stalls are what need
        // bounding.
        .connect_timeout(Duration::from_secs(settings.network_timeout_sec.min(60)))
        .read_timeout(Duration::from_secs(settings.network_timeout_sec))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(6)
        .redirect(reqwest::redirect::Policy::limited(10))
        .tcp_keepalive(Duration::from_secs(60));

    if let Some(proxy) = settings.proxy_url.as_deref() {
        let proxy = reqwest::Proxy::all(proxy)
            .map_err(|err| AppError::Other(format!("the proxy address is not usable: {err}")))?;
        builder = builder.proxy(proxy);
    } else {
        // Without an explicit proxy, honour the system configuration.
        builder = builder.use_rustls_tls();
    }

    builder
        .build()
        .map_err(|err| AppError::Network(format!("could not create the HTTP client: {err}")))
}

/// Convert a provider-supplied header list into a `HeaderMap`, dropping any
/// entry that is not a valid header rather than failing the whole download.
pub fn header_map(pairs: &[(String, String)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(value) = HeaderValue::from_str(value) else {
            continue;
        };
        map.insert(name, value);
    }
    map
}

pub fn invalidate() {
    if let Ok(mut guard) = CLIENT.write() {
        *guard = None;
    }
}
