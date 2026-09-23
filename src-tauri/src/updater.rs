//! Checking for, and installing, a newer build of the app.
//!
//! Releases have no version numbers: there is one rolling release on the
//! `latest` tag whose files are replaced in place, and the tag is moved to the
//! commit they were built from once they are up. A build knows its own commit
//! (see `build.rs`), so a newer one exists exactly when the tag points
//! somewhere else.
//!
//! The phone hands the new APK to the system installer, which asks the user
//! and checks the signature. The Windows app replaces itself: it stages the
//! installer it was installed with, waits until nothing is running, starts it
//! and exits. Nothing but this module vouches for that file, which is why the
//! desktop is stricter about where it comes from and what it hashes to.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::settings::Settings;
use crate::{log_info, net, tools};

const REPOSITORY: &str = "unsalable/downloader";
const RELEASE_TAG: &str = "latest";
const API: &str = "https://api.github.com";

#[cfg(any(windows, test))]
const NSIS_ASSET: &str = "UniversalDownloader_x64-setup.exe";
#[cfg(any(windows, test))]
const MSI_ASSET: &str = "UniversalDownloader_x64.msi";

pub const EVENT_UPDATE_PROGRESS: &str = "update://progress";

/// A newer build, as the interface is told about it and hands back to install.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdate {
    pub commit: String,
    pub asset_url: String,
    pub asset_size: u64,
    /// `sha256:<hex>`, when GitHub reports one for the file.
    pub digest: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
}

/// The commit this binary was built from, or empty when that is unknown.
pub fn build_commit() -> &'static str {
    env!("UD_BUILD_COMMIT")
}

/// The release file that can replace this copy of the app.
///
/// On the phone that is the APK for the processor this code is running on: the
/// APK is split per ABI, so that is also the one that is installed. On Windows
/// it is the installer this copy came from -- a per-user NSIS install and a
/// per-machine MSI one do not upgrade each other cleanly. `None` means this
/// copy never updates itself, which is every development build.
fn asset_name() -> Option<&'static str> {
    #[cfg(target_os = "android")]
    {
        match std::env::consts::ARCH {
            "aarch64" => Some("UniversalDownloader_android_arm64.apk"),
            "arm" => Some("UniversalDownloader_android_armv7.apk"),
            "x86_64" => Some("UniversalDownloader_android_x86_64.apk"),
            _ => None,
        }
    }
    #[cfg(windows)]
    {
        windows_asset(tauri::utils::platform::bundle_type(), std::env::consts::ARCH)
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        None
    }
}

/// The bundler stamps each installer's copy of the executable with the kind of
/// installer it went into; a binary run out of `target/` carries no stamp.
#[cfg(any(windows, test))]
fn windows_asset(
    bundle: Option<tauri::utils::config::BundleType>,
    arch: &str,
) -> Option<&'static str> {
    use tauri::utils::config::BundleType;

    if arch != "x86_64" {
        return None;
    }
    match bundle? {
        BundleType::Nsis => Some(NSIS_ASSET),
        BundleType::Msi => Some(MSI_ASSET),
        _ => None,
    }
}

#[derive(Deserialize)]
struct GitRef {
    object: GitObject,
}

