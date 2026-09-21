//! The wire format between the browser extension and the bridge host.
//!
//! Chrome's native messaging carries length-prefixed JSON over the host's stdin
//! and stdout. The browser starts the host itself, so there is no socket, no
//! port and nothing on the machine that a web page could reach.
//!
//! One rule governs every change to this file: **no message the host sends ever
//! carries cookie material.** The bridge is write-only towards the app. A status
//! reply may say that a session exists, how old it is and which account it looks
//! like, but never a cookie name or value. Adding such a field would quietly
//! turn the bridge into a way to read the session back out of the app, which is
//! the one thing an attacker who can talk to the host cannot currently do.

use serde::{Deserialize, Serialize};

/// The native messaging host name. Registered under this key for every
/// supported browser, and named by the extension in `connectNative`.
pub const HOST_NAME: &str = "com.universaldownloader.bridge";

/// The wire version. Bumped only for a breaking change; `Request` and
/// `Response` are otherwise extended with optional fields, which an older peer
/// ignores and a newer peer treats as absent.
pub const WIRE_VERSION: u32 = 1;

/// Chrome refuses a message larger than 1 MB in either direction. A push of a
/// full YouTube cookie jar is a few kilobytes, so anything approaching this is
/// a bug or an attempt to exhaust memory, and is rejected before allocation.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Extension ids the host will talk to.
///
/// Two entries, deliberately. The Chrome Web Store issues its own id at
/// publication and ignores the `key` in the manifest, so the copy installed
/// during development and the published copy have different ids and both have
/// to be accepted -- otherwise every development build stops working the day
/// the listing goes live, or the other way round.
///
/// `DEV` is derived from `extension/key.pem`, which is what pins the id of an
/// unpacked install; regenerating that key changes this constant. `STORE` is
/// filled in once the listing is approved -- until then it is empty and is
/// skipped, and `store_listed()` keeps the Settings section from offering a
/// listing that does not exist yet.
pub const EXTENSION_ID_DEV: &str = "bkoicficlaelgjpjhlddhloepoocpfoj";
pub const EXTENSION_ID_STORE: &str = "";

/// Whether the published listing exists yet. The Connection section stays out
/// of Settings until it does: its only call to action is "get the extension",
/// and a button that opens a dead store page is worse than no section.
pub fn store_listed() -> bool {
    !EXTENSION_ID_STORE.is_empty()
}

/// `chrome-extension://<id>/` origins, in the form Chrome passes as argv[1] and
/// the form `allowed_origins` takes in the host manifest.
pub fn allowed_origins() -> Vec<String> {
    [EXTENSION_ID_DEV, EXTENSION_ID_STORE]
        .iter()
        .filter(|id| !id.is_empty())
        .map(|id| format!("chrome-extension://{id}/"))
        .collect()
}

/// Whether `origin` is one of ours. Compared whole, never by prefix: an id is
/// fixed length, and a prefix match would accept `chrome-extension://<ours>x/`.
pub fn origin_allowed(origin: &str) -> bool {
    allowed_origins().iter().any(|allowed| allowed == origin)
}

/// A browser family, as the extension reports it. Used for display and to pick
/// which registry hive to repair; never trusted for an access decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Browser {
    Chrome,
    Edge,
    Brave,
    Vivaldi,
    Opera,
    Chromium,
    Unknown,
}

impl Browser {
    pub fn label(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Brave => "Brave",
            Self::Vivaldi => "Vivaldi",
            Self::Opera => "Opera",
            Self::Chromium => "Chromium",
            Self::Unknown => "Browser",
        }
    }
}

/// One cookie as the extension read it from `chrome.cookies`.
///
/// Field names match the Chrome API so the extension forwards what it got
/// without reshaping it, which keeps the translation to Netscape format in one
/// place -- here, on the Rust side, where it is tested.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cookie {
    pub domain: String,
    pub name: String,
    pub value: String,
    pub path: String,
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
    /// Seconds since the epoch. Absent for a session cookie, which the Netscape
    /// format writes with an expiry of 0.
    #[serde(default)]
    pub expiration_date: Option<f64>,
}

