//! The session at rest, and the short-lived plaintext the engine needs.
//!
//! The jar lives in `session.bin`, encrypted with DPAPI for this Windows user
//! and with a per-install random salt kept beside it. That combination is aimed
//! at the threat that actually collects sessions at scale: a copy of the file.
//! In a backup, a support archive, a synced profile folder or another user's
//! hands the blob is inert, and another program running as the same user cannot
//! open it without also taking `entropy.bin`.
//!
//! The engine cannot read any of that, so a download that needs the session
//! gets a `cookies.txt` written for it and deleted after it -- a lease. The
//! plaintext exists for the length of one yt-dlp run, in a directory nothing
//! else sweeps, and the lease's `Drop` is what removes it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::bridge::protocol::{Cookie, Push};
use crate::bridge::{cookies, state};
use crate::error::{AppError, AppResult};

/// What `session.bin` holds once decrypted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionBlob {
    #[serde(default)]
    cookies: Vec<Cookie>,
    #[serde(default)]
    captured_at: i64,
    #[serde(default)]
    signed_in: bool,
}

/// Bytes of salt mixed into DPAPI. Thirty-two because it is generated once and
/// never typed; there is no reason to be frugal with it.
#[cfg(windows)]
const ENTROPY_BYTES: usize = 32;

fn session_path() -> AppResult<PathBuf> {
    Ok(super::dir()?.join("session.bin"))
}

#[cfg(windows)]
fn entropy_path() -> AppResult<PathBuf> {
    Ok(super::dir()?.join("entropy.bin"))
}

