//! Notification-area icon and menu.
//!
//! Built in Rust rather than declared in the config, because the menu carries a
//! live download count and its labels follow the app's language setting.

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::AppState;
use crate::error::{AppError, AppResult};

pub const TRAY_ID: &str = "main-tray";

const ID_OPEN: &str = "tray-open";
const ID_STATUS: &str = "tray-status";
const ID_PAUSE_ALL: &str = "tray-pause-all";
const ID_RESUME_ALL: &str = "tray-resume-all";
const ID_SETTINGS: &str = "tray-settings";
const ID_QUIT: &str = "tray-quit";

/// Keeps handles to the items whose text changes at runtime.
pub struct TrayHandles<R: Runtime> {
    status: MenuItem<R>,
}

pub struct TrayState<R: Runtime>(pub Mutex<Option<TrayHandles<R>>>);

struct Labels {
    open: &'static str,
    pause_all: &'static str,
    resume_all: &'static str,
    settings: &'static str,
    quit: &'static str,
}

fn labels(language: &str) -> Labels {
    match language {
        "tr" => Labels {
            open: "Universal Downloader'ı aç",
            pause_all: "Tümünü duraklat",
            resume_all: "Tümünü sürdür",
            settings: "Ayarlar",
            quit: "Çıkış",
        },
        _ => Labels {
            open: "Open Universal Downloader",
            pause_all: "Pause all",
            resume_all: "Resume all",
            settings: "Settings",
            quit: "Quit",
        },
    }
}

fn status_text(language: &str, active: u32, queued: u32) -> String {
    match language {
        "tr" => format!("Aktif indirme: {active} · Sırada: {queued}"),
        _ => format!("Active downloads: {active} · Queued: {queued}"),
    }
}

pub fn build<R: Runtime>(app: &AppHandle<R>, language: &str) -> AppResult<()> {
    let text = labels(language);

    let status = MenuItem::with_id(app, ID_STATUS, status_text(language, 0, 0), false, None::<&str>)
        .map_err(tray_error)?;
    let open = MenuItem::with_id(app, ID_OPEN, text.open, true, None::<&str>).map_err(tray_error)?;
    let pause_all =
        MenuItem::with_id(app, ID_PAUSE_ALL, text.pause_all, true, None::<&str>).map_err(tray_error)?;
    let resume_all = MenuItem::with_id(app, ID_RESUME_ALL, text.resume_all, true, None::<&str>)
        .map_err(tray_error)?;
    let settings =
        MenuItem::with_id(app, ID_SETTINGS, text.settings, true, None::<&str>).map_err(tray_error)?;
    let quit = MenuItem::with_id(app, ID_QUIT, text.quit, true, None::<&str>).map_err(tray_error)?;

    let separator = PredefinedMenuItem::separator(app).map_err(tray_error)?;
    let separator2 = PredefinedMenuItem::separator(app).map_err(tray_error)?;

    let menu = Menu::with_items(
        app,
        &[
            &status,
            &separator,
            &open,
            &pause_all,
            &resume_all,
            &separator2,
            &settings,
            &quit,
        ],
    )
    .map_err(tray_error)?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| AppError::Other("the application icon is missing".into()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Universal Downloader")
        .menu(&menu)
        // Left click opens the window; the menu belongs on right click, which is
        // what Windows users expect from a tray icon.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            ID_OPEN => show_main_window(app),
            ID_PAUSE_ALL => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.queue.pause_all();
                }
            }
            ID_RESUME_ALL => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.queue.resume_all();
                }
            }
            ID_SETTINGS => {
                show_main_window(app);
                use tauri::Emitter;
                let _ = app.emit("navigate", "settings");
            }
            ID_QUIT => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.queue.shutdown();
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(tray_error)?;

    app.manage(TrayState(Mutex::new(Some(TrayHandles { status }))));
    Ok(())
}

/// Refresh the count shown at the top of the tray menu.
pub fn update_counts<R: Runtime>(app: &AppHandle<R>, language: &str, active: u32, queued: u32) {
    let Some(state) = app.try_state::<TrayState<R>>() else {
        return;
    };
    let Ok(guard) = state.0.lock() else { return };
    let Some(handles) = guard.as_ref() else { return };
    let _ = handles.status.set_text(status_text(language, active, queued));

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let tooltip = if active + queued == 0 {
            "Universal Downloader".to_string()
        } else {
            format!("Universal Downloader — {active} active, {queued} queued")
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

fn tray_error(err: tauri::Error) -> AppError {
    AppError::Other(format!("the tray icon could not be created: {err}"))
}
