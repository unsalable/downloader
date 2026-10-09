//! The shared, non-secret view of the browser link.
//!
//! Nothing in this file is a secret -- it is what Settings shows and what the
//! extension's popup is told -- so the property that matters here is not
//! confidentiality but survivability. Two processes read and write
//! `state.json`: the app, and the bridge host that Chrome starts while the app
//! is closed. Either may be a different version of this code, either may be
//! killed mid-write, and neither may ever be handed half a file.
//!
//! That is why every field is `serde(default)`, so a field one version writes
//! does not stop the other version parsing anything at all, and why every write
//! goes to a temporary file and is renamed over the real one. Where both
//! processes touch the same field the last writer wins, which is correct: they
//! write different things -- the app the toggle, the host the session -- and a
//! push that lands a millisecond after a toggle *should* be the newer truth.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bridge::protocol::{
    Browser, Cookie, Peer, Push, SessionState, Site, OTHER_SESSION_TTL_SECS, SESSION_TTL_SECS,
};
use crate::bridge::store;
use crate::error::{AppError, AppResult};

/// YouTube's session is described by the top-level fields, which every build
/// since the first reads and writes; every other site's has a record of its
/// own under the site's name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkState {
    #[serde(default = "enabled_default")]
    pub enabled: bool,

    #[serde(default)]
    pub bound: Option<Bound>,

    #[serde(default)]
    pub account_hint: Option<String>,

    #[serde(default)]
    pub last_push_at: Option<i64>,

    #[serde(default)]
    pub cookie_count: usize,

    /// Cookie *names*, and never values. A jar without `SID` is a signed-out
    /// jar, and knowing that is most of diagnosing a link that looks connected
    /// but downloads nothing -- so the names go in the support paste and the
    /// values never leave `session.bin`.
    #[serde(default)]
    pub cookie_names: Vec<String>,

    #[serde(default)]
    pub tiktok: SiteRecord,

    /// The other-site session's, with the site it is for in `domain`. Absent
    /// from every file written before there was one, and dropped by any build
    /// from before it that rewrites the file -- which leaves a jar with no
    /// push time, and so one that reads as lapsed and is deleted.
    #[serde(default)]
    pub other: SiteRecord,

    #[serde(default)]
    pub last_error: Option<String>,
}

/// What `state.json` says about a session other than YouTube's.
///
/// A binary from before this record rewrites the file with load-modify-write
/// (see `update`) and drops the field it does not know. The session then reads
/// as having no push time, which `session_state` treats as stale until the
/// browser next pushes -- the safe reading, and one that heals by itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteRecord {
    #[serde(default)]
    pub last_push_at: Option<i64>,

    #[serde(default)]
    pub cookie_count: usize,

    /// Names only, as for YouTube's jar.
    #[serde(default)]
    pub cookie_names: Vec<String>,

    /// Which site the session is for. Only the other-site record has one --
    /// TikTok's domain is fixed -- and it is left out of the file when absent,
    /// so TikTok's record is written exactly as it was before the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

/// The browser profile currently allowed to push.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bound {
    #[serde(default)]
    pub profile_id: String,
    #[serde(default = "unknown_browser")]
    pub browser: Browser,
    #[serde(default)]
    pub extension_version: String,
    #[serde(default)]
    pub profile_label: Option<String>,
}

impl Default for LinkState {
    fn default() -> Self {
        Self {
            enabled: enabled_default(),
            bound: None,
            account_hint: None,
            last_push_at: None,
            cookie_count: 0,
            cookie_names: Vec::new(),
            tiktok: SiteRecord::default(),
            other: SiteRecord::default(),
            last_error: None,
        }
    }
}

/// Mirrors the default of `Settings::browser_link_enabled`, and has to.
///
/// `state.json` only exists once something has written it, and the app writes
/// it when the toggle *changes* -- so on a fresh install there is no file at
/// all. A default of `false` here would mean the host quietly refused the very
/// first push of every new install while Settings showed the feature as on.
fn enabled_default() -> bool {
    cfg!(windows)
}

fn unknown_browser() -> Browser {
    Browser::Unknown
}

fn path() -> AppResult<PathBuf> {
    Ok(super::dir()?.join("state.json"))
}

