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
//!
//! The same holds for the one reply that describes something other than the
//! link itself: a preview says what a download would fetch -- a title, a
//! picture, a resolution, a size -- and never anything about the session that
//! may have been lent to the engine to read it.

use serde::{Deserialize, Serialize};

use crate::downloader::DownloadPreview;

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
/// the id the Chrome Web Store gave the published listing. An empty one is
/// skipped, which is how a build made before a listing existed behaved.
pub const EXTENSION_ID_DEV: &str = "bkoicficlaelgjpjhlddhloepoocpfoj";
pub const EXTENSION_ID_STORE: &str = "oikcjjcihkfmgmmmjagilnfgnfilghic";

/// Whether there is a published listing to point the user at. Settings shows
/// the Connection section either way; this decides whether it offers "get the
/// extension" or says the listing is on its way, because a button that opens
/// a dead store page is worse than no button.
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

/// The Firefox copy's id. Firefox takes it from the manifest's
/// `browser_specific_settings` rather than deriving one, so the one id serves
/// the listing on addons.mozilla.org and an unpacked copy alike. It is named in
/// the Firefox host manifest's `allowed_extensions`, the field Firefox reads in
/// place of Chrome's `allowed_origins`.
pub const FIREFOX_EXTENSION_ID: &str = "connector@universaldownloader.app";

/// Whether the browser started this host for one of our extensions, from the
/// arguments it passed.
///
/// The two families say it differently. Chrome passes the caller's origin
/// first. Firefox passes the path of the host manifest first and the
/// extension's id second. Each is compared whole, as `origin_allowed` does.
pub fn caller_allowed(args: &[String]) -> bool {
    match args {
        [_, first, ..] if origin_allowed(first) => true,
        [_, _, second, ..] => second == FIREFOX_EXTENSION_ID,
        _ => false,
    }
}

#[cfg(test)]
mod caller_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn chrome_and_firefox_each_name_the_caller_their_own_way() {
        let exe = r"C:\app\ud-bridge.exe";
        let store = format!("chrome-extension://{EXTENSION_ID_STORE}/");
        assert!(caller_allowed(&args(&[exe, &store, "--parent-window=0"])));
        assert!(caller_allowed(&args(&[exe, r"C:\x\host-manifest-firefox.json", FIREFOX_EXTENSION_ID])));

        assert!(!caller_allowed(&args(&[exe, "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"])));
        assert!(!caller_allowed(&args(&[exe, r"C:\x\m.json", "other@example.com"])));
        assert!(!caller_allowed(&args(&[exe, r"C:\x\m.json", "connector@universaldownloader.appx"])));
        assert!(!caller_allowed(&args(&[exe])));
    }
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
    Firefox,
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
            Self::Firefox => "Firefox",
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
    /// Raise the app's window. The 1.0 popup sent this from its "Open
    /// Universal Downloader" button; 1.0.3 and later no longer do, and it
    /// stays for the copies that have not updated yet. Carries nothing and
    /// returns nothing but a status.
    OpenApp(Peer),
    /// The user pressed Download on something playing in a tab. The host
    /// leaves it in the app's inbox and starts the app, which downloads it.
    Download(Download),
    /// The popup is about to offer Download on something playing in a tab and
    /// wants to say what pressing it would fetch. It carries exactly what the
    /// download would, so the answer can be the app's own: the host builds the
    /// request the app would queue and runs the analysis and the plan the
    /// download runs, then stops before a byte of media moves. Nothing is left
    /// in the inbox and the app is not started.
    ///
    /// Answered with a `preview`, or with `protected` or `unavailable` when
    /// there is none to give. A host from before this request answers
    /// `malformed`, which the popup reads as "no details" -- the reason this
    /// did not need a new wire version.
    Probe(Download),
}

