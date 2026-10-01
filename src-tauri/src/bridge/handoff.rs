//! Links the browser hands over for the app to download.
//!
//! The extension lists what a page is playing and, when the user presses
//! Download on one, sends it to the bridge host. The host cannot download
//! anything itself -- it is a short-lived helper the browser starts and kills
//! -- so it leaves the link in an inbox and starts the app, which takes
//! whatever is waiting there and queues it.
//!
//! The inbox is a directory of one small JSON file per link rather than one
//! shared file, for the same reason the rest of `bridge/` writes through a
//! rename: the host and the app are separate processes, and two links pressed
//! a moment apart must not have one host overwrite what the other wrote. Names
//! sort in the order the links arrived, so the app queues them in that order.
//!
//! Nothing here is trusted. The host checks every field before it writes, and
//! the app reads only files that parse, so a hand-written file in the inbox can
//! at worst queue a download of an ordinary web address.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::protocol::Download;
use crate::error::AppResult;
use crate::model::{DownloadMode, DownloadRequest, SourceContext, WatermarkPreference};
use crate::settings::Settings;

/// The argument the host starts the app with. The running copy receives it
/// through the single-instance plugin; a copy that is starting fresh reads the
/// inbox when its window opens and needs nothing from it.
pub const LAUNCH_ARG: &str = "--handoff";

/// How long a link waits for the app. Long enough for a cold start on a slow
/// machine; short enough that a link pressed while the app could not start is
/// not downloaded by surprise the next time it does.
pub const MAX_AGE_SECS: i64 = 10 * 60;

/// Media addresses carry signatures and tokens and run long, but a browser
/// will not send anything near this, and the inbox is not a place to store
/// megabytes on a stranger's say-so.
const MAX_URL_BYTES: usize = 8192;
const MAX_ORIGIN_BYTES: usize = 512;
const MAX_USER_AGENT_BYTES: usize = 512;
const MAX_TITLE_CHARS: usize = 300;

const KINDS: &[&str] = &["page", "stream", "video", "audio"];

/// One link waiting for the app, exactly as the inbox file holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handoff {
    pub url: String,
    /// `page`, `stream`, `video` or `audio`.
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub page_url: Option<String>,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    /// Unix seconds, stamped by the host when it accepted the link.
    pub received_at: i64,
}

impl Handoff {
    /// The request the app queues for this link: the one Home would send with
    /// the default options untouched, since the user picked the video in the
    /// browser and there is nothing left to ask them.
    ///
    /// Must stay in step with `requestFromHandoff` in `src/lib/handoff.ts`,
    /// field for field. That function builds the request the app downloads;
    /// this one builds the request the popup's preview is worked out from, and
    /// a preview of any other request would describe a download that never
    /// happens.
    pub fn to_request(&self, settings: &Settings) -> DownloadRequest {
        let mode = if self.kind == "audio" {
            DownloadMode::Audio
        } else {
            settings.default_mode
        };
        // `value || null` on the other side: an empty string is no value.
        let given = |value: &Option<String>| value.clone().filter(|value| !value.is_empty());

        DownloadRequest {
            url: self.url.clone(),
            mode,
            quality: settings.default_quality,
            video_format_id: None,
            audio_format_id: None,
            // A default container belongs to the default mode, as on Home: an
            // MP4 preference would have a song converted into a video.
            container: if mode == settings.default_mode {
                settings.default_container.clone()
            } else {
                None
            },
            watermark: WatermarkPreference::Any,
            output_dir: None,
            title: self
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_string),
            thumbnail_url: given(&self.thumbnail),
            // The download tells the site from the link; a stream's own
            // address rarely says which page it played on.
            platform: None,
            entry: None,
            audio_language: None,
            // Present even when every part of it is empty, as it is on the
            // other side. Its presence is what marks a link as handed over,
            // and that decides how it is analysed and what it is called.
            source: Some(SourceContext {
                page_url: given(&self.page_url),
                referer: given(&self.referer),
                origin: given(&self.origin),
                user_agent: given(&self.user_agent),
            }),
        }
    }
}

