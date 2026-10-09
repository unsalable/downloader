//! Finished files: opening one, showing where it is, handing it to another app.
//!
//! These used to be the opener plugin's own JS commands. Its `open_path` refuses
//! every path outside a scope this app never configured -- which is what made
//! every finished row claim its file was gone -- and the scope that would let it
//! through would let the page start any program on the disk, since the shell
//! "opens" a program by running it. So a file is checked here first: that it is
//! named by a full path, that it is still there (which has a code of its own,
//! `fileMissing`), and, before it is opened or shared, that it is a kind of file
//! this app writes. Only then is it handed to the system.
//!
//! Why a list of kinds rather than "only the paths the app recorded": those
//! live in four places (the queue, the history, the conversions held in memory
//! and the editor's export), two of which are never written down, and the page
//! can already read any file it likes through `allow_media_preview`. Opening a
//! picture or a song in its own app grants nothing that was not there before.
//! Starting a program is the one thing the page must never be able to do, and a
//! list of what may be opened fails closed: a trailing dot, no extension at all
//! or a type nobody thought of all come out as something not on it.
//!
//! The opener's Rust functions are not held to the webview's scope. That is
//! what lets this module use them, and why nothing but this module should.

use std::io;
use std::path::{Path, PathBuf};

use tauri::AppHandle;

use crate::error::{AppError, AppResult};

#[cfg(windows)]
mod windows_share;

/// Every kind of file this app writes: the containers a download, a conversion
/// or an export ends in, and the pictures a gallery saves. A download keeps the
/// extension yt-dlp reported for its stream, so the list is generous. Nothing
/// on it runs anything; a name that is not on it (a program, a script, a
/// shortcut, no extension at all, yt-dlp's `unknown_video` or `bin`) is not
/// opened from a row -- its folder still is.
///
/// A name like `x.exe:clip.mp4` has the extension `mp4` too. That is an
/// alternate stream of a file, not a program, and the shell picks what opens
/// it by that same `.mp4`, so nothing is run.
const OPENABLE: &[&str] = &[
    // video
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "flv", "f4v", "ts", "mts", "m2ts", "mpg", "mpeg",
    "3gp", "3g2", "ogv", "wmv", "asf",
    // sound
    "mp3", "m4a", "m4b", "aac", "wav", "flac", "opus", "ogg", "oga", "mka", "weba", "wma", "aiff",
    "aif", "amr", "ac3", "eac3", "mp2",
    // pictures
    "jpg", "jpeg", "jfif", "png", "webp", "gif", "avif", "bmp", "tif", "tiff", "heic", "heif",
    "jxl",
];

/// What ShellExecuteEx reports when the user closed "How do you want to open
/// this file?" without choosing. Nothing failed, so nothing is said.
#[cfg(not(target_os = "android"))]
const ERROR_CANCELLED: i32 = 1223;

/// The file `raw` names, if it is still there and is a file.
fn checked_file(raw: &str) -> AppResult<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(AppError::Other("no file was named".into()));
    }
    // A relative name would be resolved against wherever the process happens
    // to be running from, which is never where a download was saved. On
    // Windows this also turns away `\clip.mp4` and `C:clip.mp4`, which lean
    // on the current drive and the current folder on it.
    if !Path::new(raw).is_absolute() {
        return Err(AppError::Other(format!("{raw} is not a full path")));
    }
    // On Windows this also drops trailing dots and spaces the way the shell
    // will, so `evil.exe.` is judged as the `evil.exe` it would open.
    let path = std::path::absolute(raw)?;
    match std::fs::metadata(&path) {
        Err(err) if gone(&err) => Err(AppError::FileMissing(path.display().to_string())),
        Err(err) => Err(err.into()),
        Ok(meta) if !meta.is_file() => {
            Err(AppError::Other(format!("{} is not a file", path.display())))
        }
        Ok(_) => Ok(path),
    }
}

/// A missing file, folder or drive. On Windows also: an invalid drive (15),
/// a drive with nothing in it (21, a card reader or an unplugged stick), and a
/// network path or share that is not there (53, 67).
fn gone(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::NotFound
        || (cfg!(windows) && matches!(err.raw_os_error(), Some(15 | 21 | 53 | 67)))
}

/// Whether this is a kind of file the app writes (see `OPENABLE`).
fn openable(path: &Path) -> AppResult<()> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    match ext {
        Some(ext) if OPENABLE.contains(&ext.as_str()) => Ok(()),
        _ => Err(AppError::Other(format!(
            "{} is not a kind of file this app opens",
            path.display()
        ))),
    }
}

/// Both checks, for what is about to be handed to another program.
fn handable(raw: &str) -> AppResult<PathBuf> {
    let file = checked_file(raw)?;
    openable(&file)?;
    Ok(file)
}