/// Never fails. A missing file is a link that has not been set up, a torn or
/// unreadable one is a link whose record was lost, and both are the same thing
/// to every caller: defaults, and a reconnect puts it right.
pub fn load() -> LinkState {
    let Ok(path) = path() else {
        return LinkState::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return LinkState::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Read, change, write. The read is deliberately inside: the other process may
/// have written since this one last looked, and changing a field means changing
/// that field of whatever is on disk now, not of a stale copy.
fn update(change: impl FnOnce(&mut LinkState)) -> AppResult<()> {
    let mut state = load();
    change(&mut state);
    let text = serde_json::to_string_pretty(&state)?;
    write_atomic(&path()?, text.as_bytes())
}

pub fn set_enabled(enabled: bool) -> AppResult<()> {
    update(|state| state.enabled = enabled)
}

/// Forget which profile is connected, leaving the toggle alone.
pub fn unbind() -> AppResult<()> {
    update(|state| {
        state.bound = None;
        state.account_hint = None;
        state.last_error = None;
    })
}

/// Hand the binding to `peer`. Only ever reached through a deliberate Claim;
/// see the host for why a push must not do this.
pub(super) fn bind(peer: &Peer) -> AppResult<()> {
    update(|state| state.bound = Some(bound_from(peer)))
}

/// Record a stored push of `site`'s session: how many cookies were kept, which
/// names, and when.
pub(super) fn record_push(push: &Push, site: Site, kept: &[Cookie]) -> AppResult<()> {
    update(|state| apply_push(state, push, site, kept))
}

/// What recording a push changes, apart from the file it is written to.
///
/// Recording a push does not bind. Binding is `claim`, and only `claim`,
/// because a push that could bind would make "Turn off" last exactly until the
/// next cookie change: Forget clears the binding and the push arriving seconds
/// later would quietly re-create it.
fn apply_push(state: &mut LinkState, push: &Push, site: Site, kept: &[Cookie]) {
    match site {
        Site::Youtube => {
            state.account_hint = push.account_hint.clone();
            state.last_push_at = Some(push.captured_at);
            state.cookie_count = kept.len();
            state.cookie_names = names_of(kept);
        }
        // The account hint stays YouTube's: it is the one Settings shows, and a
        // TikTok push carrying none must not wipe it.
        Site::Tiktok => {
            state.tiktok = SiteRecord {
                last_push_at: Some(push.captured_at),
                cookie_count: kept.len(),
                cookie_names: names_of(kept),
                domain: None,
            }
        }
        // The capture time is the extension's clock, and it decides when the
        // jar lapses; one from the future is taken as now, so a wrong clock
        // can shorten an hour but never stretch it.
        Site::Other => {
            state.other = SiteRecord {
                last_push_at: Some(push.captured_at.min(chrono::Utc::now().timestamp())),
                cookie_count: kept.len(),
                cookie_names: names_of(kept),
                domain: push.domain.clone(),
            }
        }
    }

    refresh_bound(state, &push.peer);
    state.last_error = None;
}

/// Record a push that left `site` with nothing to lend -- the profile signed
/// out, or nothing of the site's was in it. The session's record goes, as
/// `clear_session` drops it, and the push still says which extension the bound
/// profile runs.
pub(super) fn record_signed_out(push: &Push, site: Site) -> AppResult<()> {
    update(|state| {
        clear(state, site);
        refresh_bound(state, &push.peer);
    })
}

/// Bring the binding up to date with what the bound profile says about itself.
///
/// The bound profile's extension updates itself, and only a claim used to
/// record which version it was -- so Settings went on naming the version the
/// browser was connected with, long after the browser had moved on. A push is
/// only ever accepted from the bound profile, so what it says is the truth.
fn refresh_bound(state: &mut LinkState, peer: &Peer) {
    let Some(bound) = state
        .bound
        .as_mut()
        .filter(|bound| bound.profile_id == peer.profile_id)
    else {
        return;
    };
    bound.extension_version = peer.extension_version.clone();
    // An extension that could not tell which browser it runs in says nothing
    // new, and the name it was bound with is the better one.
    if peer.browser != Browser::Unknown {
        bound.browser = peer.browser;
    }
}

/// Record the jar the engine rotated during a run.
///
/// Deliberately does not touch the push time: that field means "when the
/// browser last spoke to us", and it is what decides whether the session has
/// gone stale. A download refreshing cookies says nothing about whether the
/// browser is still running with the extension enabled, and bumping it here
/// would let a session that no browser has touched for a month look fresh
/// forever.
pub(super) fn record_rotation(site: Site, cookies: &[Cookie]) -> AppResult<()> {
    update(|state| match site {
        Site::Youtube => {
            state.cookie_count = cookies.len();
            state.cookie_names = names_of(cookies);
        }
        Site::Tiktok => {
            state.tiktok.cookie_count = cookies.len();
            state.tiktok.cookie_names = names_of(cookies);
        }
        Site::Other => {
            state.other.cookie_count = cookies.len();
            state.other.cookie_names = names_of(cookies);
        }
    })
}

/// Drop everything that describes `site`'s stored session, keeping the
/// binding: a profile that signed out is still the connected profile.
pub(super) fn clear_session(site: Site) -> AppResult<()> {
    update(|state| clear(state, site))
}

fn clear(state: &mut LinkState, site: Site) {
    match site {
        Site::Youtube => {
            state.account_hint = None;
            state.last_push_at = None;
            state.cookie_count = 0;
            state.cookie_names = Vec::new();
        }
        Site::Tiktok => state.tiktok = SiteRecord::default(),
        Site::Other => state.other = SiteRecord::default(),
    }
}

/// Leave a note for the interface and the support paste. Best effort by
/// design: this runs on paths that are already reporting a failure, and a
/// second failure there must not replace the first.
pub(super) fn record_error(message: &str) {
    let _ = update(|state| state.last_error = Some(message.to_string()));
}

/// Write via a temporary file, because the other process may be reading.
///
/// The temporary name carries this process's id: the app and the host can both
/// be saving at the same moment, and a shared scratch name would have one
/// renaming away the other's half-written bytes.
pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "state".to_string());
    let temp = path.with_file_name(format!("{name}.{}.tmp", std::process::id()));

    std::fs::write(&temp, bytes)?;

    // A rename fails on Windows while another process holds the destination
    // open, which is exactly what the other process reading this file looks
    // like. Those reads last microseconds, so a few short retries turn a
    // collision into a pause rather than into a lost push.
    let mut last = None;
    for attempt in 1..=5 {
        match std::fs::rename(&temp, path) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last = Some(err);
                std::thread::sleep(std::time::Duration::from_millis(10 * attempt));
            }
        }
    }

    let _ = std::fs::remove_file(&temp);
    Err(last
        .map(AppError::from)
        .unwrap_or_else(|| AppError::Other("could not save the browser link state".into())))
}

