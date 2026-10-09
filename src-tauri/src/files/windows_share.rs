//! The Windows share sheet. A desktop app has no CoreWindow, so the sheet is
//! reached through IDataTransferManagerInterop, keyed by the window's handle.
//! Three rules shape this file:
//!
//! - Both interop calls must be made on the thread that owns the window -- the
//!   main thread. The command runs on a worker and sends them there, which is
//!   also why `share_file` must stay `async`: a sync command already runs on
//!   the main thread and would wait on itself forever.
//! - The sheet asks for its content through DataRequested. One handler is
//!   registered per window and kept, with its manager and its token, for the
//!   window's life; it takes the file from `PENDING`. Registering one per press
//!   would stack them, every one of them answering each later request.
//! - Turning a path into a StorageFile is asynchronous, and blocking the
//!   window's thread on it can deadlock an STA. A deferral holds the sheet open
//!   while a worker thread does it.
//!
//! A panic here would end the app -- release builds abort rather than unwind --
//! so nothing in it unwraps, and the main-thread slot is borrowed with
//! `try_borrow_mut`.

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Manager, WebviewWindow};
use windows::core::{Interface, Ref, HSTRING};
use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
use windows::Foundation::TypedEventHandler;
use windows::Storage::{IStorageItem, StorageFile};
use windows::Win32::UI::Shell::IDataTransferManagerInterop;
use windows_collections::IIterable;

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::log_warn;

/// The file the next request hands over, and what the sheet says if it cannot.
struct Pending {
    file: PathBuf,
    failure: String,
}

/// One file at a time. Two rows shared within the same few milliseconds would
/// both open on the second file; a row ignores its own second press while the
/// first is under way, which is the case that actually happens.
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

/// The handler's registration on one window.
struct Registration {
    /// The window's handle, which is what says the window is still the same one.
    window: isize,
    /// Never released with the thread. The main thread's locals are dropped as
    /// the process exits, under the loader lock, and a release then would reach
    /// into a share broker that is being torn down around it. It is let go of
    /// only when a new window takes its place.
    manager: ManuallyDrop<DataTransferManager>,
    token: i64,
}

thread_local! {
    /// Main thread only.
    static REGISTERED: RefCell<Option<Registration>> = const { RefCell::new(None) };
}

fn pending() -> MutexGuard<'static, Option<Pending>> {
    PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Open the sheet over the main window with `file` in it. Resolves once the
/// sheet has been asked for; what the user does with it is the sheet's own
/// business, and closing it is not a failure.
pub async fn show(app: &AppHandle, file: PathBuf) -> AppResult<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::Other("the main window is not open".into()))?;
    let failure = failure_text(app, &file);
    *pending() = Some(Pending { file, failure });
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(present(&window));
    })
    .map_err(|err| AppError::Other(format!("the share sheet could not be asked for: {err}")))?;
    let outcome = rx
        .await
        .unwrap_or_else(|_| Err("the main thread dropped the request".into()));
    if let Err(err) = outcome {
        pending().take();
        return Err(AppError::Other(format!("the share sheet could not be opened: {err}")));
    }
    Ok(())
}

/// The sheet shows this itself, inside its own window, so it is said in the
/// language the app is in rather than in the backend's English.
fn failure_text(app: &AppHandle, file: &Path) -> String {
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let language = app
        .try_state::<AppState>()
        .map(|state| state.settings().language)
        .unwrap_or_default();
    match language.as_str() {
        "tr" => format!("{name} paylaşılamadı"),
        _ => format!("{name} couldn't be shared"),
    }
}

fn present(window: &WebviewWindow) -> Result<(), String> {
    // tauri's own `windows` 0.61 HWND. A compile error here means tauri has
    // moved on to another `windows`: move the one in Cargo.toml with it.
    let hwnd = window.hwnd().map_err(|err| err.to_string())?;
    let interop = windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>()
        .map_err(|err| err.to_string())?;
    REGISTERED.with(|slot| -> Result<(), String> {
        let mut slot = slot
            .try_borrow_mut()
            .map_err(|_| "a share is already being set up".to_string())?;
        let handle = hwnd.0 as isize;
        if slot.as_ref().map(|registered| registered.window) != Some(handle) {
            // A window made again has a new handle, and the old manager's
            // handler would never be asked for anything again.
            if let Some(old) = slot.take() {
                let _ = old.manager.RemoveDataRequested(old.token);
                drop(ManuallyDrop::into_inner(old.manager));
            }
            // SAFETY: called on the window's own thread with its live handle.
            let manager: DataTransferManager =
                unsafe { interop.GetForWindow(hwnd) }.map_err(|err| err.to_string())?;
            let token = manager
                .DataRequested(&TypedEventHandler::new(on_data_requested))
                .map_err(|err| err.to_string())?;
            *slot = Some(Registration {
                window: handle,
                manager: ManuallyDrop::new(manager),
                token,
            });
        }
        Ok(())
    })?;
    // SAFETY: as above.
    unsafe { interop.ShowShareUIForWindow(hwnd) }.map_err(|err| err.to_string())
}

fn on_data_requested(
    _: Ref<'_, DataTransferManager>,
    args: Ref<'_, DataRequestedEventArgs>,
) -> windows::core::Result<()> {
    let Some(args) = args.as_ref() else { return Ok(()) };
    // Nothing asked for (another way into the sheet): the sheet says so itself.
    let Some(Pending { file, failure }) = pending().take() else { return Ok(()) };
    let request = args.Request()?;
    let package = request.Data()?;
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Without a title the sheet refuses the package outright.
    package.Properties()?.SetTitle(&HSTRING::from(name.as_str()))?;
    let deferral = request.GetDeferral()?;
    std::thread::spawn(move || {
        if let Err(err) = storage_items(&file).and_then(|items| package.SetStorageItems(&items, true)) {
            log_warn!("share", "{} could not be put on the share sheet: {err}", file.display());
            let _ = request.FailWithDisplayText(&HSTRING::from(failure.as_str()));
        }
        let _ = deferral.Complete();
    });
    Ok(())
}

/// The file as the sheet takes it. Apart from the handler so a test can run it.
fn storage_items(path: &Path) -> windows::core::Result<IIterable<IStorageItem>> {
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path))?.get()?;
    Ok(vec![Some(file.cast::<IStorageItem>()?)].into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ud-share-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// On a plain thread, as the deferral's worker is; no sheet is shown.
    #[test]
    fn a_saved_file_becomes_one_share_item() {
        let dir = scratch("present");
        let clip = dir.join("clip.mp4");
        std::fs::write(&clip, b"x").unwrap();
        let items = storage_items(&clip).unwrap();
        assert!(items.First().unwrap().HasCurrent().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_fails_to_resolve() {
        let dir = scratch("gone");
        let Err(err) = storage_items(&dir.join("gone.mp4")) else {
            panic!("a file that is not there was resolved");
        };
        assert_eq!(err.code().0 as u32, 0x8007_0002);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