/// Runs on a thread that may wait. A network drive that has gone away can
/// keep `metadata` waiting for the network's timeout, and ShellExecute can
/// stall on one, and neither may hold an async worker while it does.
async fn on_worker<T: Send + 'static>(
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AppError::Other(format!("the file could not be looked at: {err}")))?
}

/// Open a finished file in the app the system has for its type.
pub async fn open(app: AppHandle, raw: String) -> AppResult<()> {
    #[cfg(target_os = "android")]
    {
        let file = on_worker(move || handable(&raw)).await?;
        crate::android::open_file(app, file.to_string_lossy().into_owned()).await
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        on_worker(move || {
            let file = handable(&raw)?;
            // ShellExecute's documentation asks for COM on the calling thread:
            // what opens the file may be a shell extension or a Store app.
            #[cfg(windows)]
            let _com = Com::sta();
            settle(
                tauri_plugin_opener::open_path(&file, None::<&str>),
                "the file could not be opened",
            )
        })
        .await
    }
}

/// Show where a finished file is. A phone has no folder window to select it
/// in; the system's Downloads view, which is where it is, stands in.
pub async fn reveal(app: AppHandle, raw: String) -> AppResult<()> {
    #[cfg(target_os = "android")]
    {
        on_worker(move || checked_file(&raw)).await?;
        crate::android::open_downloads(app).await
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        on_worker(move || {
            let file = checked_file(&raw)?;
            // No type check: selecting a file in a folder window runs nothing.
            // The plugin sets COM up on this thread itself, and never takes it
            // down again; a blocking-pool thread stays in that apartment after
            // a reveal, as an async worker did when the page called it.
            settle(
                tauri_plugin_opener::reveal_item_in_dir(&file),
                "the folder could not be shown",
            )
        })
        .await
    }
}

/// Hand a finished file to another app through the system's share sheet.
/// Held to the same list as opening: the database or the browser link's
/// session file must not be one press on the page away from another app.
pub async fn share(app: AppHandle, raw: String) -> AppResult<()> {
    let file = on_worker(move || handable(&raw)).await?;
    #[cfg(target_os = "android")]
    {
        crate::android::share_file(app, file.to_string_lossy().into_owned()).await
    }
    #[cfg(windows)]
    {
        windows_share::show(&app, file).await
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = (app, file);
        Err(AppError::Other("sharing is not available on this system".into()))
    }
}

/// The logs folder, for a support request. It takes no path, so it cannot be
/// pointed anywhere else. The current log is selected in it when there is one,
/// which goes the same way as a row's folder button; opening the bare folder
/// is only for the first minutes of an install that has not logged anything.
pub async fn open_log_dir() -> AppResult<()> {
    #[cfg(not(target_os = "android"))]
    {
        on_worker(|| {
            let dir = crate::paths::logs_dir()?;
            let log = dir.join("app.log");
            if log.is_file() {
                settle(
                    tauri_plugin_opener::reveal_item_in_dir(&log),
                    "the logs folder could not be shown",
                )
            } else {
                #[cfg(windows)]
                let _com = Com::sta();
                settle(
                    tauri_plugin_opener::open_path(&dir, None::<&str>),
                    "the logs folder could not be shown",
                )
            }
        })
        .await
    }
    #[cfg(target_os = "android")]
    {
        Err(AppError::Other("the logs folder is only shown on the desktop".into()))
    }
}

/// What a launch through the opener came to.
#[cfg(not(target_os = "android"))]
fn settle(result: Result<(), tauri_plugin_opener::Error>, doing: &str) -> AppResult<()> {
    match result {
        Ok(()) => Ok(()),
        Err(tauri_plugin_opener::Error::Io(err))
            if cfg!(windows) && err.raw_os_error() == Some(ERROR_CANCELLED) =>
        {
            Ok(())
        }
        // Gone in the moment between the check and the launch.
        Err(tauri_plugin_opener::Error::Io(err)) if gone(&err) => {
            Err(AppError::FileMissing(err.to_string()))
        }
        // `Error` is non_exhaustive.
        Err(err) => Err(AppError::Other(format!("{doing}: {err}"))),
    }
}

/// COM for this thread while the guard lives. Undone only if this call is the
/// one that did it (S_OK, or S_FALSE for a thread already in the same
/// apartment); a thread already in the other one (RPC_E_CHANGED_MODE) is left
/// as it was.
#[cfg(windows)]
struct Com(bool);

#[cfg(windows)]
impl Com {
    fn sta() -> Self {
        use windows::Win32::System::Com::{
            CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
        };
        // SAFETY: balanced by `Drop` on this same thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        Com(hr.is_ok())
    }
}

