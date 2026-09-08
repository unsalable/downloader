//! System notifications.
//!
//! Deliberately quiet: only completion and failure, only when the user has left
//! them enabled, and never for routine state changes like a download starting.

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::model::{AppErrorInfo, HistoryEntry};
use crate::settings::Settings;

fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_string()
}

pub fn download_complete(app: &AppHandle, settings: &Settings, entry: &HistoryEntry) {
    if !settings.notifications_enabled || !settings.notify_on_complete {
        return;
    }

    let _ = app
        .notification()
        .builder()
        .title("Download complete")
        .body(file_name(&entry.file_path))
        .show();
}

pub fn download_failed(app: &AppHandle, settings: &Settings, title: &str, error: &AppErrorInfo) {
    if !settings.notifications_enabled || !settings.notify_on_error {
        return;
    }

    // The notification carries the same plain-language summary the UI shows,
    // never the technical detail.
    let _ = app
        .notification()
        .builder()
        .title("Download failed")
        .body(format!("{title}\n{}", error.title))
        .show();
}