/// A video or sound the extension saw a page play, as it arrives on the wire,
/// whether to be downloaded or only probed.
///
/// Everything but the address is optional and everything is untrusted: it is
/// checked and cut down by `handoff::validate` before any of it is written
/// anywhere, and an older extension that sends less still gets its download.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    #[serde(flatten)]
    pub peer: Peer,
    pub url: String,
    /// `page`, `stream`, `video` or `audio`. Anything else, or nothing, is
    /// read as `page`: the app analyses a page the most carefully.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// The address of the tab the media was playing in.
    #[serde(default)]
    pub page_url: Option<String>,
    /// The `Referer` the browser sent for the media, or the extension's best
    /// reading of it.
    #[serde(default)]
    pub referer: Option<String>,
    /// The `Origin` the browser sent, which it only does for a script's
    /// request -- the shape a stream player's requests take.
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
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
///
/// `preview` is the answer to a `probe` and appears on nothing else. Like every
/// field added since the first version it is left out rather than sent empty,
/// so an extension that has never heard of it reads the rest unchanged.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub v: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<HostStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Preview>,
}

impl Response {
    pub fn ok(status: HostStatus) -> Self {
        Self {
            v: WIRE_VERSION,
            ok: true,
            status: Some(status),
            error: None,
            preview: None,
        }
    }

    /// The answer to a probe that found something to download.
    pub fn previewed(status: HostStatus, preview: Preview) -> Self {
        Self {
            preview: Some(preview),
            ..Self::ok(status)
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
            preview: None,
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
    /// A probe found a service that encrypts what it streams. Nothing the app
    /// does can download from it, which is the one answer that changes what
    /// the popup offers: it says so and leaves the button out.
    Protected,
    /// A probe found nothing it could describe: a link no provider reads, one
    /// that is gone, a site that did not answer in time, or an app without
    /// its engine. All of these mean the same to the popup -- no details --
    /// so they are one code, and the download itself is where the app
    /// explains which it was.
    Unavailable,
}

/// What the host tells the popup about itself. Read the module header before
/// adding a field: nothing here may describe cookie contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    pub app_version: String,
    /// The host's own executable path. The only tell a user has that the
    /// registry entry still points at the real app and not at something that
    /// overwrote it. The 1.0 popup displayed it; Settings' diagnostics still
    /// carry the same path.
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
    /// Whether this host understands `download`. Always true from this one;
    /// the field is what lets the popup tell an app too old to take a link,
    /// which leaves it out, from one that simply has not been asked yet.
    #[serde(default)]
    pub can_download: bool,
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

/// What pressing Download on a probed link would fetch, as the app worked it
/// out: the analysis and the plan the download itself runs, stopped before a
/// byte of media moves. Read the module header before adding a field.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// What the finished download is called: the source's own title, or for
    /// a stream a page played, the title of the tab it played in.
    pub title: String,
    /// `http` or `https` only. The popup draws it as an image, and a picture
    /// a page made for itself (`data:`, `blob:`) or anything stranger is left
    /// out rather than handed to it.
    pub thumbnail: Option<String>,
    pub duration_sec: Option<f64>,
    /// The plan's own label for the stream it chose: "1080p", "128 kbps",
    /// "1080x1350".
    pub quality_label: String,
    /// The finished file's extension, lowercase as the plan has it: `mkv`,
    /// `mp4`, `m4a`.
    pub container: String,
    /// The sizes the source states for the chosen streams, or what their
    /// bitrate comes to over the running time when it states none. Absent
    /// when neither is known -- which includes every live stream, whose size
    /// is not decided until it ends.
    pub estimated_bytes: Option<u64>,
    /// The site's display name, as the app shows it: "YouTube", "Web page".
    pub platform: String,
    pub is_live: bool,
    /// The plan takes sound and no picture: the page played a sound, Audio is
    /// the user's default, or the media has no picture to take.
    pub audio_only: bool,
}

/// Longer than any title a site gives a video, and short enough that a page
/// with an absurd `<title>` cannot push a reply past `MAX_MESSAGE_BYTES`. The
/// same bound the host puts on a title the extension sends.
const MAX_PREVIEW_TITLE_CHARS: usize = 300;

impl From<DownloadPreview> for Preview {
    fn from(preview: DownloadPreview) -> Self {
        Self {
            title: preview.title.chars().take(MAX_PREVIEW_TITLE_CHARS).collect(),
            thumbnail: preview
                .thumbnail_url
                .and_then(|url| super::handoff::web_address(Some(&url)).ok().flatten()),
            duration_sec: preview.duration_sec,
            quality_label: preview.quality_label,
            container: preview.container,
            estimated_bytes: preview.estimated_bytes,
            platform: preview.platform_label,
            is_live: preview.is_live,
            audio_only: preview.audio_only,
        }
    }
}

/// How long a pushed session is trusted without a refresh.
///
/// Seven days rather than a fortnight: the extension refreshes on every cookie
/// change and on a daily alarm, so a jar this old means the browser has not run
/// with the extension enabled for a week, and a week-old Google session is
/// worth less than the risk of keeping it.
pub const SESSION_TTL_SECS: i64 = 60 * 60 * 24 * 7;

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> HostStatus {
        HostStatus {
            app_version: "1.0.0".into(),
            host_path: "C:\\ud-bridge.exe".into(),
            enabled: true,
            bound: false,
            bound_browser: None,
            bound_profile_label: None,
            session: SessionState::None,
            account_hint: None,
            last_push_at: None,
            can_download: true,
        }
    }