fn bound_from(peer: &Peer) -> Bound {
    Bound {
        profile_id: peer.profile_id.clone(),
        browser: peer.browser,
        extension_version: peer.extension_version.clone(),
        profile_label: peer.profile_label.clone(),
    }
}

fn names_of(cookies: &[Cookie]) -> Vec<String> {
    cookies.iter().map(|cookie| cookie.name.clone()).collect()
}

/// Whether an other-site session captured at `captured_at` is still inside
/// its hour at `now`. One with no capture time recorded is not: the record was
/// lost, and a jar nobody can date is a jar to delete.
pub(super) fn other_fresh(captured_at: Option<i64>, now: i64) -> bool {
    captured_at.is_some_and(|at| now - at <= OTHER_SESSION_TTL_SECS)
}

impl LinkState {
    /// How much `site`'s stored session is worth right now.
    ///
    /// The blob on disk is the authority for whether a session exists at all;
    /// `state.json` only describes it. They are written a moment apart by the
    /// same call, and if a crash lands between them it is the blob that decides
    /// -- a jar the app cannot see may as well not exist, and a record with no
    /// jar behind it would have the interface promising a download that then
    /// fails with nothing to explain it.
    pub fn session_state(&self, site: Site) -> SessionState {
        if !store::session_exists(site) {
            return SessionState::None;
        }
        self.session_state_at(site, chrono::Utc::now().timestamp())
    }