/// Check what the extension sent and cut it down to what the app may use.
///
/// The error is a short sentence for the popup's log, never the input itself:
/// the input is a page's address, and the reply travels back into the browser.
pub fn validate(download: &Download, received_at: i64) -> Result<Handoff, &'static str> {
    let url = web_address(Some(download.url.as_str()))?.ok_or("the link is empty")?;
    let page_url = web_address(download.page_url.as_deref())
        .map_err(|_| "the page address is not a web address")?;
    let referer = web_address(download.referer.as_deref())
        .map_err(|_| "the referring address is not a web address")?;

    let origin = match present(download.origin.as_deref()) {
        // What the browser sends for a request from a sandboxed frame or a
        // local file. It names no site, which is the same as naming none.
        Some("null") | None => None,
        Some(origin) => Some(web_origin(origin).ok_or("the origin is not a web origin")?),
    };

    let user_agent = match present(download.user_agent.as_deref()) {
        None => None,
        Some(agent) if agent.len() > MAX_USER_AGENT_BYTES || has_control(agent) => {
            return Err("the user agent is not one a browser sends");
        }
        Some(agent) => Some(agent.to_string()),
    };

    // A poster the page drew for itself (`data:`, `blob:`) is common and is
    // only a picture on a download row; it is not worth refusing the link for.
    let thumbnail = web_address(download.thumbnail.as_deref()).ok().flatten();

    let kind = download
        .kind
        .as_deref()
        .map(|kind| kind.trim().to_ascii_lowercase())
        .filter(|kind| KINDS.contains(&kind.as_str()))
        .unwrap_or_else(|| "page".to_string());

    Ok(Handoff {
        url,
        kind,
        title: download.title.as_deref().and_then(clean_title),
        page_url,
        referer,
        origin,
        user_agent,
        thumbnail,
        received_at,
    })
}

/// `%APPDATA%\UniversalDownloader\bridge\inbox`. Not created by asking: only a
/// deposit needs it to exist.
pub fn inbox() -> AppResult<PathBuf> {
    Ok(super::dir()?.join("inbox"))
}

/// Leave a link for the app. Called by the host.
pub fn deposit(handoff: &Handoff) -> AppResult<PathBuf> {
    deposit_in(&inbox()?, handoff)
}

/// Write `handoff` into `dir` under a name that sorts after every link already
/// there.
///
/// Milliseconds first, so names sort by arrival; then the process id and a
/// counter, so two hosts in the same millisecond -- two profiles, or one
/// impatient double press -- cannot pick the same name. Both numbers that vary
/// in length are padded, or `…-10.json` would sort before `…-9.json`.
pub fn deposit_in(dir: &Path, handoff: &Handoff) -> AppResult<PathBuf> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    std::fs::create_dir_all(dir)?;
    let name = format!(
        "{:013}-{}-{:06}.json",
        chrono::Utc::now().timestamp_millis().max(0),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let path = dir.join(name);
    let bytes = serde_json::to_vec(handoff)?;
    // The temporary name ends in `.tmp`, so a reader listing `*.json` never
    // sees a link half written.
    super::state::write_atomic(&path, &bytes)?;
    Ok(path)
}

/// Every link waiting for the app, oldest first. Called by the app.
pub fn take_all() -> AppResult<Vec<Handoff>> {
    take_from(&inbox()?, chrono::Utc::now().timestamp())
}

/// Read and remove what `dir` holds, as of `now` in Unix seconds.
///
/// A link is returned only once its file is gone. The window can ask twice at
/// the same moment -- once as it mounts and once for the event of the launch
/// that started it -- and a link both calls read must still be downloaded only
/// once; whichever call fails to delete it leaves it to the other.
///
/// Files that do not parse, and links older than `MAX_AGE_SECS`, are deleted
/// and not returned: nothing would ever make them useful.
pub fn take_from(dir: &Path, now: i64) -> AppResult<Vec<Handoff>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };

    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json") && path.is_file())
        .collect();
    files.sort();

    let mut taken = Vec::new();
    for path in files {
        // Gone already, or still held by whoever is writing it: either way
        // not this call's to take.
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if std::fs::remove_file(&path).is_err() {
            continue;
        }
        let Ok(handoff) = serde_json::from_slice::<Handoff>(&bytes) else {
            crate::log_warn!("bridge", "dropped an unreadable link from the inbox");
            continue;
        };
        if now - handoff.received_at > MAX_AGE_SECS {
            crate::log_warn!("bridge", "dropped a link that waited too long for the app");
            continue;
        }
        taken.push(handoff);
    }

    Ok(taken)
}

/// A value the extension actually filled in.
fn present(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

/// An `http` or `https` address with a host, or nothing when none was given.
///
/// Checked on the text as sent as well as on the parsed address: the parser
/// quietly removes tabs and line breaks, which a header built from this later
/// must never carry and which a real browser never sends. The preview's
/// picture is held to the same rule on its way back to the popup.
pub(super) fn web_address(value: Option<&str>) -> Result<Option<String>, &'static str> {
    let Some(value) = present(value) else {
        return Ok(None);
    };
    if value.len() > MAX_URL_BYTES {
        return Err("the link is too long");
    }
    if has_control(value) {
        return Err("the link is not a web address");
    }
    let parsed = reqwest::Url::parse(value).map_err(|_| "the link is not a web address")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none_or(str::is_empty) {
        return Err("the link is not a web address");
    }
    Ok(Some(value.to_string()))
}

