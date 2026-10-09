//! The browser link: letting a signed-in browser lend the app its YouTube and
//! TikTok sessions, so content the user already pays for -- or is old enough
//! for -- can be downloaded, and letting the user send what a page is playing
//! to the app to download.
//!
//! The browser never hands cookies to the app directly. A small extension reads
//! them with `chrome.cookies` and pushes them through Chrome's own native
//! messaging channel to `ud-bridge.exe`, which Chrome starts for it. Nothing
//! listens on a port, so nothing a web page can reach is involved, and the only
//! process the browser will start for that extension is the one named in a
//! registry value under the user's own hive.
//!
//! Two processes share this module: the app, and the bridge host. That is why
//! everything here works through files and holds no in-memory state -- the host
//! runs while the app is closed, which is most of the time.
//!
//! Layout under `%APPDATA%\UniversalDownloader\bridge`:
//!
//! * `session.bin` -- YouTube's cookie jar, encrypted with DPAPI for this
//!   Windows user. A copy of this file on another machine, in a backup or in a
//!   support archive is inert, which is the threat that actually collects
//!   sessions at scale.
//! * `session-tiktok.bin` -- TikTok's, sealed the same way. A jar of its own
//!   rather than one shared file, so each site's switch in the extension can
//!   lend or withdraw its session without touching the other's, and the engine
//!   is only ever handed the cookies of the site it is reading.
//! * `session-other.bin` -- one more site's, whichever the user last pressed
//!   İndir on in the extension with its "Other sites" switch on: that site's
//!   domain and its cookies alone, sealed the same way, replaced by the next
//!   such press and deleted an hour after it was captured. Lent on the first
//!   engine run of a link on that site, rather than after a refusal as the
//!   two above are, because no wall the engine reports says "this site would
//!   show it to you signed in" in words the app could tell apart.
//! * `entropy.bin` -- per-install random bytes mixed into that encryption, so
//!   the blobs cannot be decrypted by another app running as the same user
//!   without also stealing this file.
//! * `state.json` -- what the interface and the popup show: which profile is
//!   bound, how old the session is, whether the feature is on. No cookie
//!   material, because the host answers the popup's questions from it and the
//!   popup lives in the browser.
//! * `leases/` -- plaintext `cookies.txt` handed to yt-dlp, one per run,
//!   deleted when the run ends. Deliberately not `temp_dir()`: the
//!   `sweep_temp_files` command clears that on demand and would pull the jar
//!   out from under a live download.
//! * `inbox/` -- links the user sent from the browser to download, one file
//!   each, left by the host and taken by the app (see `handoff`).
//!
//! Both processes write `state.json`, so every write is a write-then-rename and
//! every read tolerates a torn or missing file by falling back to defaults.

pub mod handoff;
pub mod protocol;

mod cookies;
mod state;
mod store;

#[cfg(windows)]
mod registry;

use std::path::{Path, PathBuf};

use crate::error::AppResult;
use crate::model::BridgeStatus;
use crate::settings::Settings;

pub use protocol::{Browser, HostStatus, SessionState, Site, HOST_NAME};
pub use state::LinkState;
pub use store::CookieLease;

/// Whether this build can host a browser link at all.
///
/// Windows only, and not because of the cookie handling: native messaging is
/// registered through the registry, and the installer, the tray and autostart
/// already make Windows the only desktop target this app ships. A phone has no
/// desktop browser to link to, so the whole feature compiles out there.
pub const fn supported() -> bool {
    cfg!(windows)
}