#[derive(Deserialize)]
struct GitObject {
    sha: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Release {
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    digest: Option<String>,
}

#[cfg(any(not(target_os = "android"), test))]
#[derive(Deserialize)]
struct Comparison {
    status: String,
}

#[cfg(any(not(target_os = "android"), test))]
impl Comparison {
    /// GitHub describes the head of a comparison as `ahead`, `behind`,
    /// `diverged` or `identical`. Only the first is a newer build.
    fn head_is_ahead(&self) -> bool {
        self.status == "ahead"
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    path: &str,
) -> AppResult<T> {
    let url = format!("{API}{path}");
    let response = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(AppError::from_status(response.status().as_u16(), url));
    }
    Ok(response.json::<T>().await?)
}

/// The commit the release tag points at. A lightweight tag names the commit
/// directly; an annotated one names a tag object that has to be followed.
async fn tagged_commit(client: &reqwest::Client) -> AppResult<String> {
    let reference: GitRef =
        get_json(client, &format!("/repos/{REPOSITORY}/git/ref/tags/{RELEASE_TAG}")).await?;
    if reference.object.kind != "tag" {
        return Ok(reference.object.sha);
    }
    let tag: GitRef = get_json(
        client,
        &format!("/repos/{REPOSITORY}/git/tags/{}", reference.object.sha),
    )
    .await?;
    Ok(tag.object.sha)
}

/// Whether the release was built from a commit that comes after this build's.
///
/// A tag that merely differs is not enough for an app that installs without
/// asking: a build made from a commit newer than the release would replace
/// itself with the older one, every launch. GitHub answering 404 means it has
/// never seen this build's commit -- a local one that was not pushed -- and
/// that is not behind anything either.
#[cfg(not(target_os = "android"))]
async fn release_is_ahead(client: &reqwest::Client, current: &str, tagged: &str) -> AppResult<bool> {
    // The second page of one commit per page: the answer is in the envelope,
    // and the first page is the only one that carries every changed file with
    // its patch, which runs to hundreds of kilobytes.
    let path = format!("/repos/{REPOSITORY}/compare/{current}...{tagged}?per_page=1&page=2");
    match get_json::<Comparison>(client, &path).await {
        Ok(comparison) => Ok(comparison.head_is_ahead()),
        // GitHub has never seen this build's commit. Both answers are "do not
        // install", but only one of them is a build that can never update
        // itself, and a support log that cannot tell them apart is no help.
        Err(AppError::NotFound { .. }) => {
            // Named by path rather than imported: this is the module's only
            // warning, and a phone does not build this function, so the import
            // would sit unused there and fail its lints.
            crate::log_warn!(
                "updater",
                "github does not know commit {current}; this build cannot tell whether it is behind"
            );
            Ok(false)
        }
        Err(err) => Err(err),
    }
}

/// Look for a newer build. `None` means this one is current -- or that there is
/// nothing this copy could install.
pub async fn check(settings: &Settings) -> AppResult<Option<AppUpdate>> {
    let current = build_commit();
    let Some(wanted) = asset_name() else {
        return Ok(None);
    };
    if current.is_empty() {
        return Ok(None);
    }

    let client = net::client(settings)?;
    let tagged = tagged_commit(&client).await?;
    if tagged.eq_ignore_ascii_case(current) {
        return Ok(None);
    }

    // The phone asks before it installs and Android refuses a downgrade by
    // itself, so only the desktop pays for the extra request.
    #[cfg(not(target_os = "android"))]
    if !release_is_ahead(&client, current, &tagged).await? {
        log_info!("updater", "build {current} is not behind {tagged}");
        return Ok(None);
    }

    let release: Release =
        get_json(&client, &format!("/repos/{REPOSITORY}/releases/tags/{RELEASE_TAG}")).await?;
    let Some(asset) = release.assets.into_iter().find(|asset| asset.name == wanted) else {
        return Ok(None);
    };

    log_info!("updater", "build {current} is behind {tagged}");
    Ok(Some(AppUpdate {
        commit: tagged,
        asset_url: asset.browser_download_url,
        asset_size: asset.size,
        digest: asset.digest,
        published_at: release.published_at,
    }))
}

/// Where a build's installer is kept in `dir`. Named after the commit, so one
/// complete copy is recognised and reused, and after the release file's own
/// extension, because that is what Windows decides how to run it by.
fn staged_path(update: &AppUpdate, dir: &Path, asset: &str) -> PathBuf {
    let short = update.commit.get(..12).unwrap_or(&update.commit);
    let extension = Path::new(asset)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("bin");
    dir.join(format!("UniversalDownloader-{short}.{extension}"))
}

/// Fetch the new installer into `dir` and return its path.
///
/// The file is named after the commit, so asking again -- after backing out of
/// the system installer, say, or on the next launch -- reuses a complete copy
/// instead of fetching it again. Copies of any other build are removed.
pub async fn download(
    update: &AppUpdate,
    dir: &Path,
    settings: &Settings,
    on_progress: tools::InstallProgress<'_>,
) -> AppResult<PathBuf> {
    let asset = asset_name().ok_or_else(nothing_to_install)?;
    // The update comes back from the interface, and on the desktop the file is
    // run as it is: nothing is fetched until it is known to be ours.
    #[cfg(not(target_os = "android"))]
    vet(update, asset)?;

    std::fs::create_dir_all(dir)?;
    let target = staged_path(update, dir, asset);

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path() != target {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    if verify(&target, update).is_err() {
        tools::download_to_file(&update.asset_url, &target, settings, on_progress).await?;
    }
    if let Err(err) = verify(&target, update) {
        let _ = std::fs::remove_file(&target);
        return Err(err);
    }
    Ok(target)
}

/// The installer `download` staged for this build, checked again.
///
/// Minutes or hours pass between staging a file and running it, in a folder
/// any program of this user can write to, so what is about to be started is
/// hashed once more rather than trusted for having been good earlier.
#[cfg(windows)]
pub fn staged(update: &AppUpdate, dir: &Path) -> AppResult<PathBuf> {
    let asset = asset_name().ok_or_else(nothing_to_install)?;
    vet(update, asset)?;
    let target = staged_path(update, dir, asset);
    verify(&target, update)?;
    Ok(target)
}

/// Remove the installer of the build that is now running: the update it was
/// staged for has happened. One staged for a newer build is left alone.
#[cfg(windows)]
pub fn sweep_applied(dir: &Path) {
    let current = build_commit();
    let Some(short) = current.get(..12) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().contains(short) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn nothing_to_install() -> AppError {
    AppError::Other("this copy of the app does not update itself".into())
}

/// Refuse an update the desktop app should not run.
///
/// An APK is checked by Android against the key the installed app was signed
/// with. Nothing does that for a Windows installer, so here the digest is
/// required rather than welcome, the address has to be this project's own
/// release, and the commit -- which becomes part of a file name -- has to be
/// nothing but a hash.
#[cfg(any(not(target_os = "android"), test))]
fn vet(update: &AppUpdate, asset: &str) -> AppResult<()> {
    let refuse = |why: &str| Err(AppError::Other(format!("refused the update: {why}")));

    let commit = &update.commit;
    if !(7..=64).contains(&commit.len()) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return refuse("the commit is not a hash");
    }

    let hash = update
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"));
    if !hash.is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())) {
        return refuse("the release does not publish a sha256 digest for it");
    }