fn leases_dir() -> AppResult<PathBuf> {
    let dir = super::dir()?.join("leases");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Whether a jar exists at all, without decrypting it. `session_state` asks
/// this on every status poll, and polling must not touch DPAPI.
pub(super) fn session_exists() -> bool {
    session_path().map(|path| path.is_file()).unwrap_or(false)
}

/// Store what a browser profile pushed.
///
/// A signed-out push deletes the jar instead of storing it. A cookie jar that
/// has outlived its sign-in cannot download anything, so keeping one is all
/// liability and no benefit -- and "the browser signed out" is precisely the
/// moment the user would expect the app to have let go.
pub fn save(push: &Push) -> AppResult<()> {
    if !push.signed_in {
        return forget();
    }

    let blob = SessionBlob {
        cookies: push.cookies.clone(),
        captured_at: push.captured_at,
        signed_in: true,
    };

    save_blob(&blob)?;
    state::record_push(push)
}

/// Delete the stored session, from a sign-out, a Forget or the feature being
/// turned off. The binding is not touched: a profile that signed out is still
/// the connected profile.
pub fn forget() -> AppResult<()> {
    if let Ok(path) = session_path() {
        let _ = std::fs::remove_file(path);
    }
    state::clear_session()
}

/// Write the session out as a `cookies.txt` for one engine run.
///
/// `None` is the ordinary answer, not a failure: no session stored, a session
/// that outlived its sign-in, or a blob this Windows account can no longer
/// decrypt. Every one of those means the download simply runs without cookies.
pub fn lease() -> AppResult<Option<CookieLease>> {
    let Some(blob) = load_blob()? else {
        return Ok(None);
    };
    if !blob.signed_in || blob.cookies.is_empty() {
        return Ok(None);
    }

    let path = leases_dir()?.join(lease_name());
    let mut text = cookies::write(&blob.cookies);
    // Nothing widens the permissions here: everything under %APPDATA% is
    // already readable by this user alone, and the file is gone within a run.
    let written = std::fs::write(&path, text.as_bytes());
    scrub(&mut text);
    // A write that failed part way still leaves cookies on disk, and no lease
    // exists yet whose drop would take them away again.
    if let Err(err) = written {
        let _ = std::fs::remove_file(&path);
        return Err(err.into());
    }

    Ok(Some(CookieLease { path }))
}

/// Remove lease files a crash left behind.
///
/// Run once at startup, where every file in this directory is by definition
/// orphaned: a single-instance app means no other copy is mid-download, and a
/// lease is never resumable state the way a partial download is.
pub fn sweep_leases() -> AppResult<()> {
    let dir = leases_dir()?;
    for entry in std::fs::read_dir(&dir)? {
        let Ok(entry) = entry else { continue };
        if entry.metadata().map(|meta| meta.is_file()).unwrap_or(false) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// A `cookies.txt` that exists for the length of one engine run.
pub struct CookieLease {
    path: PathBuf,
}

impl CookieLease {
    /// The file to hand the engine, as `--cookies` wants it.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Take back what the engine rotated.
    ///
    /// yt-dlp rewrites the cookie file it was given when it exits, with
    /// whatever the server refreshed during the run. Folding that back into the
    /// blob is what keeps a session current through ordinary use, without the
    /// browser having to push again -- and a user who downloads every day then
    /// never sees the link go quiet.
    ///
    /// Failures are logged and swallowed. This runs after a download that has
    /// already succeeded, and losing a rotation costs nothing the next push
    /// will not fix; failing the download would cost the user the file.
    pub fn fold_back(&self) {
        let Ok(mut text) = std::fs::read_to_string(&self.path) else {
            return;
        };
        let rotated = cookies::read(&text);
        scrub(&mut text);
        if rotated.is_empty() {
            return;
        }

        // Only ever an update of a session that is still there. If the jar was
        // forgotten while the download ran -- the user pressed Disconnect, the
        // browser signed out -- then this run's copy is exactly what they asked
        // to be rid of, and writing it back would undo that.
        let Ok(Some(mut blob)) = load_blob() else {
            return;
        };
        blob.cookies = rotated;

        if let Err(err) = save_blob(&blob) {
            crate::log_warn!("bridge", "could not store the refreshed session: {err}");
            return;
        }
        if let Err(err) = state::record_rotation(&blob.cookies) {
            crate::log_warn!("bridge", "could not record the refreshed session: {err}");
        }
    }
}

impl Drop for CookieLease {
    /// The release profile is built with `panic = "abort"` (see `Cargo.toml`),
    /// so this does not run if the process dies -- which is exactly why
    /// `sweep_leases` exists at startup. Drop is the common case; the sweep is
    /// the one that has to be true.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A name no other lease can take: the process, a counter within it, and the
/// clock. Two downloads starting in the same nanosecond in the same process
/// would still differ by the counter, and two processes differ by the pid.
fn lease_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0);

    format!(
        "{}-{}-{nanos}.txt",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn save_blob(blob: &SessionBlob) -> AppResult<()> {
    let mut plain = serde_json::to_vec(blob)?;
    let sealed = protect(&plain);
    scrub_bytes(&mut plain);

    state::write_atomic(&session_path()?, &sealed?)
}

/// Decrypt the stored jar, or explain to the interface why there is none.
///
/// A decrypt failure is not an error the user should be shown as an error. An
/// administrator resetting the Windows password destroys the DPAPI master key,
/// and from then on this blob can never be opened again by anyone, including
/// its owner. The only useful answer is "reconnect your browser", so the
/// unopenable file goes and the reason is left where the interface can say it.
fn load_blob() -> AppResult<Option<SessionBlob>> {
    let path = session_path()?;
    let Ok(sealed) = std::fs::read(&path) else {
        return Ok(None);
    };

    let Some(mut plain) = unprotect(&sealed) else {
        let _ = std::fs::remove_file(&path);
        state::record_error(
            "the stored browser session could not be decrypted on this Windows account -- reconnect the browser",
        );
        let _ = state::clear_session();
        return Ok(None);
    };

    let blob = serde_json::from_slice::<SessionBlob>(&plain).ok();
    scrub_bytes(&mut plain);

    if blob.is_none() {
        let _ = std::fs::remove_file(&path);
        state::record_error("the stored browser session was unreadable -- reconnect the browser");
        let _ = state::clear_session();
    }

    Ok(blob)
}

/// Overwrite a buffer that held cookies before it is freed.
///
/// Hygiene against a crash dump or a page that reaches the swap file, not a
/// defence against anyone debugging this process: the allocator, serde and the
/// operating system have all had copies by now and none of them can be recalled.
fn scrub_bytes(bytes: &mut [u8]) {
    bytes.fill(0);
}

fn scrub(text: &mut str) {
    // Zero bytes are valid UTF-8, so the string stays well formed.
    unsafe { text.as_bytes_mut() }.fill(0);
}

#[cfg(windows)]
fn protect(plain: &[u8]) -> AppResult<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let entropy = entropy_or_create()?;
    let input = blob(plain);
    let salt = blob(&entropy);
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    // UI_FORBIDDEN because this runs inside the bridge host as often as inside
    // the app, and Chrome starts that with no window for a prompt to appear on.
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            &salt,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(AppError::Other(
            "Windows would not encrypt the browser session".into(),
        ));
    }

    let sealed =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe { LocalFree(output.pbData.cast()) };

    Ok(sealed)
}

#[cfg(windows)]
fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    // Reading never creates the salt. If it is gone the blob is unopenable by
    // anyone, and generating a fresh one here would only seal the loss in.
    let entropy = entropy()?;
    let input = blob(sealed);
    let salt = blob(&entropy);
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            &salt,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return None;
    }

    let plain =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe { LocalFree(output.pbData.cast()) };

    Some(plain)
}