/// `%APPDATA%\UniversalDownloader\bridge`, created on first use.
pub fn dir() -> AppResult<PathBuf> {
    let dir = crate::paths::root()?.join("bridge");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// An address's host, lowercase and without its `www.`.
fn host_of(url: &str) -> String {
    url.split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_ascii_lowercase()
}

impl Site {
    /// The site whose stored session an address could use, if any.
    ///
    /// The link exists for two things -- content behind a YouTube membership,
    /// and TikTok posts shown only to a signed-in viewer -- and a cookie jar
    /// that is never written to disk is a cookie jar that cannot leak.
    /// Everything else the app downloads runs exactly as it did before.
    ///
    /// YouTube by whole host, as it always was. TikTok by domain and
    /// subdomain, the way `photos::tiktok` reads a TikTok address: its jar is
    /// scoped to `.tiktok.com`, so a regional or short-link host it does not
    /// list by name still belongs to it, and the engine sends it nowhere else.
    ///
    /// Never `Other`: which site that session is for is a fact about the jar,
    /// not about the address, and `other_lease` is what asks it.
    pub fn for_url(url: &str) -> Option<Site> {
        let host = host_of(url);
        if matches!(
            host.as_str(),
            "youtube.com"
                | "m.youtube.com"
                | "music.youtube.com"
                | "youtu.be"
                | "youtube-nocookie.com"
        ) {
            return Some(Site::Youtube);
        }
        (host == "tiktok.com" || host.ends_with(".tiktok.com")).then_some(Site::Tiktok)
    }
}

/// Whether a download should bother asking for a lease of `site`'s session:
/// the feature is on, the platform supports it, and a session for that site
/// exists that has not gone stale.
///
/// The app's switch stays one switch for every site. Which sites the browser
/// lends is the extension's to say, one switch each, and a site it does not
/// lend simply has no session here.
pub fn available(settings: &Settings, site: Site) -> bool {
    supported()
        && settings.browser_link_enabled
        && state::load().session_state(site) == SessionState::Fresh
}

/// Materialise `site`'s stored session as a `cookies.txt` for one engine run.
///
/// Returns `None` when there is nothing usable, which is the common case and
/// never an error. The file is deleted when the lease is dropped; see
/// `CookieLease::fold_back` for what happens to the copy yt-dlp rewrites.
pub fn lease(settings: &Settings, site: Site) -> Option<CookieLease> {
    if !available(settings, site) {
        return None;
    }
    match store::lease(site) {
        Ok(lease) => lease,
        Err(err) => {
            crate::log_warn!("bridge", "could not prepare the browser session: {err}");
            None
        }
    }
}

/// The other-site jar as `lends_other` weighs it: the site it is for and when
/// it was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtherJar<'a> {
    pub domain: &'a str,
    pub captured_at: i64,
}

/// Whether a run reading `url`, handed over from `page_url`, should be lent
/// the other-site session `jar` at `now`. Pure, so every rule can be tested
/// without a jar on disk.
///
/// - The app's switch is on, and there is a jar inside its hour.
/// - The address is not YouTube's or TikTok's. Those have sessions of their
///   own, lent by their own rules, and an Instagram jar is nothing to them.
/// - The address's host is the jar's site or under it -- `www.instagram.com`
///   for `instagram.com` -- or the page it was handed over from is. The page
///   matters because a page's own player stream is often served from a host
///   of the site's that the address is not on, and the engine reads such a
///   stream with the page as its referer. Lending on the page alone is safe
///   for the same reason the engine is safe to lend at all: yt-dlp sends a
///   cookie only to a host its domain matches, so a jar handed to a run on
///   some CDN's address goes unused rather than to the CDN.
pub fn lends_other(
    enabled: bool,
    jar: Option<OtherJar<'_>>,
    url: &str,
    page_url: Option<&str>,
    now: i64,
) -> bool {
    let Some(jar) = jar else {
        return false;
    };
    if !enabled
        || !state::other_fresh(Some(jar.captured_at), now)
        || !protocol::other_domain_ok(jar.domain)
        || Site::for_url(url).is_some()
    {
        return false;
    }
    // Parsed properly rather than cut out of the text: `user@host` and a port
    // are where a hand-rolled reading of an address goes wrong, and this one
    // decides where a session is sent.
    let on_site = |address: &str| {
        reqwest::Url::parse(address).ok().is_some_and(|parsed| {
            matches!(parsed.scheme(), "http" | "https")
                && parsed
                    .host_str()
                    .is_some_and(|host| protocol::under_domain(host, jar.domain))
        })
    };
    on_site(url) || page_url.is_some_and(on_site)
}

/// The other-site session, as a `cookies.txt` for the first engine run of
/// `url`, when `lends_other` says so and that jar was not already refused for
/// this address (see `CookieLease::refused`).
///
/// The decision is taken on `state.json`, which costs no decryption; the jar
/// is only opened once it is known to be wanted, and the lease checks the
/// jar's own capture time again.
pub fn other_lease(settings: &Settings, url: &str, page_url: Option<&str>) -> Option<CookieLease> {
    // The switch first, before anything on disk is looked at: with it off --
    // as every test that runs the pipeline sets it -- nothing under `bridge/`
    // is so much as listed.
    if !supported() || !settings.browser_link_enabled || !store::session_exists(Site::Other) {
        return None;
    }
    let link = state::load();
    let captured_at = link.other.last_push_at?;
    let jar = link.other.domain.as_deref().map(|domain| OtherJar {
        domain,
        captured_at,
    });
    let now = chrono::Utc::now().timestamp();
    if !lends_other(settings.browser_link_enabled, jar, url, page_url, now)
        || store::refused_before(url, captured_at)
    {
        return None;
    }
    match store::lease(Site::Other) {
        Ok(lease) => lease,
        Err(err) => {
            crate::log_warn!("bridge", "could not prepare the other-site session: {err}");
            None
        }
    }
}

