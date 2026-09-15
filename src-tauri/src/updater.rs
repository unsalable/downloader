//! Checking for, and installing, a newer build of the Android app.
//!
//! Releases have no version numbers: there is one rolling release on the
//! `latest` tag whose files are replaced in place, and the tag is moved to the
//! commit they were built from once they are up. A build knows its own commit
//! (see `build.rs`), so a newer one exists exactly when the tag points
//! somewhere else.
//!
//! Only the phone installs updates this way. The desktop installer is a
//! separate download the user runs.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::settings::Settings;
use crate::{log_info, net, tools};

const REPOSITORY: &str = "unsalable/downloader";
const RELEASE_TAG: &str = "latest";
const API: &str = "https://api.github.com";

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
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

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
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

/// The release file built for the processor this code is running on. The APK
/// is split per ABI, so that is also the one that is installed.
fn asset_name() -> Option<&'static str> {
    if !cfg!(target_os = "android") {
        return None;
    }
    match std::env::consts::ARCH {
        "aarch64" => Some("UniversalDownloader_android_arm64.apk"),
        "arm" => Some("UniversalDownloader_android_armv7.apk"),
        "x86_64" => Some("UniversalDownloader_android_x86_64.apk"),
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

/// Look for a newer build. `None` means this one is current -- or that there is
/// nothing this device could install.
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

/// Fetch the new APK into `dir` and return its path.
///
/// The file is named after the commit, so pressing Update again -- after
/// backing out of the system installer, say -- reuses a complete copy instead
/// of fetching it again. Copies of any other build are removed.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub async fn download(
    update: &AppUpdate,
    dir: &Path,
    settings: &Settings,
    on_progress: tools::InstallProgress<'_>,
) -> AppResult<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let short = update.commit.get(..12).unwrap_or(&update.commit);
    let target = dir.join(format!("UniversalDownloader-{short}.apk"));

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path() != target {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let reusable = std::fs::metadata(&target).map(|meta| meta.len()).ok() == Some(update.asset_size)
        && verify_digest(&target, update.digest.as_deref()).is_ok();
    if !reusable {
        tools::download_to_file(&update.asset_url, &target, settings, on_progress).await?;
    }

    let size = std::fs::metadata(&target)?.len();
    if size != update.asset_size {
        let _ = std::fs::remove_file(&target);
        return Err(AppError::Network(format!(
            "the update arrived incomplete ({size} of {} bytes)",
            update.asset_size
        )));
    }
    if let Err(err) = verify_digest(&target, update.digest.as_deref()) {
        let _ = std::fs::remove_file(&target);
        return Err(err);
    }
    Ok(target)
}

/// Compare a file with a GitHub `sha256:<hex>` digest. No digest to compare
/// with is not a failure: the installer still refuses an APK that was not
/// signed with the same key as the installed app.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
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

    #[test]
    fn a_matching_digest_passes_and_a_different_one_fails() {
        let dir = scratch("digest");
        let file = dir.join("app.apk");
        std::fs::write(&file, b"hello").unwrap();
        // sha256("hello")
        let good = "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
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
    fn the_desktop_build_never_looks_for_an_apk() {
        if !cfg!(target_os = "android") {
            assert_eq!(asset_name(), None);
        }
    }
}