    // The whole address rather than how it starts: a longer path could climb
    // out of the repository with `..` and still begin the right way.
    let ours = format!("https://github.com/{REPOSITORY}/releases/download/{RELEASE_TAG}/{asset}");
    if update.asset_url != ours {
        return refuse("the file is not from this project's releases");
    }

    Ok(())
}

/// Whether the file at `path` is the complete, undamaged release file.
fn verify(path: &Path, update: &AppUpdate) -> AppResult<()> {
    let size = std::fs::metadata(path)?.len();
    if size != update.asset_size {
        return Err(AppError::Network(format!(
            "the update arrived incomplete ({size} of {} bytes)",
            update.asset_size
        )));
    }
    verify_digest(path, update.digest.as_deref())
}

/// Compare a file with a GitHub `sha256:<hex>` digest. No digest to compare
/// with is not a failure here: the phone's installer still refuses an APK that
/// was not signed with the same key as the installed app, and the desktop has
/// already turned away an update without one (see `vet`).
fn verify_digest(path: &Path, digest: Option<&str>) -> AppResult<()> {
    use sha2::{Digest, Sha256};

    let Some(expected) = digest.and_then(|value| value.strip_prefix("sha256:")) else {
        return Ok(());
    };

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    if hex::encode(hasher.finalize()).eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(AppError::Network("the downloaded update is damaged".into()))
    }
}

/// Start the staged installer and leave it running on its own.
///
/// The caller exits the app straight afterwards; the installer replaces the
/// files and, because it is asked to, starts the new build when it is done.
/// `minimized` carries a window that was hidden in the tray across the
/// restart, so an update at sign-in does not end with a window nobody opened.
#[cfg(windows)]
pub fn launch_installer(installer: &Path, minimized: bool) -> AppResult<()> {
    use std::process::{Command, Stdio};

    // `ud-bridge.exe` sits beside the app and is replaced with it. A browser
    // starts it for a moment whenever the session changes, and an installer
    // that finds the file open stops to ask about it -- which is not a
    // question a passive update has anyone to put to.
    stop_bridge_host();

    let (program, args) = installer_command(installer, minimized);
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|err| AppError::Other(format!("could not start the installer: {err}")))
}

/// The program to start for an installer, and what to start it with.
///
/// Both forms show progress and ask nothing, and both start the app again when
/// they finish. The NSIS flags are the ones Tauri's installer template
/// defines: `/P` is passive, `/UPDATE` leaves shortcuts and settings as they
/// are, `/R` relaunches and `/ARGS` is what the relaunch is given. An MSI is
/// run by `msiexec`, which is never allowed to restart Windows unasked.
#[cfg(windows)]
fn installer_command(installer: &Path, minimized: bool) -> (PathBuf, Vec<std::ffi::OsString>) {
    let is_msi = installer
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("msi"));

    if is_msi {
        let mut args = vec!["/i".into(), installer.as_os_str().to_os_string()];
        args.extend(["/passive", "/promptrestart", "AUTOLAUNCHAPP=True"].map(Into::into));
        if minimized {
            args.push("LAUNCHAPPARGS=--minimized".into());
        }
        (system_program("msiexec.exe"), args)
    } else {
        let mut args: Vec<std::ffi::OsString> = ["/P", "/UPDATE", "/R"].map(Into::into).to_vec();
        if minimized {
            args.extend(["/ARGS", "--minimized"].map(Into::into));
        }
        (installer.to_path_buf(), args)
    }
}

