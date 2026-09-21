//! Teaching each browser where the host is.
//!
//! Native messaging has no discovery: a browser starts a host only when a
//! registry value under the user's own hive names a manifest, and that manifest
//! names both the executable and the extension ids allowed to reach it. Two
//! files and one value are the whole mechanism, which is what makes it safe --
//! nothing listens, nothing is reachable from a web page -- and also what makes
//! it fragile, because anything that can write to `HKCU` can point a browser
//! somewhere else. The app rewrites its values on every launch and shows the
//! path the host actually ran from, so a value that has been taken is visible
//! rather than merely broken.
//!
//! Only `HKEY_CURRENT_USER`, never the machine hive: this is a per-user app
//! that installs without administrator rights, and a machine-wide value would
//! point every account on the computer at one user's copy.

use std::path::{Path, PathBuf};

use serde::Serialize;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::bridge::protocol;
use crate::error::{AppError, AppResult};

/// Where each Chromium family looks, and who each entry actually serves.
///
/// Chrome, Vivaldi and Opera all read Chrome's key on Windows -- neither
/// Vivaldi nor Opera defines a hive of its own there, which is why there is no
/// entry for them. Edge, Brave and plain Chromium each do. Getting one of these
/// wrong is invisible until a user reports that the link works in one browser
/// and not in another, so they are written out rather than derived.
const VENDORS: &[(&str, &str)] = &[
    (
        "Chrome, Vivaldi and Opera",
        r"Software\Google\Chrome\NativeMessagingHosts",
    ),
    ("Edge", r"Software\Microsoft\Edge\NativeMessagingHosts"),
    (
        "Brave",
        r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
    ),
    ("Chromium", r"Software\Chromium\NativeMessagingHosts"),
];

/// The manifest Chrome reads before it will start anything.
#[derive(Serialize)]
struct HostManifest {
    name: &'static str,
    description: &'static str,
    path: String,
    /// Chrome accepts exactly one value here.
    #[serde(rename = "type")]
    kind: &'static str,
    allowed_origins: Vec<String>,
}

fn manifest_path() -> AppResult<PathBuf> {
    Ok(super::dir()?.join("host-manifest.json"))
}

/// Point every supported browser at `host`.
///
/// A failure for one vendor is not a failure overall. These keys live under the
/// user's own hive and are created whether or not the browser is installed, so
/// a single refusal means something specific -- a policy, a locked hive -- and
/// the browsers that did take the value still work. Only a clean sweep of
/// failures is worth telling the user about.
pub fn register(host: &Path) -> AppResult<()> {
    let manifest = HostManifest {
        name: protocol::HOST_NAME,
        description:
            "Lets Universal Downloader use this browser's session for content you are signed in to.",
        path: host.to_string_lossy().into_owned(),
        kind: "stdio",
        allowed_origins: protocol::allowed_origins(),
    };

    // Written through a temporary file like everything else under `bridge/`:
    // this runs on every launch, and a browser that reads the manifest halfway
    // through a rewrite refuses to start the host rather than waiting.
    let path = manifest_path()?;
    let text = serde_json::to_string_pretty(&manifest)?;
    crate::bridge::state::write_atomic(&path, text.as_bytes())?;

    let value = path.to_string_lossy().into_owned();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut failures = Vec::new();

    for (vendor, subkey) in VENDORS {
        let key = format!(r"{subkey}\{}", protocol::HOST_NAME);
        // The default (unnamed) value is where a browser looks; a named one is
        // ignored, silently.
        let written = hkcu
            .create_subkey(&key)
            .and_then(|(key, _)| key.set_value("", &value));
        if let Err(err) = written {
            failures.push(format!("{vendor}: {err}"));
        }
    }

    if failures.len() == VENDORS.len() {
        return Err(AppError::Other(format!(
            "no browser could be pointed at the bridge helper ({})",
            failures.join("; ")
        )));
    }
    if !failures.is_empty() {
        crate::log_warn!(
            "bridge",
            "some browsers were not registered: {}",
            failures.join("; ")
        );
    }

    Ok(())
}

