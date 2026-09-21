//! The browser link: letting a signed-in browser lend the app its YouTube
//! session, so content the user already pays for can be downloaded.
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
//! * `session.bin` -- the cookie jar, encrypted with DPAPI for this Windows
//!   user. A copy of this file on another machine, in a backup or in a support
//!   archive is inert, which is the threat that actually collects sessions at
//!   scale.
//! * `entropy.bin` -- per-install random bytes mixed into that encryption, so
//!   the blob cannot be decrypted by another app running as the same user
//!   without also stealing this file.
//! * `state.json` -- what the interface and the popup show: which profile is
//!   bound, how old the session is, whether the feature is on. No cookie
//!   material, because the host answers the popup's questions from it and the
//!   popup lives in the browser.
//! * `leases/` -- plaintext `cookies.txt` handed to yt-dlp, one per run,
//!   deleted when the run ends. Deliberately not `temp_dir()`: the
//!   `sweep_temp_files` command clears that on demand and would pull the jar
//!   out from under a live download.
//!
//! Both processes write `state.json`, so every write is a write-then-rename and
//! every read tolerates a torn or missing file by falling back to defaults.

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

pub use protocol::{Browser, HostStatus, SessionState, HOST_NAME};
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

/// Hosts whose media is worth spending the session on.
///
/// The link exists for one thing -- content behind a YouTube membership -- and
/// a cookie jar that is never written to disk is a cookie jar that cannot leak.
/// Everything else the app downloads runs exactly as it did before.
pub fn wants_cookies(url: &str) -> bool {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_ascii_lowercase();

    matches!(
        host.as_str(),
        "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be" | "youtube-nocookie.com"
    )
}

/// Whether a download should bother asking for a lease: the feature is on, the
/// platform supports it, and a session exists that has not gone stale.
pub fn available(settings: &Settings) -> bool {
    supported() && settings.browser_link_enabled && state::load().session_state() == SessionState::Fresh
}

/// Materialise the stored session as a `cookies.txt` for one engine run.
///
/// Returns `None` when there is nothing usable, which is the common case and
/// never an error. The file is deleted when the lease is dropped; see
/// `CookieLease::fold_back` for what happens to the copy yt-dlp rewrites.
pub fn lease(settings: &Settings) -> Option<CookieLease> {
    if !available(settings) {
        return None;
    }
    match store::lease() {
        Ok(lease) => lease,
        Err(err) => {
            crate::log_warn!("bridge", "could not prepare the browser session: {err}");
            None
        }
    }
}

/// Remove lease files a crash left behind. Run once at startup; a lease that
/// outlives its process is never resumable state, unlike a partial download.
pub fn sweep_leases() {
    if let Err(err) = store::sweep_leases() {
        crate::log_warn!("bridge", "lease sweep failed: {err}");
    }
}

/// What Settings shows. Cheap enough to poll: it reads one small JSON file and
/// stats another, and never decrypts the session.
pub fn status(settings: &Settings, app_version: &str) -> BridgeStatus {
    let link = state::load();
    let host = host_path();

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
        session: link.session_state(),
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

/// Forget the browser: delete the session and the binding, leaving the registry
/// registration alone so reconnecting is one click rather than a repair.
pub fn disconnect() -> AppResult<()> {
    store::forget()?;
    state::unbind()?;
    Ok(())
}

/// Record that the feature was turned off, so the host refuses pushes and
/// stores nothing even while the app is closed -- which is when most pushes
/// arrive. Turning it off also drops the session already held.
pub fn set_enabled(enabled: bool) -> AppResult<()> {
    state::set_enabled(enabled)?;
    if !enabled {
        store::forget()?;
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

/// Remove the registry values. Called when the user turns the link off, so a
/// disabled feature leaves nothing behind that a browser could still start.
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
    if let Some(error) = &link.last_error {
        out.push_str(&format!("last error: {error}\n"));
    }

    out
}

/// Cookie names that carry a Google session, for the log redactor. A line that
/// mentions any of these is a line that may contain the session itself.
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