/// Remove lease files a crash left behind, and an other-site session whose
/// hour ran out while the app was closed. Run once at startup; a lease that
/// outlives its process is never resumable state, unlike a partial download.
pub fn sweep_leases() {
    if let Err(err) = store::sweep_leases() {
        crate::log_warn!("bridge", "lease sweep failed: {err}");
    }
    store::drop_lapsed();
}

/// What Settings shows. Cheap enough to poll: it reads one small JSON file and
/// stats another, and never decrypts the session.
pub fn status(settings: &Settings, app_version: &str) -> BridgeStatus {
    // Settings polls this while it is open, which makes it one of the moments
    // a lapsed other-site jar is noticed and let go.
    store::drop_lapsed();
    let link = state::load();
    let host = host_path();
    let other_session = link.session_state(Site::Other);

    BridgeStatus {
        supported: supported(),
        store_listed: protocol::store_listed(),
        enabled: settings.browser_link_enabled,
        // The registry values are what make the browser able to start the host
        // at all; if they are gone or point elsewhere, "install the extension"
        // is the wrong advice and Repair is the right one.
        registered: registered(),
        connected: link.bound.is_some(),
        browser: link.bound.as_ref().map(|b| b.browser.label().to_string()),
        profile_label: link.bound.as_ref().and_then(|b| b.profile_label.clone()),
        account_hint: link.account_hint.clone(),
        extension_version: link.bound.as_ref().map(|b| b.extension_version.clone()),
        last_push_at: link.last_push_at,
        session: link.session_state(Site::Youtube),
        tiktok_session: link.session_state(Site::Tiktok),
        tiktok_last_push_at: link.tiktok.last_push_at,
        // The site is named only while its session is there to be lent.
        other_domain: (other_session == SessionState::Fresh)
            .then(|| link.other.domain.clone())
            .flatten(),
        other_session,
        host_path: host.map(|p| p.to_string_lossy().into_owned()),
        app_version: app_version.to_string(),
        // The published id once there is one, so the interface can name the
        // real listing and a lookalike is recognisable; the development id
        // until then, which is what an unpacked install actually carries.
        extension_id: if protocol::store_listed() {
            protocol::EXTENSION_ID_STORE.to_string()
        } else {
            protocol::EXTENSION_ID_DEV.to_string()
        },
    }
}

/// Forget the browser: delete every site's session and the binding, leaving
/// the registry registration alone so reconnecting is one click rather than a
/// repair.
pub fn disconnect() -> AppResult<()> {
    store::forget_all()?;
    state::unbind()?;
    Ok(())
}

/// Record that the feature was turned off, so the host refuses pushes and
/// stores nothing even while the app is closed -- which is when most pushes
/// arrive. Turning it off also drops every session already held.
pub fn set_enabled(enabled: bool) -> AppResult<()> {
    state::set_enabled(enabled)?;
    if !enabled {
        store::forget_all()?;
    }
    Ok(())
}

/// Where `ud-bridge.exe` is.
///
/// Beside the app in an installed copy; under the Tauri resource directory if a
/// future bundle moves it; and in `target/{debug,release}` for a development
/// run, which is the only way `tauri dev` can register a working host.
pub fn host_path() -> Option<PathBuf> {
    const EXE: &str = if cfg!(windows) { "ud-bridge.exe" } else { "ud-bridge" };

    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;

    let candidates = [
        dir.join(EXE),
        dir.join("resources").join(EXE),
        // `tauri dev` runs the app straight out of the target directory, where
        // cargo has already built every bin in the package.
        dir.join("..").join(EXE),
    ];

    candidates.into_iter().find(|path| path.is_file())
}