    /// `session_state` for a jar that exists, at `now`: the part of it that
    /// touches no file, which is what the tests can ask about.
    ///
    /// The other-site session has no stale state. It is lent for the download
    /// İndir started and lapses an hour after it was captured; past that it
    /// is no session at all, and the next sweep (`drop_lapsed`) deletes it.
    pub(super) fn session_state_at(&self, site: Site, now: i64) -> SessionState {
        let last_push_at = match site {
            Site::Youtube => self.last_push_at,
            Site::Tiktok => self.tiktok.last_push_at,
            Site::Other => {
                return if other_fresh(self.other.last_push_at, now) {
                    SessionState::Fresh
                } else {
                    SessionState::None
                };
            }
        };

        // No recorded push time means the record was lost while the jar
        // survived -- an older build rewriting this file drops TikTok's, for
        // one. Treating that as ancient rather than as fresh is the
        // conservative reading, and the next push from the browser heals it.
        let age = now - last_push_at.unwrap_or(0);
        if age > SESSION_TTL_SECS {
            SessionState::Stale
        } else {
            SessionState::Fresh
        }
    }

    pub fn is_bound_to(&self, profile_id: &str) -> bool {
        self.bound
            .as_ref()
            .is_some_and(|bound| bound.profile_id == profile_id)
    }

    /// The bridge host's way in.
    ///
    /// `ud-bridge.exe` is a separate program that links this crate as a
    /// library, so the private modules behind `bridge` are out of its reach and
    /// these four entry points are the whole of what it can do. That is a
    /// useful shape rather than an accident: the host is the one process a
    /// browser can start, and the list of things it may change is short enough
    /// to read in one go.
    pub fn read() -> Self {
        load()
    }

    /// Store a pushed session as `site`'s and record it.
    ///
    /// Whether this peer is *allowed* to push is the caller's decision, not
    /// this function's: the host answers an unpaired profile with an error
    /// rather than with a silent refusal, and only it knows how to say so.
    /// Which site the push is for is the caller's to have checked too.
    pub fn accept_push(push: &Push, site: Site) -> AppResult<()> {
        store::save(push, site)
    }

    /// Move the binding to `peer`, which is what the Connect button does --
    /// and what either of the extension's session switches does when another
    /// profile holds the binding.
    ///
    /// Taking the binding from another profile deletes every jar that profile
    /// pushed, not only the one for the site whose switch was pressed: the
    /// binding is one, and so is the answer to whose sessions these are.
    pub fn claim(peer: &Peer) -> AppResult<()> {
        // A stored jar belongs to the profile that pushed it. Carrying it over
        // to a profile that just took the binding would present one account's
        // session as another's, which is how a work profile ends up quietly
        // downloading with a personal membership.
        if !load().is_bound_to(&peer.profile_id) {
            store::forget_all()?;
        }
        bind(peer)
    }

    /// Delete every session and the binding, from either side of the link.
    pub fn forget() -> AppResult<()> {
        store::forget_all()?;
        unbind()
    }