/// `scheme://host[:port]` and nothing more, the only shape an `Origin` header
/// takes.
fn web_origin(value: &str) -> Option<String> {
    if value.len() > MAX_ORIGIN_BYTES || has_control(value) {
        return None;
    }
    let (_, authority) = value.split_once("://")?;
    if authority.is_empty() || authority.contains(['/', '?', '#', '@', '\\']) {
        return None;
    }
    let parsed = reqwest::Url::parse(value).ok()?;
    (matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some_and(|host| !host.is_empty()))
        .then(|| value.to_string())
}

/// A page's title as a file name will later be made from it: line breaks and
/// tabs become spaces, other control characters go, and it stops at
/// `MAX_TITLE_CHARS`.
fn clean_title(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter_map(|c| match c {
            c if c.is_control() && c.is_whitespace() => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect();
    let cut: String = cleaned.trim().chars().take(MAX_TITLE_CHARS).collect();
    let cut = cut.trim_end();
    (!cut.is_empty()).then(|| cut.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::protocol::{Browser, Request};

    fn wire(extra: serde_json::Value) -> Download {
        let mut body = serde_json::json!({
            "type": "download",
            "v": 1,
            "profileId": "profile-a",
            "browser": "chrome",
            "extensionVersion": "1.0.4",
            "url": "https://cdn.example/hls/master.m3u8",
        });
        for (key, value) in extra.as_object().unwrap() {
            body[key] = value.clone();
        }
        match serde_json::from_value::<Request>(body).unwrap() {
            Request::Download(download) => download,
            other => panic!("parsed as {other:?}"),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ud-inbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn handoff(url: &str, received_at: i64) -> Handoff {
        Handoff {
            url: url.to_string(),
            kind: "stream".to_string(),
            title: None,
            page_url: None,
            referer: None,
            origin: None,
            user_agent: None,
            thumbnail: None,
            received_at,
        }
    }

    #[test]
    fn the_wire_request_carries_the_peer_and_every_field() {
        let download = wire(serde_json::json!({
            "kind": "stream",
            "title": "Bölüm 5",
            "pageUrl": "https://site.example/watch/5",
            "referer": "https://player.example/",
            "origin": "https://player.example",
            "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
            "thumbnail": "https://site.example/poster.jpg",
        }));
        assert_eq!(download.peer.v, 1);
        assert_eq!(download.peer.profile_id, "profile-a");
        assert_eq!(download.peer.browser, Browser::Chrome);
        assert_eq!(download.peer.extension_version, "1.0.4");
        assert_eq!(download.kind.as_deref(), Some("stream"));
        assert_eq!(download.page_url.as_deref(), Some("https://site.example/watch/5"));
        assert_eq!(download.origin.as_deref(), Some("https://player.example"));

        let handoff = validate(&download, 1_000).unwrap();
        assert_eq!(handoff.url, "https://cdn.example/hls/master.m3u8");
        assert_eq!(handoff.kind, "stream");
        assert_eq!(handoff.title.as_deref(), Some("Bölüm 5"));
        assert_eq!(handoff.referer.as_deref(), Some("https://player.example/"));
        assert_eq!(handoff.user_agent.as_deref(), Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"));
        assert_eq!(handoff.thumbnail.as_deref(), Some("https://site.example/poster.jpg"));
        assert_eq!(handoff.received_at, 1_000);
    }

    #[test]
    fn only_the_address_is_required_and_an_unknown_kind_is_a_page() {
        let download = wire(serde_json::json!({}));
        let handoff = validate(&download, 0).unwrap();
        assert_eq!(handoff.kind, "page");
        assert_eq!(handoff.title, None);
        assert_eq!(handoff.page_url, None);

        let download = wire(serde_json::json!({ "kind": "playlist", "pageUrl": "", "origin": null }));
        let handoff = validate(&download, 0).unwrap();
        assert_eq!(handoff.kind, "page");
        assert_eq!(handoff.page_url, None);
        assert_eq!(handoff.origin, None);
    }

    #[test]
    fn the_file_shape_is_camel_case() {
        let text = serde_json::to_string(&handoff("https://a.example/v.mp4", 5)).unwrap();
        assert!(text.contains(r#""receivedAt":5"#), "{text}");
        assert!(text.contains(r#""pageUrl""#), "{text}");
        assert!(text.contains(r#""userAgent""#), "{text}");
    }

    #[test]
    fn only_web_addresses_are_accepted() {
        for good in ["https://cdn.example/a.mp4", "http://192.168.1.4:8080/live.m3u8"] {
            let download = wire(serde_json::json!({ "url": good }));
            assert!(validate(&download, 0).is_ok(), "{good}");
        }
        for bad in [
            "javascript:alert(1)",
            "file:///C:/Windows/win.ini",
            "data:video/mp4;base64,AAAA",
            "blob:https://site.example/1234",
            "ftp://files.example/a.mp4",
            "https://",
            "not a link",
            "https://cdn.example/a\r\nX-Injected: 1",
            "https://cdn.example/a\tb.mp4",
        ] {
            let download = wire(serde_json::json!({ "url": bad }));
            assert!(validate(&download, 0).is_err(), "accepted {bad:?}");
        }

        let long = format!("https://cdn.example/{}", "a".repeat(MAX_URL_BYTES));
        assert!(validate(&wire(serde_json::json!({ "url": long })), 0).is_err());

        // The other addresses are held to the same rule when they are given.
        assert!(validate(&wire(serde_json::json!({ "pageUrl": "javascript:void(0)" })), 0).is_err());
        assert!(validate(&wire(serde_json::json!({ "referer": "file:///etc/passwd" })), 0).is_err());
    }

    #[test]
    fn a_poster_the_page_drew_for_itself_is_dropped_not_refused() {
        let download = wire(serde_json::json!({ "thumbnail": "data:image/png;base64,AAAA" }));
        assert_eq!(validate(&download, 0).unwrap().thumbnail, None);
    }

    #[test]
    fn an_origin_has_no_path() {
        let ok = |origin: &str| validate(&wire(serde_json::json!({ "origin": origin })), 0);
        assert_eq!(ok("https://player.example").unwrap().origin.as_deref(), Some("https://player.example"));
        assert_eq!(ok("http://localhost:3000").unwrap().origin.as_deref(), Some("http://localhost:3000"));
        assert_eq!(ok("null").unwrap().origin, None);

        for bad in [
            "https://player.example/",
            "https://player.example/embed",
            "https://player.example?x=1",
            "https://user@player.example",
            "chrome-extension://abc",
            "player.example",
        ] {
            assert!(ok(bad).is_err(), "accepted {bad:?}");
        }
        assert!(ok(&format!("https://{}.example", "a".repeat(MAX_ORIGIN_BYTES))).is_err());
    }

    #[test]
    fn a_user_agent_is_a_single_short_line() {
        let ok = |agent: &str| validate(&wire(serde_json::json!({ "userAgent": agent })), 0);
        assert!(ok("Mozilla/5.0").is_ok());
        assert!(ok("Mozilla/5.0\r\nCookie: x").is_err());
        assert!(ok(&"a".repeat(MAX_USER_AGENT_BYTES + 1)).is_err());
    }

    #[test]
    fn a_title_is_trimmed_cleaned_and_capped() {
        let title = |raw: &str| {
            validate(&wire(serde_json::json!({ "title": raw })), 0)
                .unwrap()
                .title
        };
        assert_eq!(title("  Bölüm 5\u{7}\n ").as_deref(), Some("Bölüm 5"));
        assert_eq!(title("Part\t1\r\nfinale").as_deref(), Some("Part 1  finale"));
        assert_eq!(title(" \u{0}\n ").as_deref(), None);

        let long = title(&"ş".repeat(400)).unwrap();
        assert_eq!(long.chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn links_come_back_in_the_order_they_arrived_and_only_once() {
        let dir = scratch("order");
        for index in 0..12 {
            deposit_in(&dir, &handoff(&format!("https://a.example/{index}.mp4"), 100)).unwrap();
        }

        let taken = take_from(&dir, 100).unwrap();
        let urls: Vec<_> = taken.iter().map(|item| item.url.as_str()).collect();
        let expected: Vec<_> = (0..12).map(|index| format!("https://a.example/{index}.mp4")).collect();
        assert_eq!(urls, expected);

        // Taking deletes: nothing is left for a second call.
        assert!(take_from(&dir, 100).unwrap().is_empty());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_link_that_waited_too_long_is_dropped() {
        let dir = scratch("expiry");
        deposit_in(&dir, &handoff("https://a.example/old.mp4", 1_000)).unwrap();
        deposit_in(&dir, &handoff("https://a.example/new.mp4", 1_000 + MAX_AGE_SECS)).unwrap();

        let taken = take_from(&dir, 1_001 + MAX_AGE_SECS).unwrap();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].url, "https://a.example/new.mp4");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the old one is deleted too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_does_not_parse_is_dropped_and_the_rest_still_arrive() {
        let dir = scratch("garbage");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("0000000000001-1-000000.json"), b"{\"url\":").unwrap();
        // A write still in flight is not a link yet, and is left alone.
        std::fs::write(dir.join("0000000000002-1-000000.json.1.tmp"), b"{}").unwrap();
        deposit_in(&dir, &handoff("https://a.example/v.mp4", 50)).unwrap();

        let taken = take_from(&dir, 50).unwrap();
        assert_eq!(taken.len(), 1);
        assert!(!dir.join("0000000000001-1-000000.json").exists());
        assert!(dir.join("0000000000002-1-000000.json.1.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_inbox_that_was_never_made_is_empty() {
        let dir = scratch("missing");
        assert!(take_from(&dir, 0).unwrap().is_empty());
        assert!(!dir.exists());
    }

    // The four cases below are the ones `src/lib/handoff.test.ts` holds
    // `requestFromHandoff` to. The two functions have to agree, so they are
    // held to the same examples.

    fn defaults() -> Settings {
        Settings {
            default_mode: DownloadMode::Video,
            default_quality: crate::model::QualityPreference::MaxHeight { height: 1080 },
            default_container: Some("mp4".into()),
            ..Settings::default()
        }
    }

    fn stream() -> Handoff {
        Handoff {
            url: "https://cdn.example.com/hls/master.m3u8".into(),
            kind: "stream".into(),
            title: Some("Bölüm 5".into()),
            page_url: Some("https://dizi.example.com/izle/5".into()),
            referer: Some("https://player.example.com/".into()),
            origin: Some("https://player.example.com".into()),
            user_agent: Some("Mozilla/5.0".into()),
            thumbnail: Some("https://cdn.example.com/poster.jpg".into()),
            received_at: 1_790_000_000,
        }
    }

    #[test]
    fn a_handed_over_link_is_asked_for_as_home_would_with_the_default_options() {
        let request = stream().to_request(&defaults());
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "url": "https://cdn.example.com/hls/master.m3u8",
                "mode": "video",
                "quality": { "type": "maxHeight", "height": 1080 },
                "videoFormatId": null,
                "audioFormatId": null,
                "container": "mp4",
                "watermark": "any",
                "outputDir": null,
                "title": "Bölüm 5",
                "thumbnailUrl": "https://cdn.example.com/poster.jpg",
                "platform": null,
                "entry": null,
                "audioLanguage": null,
                "source": {
                    "pageUrl": "https://dizi.example.com/izle/5",
                    "referer": "https://player.example.com/",
                    "origin": "https://player.example.com",
                    "userAgent": "Mozilla/5.0",
                },
            })
        );
    }

    #[test]
    fn a_sound_is_asked_for_as_a_sound_without_the_video_container() {
        let request = Handoff {
            kind: "audio".into(),
            ..stream()
        }
        .to_request(&defaults());
        assert_eq!(request.mode, DownloadMode::Audio);
        assert_eq!(request.container, None);

        // Every other kind takes the default mode and its container.
        for kind in ["page", "stream", "video"] {
            let request = Handoff {
                kind: kind.into(),
                ..stream()
            }
            .to_request(&defaults());
            assert_eq!(request.mode, DownloadMode::Video, "{kind}");
            assert_eq!(request.container.as_deref(), Some("mp4"), "{kind}");
        }
    }

    #[test]
    fn the_container_is_kept_when_the_default_is_already_sound() {
        let settings = Settings {
            default_mode: DownloadMode::Audio,
            default_container: Some("m4a".into()),
            ..defaults()
        };
        let request = Handoff {
            kind: "video".into(),
            ..stream()
        }
        .to_request(&settings);
        assert_eq!(request.mode, DownloadMode::Audio);
        assert_eq!(request.container.as_deref(), Some("m4a"));
    }

    #[test]
    fn what_the_browser_did_not_know_is_left_out_but_the_source_stays() {
        let request = Handoff {
            kind: "page".into(),
            title: Some("   ".into()),
            referer: None,
            origin: None,
            user_agent: Some(String::new()),
            thumbnail: None,
            ..stream()
        }
        .to_request(&defaults());
        assert_eq!(request.title, None);
        assert_eq!(request.thumbnail_url, None);
        assert_eq!(
            request.source,
            Some(SourceContext {
                page_url: Some("https://dizi.example.com/izle/5".into()),
                ..SourceContext::default()
            })
        );

        let bare = handoff("https://a.example/v.mp4", 0);
        assert_eq!(bare.to_request(&defaults()).source, Some(SourceContext::default()));
    }
}