/// Whether every supported browser's registry value names our host.
pub fn registered() -> bool {
    #[cfg(windows)]
    {
        registry::is_registered()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Point every supported browser at `ud-bridge.exe`.
///
/// Runs at startup and behind the Repair button. Rewriting on every launch is
/// what heals an entry a browser update, a cleaner or a second copy of the app
/// disturbed -- and it is why the popup displays the host path it actually
/// talked to, since the same mechanism is what an attacker would use.
pub fn register() -> AppResult<()> {
    #[cfg(windows)]
    {
        let Some(host) = host_path() else {
            return Err(crate::error::AppError::Other(
                "the bridge helper is missing from this installation".into(),
            ));
        };
        registry::register(&host)
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

/// Remove the registry values.
///
/// No longer called when the user turns the browser sessions off. The host
/// also carries the links the extension sends to download, which have nothing
/// to do with those sessions, so it stays registered and the toggle only stops
/// the host storing cookies. Kept for a path that really is leaving, such as
/// an uninstall.
pub fn unregister() -> AppResult<()> {
    #[cfg(windows)]
    {
        registry::unregister()
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

/// A support paste: everything needed to diagnose a broken link and nothing
/// that would compromise the user if they post it publicly.
///
/// Cookie *names* are safe and diagnostic -- a jar without `SID` is a signed-out
/// jar -- but values never appear, and neither does the account beyond the mask
/// already shown in the interface.
pub fn diagnostics(settings: &Settings, app_version: &str) -> String {
    let status = status(settings, app_version);
    let link = state::load();
    let mut out = String::new();

    out.push_str("Universal Downloader -- browser link diagnostics\n");
    out.push_str(&format!("app: {app_version}\n"));
    out.push_str(&format!("supported: {}\n", status.supported));
    out.push_str(&format!("enabled: {}\n", status.enabled));
    out.push_str(&format!("store listing: {}\n", status.store_listed));
    out.push_str(&format!(
        "helper: {}\n",
        status.host_path.as_deref().unwrap_or("not found")
    ));
    out.push_str(&format!("registered: {}\n", status.registered));

    #[cfg(windows)]
    for (browser, value) in registry::describe() {
        out.push_str(&format!("  {browser}: {value}\n"));
    }

    out.push_str(&format!(
        "browser: {} ({})\n",
        status.browser.as_deref().unwrap_or("none"),
        status.profile_label.as_deref().unwrap_or("no profile name")
    ));
    out.push_str(&format!(
        "extension: {}\n",
        status.extension_version.as_deref().unwrap_or("none")
    ));
    out.push_str(&format!("session: {:?}\n", status.session));
    out.push_str(&format!(
        "last push: {}\n",
        status
            .last_push_at
            .map(|t| format!("{t} (epoch seconds)"))
            .unwrap_or_else(|| "never".into())
    ));
    out.push_str(&format!("cookie count: {}\n", link.cookie_count));
    out.push_str(&format!("cookie names: {}\n", link.cookie_names.join(", ")));
    // TikTok's jar is described the same way and under its own name, so a
    // paste never leaves the reader guessing which site a count belongs to.
    out.push_str(&format!("tiktok session: {:?}\n", status.tiktok_session));
    out.push_str(&format!(
        "tiktok last push: {}\n",
        status
            .tiktok_last_push_at
            .map(|t| format!("{t} (epoch seconds)"))
            .unwrap_or_else(|| "never".into())
    ));
    out.push_str(&format!(
        "tiktok cookie count: {}\n",
        link.tiktok.cookie_count
    ));
    out.push_str(&format!(
        "tiktok cookie names: {}\n",
        link.tiktok.cookie_names.join(", ")
    ));
    // The other site's by name too: which site it is for is what a reader of
    // the paste would ask first, and it is the site, not the account.
    out.push_str(&format!("other session: {:?}\n", status.other_session));
    out.push_str(&format!(
        "other site: {}\n",
        link.other.domain.as_deref().unwrap_or("none")
    ));
    out.push_str(&format!("other cookie count: {}\n", link.other.cookie_count));
    out.push_str(&format!(
        "other cookie names: {}\n",
        link.other.cookie_names.join(", ")
    ));
    if let Some(error) = &link.last_error {
        out.push_str(&format!("last error: {error}\n"));
    }

    out
}

/// Cookie names that carry a Google or TikTok session, for the log redactor. A
/// line that mentions any of these is a line that may contain the session
/// itself.
///
/// None of TikTok's appears as a bare word in the engine's own TikTok errors,
/// so naming them here masks a leaked value without swallowing the sentence
/// that explains a refusal.
///
/// The other-site session can be any site's, so no list covers it; the last
/// few are the sign-in cookies of the sites it is most likely lent for --
/// Instagram's `sessionid` is TikTok's name too, X's `auth_token` and `ct0`,
/// Facebook's `c_user` and `xs`, Reddit's `reddit_session` -- chosen, like
/// TikTok's, from names that are not words a refusal would be written in.
pub const SENSITIVE_COOKIE_NAMES: &[&str] = &[
    "SID",
    "HSID",
    "SSID",
    "APISID",
    "SAPISID",
    "LOGIN_INFO",
    "__Secure-1PSID",
    "__Secure-3PSID",
    "__Secure-1PSIDTS",
    "__Secure-3PSIDTS",
    "__Secure-1PAPISID",
    "__Secure-3PAPISID",
    "SIDCC",
    "PREF",
    "sessionid",
    "sessionid_ss",
    "sid_tt",
    "sid_guard",
    "uid_tt",
    "uid_tt_ss",
    "sid_ucp_v1",
    "ssid_ucp_v1",
    "cmpl_token",
    "multi_sids",
    "odin_tt",
    "msToken",
    "ds_user_id",
    "auth_token",
    "ct0",
    "c_user",
    "xs",
    "reddit_session",
    "token_v2",
];

/// Whether `path` is one of our lease files, so a log line naming it can be
/// recognised and cut down before it is written.
pub fn is_lease_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("txt")
        && path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("leases")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_900_000_000;

    fn jar(domain: &str) -> Option<OtherJar<'_>> {
        Some(OtherJar {
            domain,
            captured_at: NOW - 60,
        })
    }

    #[test]
    fn the_other_session_is_lent_to_an_address_on_its_site() {
        let instagram = jar("instagram.com");
        for url in [
            "https://www.instagram.com/p/DQ3zR6-DPGm/",
            "https://instagram.com/reel/abc/",
            "http://i.instagram.com/api/v1/media/1/info/",
            "https://WWW.Instagram.com:443/p/x/",
        ] {
            assert!(lends_other(true, instagram, url, None, NOW), "{url}");
        }
    }

    /// The page it was handed over from counts as much as the address: a
    /// page's player stream is often on a host the address is not.
    #[test]
    fn the_other_session_is_lent_by_the_page_a_link_came_from() {
        assert!(lends_other(
            true,
            jar("reddit.com"),
            "https://v.redd.it/abc123/HLSPlaylist.m3u8",
            Some("https://www.reddit.com/r/videos/comments/1/a/"),
            NOW,
        ));
        assert!(lends_other(
            true,
            jar("vimeo.com"),
            "https://player.vimeo.com/video/76979871",
            Some("https://example.org/blog"),
            NOW,
        ));
    }

    #[test]
    fn the_other_session_is_not_lent_anywhere_else() {
        let instagram = jar("instagram.com");
        for (url, page) in [
            ("https://vimeo.com/76979871", None),
            ("https://evilinstagram.com/p/x/", None),
            ("https://instagram.com.evil.test/p/x/", None),
            ("https://scontent.cdninstagram.com/v/a.mp4", Some("https://example.org/")),
            ("https://instagram.com@evil.test/p/x/", None),
            ("ftp://www.instagram.com/p/x/", None),
            ("not an address", Some("also not one")),
        ] {
            assert!(!lends_other(true, instagram, url, page, NOW), "{url} from {page:?}");
        }
    }

    /// YouTube and TikTok have sessions of their own; an other-site jar is
    /// never lent to them, whatever page they were found on.
    #[test]
    fn the_other_session_is_never_lent_to_youtube_or_tiktok() {
        for url in [
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
            "https://youtu.be/jNQXAC9IVRw",
            "https://www.tiktok.com/@someone/video/1",
        ] {
            assert!(
                !lends_other(true, jar("reddit.com"), url, Some("https://www.reddit.com/r/a/"), NOW),
                "{url}"
            );
        }
    }

    #[test]
    fn a_lapsed_or_missing_jar_or_the_switch_off_lends_nothing() {
        let url = "https://www.instagram.com/p/x/";
        assert!(!lends_other(false, jar("instagram.com"), url, None, NOW));
        assert!(!lends_other(true, None, url, None, NOW));
        let lapsed = Some(OtherJar {
            domain: "instagram.com",
            captured_at: NOW - protocol::OTHER_SESSION_TTL_SECS - 1,
        });
        assert!(!lends_other(true, lapsed, url, None, NOW));
        let last_second = Some(OtherJar {
            domain: "instagram.com",
            captured_at: NOW - protocol::OTHER_SESSION_TTL_SECS,
        });
        assert!(lends_other(true, last_second, url, None, NOW));
        // A record naming something no push could have stored lends nothing.
        assert!(!lends_other(true, jar("www.instagram.com"), url, None, NOW));
        assert!(!lends_other(true, jar(""), url, None, NOW));
    }
}