    /// Delete the other-site session once its hour is up.
    ///
    /// The engine never leases a lapsed one, so this is not what keeps it
    /// from being used; it is what keeps it from being kept. Swept by the host
    /// on every request it handles, and by the app as it starts and whenever
    /// Settings reads the link -- the moments something is running to do it.
    pub fn drop_lapsed() {
        store::drop_lapsed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> Peer {
        Peer {
            v: 1,
            profile_id: "profile-a".to_string(),
            browser: Browser::Chrome,
            extension_version: "1.0.0".to_string(),
            profile_label: Some("Work".to_string()),
        }
    }

    #[test]
    fn a_missing_file_leaves_the_feature_on() {
        let state: LinkState = serde_json::from_str("{}").unwrap();
        assert_eq!(state.enabled, cfg!(windows));
        assert!(state.bound.is_none());
        assert!(state.cookie_names.is_empty());
    }

    #[test]
    fn a_field_written_by_a_newer_version_does_not_break_this_one() {
        let state: LinkState =
            serde_json::from_str(r#"{"enabled":true,"somethingAddedLater":{"a":1}}"#).unwrap();
        assert!(state.enabled);
    }

    #[test]
    fn a_half_written_file_is_read_as_defaults_rather_than_as_an_error() {
        assert!(serde_json::from_str::<LinkState>(r#"{"enabled":tr"#).is_err());
        let state: LinkState = serde_json::from_str(r#"{"enabled":tr"#).unwrap_or_default();
        assert_eq!(state.enabled, cfg!(windows));
    }

    #[test]
    fn the_shape_on_disk_is_camel_case() {
        let text = serde_json::to_string(&LinkState::default()).unwrap();
        assert!(text.contains(r#""lastPushAt""#), "{text}");
        assert!(text.contains(r#""cookieNames""#), "{text}");
        assert!(text.contains(r#""accountHint""#), "{text}");
    }

    /// The host refuses a push unless the profile sending it holds the
    /// binding, and nothing holds the binding until a Claim. Without that,
    /// turning the link off would last only until the next cookie change:
    /// Forget clears the binding, and a push that could bind would quietly
    /// re-create it seconds later.
    #[test]
    fn nothing_is_bound_until_a_profile_claims() {
        assert!(!LinkState::default().is_bound_to(&peer().profile_id));

        let claimed = LinkState {
            bound: Some(bound_from(&peer())),
            ..LinkState::default()
        };
        assert!(claimed.is_bound_to(&peer().profile_id));
    }

    #[test]
    fn a_binding_belongs_to_one_profile_only() {
        let state = LinkState {
            bound: Some(bound_from(&peer())),
            ..LinkState::default()
        };
        assert!(state.is_bound_to("profile-a"));
        assert!(!state.is_bound_to("profile-b"));
        assert!(!LinkState::default().is_bound_to("profile-a"));
    }

    #[test]
    fn only_names_are_recorded_never_values() {
        let cookies = vec![Cookie {
            domain: ".youtube.com".to_string(),
            name: "SID".to_string(),
            value: "the-actual-session".to_string(),
            path: "/".to_string(),
            secure: true,
            http_only: true,
            expiration_date: None,
        }];
        let names = names_of(&cookies);
        assert_eq!(names, vec!["SID".to_string()]);

        let state = LinkState {
            cookie_names: names,
            ..LinkState::default()
        };
        let text = serde_json::to_string(&state).unwrap();
        assert!(!text.contains("the-actual-session"), "{text}");
    }

    fn push(profile_id: &str, version: &str, site: Site) -> Push {
        Push {
            peer: Peer {
                profile_id: profile_id.to_string(),
                extension_version: version.to_string(),
                ..peer()
            },
            site: Some(site),
            domain: (site == Site::Other).then(|| "instagram.com".to_string()),
            signed_in: true,
            account_hint: None,
            captured_at: 1_900_000_000,
            cookies: vec![Cookie {
                domain: ".tiktok.com".to_string(),
                name: "sessionid".to_string(),
                value: "the-actual-tiktok-session".to_string(),
                path: "/".to_string(),
                secure: true,
                http_only: true,
                expiration_date: None,
            }],
        }
    }

    /// Every state file written before TikTok's record existed has to open,
    /// as one with no TikTok session in it.
    #[test]
    fn a_state_file_from_before_tiktok_still_opens() {
        let state: LinkState = serde_json::from_str(r#"{"enabled":true,"lastPushAt":5}"#).unwrap();
        assert_eq!(state.last_push_at, Some(5));
        assert_eq!(state.tiktok, SiteRecord::default());
    }

    #[test]
    fn a_tiktok_push_is_recorded_apart_from_youtubes_and_never_its_values() {
        let mut state = LinkState {
            account_hint: Some("m•••@gmail.com".to_string()),
            last_push_at: Some(7),
            ..LinkState::default()
        };
        let pushed = push("profile-a", "1.0.5", Site::Tiktok);
        apply_push(&mut state, &pushed, Site::Tiktok, &pushed.cookies);

        assert_eq!(state.last_push_at, Some(7));
        assert_eq!(state.account_hint.as_deref(), Some("m•••@gmail.com"));
        assert_eq!(state.tiktok.last_push_at, Some(1_900_000_000));
        assert_eq!(state.tiktok.cookie_count, 1);

        let text = serde_json::to_string(&state).unwrap();
        assert!(
            text.contains(
                r#""tiktok":{"lastPushAt":1900000000,"cookieCount":1,"cookieNames":["sessionid"]}"#
            ),
            "{text}"
        );
        assert!(!text.contains("the-actual-tiktok-session"), "{text}");
    }

    /// Every state file written before the other-site record existed opens,
    /// as one with no other-site session -- and TikTok's record is written
    /// exactly as it was, with no `domain` for an older build to trip on.
    #[test]
    fn a_state_file_from_before_other_sites_still_opens() {
        let state: LinkState = serde_json::from_str(
            r#"{"enabled":true,"tiktok":{"lastPushAt":9,"cookieCount":1,"cookieNames":["sessionid"]}}"#,
        )
        .unwrap();
        assert_eq!(state.tiktok.last_push_at, Some(9));
        assert_eq!(state.other, SiteRecord::default());
        assert_eq!(state.session_state_at(Site::Other, 9), SessionState::None);

        let text = serde_json::to_string(&state).unwrap();
        assert!(!text.contains("domain"), "{text}");
    }

    #[test]
    fn an_other_push_is_recorded_with_its_site_and_never_its_values() {
        let mut state = LinkState {
            last_push_at: Some(7),
            ..LinkState::default()
        };
        let mut pushed = push("profile-a", "1.0.5", Site::Other);
        pushed.cookies[0].domain = ".instagram.com".into();
        pushed.captured_at = 1_700_000_000;
        apply_push(&mut state, &pushed, Site::Other, &pushed.cookies);

        assert_eq!(state.last_push_at, Some(7));
        assert_eq!(state.tiktok, SiteRecord::default());
        assert_eq!(state.other.last_push_at, Some(1_700_000_000));
        assert_eq!(state.other.domain.as_deref(), Some("instagram.com"));
        assert_eq!(state.other.cookie_names, ["sessionid"]);

        let text = serde_json::to_string(&state).unwrap();
        assert!(text.contains(r#""domain":"instagram.com""#), "{text}");
        assert!(!text.contains("the-actual-tiktok-session"), "{text}");

        // Fresh for its hour, then no session at all -- never stale.
        assert_eq!(state.session_state_at(Site::Other, 1_700_000_000 + 3_600), SessionState::Fresh);
        assert_eq!(state.session_state_at(Site::Other, 1_700_000_000 + 3_601), SessionState::None);

        clear(&mut state, Site::Other);
        assert_eq!(state.other, SiteRecord::default());
    }

    /// The extension's clock decides when the jar lapses, and a clock that
    /// runs ahead may shorten the hour but never stretch it.
    #[test]
    fn an_other_push_from_the_future_is_dated_now() {
        let mut state = LinkState::default();
        let mut pushed = push("profile-a", "1.0.5", Site::Other);
        pushed.captured_at = i64::MAX / 2;
        apply_push(&mut state, &pushed, Site::Other, &pushed.cookies);
        let recorded = state.other.last_push_at.unwrap();
        assert!(recorded <= chrono::Utc::now().timestamp());
    }

    /// Chrome and Firefox update the extension on their own, and Settings
    /// names the version it reports; only a claim used to record it.
    #[test]
    fn a_push_refreshes_the_bound_extension_version() {
        let mut state = LinkState {
            bound: Some(bound_from(&Peer {
                extension_version: "1.0.4".to_string(),
                ..peer()
            })),
            ..LinkState::default()
        };

        let ours = push("profile-a", "1.0.5", Site::Youtube);
        apply_push(&mut state, &ours, Site::Youtube, &ours.cookies);
        assert_eq!(state.bound.as_ref().unwrap().extension_version, "1.0.5");

        let theirs = push("profile-b", "2.0.0", Site::Tiktok);
        apply_push(&mut state, &theirs, Site::Tiktok, &theirs.cookies);
        assert_eq!(state.bound.as_ref().unwrap().extension_version, "1.0.5");
        assert_eq!(state.bound.as_ref().unwrap().profile_id, "profile-a");

        // A push that signs a site out says the same about the extension.
        let signed_out = push("profile-a", "1.0.6", Site::Tiktok);
        clear(&mut state, Site::Tiktok);
        refresh_bound(&mut state, &signed_out.peer);
        assert_eq!(state.bound.as_ref().unwrap().extension_version, "1.0.6");
    }
}