    fn found() -> DownloadPreview {
        DownloadPreview {
            title: "Me at the zoo".into(),
            thumbnail_url: Some("https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg".into()),
            duration_sec: Some(19.0),
            quality_label: "240p".into(),
            container: "mp4".into(),
            estimated_bytes: Some(791_000),
            platform_label: "YouTube".into(),
            is_live: false,
            audio_only: false,
        }
    }

    #[test]
    fn a_probe_carries_what_a_download_carries() {
        let request: Request = serde_json::from_value(serde_json::json!({
            "type": "probe", "v": 1, "profileId": "p", "browser": "edge",
            "extensionVersion": "1.0.4", "url": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
            "kind": "page", "title": "Me at the zoo", "pageUrl": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        }))
        .unwrap();
        let Request::Probe(probe) = request else {
            panic!("not read as a probe");
        };
        assert_eq!(probe.peer.browser, Browser::Edge);
        assert_eq!(probe.kind.as_deref(), Some("page"));
        assert_eq!(probe.title.as_deref(), Some("Me at the zoo"));
    }

    #[test]
    fn only_the_answer_to_a_probe_carries_a_preview() {
        let plain = serde_json::to_value(Response::ok(status())).unwrap();
        assert!(plain.get("preview").is_none(), "{plain}");

        let answer = serde_json::to_value(Response::previewed(status(), found().into())).unwrap();
        assert_eq!(answer["ok"], true);
        assert!(answer.get("status").is_some());
        assert_eq!(
            answer["preview"],
            serde_json::json!({
                "title": "Me at the zoo",
                "thumbnail": "https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg",
                "durationSec": 19.0,
                "qualityLabel": "240p",
                "container": "mp4",
                "estimatedBytes": 791_000,
                "platform": "YouTube",
                "isLive": false,
                "audioOnly": false,
            })
        );
    }

    #[test]
    fn the_refusals_of_a_probe_have_names_of_their_own() {
        let protected = serde_json::to_value(Response::err(ErrorCode::Protected, "x")).unwrap();
        assert_eq!(protected["error"]["code"], "protected");
        assert!(protected.get("preview").is_none());
        let unavailable = serde_json::to_value(Response::err(ErrorCode::Unavailable, "x")).unwrap();
        assert_eq!(unavailable["error"]["code"], "unavailable");
    }

    #[test]
    fn a_preview_hands_the_popup_only_a_web_picture_and_a_bounded_title() {
        for kept in ["https://i.ytimg.com/vi/a/hq.jpg", "http://cdn.example/poster.png"] {
            let preview = Preview::from(DownloadPreview {
                thumbnail_url: Some(kept.into()),
                ..found()
            });
            assert_eq!(preview.thumbnail.as_deref(), Some(kept));
        }
        for dropped in [
            "data:image/png;base64,AAAA",
            "blob:https://site.example/1234",
            "file:///C:/Users/me/Pictures/a.jpg",
            "javascript:alert(1)",
            "",
        ] {
            let preview = Preview::from(DownloadPreview {
                thumbnail_url: Some(dropped.into()),
                ..found()
            });
            assert_eq!(preview.thumbnail, None, "kept {dropped:?}");
        }

        let preview = Preview::from(DownloadPreview {
            title: "ş".repeat(5_000),
            ..found()
        });
        assert_eq!(preview.title.chars().count(), MAX_PREVIEW_TITLE_CHARS);
    }
}
