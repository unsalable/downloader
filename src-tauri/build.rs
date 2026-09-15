use std::path::Path;
use std::process::Command;

fn main() {
    embed_commit();
    tauri_build::build()
}

/// Record the commit this build was made from.
///
/// Releases carry no version number -- one rolling release is replaced in
/// place -- so the commit is what tells two builds apart. The Android app
/// compares it with the commit the release tag points at to learn whether a
/// newer build exists. A build made outside a git checkout records nothing and
/// never reports an update.
fn embed_commit() {
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|sha| sha.len() == 40)
        .unwrap_or_default();
    println!("cargo:rustc-env=UD_BUILD_COMMIT={commit}");

    // A new commit has to rebuild this crate, or the old hash would be kept.
    // Cargo reruns a script on every build for a path that does not exist, so
    // only paths that do are named.
    let git = Path::new("../.git");
    let mut watched = vec![git.join("HEAD"), git.join("packed-refs")];
    if let Ok(head) = std::fs::read_to_string(git.join("HEAD")) {
        if let Some(reference) = head.trim().strip_prefix("ref: ") {
            watched.push(git.join(reference));
        }
    }
    for path in watched.iter().filter(|path| path.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
