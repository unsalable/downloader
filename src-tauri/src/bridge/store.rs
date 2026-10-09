//! The sessions at rest, and the short-lived plaintext the engine needs.
//!
//! Each site's jar lives in a file of its own -- YouTube's in `session.bin`,
//! TikTok's in `session-tiktok.bin`, and the one other site İndir last lent a
//! sign-in for in `session-other.bin` -- encrypted with DPAPI for this Windows
//! user and with one per-install random salt kept beside them. That
//! combination is aimed at the threat that actually collects sessions at
//! scale: a copy of the file. In a backup, a support archive, a synced profile
//! folder or another user's hands the blob is inert, and another program
//! running as the same user cannot open it without also taking `entropy.bin`.
//!
//! The engine cannot read any of that, so a download that needs the session
//! gets a `cookies.txt` written for it and deleted after it -- a lease. The
//! plaintext exists for the length of one yt-dlp run, in a directory nothing
//! else sweeps, and the lease's `Drop` is what removes it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::bridge::protocol::{other_domain_ok, under_domain, Cookie, Push, Site};
use crate::bridge::{cookies, state};
use crate::error::{AppError, AppResult};

/// What a session file holds once decrypted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionBlob {
    #[serde(default)]
    cookies: Vec<Cookie>,
    #[serde(default)]
    captured_at: i64,
    #[serde(default)]
    signed_in: bool,
    /// The site an other-site jar is for. The jar's own copy is the one a
    /// rotation goes by, so the cookies and the name they are kept under can
    /// never come from two different pushes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
}

/// Bytes of salt mixed into DPAPI. Thirty-two because it is generated once and
/// never typed; there is no reason to be frugal with it.
#[cfg(windows)]
const ENTROPY_BYTES: usize = 32;

/// Where `site`'s jar is kept. YouTube's keeps the name every earlier build
/// reads, so updating the app never loses a session that was already lent.
fn file_name(site: Site) -> &'static str {
    match site {
        Site::Youtube => "session.bin",
        Site::Tiktok => "session-tiktok.bin",
        Site::Other => "session-other.bin",
    }
}