/// Whether every vendor value still names our manifest.
///
/// All of them, not any: the point of this answer is whether the link will work
/// in whichever browser the user happens to open, and one stale hive is exactly
/// the case that presents as "it stopped working" for no visible reason. The
/// manifest itself has to be there too -- a value naming a file that an
/// uninstall took is registered and dead.
pub fn is_registered() -> bool {
    let Ok(expected) = manifest_path() else {
        return false;
    };
    if !expected.is_file() || !manifest_names_our_host() {
        return false;
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    VENDORS.iter().all(|(_, subkey)| {
        let key = format!(r"{subkey}\{}", protocol::HOST_NAME);
        hkcu.open_subkey(&key)
            .and_then(|key| key.get_value::<String, _>(""))
            .map(|value| same_path(&value, &expected))
            .unwrap_or(false)
    })
}

/// Whether the manifest on disk still names the helper this installation ships.
///
/// The file is read rather than assumed, because rewriting the `path` inside it
/// redirects the browser just as effectively as changing the registry value and
/// is quieter: the value still points where we put it, so a check that compared
/// only the value would report a healthy link while the browser started someone
/// else's program.
fn manifest_names_our_host() -> bool {
    let Ok(path) = manifest_path() else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(body) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let Some(named) = body.get("path").and_then(|value| value.as_str()) else {
        return false;
    };

    super::host_path()
        .map(|host| same_path(named, &host))
        .unwrap_or(false)
}

/// Every vendor's current value, for the support paste. A stale or hijacked
/// entry is invisible in the interface and obvious here, which is the whole
/// reason these lines are in the paste at all.
pub fn describe() -> Vec<(String, String)> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    VENDORS
        .iter()
        .map(|(vendor, subkey)| {
            let key = format!(r"{subkey}\{}", protocol::HOST_NAME);
            let value = hkcu
                .open_subkey(&key)
                .and_then(|key| key.get_value::<String, _>(""))
                .unwrap_or_else(|_| "not set".to_string());
            ((*vendor).to_string(), value)
        })
        .collect()
}

/// Remove the values, and the manifest with them, so a browser cannot start the
/// helper at all once the user has turned the link off.
pub fn unregister() -> AppResult<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for (_, subkey) in VENDORS {
        let key = format!(r"{subkey}\{}", protocol::HOST_NAME);
        let _ = hkcu.delete_subkey(&key);
    }

    if let Ok(path) = manifest_path() {
        let _ = std::fs::remove_file(path);
    }

    Ok(())
}

/// Windows paths differ in case and in stray quoting without differing in
/// meaning, and a value written by an older build of this app may well be
/// spelled differently from one written today.
fn same_path(value: &str, expected: &Path) -> bool {
    let value = value.trim().trim_matches('"');
    let expected = expected.to_string_lossy();
    value.eq_ignore_ascii_case(expected.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vendor_key_ends_at_the_host_name() {
        for (_, subkey) in VENDORS {
            assert!(subkey.ends_with("NativeMessagingHosts"), "{subkey}");
            assert!(!subkey.starts_with('\\'), "{subkey}");
        }
    }

    #[test]
    fn a_path_compares_the_same_however_it_is_spelled() {
        let expected = PathBuf::from(r"C:\Users\Someone\AppData\Roaming\UD\host-manifest.json");
        assert!(same_path(
            r"c:\users\someone\appdata\roaming\ud\host-manifest.json",
            &expected
        ));
        assert!(same_path(
            "\"C:\\Users\\Someone\\AppData\\Roaming\\UD\\host-manifest.json\" ",
            &expected
        ));
        assert!(!same_path(
            r"C:\Somewhere\Else\host-manifest.json",
            &expected
        ));
    }

    #[test]
    fn the_manifest_names_what_chrome_reads() {
        let manifest = HostManifest {
            name: protocol::HOST_NAME,
            description: "test",
            path: r"C:\app\ud-bridge.exe".to_string(),
            kind: "stdio",
            allowed_origins: vec!["chrome-extension://abc/".to_string()],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains(r#""type":"stdio""#), "{json}");
        assert!(json.contains(r#""allowed_origins""#), "{json}");
    }
}