#[cfg(windows)]
fn blob(bytes: &[u8]) -> windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB {
    windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        // The API takes this blob by const pointer and does not write through
        // it; the struct simply has no const form.
        pbData: bytes.as_ptr() as *mut u8,
    }
}

#[cfg(windows)]
fn entropy() -> Option<Vec<u8>> {
    let bytes = std::fs::read(entropy_path().ok()?).ok()?;
    (bytes.len() == ENTROPY_BYTES).then_some(bytes)
}

#[cfg(windows)]
fn entropy_or_create() -> AppResult<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };

    if let Some(existing) = entropy() {
        return Ok(existing);
    }

    let mut bytes = vec![0u8; ENTROPY_BYTES];
    // A null algorithm handle with the system-preferred flag is the documented
    // way to ask for bytes without opening a provider first, which matters in
    // the host: it is started, asked one question and killed.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            ENTROPY_BYTES as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(AppError::Other(
            "Windows would not produce random bytes for the browser session".into(),
        ));
    }

    // Two processes can reach a first use at once -- the app registering at
    // startup and a host the browser has just launched. Creating the file
    // exclusively means the loser adopts the winner's bytes rather than
    // renaming its own over a key some session was already sealed with, which
    // would leave that session undecryptable for no reason the user could act
    // on.
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(entropy_path()?)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(&bytes)?;
            file.sync_all()?;
            Ok(bytes)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => entropy().ok_or_else(|| {
            AppError::Other("the key for the stored browser session could not be read".into())
        }),
        Err(err) => Err(err.into()),
    }
}

/// The phone builds this crate too, and there is no browser on it to link to.
/// Everything above still has to compile; none of it has to work.
#[cfg(not(windows))]
fn protect(_plain: &[u8]) -> AppResult<Vec<u8>> {
    Err(AppError::Other(
        "the browser link is a Windows feature".into(),
    ))
}

#[cfg(not(windows))]
fn unprotect(_sealed: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_leases_never_share_a_name() {
        let names: Vec<String> = (0..64).map(|_| lease_name()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
        assert!(names.iter().all(|name| name.ends_with(".txt")));
    }

    #[test]
    fn a_blob_survives_a_round_trip_through_json() {
        let blob = SessionBlob {
            cookies: vec![Cookie {
                domain: ".youtube.com".into(),
                name: "SID".into(),
                value: "value".into(),
                path: "/".into(),
                secure: true,
                http_only: true,
                expiration_date: Some(1_900_000_000.0),
            }],
            captured_at: 1_700_000_000,
            signed_in: true,
        };

        let text = serde_json::to_string(&blob).unwrap();
        let back: SessionBlob = serde_json::from_str(&text).unwrap();
        assert_eq!(back.cookies.len(), 1);
        assert_eq!(back.cookies[0].name, "SID");
        assert!(back.cookies[0].http_only);
        assert_eq!(back.captured_at, 1_700_000_000);
        assert!(back.signed_in);
    }

    /// A blob written by an older build, before a field existed, must still
    /// open: the alternative is a session silently lost on every app update.
    #[test]
    fn a_blob_missing_a_field_still_opens() {
        let back: SessionBlob = serde_json::from_str("{}").unwrap();
        assert!(back.cookies.is_empty());
        assert!(!back.signed_in);
    }
}