fn session_path(site: Site) -> AppResult<PathBuf> {
    Ok(super::dir()?.join(file_name(site)))
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

/// Whether `site`'s jar exists at all, without decrypting it.
/// `session_state` asks this on every status poll, and polling must not touch
/// DPAPI.
pub(super) fn session_exists(site: Site) -> bool {
    session_path(site)
        .map(|path| path.is_file())
        .unwrap_or(false)
}

/// Whether a cookie belongs in `site`'s jar. `jar_domain` is the site an
/// other-site jar is for, and means nothing for the others.
///
/// YouTube's is kept as pushed: the extension reads youtube.com alone, and
/// narrowing it here would only risk a cookie Google moves. TikTok's keeps
/// tiktok.com and its subdomains. That is also what drops the cookies yt-dlp
/// writes back for the hosts TikTok streams from, which would otherwise be
/// folded into the jar and handed out with it on every later run. The other
/// site's keeps its own domain and subdomains, for both reasons: the extension
/// reads that domain alone, and a page's CDN is someone else's.
pub(super) fn belongs(site: Site, jar_domain: Option<&str>, domain: &str) -> bool {
    match site {
        Site::Youtube => true,
        Site::Tiktok => under_domain(domain, "tiktok.com"),
        Site::Other => jar_domain.is_some_and(|jar| under_domain(domain, jar)),
    }
}

/// The site an `other` push is for, if it names one this side can store.
/// The host has checked already; this is the store not taking its word.
fn other_domain_of(push: &Push) -> Option<String> {
    push.domain.clone().filter(|domain| other_domain_ok(domain))
}

/// The cookies of a push that go into `site`'s jar: none from a signed-out
/// push, and from a signed-in one only those that belong to the site.
fn kept_cookies(push: &Push, site: Site, jar_domain: Option<&str>) -> Vec<Cookie> {
    if !push.signed_in {
        return Vec::new();
    }
    push.cookies
        .iter()
        .filter(|cookie| belongs(site, jar_domain, &cookie.domain))
        .cloned()
        .collect()
}

/// Store what a browser profile pushed for `site`.
///
/// A signed-out push deletes that site's jar instead of storing it. A cookie
/// jar that has outlived its sign-in cannot download anything, so keeping one
/// is all liability and no benefit -- and "the browser signed out" is
/// precisely the moment the user would expect the app to have let go. A push
/// with nothing in it that belongs to the site is the same: nothing to lend.
///
/// An other-site push replaces whatever other-site jar was held, whichever
/// site that was for: there is one slot, and it holds the site of the last
/// press of İndir.
pub fn save(push: &Push, site: Site) -> AppResult<()> {
    let domain = if site == Site::Other {
        other_domain_of(push)
    } else {
        None
    };
    let kept = kept_cookies(push, site, domain.as_deref());
    if kept.is_empty() {
        remove_jar(site);
        return state::record_signed_out(push, site);
    }

    let blob = SessionBlob {
        cookies: kept,
        // As `state::apply_push` records it: never later than now, so a clock
        // that runs ahead cannot keep an other-site jar past its hour.
        captured_at: push.captured_at.min(chrono::Utc::now().timestamp()),
        signed_in: true,
        domain,
    };

    save_blob(site, &blob)?;
    state::record_push(push, site, &blob.cookies)
}

/// Delete `site`'s stored session, and what `state.json` says about it. The
/// binding is not touched: a profile that signed out is still the connected
/// profile.
pub fn forget(site: Site) -> AppResult<()> {
    remove_jar(site);
    state::clear_session(site)
}

fn remove_jar(site: Site) {
    if let Ok(path) = session_path(site) {
        let _ = std::fs::remove_file(path);
    }
}

/// Delete every site's stored session, from a Forget, a Disconnect, a claim by
/// another profile or the feature being turned off. Each of those is about the
/// link as a whole, and a jar left behind for one site would outlive the
/// decision to let go of it -- which is why one site's record failing to save
/// does not stop the next site's jar being deleted.
pub fn forget_all() -> AppResult<()> {
    let mut outcome = Ok(());
    for site in Site::ALL {
        let forgotten = forget(site);
        if outcome.is_ok() {
            outcome = forgotten;
        }
    }
    outcome
}

/// Write `site`'s session out as a `cookies.txt` for one engine run.
///
/// `None` is the ordinary answer, not a failure: no session stored, a session
/// that outlived its sign-in, or a blob this Windows account can no longer
/// decrypt. Every one of those means the download simply runs without cookies.
pub fn lease(site: Site) -> AppResult<Option<CookieLease>> {
    let Some(blob) = load_blob(site)? else {
        return Ok(None);
    };
    if !blob.signed_in || blob.cookies.is_empty() {
        return Ok(None);
    }
    // The jar's own capture time decides, not only the record's: the two are
    // written a moment apart, and a lapsed jar goes the moment it is found.
    if site == Site::Other
        && !state::other_fresh(Some(blob.captured_at), chrono::Utc::now().timestamp())
    {
        let _ = forget(Site::Other);
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

    Ok(Some(CookieLease {
        path,
        site,
        captured_at: blob.captured_at,
    }))
}

/// Delete the other-site jar if its hour is up, by the record in
/// `state.json` -- a stat and a small read, and no decryption, so it is cheap
/// enough for every status poll. Best effort: a sweep that fails leaves a jar
/// no lease will hand out, and the next sweep tries again.
pub(super) fn drop_lapsed() {
    if !session_exists(Site::Other) {
        return;
    }
    let link = state::load();
    if !state::other_fresh(link.other.last_push_at, chrono::Utc::now().timestamp()) {
        if let Err(err) = forget(Site::Other) {
            crate::log_warn!("bridge", "could not delete a lapsed other-site session: {err}");
        }
    }
}

/// The addresses the other-site session was lent for and refused on, each
/// with the capture time of the jar that was refused.
///
/// This is the other-site session's `session_tried`. It is lent on the first
/// attempt rather than after a refusal, so the engine has no second run to
/// mark; but the queue repeats a download that failed with an access error on
/// its own (`queue.rs`), and every repeat would lend the same jar again --
/// spending the account, a few seconds apart, on a post the site has just
/// turned it away from. A refusal is remembered here for the life of the
/// process, against that one jar: the repeats run without it, and the next
/// press of İndir, which sends a new jar, is lent again.
static REFUSED: Mutex<RefusalMemo> = Mutex::new(RefusalMemo::new());

/// Few enough to search by walking, and more than the downloads a user could
/// have running at once.
const REFUSALS_KEPT: usize = 32;

#[derive(Debug)]
struct RefusalMemo {
    entries: Vec<(String, i64)>,
}

impl RefusalMemo {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn note(&mut self, url: &str, captured_at: i64) {
        if self.holds(url, captured_at) {
            return;
        }
        if self.entries.len() >= REFUSALS_KEPT {
            self.entries.remove(0);
        }
        self.entries.push((url.to_string(), captured_at));
    }

    fn holds(&self, url: &str, captured_at: i64) -> bool {
        self.entries
            .iter()
            .any(|(seen, at)| seen == url && *at == captured_at)
    }
}

/// Whether the other-site jar captured at `captured_at` was already refused
/// for `url`.
pub(super) fn refused_before(url: &str, captured_at: i64) -> bool {
    REFUSED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .holds(url, captured_at)
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
    /// Whose jar this is a copy of, and so the one jar a rotation may go back
    /// into.
    site: Site,
    /// When the jar was captured, which tells one other-site jar from the
    /// next one İndir sends.
    captured_at: i64,
}

impl CookieLease {
    /// The file to hand the engine, as `--cookies` wants it.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Say that the run this lease was handed to failed with `err` on `url`.
    ///
    /// Only the other-site session keeps count (see `REFUSED`), and only of a
    /// refusal about access -- the site turning the account away. A network
    /// error or a full disk says nothing about the account, and the next
    /// attempt may lend the session again.
    pub fn refused(&self, url: &str, err: &AppError) {
        let about_access = matches!(
            err,
            AppError::Forbidden { .. } | AppError::MembershipRequired { .. }
        );
        if self.site != Site::Other || !about_access {
            return;
        }
        REFUSED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .note(url, self.captured_at);
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
        let Ok(Some(mut blob)) = load_blob(self.site) else {
            return;
        };
        // The engine writes back every cookie the run collected, the stream
        // hosts' among them; only what belongs to the site goes into its jar.
        let kept: Vec<Cookie> = rotated
            .into_iter()
            .filter(|cookie| belongs(self.site, blob.domain.as_deref(), &cookie.domain))
            .collect();
        if kept.is_empty() {
            return;
        }
        blob.cookies = kept;

        if let Err(err) = save_blob(self.site, &blob) {
            crate::log_warn!("bridge", "could not store the refreshed session: {err}");
            return;
        }
        if let Err(err) = state::record_rotation(self.site, &blob.cookies) {
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

fn save_blob(site: Site, blob: &SessionBlob) -> AppResult<()> {
    let mut plain = serde_json::to_vec(blob)?;
    let sealed = protect(&plain);
    scrub_bytes(&mut plain);

    state::write_atomic(&session_path(site)?, &sealed?)
}

/// Decrypt the stored jar, or explain to the interface why there is none.
///
/// A decrypt failure is not an error the user should be shown as an error. An
/// administrator resetting the Windows password destroys the DPAPI master key,
/// and from then on this blob can never be opened again by anyone, including
/// its owner. The only useful answer is "reconnect your browser", so the
/// unopenable file goes and the reason is left where the interface can say it.
fn load_blob(site: Site) -> AppResult<Option<SessionBlob>> {
    let path = session_path(site)?;
    let Ok(sealed) = std::fs::read(&path) else {
        return Ok(None);
    };

    let Some(mut plain) = unprotect(&sealed) else {
        let _ = std::fs::remove_file(&path);
        state::record_error(
            "the stored browser session could not be decrypted on this Windows account -- reconnect the browser",
        );
        let _ = state::clear_session(site);
        return Ok(None);
    };

    let blob = serde_json::from_slice::<SessionBlob>(&plain).ok();
    scrub_bytes(&mut plain);

    if blob.is_none() {
        let _ = std::fs::remove_file(&path);
        state::record_error("the stored browser session was unreadable -- reconnect the browser");
        let _ = state::clear_session(site);
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
            domain: None,
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

    /// YouTube's jar keeps the name every earlier build reads and writes, or
    /// updating the app would quietly drop the session the user already lent.
    #[test]
    fn each_site_has_a_file_of_its_own() {
        assert_eq!(file_name(Site::Youtube), "session.bin");
        assert_eq!(file_name(Site::Tiktok), "session-tiktok.bin");
        assert_eq!(file_name(Site::Other), "session-other.bin");
    }

    #[test]
    fn a_tiktok_jar_keeps_only_tiktok_cookies() {
        for kept in [
            ".tiktok.com",
            "www.tiktok.com",
            "tiktok.com",
            "v16-webapp-prime.tiktok.com",
            ".TikTok.com",
        ] {
            assert!(belongs(Site::Tiktok, None, kept), "dropped {kept}");
        }
        for dropped in [
            ".youtube.com",
            "tiktok.com.evil.test",
            ".nottiktok.com",
            ".tiktokv.com",
            ".tiktokcdn.com",
            "",
        ] {
            assert!(!belongs(Site::Tiktok, None, dropped), "kept {dropped}");
        }
        // YouTube's is kept as the extension pushed it.
        for anything in [".youtube.com", ".google.com", "accounts.youtube.com"] {
            assert!(belongs(Site::Youtube, None, anything));
        }
    }

    fn cookie(domain: &str, name: &str) -> Cookie {
        Cookie {
            domain: domain.into(),
            name: name.into(),
            value: "value".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            expiration_date: None,
        }
    }

    fn other_push(domain: Option<&str>, signed_in: bool, cookies: Vec<Cookie>) -> Push {
        Push {
            peer: crate::bridge::protocol::Peer {
                v: 1,
                profile_id: "p".into(),
                browser: crate::bridge::protocol::Browser::Chrome,
                extension_version: "1.0.5".into(),
                profile_label: None,
            },
            site: Some(Site::Other),
            domain: domain.map(str::to_string),
            signed_in,
            account_hint: None,
            captured_at: 1_900_000_000,
            cookies,
        }
    }

    /// The other-site jar keeps the one site it was pushed for, subdomains
    /// included, and nothing a page's CDN or another site set.
    #[test]
    fn an_other_jar_keeps_only_its_own_sites_cookies() {
        let push = other_push(
            Some("instagram.com"),
            true,
            vec![
                cookie(".instagram.com", "sessionid"),
                cookie("www.instagram.com", "csrftoken"),
                cookie("i.instagram.com", "ds_user_id"),
                cookie(".cdninstagram.com", "x"),
                cookie(".facebook.com", "c_user"),
                cookie("instagram.com.evil.test", "y"),
                cookie(".youtube.com", "SID"),
            ],
        );
        let kept = kept_cookies(&push, Site::Other, other_domain_of(&push).as_deref());
        let names: Vec<&str> = kept.iter().map(|cookie| cookie.name.as_str()).collect();
        assert_eq!(names, ["sessionid", "csrftoken", "ds_user_id"]);

        // A write-back from the engine is held to the same rule, by the jar's
        // own domain; a jar without one keeps nothing.
        assert!(belongs(Site::Other, Some("instagram.com"), ".instagram.com"));
        assert!(!belongs(Site::Other, Some("instagram.com"), "scontent.cdninstagram.com"));
        assert!(!belongs(Site::Other, None, ".instagram.com"));
    }

    /// Nothing is kept from a push the host should not have let through -- no
    /// domain, or a bad one -- nor from a signed-out browser.
    #[test]
    fn an_other_push_without_a_proper_site_or_sign_in_keeps_nothing() {
        let cookies = || vec![cookie(".instagram.com", "sessionid")];
        for push in [
            other_push(None, true, cookies()),
            other_push(Some("www.instagram.com"), true, cookies()),
            other_push(Some("youtube.com"), true, vec![cookie(".youtube.com", "SID")]),
            other_push(Some("instagram.com"), false, cookies()),
        ] {
            let kept = kept_cookies(&push, Site::Other, other_domain_of(&push).as_deref());
            assert!(kept.is_empty(), "{:?} kept {kept:?}", push.domain);
        }
    }

    #[test]
    fn an_other_jar_lapses_an_hour_after_it_was_captured() {
        let captured = 1_900_000_000;
        assert!(state::other_fresh(Some(captured), captured));
        assert!(state::other_fresh(Some(captured), captured + 3_600));
        assert!(!state::other_fresh(Some(captured), captured + 3_601));
        assert!(!state::other_fresh(None, captured));
    }

    #[test]
    fn a_blob_keeps_the_site_an_other_jar_is_for() {
        let blob = SessionBlob {
            cookies: vec![cookie(".instagram.com", "sessionid")],
            captured_at: 1_900_000_000,
            signed_in: true,
            domain: Some("instagram.com".into()),
        };
        let back: SessionBlob =
            serde_json::from_str(&serde_json::to_string(&blob).unwrap()).unwrap();
        assert_eq!(back.domain.as_deref(), Some("instagram.com"));
        // YouTube's and TikTok's blobs are written as they always were.
        let plain = SessionBlob {
            domain: None,
            ..blob
        };
        let written = serde_json::to_value(&plain).unwrap();
        assert!(written.get("domain").is_none(), "{written}");
    }

    /// A refusal is remembered against the one jar that was refused: the same
    /// address is not lent that jar again, a new jar is lent, and another
    /// address was never refused.
    #[test]
    fn a_refused_jar_is_not_lent_again_for_the_same_address() {
        let mut memo = RefusalMemo::new();
        let post = "https://www.instagram.com/p/abc/";
        assert!(!memo.holds(post, 1));
        memo.note(post, 1);
        memo.note(post, 1);
        assert!(memo.holds(post, 1));
        assert_eq!(memo.entries.len(), 1);
        assert!(!memo.holds(post, 2));
        assert!(!memo.holds("https://www.instagram.com/p/def/", 1));

        for n in 0..REFUSALS_KEPT {
            memo.note(&format!("https://site.example/{n}"), 1);
        }
        assert_eq!(memo.entries.len(), REFUSALS_KEPT);
        assert!(!memo.holds(post, 1), "the oldest refusal makes room");
    }
}