#[cfg(windows)]
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: pairs the successful CoInitializeEx above.
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ud-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_file_that_is_gone_is_missing_not_failed() {
        let dir = scratch("gone");
        let err = checked_file(&text(&dir.join("gone.mp4"))).unwrap_err();
        assert!(matches!(err, AppError::FileMissing(_)), "{err:?}");
        assert_eq!(err.code(), "fileMissing");
        assert!(!err.retryable());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_in_a_folder_that_is_gone_is_missing() {
        let dir = scratch("no-folder");
        let err = checked_file(&text(&dir.join("no-such-dir").join("clip.mp4"))).unwrap_err();
        assert_eq!(err.code(), "fileMissing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_is_refused_but_not_called_missing() {
        let dir = scratch("folder");
        let err = checked_file(&text(&dir)).unwrap_err();
        assert_ne!(err.code(), "fileMissing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Refused before anything is looked up: had the name been resolved
    /// against the working directory, the answer would have been "missing".
    #[test]
    fn a_relative_name_is_refused() {
        let mut names = vec!["clip.mp4", "./clip.mp4", "../clip.mp4"];
        if cfg!(windows) {
            names.extend([r"\clip.mp4", "C:clip.mp4"]);
        }
        for name in names {
            let err = checked_file(name).unwrap_err();
            assert_ne!(err.code(), "fileMissing", "{name}");
        }
        assert!(checked_file("   ").is_err());
    }

    #[test]
    fn an_existing_media_file_passes_both_checks() {
        let dir = scratch("present");
        let clip = dir.join("clip.mp4");
        std::fs::write(&clip, b"x").unwrap();
        let file = checked_file(&text(&clip)).unwrap();
        assert!(openable(&file).is_ok());
        assert!(handable(&format!("  {}  ", text(&clip))).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A container the app learns to write must be one its rows can open.
    #[test]
    fn every_format_the_app_writes_is_openable() {
        use crate::providers::detect::{AUDIO_EXTENSIONS, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS};
        let written = VIDEO_EXTENSIONS
            .iter()
            .chain(AUDIO_EXTENSIONS)
            .chain(IMAGE_EXTENSIONS)
            .chain(crate::converter::VIDEO_FORMATS.iter())
            .chain(crate::converter::AUDIO_FORMATS.iter())
            .chain(crate::export::CONTAINERS.iter());
        for ext in written {
            assert!(openable(Path::new(&format!("x.{ext}"))).is_ok(), "{ext}");
        }
    }

    #[test]
    fn programs_scripts_and_shortcuts_are_not_opened() {
        let names = [
            "setup.exe", "run.bat", "run.cmd", "x.com", "x.scr", "x.lnk", "x.url", "x.hta", "x.js",
            "x.vbs", "x.ps1", "x.msi", "x.reg", "x.cpl", "x.jar", "x.py", "x.apk", "x.html",
            "x.svg", "noext", "x.bin", "x.unknown_video", "x.mp4.exe", "x.",
        ];
        for name in names {
            let err = openable(Path::new(name)).unwrap_err();
            assert_ne!(err.code(), "fileMissing", "{name}");
        }
    }

    #[test]
    fn extension_case_does_not_matter() {
        assert!(openable(Path::new("CLIP.MP4")).is_ok());
        assert!(openable(Path::new("Photo.JPG")).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn a_trailing_dot_cannot_hide_a_program() {
        let dir = scratch("trailing-dot");
        std::fs::write(dir.join("evil.exe"), b"").unwrap();
        let file = checked_file(&format!(r"{}\evil.exe.", dir.display())).unwrap();
        assert_eq!(file.extension().and_then(|ext| ext.to_str()), Some("exe"));
        assert!(openable(&file).is_err());
        assert!(handable(&format!(r"{}\evil.exe. ", dir.display())).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn a_launch_that_found_nothing_is_missing() {
        let found_nothing = tauri_plugin_opener::Error::Io(io::Error::from_raw_os_error(2));
        assert_eq!(settle(Err(found_nothing), "x").unwrap_err().code(), "fileMissing");
    }

    #[cfg(windows)]
    #[test]
    fn closing_open_with_is_not_a_failure() {
        let cancelled = tauri_plugin_opener::Error::Io(io::Error::from_raw_os_error(ERROR_CANCELLED));
        assert!(settle(Err(cancelled), "x").is_ok());
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn any_other_refusal_is_not_called_missing() {
        let refused = settle(Err(tauri_plugin_opener::Error::UnsupportedPlatform), "x").unwrap_err();
        assert_eq!(refused.code(), "unknown");
        let denied = tauri_plugin_opener::Error::Io(io::Error::from_raw_os_error(5));
        assert_ne!(settle(Err(denied), "x").unwrap_err().code(), "fileMissing");
    }

    #[test]
    fn file_missing_reads_as_its_own_error() {
        let info = AppError::FileMissing("x".into()).to_info();
        assert_eq!(info.code, "fileMissing");
        assert!(!info.retryable);
        assert_eq!(info.title, "File moved or deleted");
    }
}