/// A program that ships with Windows, by its full path: a bare name is looked
/// for beside the app first, which is a folder the user's programs can write to.
#[cfg(windows)]
fn system_program(name: &str) -> PathBuf {
    std::env::var_os("SYSTEMROOT")
        .map(|root| PathBuf::from(root).join("System32").join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// End any running native messaging host. It holds no state -- every message
/// is one process that answers from files and exits -- so the worst this costs
/// is one push, which the extension sends again at the next cookie change.
#[cfg(windows)]
fn stop_bridge_host() {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let _ = Command::new(system_program("taskkill.exe"))
        .args(["/F", "/IM", "ud-bridge.exe"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ud-updater-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    const GOOD_DIGEST: &str =
        "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn desktop_update() -> AppUpdate {
        AppUpdate {
            commit: "05b60a1c2b1b5d97eaf1e60eda459987c70f717b".into(),
            asset_url: format!(
                "https://github.com/unsalable/downloader/releases/download/latest/{NSIS_ASSET}"
            ),
            asset_size: 5,
            digest: Some(GOOD_DIGEST.into()),
            published_at: None,
        }
    }

    #[test]
    fn a_matching_digest_passes_and_a_different_one_fails() {
        let dir = scratch("digest");
        let file = dir.join("app.apk");
        std::fs::write(&file, b"hello").unwrap();
        // sha256("hello")
        let good = GOOD_DIGEST;
        let bad = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

        assert!(verify_digest(&file, Some(good)).is_ok());
        assert!(verify_digest(&file, Some(&good.to_uppercase().replace("SHA256", "sha256"))).is_ok());
        assert!(verify_digest(&file, Some(bad)).is_err());
    }

    #[test]
    fn a_missing_or_unknown_digest_is_not_a_failure() {
        let dir = scratch("no-digest");
        let file = dir.join("app.apk");
        std::fs::write(&file, b"hello").unwrap();

        assert!(verify_digest(&file, None).is_ok());
        assert!(verify_digest(&file, Some("md5:abc")).is_ok());
    }

    #[test]
    fn a_staged_file_has_to_be_whole_and_undamaged() {
        let dir = scratch("verify");
        let update = desktop_update();
        let file = staged_path(&update, &dir, NSIS_ASSET);

        assert!(verify(&file, &update).is_err(), "a missing file");
        std::fs::write(&file, b"hell").unwrap();
        assert!(verify(&file, &update).is_err(), "a short file");
        std::fs::write(&file, b"jello").unwrap();
        assert!(verify(&file, &update).is_err(), "the right size, the wrong bytes");
        std::fs::write(&file, b"hello").unwrap();
        assert!(verify(&file, &update).is_ok());
    }

    #[test]
    fn a_staged_file_keeps_the_extension_windows_runs_it_by() {
        let update = desktop_update();
        let dir = Path::new("updates");

        assert_eq!(
            staged_path(&update, dir, NSIS_ASSET),
            dir.join("UniversalDownloader-05b60a1c2b1b.exe")
        );
        assert_eq!(
            staged_path(&update, dir, MSI_ASSET),
            dir.join("UniversalDownloader-05b60a1c2b1b.msi")
        );
    }

    #[test]
    fn release_json_yields_the_fields_an_update_needs() {
        let release: Release = serde_json::from_str(
            r#"{
                "published_at": "2026-09-15T00:00:00Z",
                "assets": [{
                    "name": "UniversalDownloader_android_arm64.apk",
                    "size": 57268738,
                    "browser_download_url": "https://github.com/x/y/releases/download/latest/a.apk",
                    "digest": "sha256:abc",
                    "content_type": "application/vnd.android.package-archive"
                }]
            }"#,
        )
        .unwrap();

        let asset = &release.assets[0];
        assert_eq!(asset.size, 57268738);
        assert_eq!(asset.digest.as_deref(), Some("sha256:abc"));
    }

    #[test]
    fn an_annotated_tag_is_recognised_as_one() {
        let lightweight: GitRef =
            serde_json::from_str(r#"{"ref":"refs/tags/latest","object":{"sha":"abc","type":"commit"}}"#)
                .unwrap();
        let annotated: GitRef =
            serde_json::from_str(r#"{"ref":"refs/tags/latest","object":{"sha":"def","type":"tag"}}"#)
                .unwrap();

        assert_eq!(lightweight.object.kind, "commit");
        assert_eq!(annotated.object.kind, "tag");
    }

    #[test]
    fn a_build_that_no_installer_made_never_updates() {
        // Tests run out of `target/`, like `tauri dev` does.
        assert_eq!(asset_name(), None);
    }

    #[test]
    fn the_installer_matches_how_the_app_was_installed() {
        use tauri::utils::config::BundleType;

        assert_eq!(windows_asset(Some(BundleType::Nsis), "x86_64"), Some(NSIS_ASSET));
        assert_eq!(windows_asset(Some(BundleType::Msi), "x86_64"), Some(MSI_ASSET));
        assert_eq!(windows_asset(None, "x86_64"), None);
        assert_eq!(windows_asset(Some(BundleType::Deb), "x86_64"), None);
        // Only an x64 installer is published.
        assert_eq!(windows_asset(Some(BundleType::Nsis), "aarch64"), None);
    }

    #[cfg(windows)]
    #[test]
    fn each_installer_is_started_passive_and_told_to_relaunch() {
        let args = |path: &str, minimized: bool| {
            installer_command(Path::new(path), minimized)
                .1
                .into_iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        };

        assert_eq!(args(r"C:\u\setup.exe", false), "/P /UPDATE /R");
        assert_eq!(args(r"C:\u\setup.exe", true), "/P /UPDATE /R /ARGS --minimized");
        assert_eq!(
            args(r"C:\u\app.msi", false),
            r"/i C:\u\app.msi /passive /promptrestart AUTOLAUNCHAPP=True"
        );
        assert!(args(r"C:\u\app.MSI", true).ends_with("LAUNCHAPPARGS=--minimized"));

        let (program, _) = installer_command(Path::new(r"C:\u\app.msi"), false);
        assert!(program.ends_with("msiexec.exe"));
        let (program, _) = installer_command(Path::new(r"C:\u\setup.exe"), false);
        assert_eq!(program, Path::new(r"C:\u\setup.exe"));
    }

    #[test]
    fn only_a_release_that_is_ahead_is_an_update() {
        let status = |json: &str| serde_json::from_str::<Comparison>(json).unwrap().head_is_ahead();

        assert!(status(r#"{"status":"ahead","ahead_by":4,"behind_by":0,"commits":[]}"#));
        assert!(!status(r#"{"status":"behind","ahead_by":0,"behind_by":2}"#));
        assert!(!status(r#"{"status":"diverged","ahead_by":1,"behind_by":1}"#));
        assert!(!status(r#"{"status":"identical","ahead_by":0,"behind_by":0}"#));
    }

    #[test]
    fn an_update_from_this_projects_release_is_accepted() {
        assert!(vet(&desktop_update(), NSIS_ASSET).is_ok());
    }

    #[test]
    fn an_update_without_a_sha256_digest_is_refused() {
        for digest in [None, Some("md5:abc"), Some("sha256:abc"), Some("sha256:")] {
            let update = AppUpdate {
                digest: digest.map(str::to_string),
                ..desktop_update()
            };
            assert!(vet(&update, NSIS_ASSET).is_err(), "{digest:?}");
        }
    }

    #[test]
    fn an_update_from_anywhere_else_is_refused() {
        let file = NSIS_ASSET;
        for url in [
            format!("https://example.com/{file}"),
            format!("http://github.com/unsalable/downloader/releases/download/latest/{file}"),
            format!("https://github.com/someone/else/releases/download/latest/{file}"),
            format!("https://github.com/unsalable/downloader/releases/download/../../../../someone/else/releases/download/latest/{file}"),
            format!("https://github.com/unsalable/downloader/releases/download/../{file}"),
            format!("https://github.com/unsalable/downloader/releases/download/{file}"),
            // There is one release, and it is not this one.
            format!("https://github.com/unsalable/downloader/releases/download/nightly/{file}"),
            format!("https://github.com/unsalable/downloader/releases/download/latest/{file}?x=1"),
            "https://github.com/unsalable/downloader/releases/download/latest/evil.exe".to_string(),
            // The other installer kind is not what this copy was installed with.
            format!("https://github.com/unsalable/downloader/releases/download/latest/{MSI_ASSET}"),
        ] {
            let update = AppUpdate {
                asset_url: url.clone(),
                ..desktop_update()
            };
            assert!(vet(&update, file).is_err(), "{url}");
        }
    }

    #[test]
    fn a_commit_that_is_not_a_hash_is_refused() {
        for commit in ["", "abc", "..\\..\\evil", "05b60a1c2b1b/../x", "latest-build"] {
            let update = AppUpdate {
                commit: commit.into(),
                ..desktop_update()
            };
            assert!(vet(&update, NSIS_ASSET).is_err(), "{commit:?}");
        }
    }
}
