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

use crate::bridge::protocol::{Browser, Cookie, Peer, Push, SessionState, SESSION_TTL_SECS};
use crate::bridge::store;
use crate::error::{AppError, AppResult};

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
    pub last_error: Option<String>,
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

/// Record a stored push: how many cookies, which names, and when.
///
/// This also binds when nothing is bound yet, which is what makes the first
/// push after installing the extension connect without a second step.
pub(super) fn record_push(push: &Push) -> AppResult<()> {
    update(|state| {
        // Recording a push does not bind. Binding is `claim`, and only
        // `claim`, because a push that could bind would make "Turn off" last
        // exactly until the next cookie change: Forget clears the binding and
        // the push arriving seconds later would quietly re-create it.
        state.account_hint = push.account_hint.clone();
        state.last_push_at = Some(push.captured_at);
        state.cookie_count = push.cookies.len();
        state.cookie_names = names_of(&push.cookies);
        state.last_error = None;
    })
}

/// Record the jar the engine rotated during a run.
///
/// Deliberately does not touch `last_push_at`: that field means "when the
/// browser last spoke to us", and it is what decides whether the session has
/// gone stale. A download refreshing cookies says nothing about whether the
/// browser is still running with the extension enabled, and bumping it here
/// would let a session that no browser has touched for a month look fresh
/// forever.
pub(super) fn record_rotation(cookies: &[Cookie]) -> AppResult<()> {
    update(|state| {
        state.cookie_count = cookies.len();
        state.cookie_names = names_of(cookies);
    })
}

/// Drop everything that describes a stored session, keeping the binding: a
/// profile that signed out is still the connected profile.
pub(super) fn clear_session() -> AppResult<()> {
    update(|state| {
        state.account_hint = None;
        state.last_push_at = None;
        state.cookie_count = 0;
        state.cookie_names = Vec::new();
    })
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

impl LinkState {
    /// How much a stored session is worth right now.
    ///
    /// The blob on disk is the authority for whether a session exists at all;
    /// `state.json` only describes it. They are written a moment apart by the
    /// same call, and if a crash lands between them it is the blob that decides
    /// -- a jar the app cannot see may as well not exist, and a record with no
    /// jar behind it would have the interface promising a download that then
    /// fails with nothing to explain it.
    pub fn session_state(&self) -> SessionState {
        if !store::session_exists() {
            return SessionState::None;
        }

        // No recorded push time means the record was lost while the jar
        // survived. Treating that as ancient rather than as fresh is the
        // conservative reading, and a reconnect costs the user one click.
        let age = chrono::Utc::now().timestamp() - self.last_push_at.unwrap_or(0);
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

    /// Store a pushed session and record it.
    ///
    /// Whether this peer is *allowed* to push is the caller's decision, not
    /// this function's: the host answers an unpaired profile with an error
    /// rather than with a silent refusal, and only it knows how to say so.
    pub fn accept_push(push: &Push) -> AppResult<()> {
        store::save(push)
    }

    /// Move the binding to `peer`, which is what the Connect button does.
    pub fn claim(peer: &Peer) -> AppResult<()> {
        // A stored jar belongs to the profile that pushed it. Carrying it over
        // to a profile that just took the binding would present one account's
        // session as another's, which is how a work profile ends up quietly
        // downloading with a personal membership.
        if !load().is_bound_to(&peer.profile_id) {
            store::forget()?;
        }
        bind(peer)
    }

    /// Delete the session and the binding, from either side of the link.
    pub fn forget() -> AppResult<()> {
        store::forget()?;
        unbind()
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
}
