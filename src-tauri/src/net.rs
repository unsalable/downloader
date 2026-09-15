//! HTTP client construction.
//!
//! One pooled client is shared by every download so connections are reused
//! across a queue rather than renegotiated per file. It is rebuilt only when a
//! setting that affects transport (proxy, timeout, user agent) changes.

pub mod dns;

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

    // A phone's DNS is whatever its network, VPN or Private DNS setting makes
    // it, and it is not always answering.
    #[cfg(target_os = "android")]
    {
        builder = builder.dns_resolver(std::sync::Arc::new(dns::FallbackResolver));
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

/// How the resolvers of the platforms the app and its tools run on word "that
/// host name could not be looked up".
const LOOKUP_FAILURES: &[&str] = &[
    // Android
    "no address associated with hostname",
    // Linux
    "name or service not known",
    "temporary failure in name resolution",
    // macOS
    "nodename nor servname provided",
    // Windows
    "no such host is known",
    "getaddrinfo failed",
    // Rust's own wording, whatever the platform
    "failed to lookup address information",
    "dns error",
];

/// Whether an error's text says that a host name could not be looked up.
pub fn is_lookup_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    LOOKUP_FAILURES.iter().any(|needle| lower.contains(needle))
}

/// A failed lookup, told apart by what Android knows about the connection.
///
/// "No address associated with hostname" is all an app is told whether the
/// phone is offline, Android or a VPN is not letting this app online, or a
/// DNS server is not answering -- and by the time it reaches here, the public
/// resolvers have not answered either. What the user should do differs, so
/// the phone is asked which it is. Its answer also goes into the technical
/// details, which is what someone helping the user will look at.
#[cfg(target_os = "android")]
pub async fn explain_failure(app: &tauri::AppHandle, err: AppError) -> AppError {
    let AppError::Network(detail) = &err else {
        return err;
    };
    if !is_lookup_failure(detail) {
        return err;
    }
    match crate::android::network_status(app.clone()).await {
        Ok(status) => {
            let detail = format!("{detail} [Android: {}]", status.describe());
            if status.connected {
                AppError::NetworkBlocked(detail)
            } else {
                AppError::Offline(detail)
            }
        }
        Err(status_err) => {
            crate::log_warn!("net", "the connection could not be checked: {status_err}");
            err
        }
    }
}

/// On the desktop, a failed lookup is reported as it is.
#[cfg(not(target_os = "android"))]
pub async fn explain_failure(_app: &tauri::AppHandle, err: AppError) -> AppError {
    err
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_failures_are_recognised_in_every_wording() {
        let failures = [
            // The engine on the phone the report came from.
            "ERROR: [vm.tiktok] ZSq4hQv5y: Unable to download webpage: [Errno 7] No address associated with hostname (caused by TransportError('[Errno 7] No address associated with hostname'))",
            "error sending request for url (https://vt.tiktok.com/ZSq4hQv5y/): client error (Connect): dns error: failed to lookup address information: No address associated with hostname",
            "ERROR: Unable to download webpage: <urlopen error [Errno -2] Name or service not known>",
            "ERROR: Unable to download webpage: [Errno 11001] getaddrinfo failed",
            "ERROR: Unable to download webpage: [Errno 8] nodename nor servname provided, or not known",
        ];
        for failure in failures {
            assert!(is_lookup_failure(failure), "not recognised: {failure}");
        }

        assert!(!is_lookup_failure("ERROR: Unable to download webpage: HTTP Error 404: Not Found"));
        assert!(!is_lookup_failure("error sending request: operation timed out"));
    }
}