/// Extension -> host.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Request {
    /// Sent when the popup opens. Reports nothing and changes nothing; it is
    /// how the popup learns whether the app is installed and what it thinks.
    Status(Peer),
    /// The session, pushed after a sign-in, a cookie change or the daily alarm.
    Push(Push),
    /// The user pressed Connect in a profile while another profile holds the
    /// binding. Deliberate and user-initiated, and the only way a second
    /// profile can take over -- a push never steals the binding silently.
    Claim(Peer),
    /// The user turned the connection off from the popup. Deletes the stored
    /// session immediately, whether or not the app is running.
    Forget(Peer),
    /// Raise the app's window, for the popup's "Open Universal Downloader"
    /// button. Carries nothing and returns nothing but a status.
    OpenApp(Peer),
}

/// Who is speaking. Present on every request so the host can bind, and check,
/// one browser profile at a time.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub v: u32,
    /// A random id the extension generates once per browser profile and keeps
    /// in `chrome.storage.local`. Chrome runs one host process per profile, so
    /// without this the app cannot tell a work profile from a personal one and
    /// the two overwrite each other's sessions -- which reaches the user as
    /// "members-only downloads work sometimes".
    pub profile_id: String,
    #[serde(default = "unknown_browser")]
    pub browser: Browser,
    pub extension_version: String,
    /// The profile's display name, when the extension can see one. Shown in the
    /// app so "Chrome is connected" can say *which* Chrome window.
    #[serde(default)]
    pub profile_label: Option<String>,
}

fn unknown_browser() -> Browser {
    Browser::Unknown
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Push {
    #[serde(flatten)]
    pub peer: Peer,
    /// False when the profile is not signed in to YouTube. The host deletes the
    /// stored session on a signed-out push rather than keeping a jar that has
    /// outlived its sign-in.
    pub signed_in: bool,
    /// A masked hint at the account, for the app to show. Never an exact
    /// address the user did not ask to have displayed.
    #[serde(default)]
    pub account_hint: Option<String>,
    pub captured_at: i64,
    pub cookies: Vec<Cookie>,
}

/// Host -> extension. Exactly one per request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub v: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<HostStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

impl Response {
    pub fn ok(status: HostStatus) -> Self {
        Self {
            v: WIRE_VERSION,
            ok: true,
            status: Some(status),
            error: None,
        }
    }

    pub fn err(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            v: WIRE_VERSION,
            ok: false,
            status: None,
            error: Some(WireError {
                code,
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The user turned the connection off in the app. The host honours this
    /// itself, so turning it off stops cookies being stored even while the app
    /// is closed.
    Disabled,
    /// Another browser profile holds the binding. The popup offers Connect,
    /// which sends `Claim`.
    Unpaired,
    /// The extension is newer than this app and speaks something it does not
    /// understand. The popup says to update the app rather than failing blankly.
    Version,
    /// Malformed message, or one too large to be a cookie jar.
    Malformed,
    Internal,
}

/// What the host tells the popup about itself. Read the module header before
/// adding a field: nothing here may describe cookie contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    pub app_version: String,
    /// The host's own executable path. The only tell a user has that the
    /// registry entry still points at the real app and not at something that
    /// overwrote it, so the popup displays it.
    pub host_path: String,
    pub enabled: bool,
    /// Whether the profile that asked is the bound one.
    pub bound: bool,
    #[serde(default)]
    pub bound_browser: Option<Browser>,
    #[serde(default)]
    pub bound_profile_label: Option<String>,
    pub session: SessionState,
    #[serde(default)]
    pub account_hint: Option<String>,
    #[serde(default)]
    pub last_push_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    /// No session stored: never connected, signed out, or deliberately forgotten.
    None,
    /// Stored and inside its lifetime.
    Fresh,
    /// Stored but older than `SESSION_TTL_SECS`. Treated as absent when a
    /// download asks for it, and reported separately so the app can say the
    /// connection went quiet instead of failing at the next members-only link
    /// with nothing to explain it.
    Stale,
}

/// How long a pushed session is trusted without a refresh.
///
/// Seven days rather than a fortnight: the extension refreshes on every cookie
/// change and on a daily alarm, so a jar this old means the browser has not run
/// with the extension enabled for a week, and a week-old Google session is
/// worth less than the risk of keeping it.
pub const SESSION_TTL_SECS: i64 = 60 * 60 * 24 * 7;
