//! Application wiring.
//!
//! Everything that has to happen once -- opening the database, discovering the
//! external tools, restoring the queue, building the tray -- happens here, and
//! nothing else in the crate reaches for global state.

#[cfg(target_os = "android")]
pub mod android;
pub mod bridge;
pub mod cache;
pub mod commands;
pub mod converter;
pub mod db;
pub mod downloader;
pub mod editor_media;
pub mod error;
pub mod export;
pub mod ffmpeg;
pub mod filename;
pub mod logging;
pub mod model;
pub mod net;
pub mod notify;
pub mod paths;
pub mod process;
pub mod providers;
pub mod queue;
pub mod range;
pub mod settings;
pub mod tools;
#[cfg(desktop)]
pub mod tray;
pub mod updater;
pub mod util;

use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager};
#[cfg(desktop)]
use tauri::{Listener, WindowEvent};

use commands::AppState;
use converter::ConvertManager;
use db::Database;
use editor_media::TimelineManager;
use export::ExportManager;
use queue::QueueManager;
use range::RangeFetchManager;

/// Temp files older than a day are leftovers from a crash, not resumable state.
const TEMP_SWEEP_AGE_SECS: u64 = 60 * 60 * 24;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(windows)]
    {
        // A tray application must not start a second copy: the new process
        // would fight the old one over the queue database.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tray::show_main_window(app);
        }));
        builder = builder.plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ));
    }

    #[cfg(target_os = "android")]
    {
        builder = builder.plugin(android::plugin());
    }

    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_os::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // The data and download directories come from the OS on Android,
            // and everything below reads them.
            #[cfg(target_os = "android")]
            android::init(&handle)?;

            let database = Arc::new(Database::open(&paths::database_path()?)?);
            let loaded = database.load_settings().unwrap_or_default();
            logging::set_debug_enabled(loaded.debug_logging);
            log_info!("app", "starting version {}", app.package_info().version);

            let settings = Arc::new(Mutex::new(loaded.clone()));
            let queue = QueueManager::new(handle.clone(), Arc::clone(&database), Arc::clone(&settings));

            queue.restore();
            queue.spawn_scheduler();

            // Conversions are not restored across a restart: an interrupted
            // encode has no resumable state, and re-running one the user did not
            // ask for again would burn a CPU at sign-in.
            let converter = ConvertManager::new(handle.clone(), Arc::clone(&settings));
            converter.spawn_scheduler();

            // An export is one file at a time, started from a screen the user
            // is looking at, so there is nothing to schedule and nothing to
            // restore -- only somewhere for the running one to live.
            let exporter = ExportManager::new(handle.clone(), Arc::clone(&settings));

            // The timeline is a third manager rather than part of the export,
            // because the two wait for unrelated things: a redraw happens
            // constantly while nothing is being exported, and cancelling one
            // must not be cancelling the other.
            let timeline = TimelineManager::new(handle.clone(), Arc::clone(&settings));

            // Bringing a link in is a fourth, because it waits on the network
            // rather than on a processor and is the only one of them that can
            // be running while the user is still deciding what to do with what
            // it brings.
            let fetcher = RangeFetchManager::new(handle.clone(), Arc::clone(&settings));

            app.manage(AppState {
                db: Arc::clone(&database),
                settings: Arc::clone(&settings),
                queue: Arc::clone(&queue),
                converter: Arc::clone(&converter),
                exporter: Arc::clone(&exporter),
                timeline: Arc::clone(&timeline),
                fetcher: Arc::clone(&fetcher),
            });

            #[cfg(desktop)]
            if let Err(err) = tray::build(&handle, &loaded.language) {
                log_error!("app", "tray unavailable: {err}");
            }

            #[cfg(target_os = "android")]
            android::keep_alive_while_busy(&handle);

            // Keep the tray's count in step with the queue without polling.
            #[cfg(desktop)]
            {
                let handle = handle.clone();
                let language = loaded.language.clone();
                let queue = Arc::clone(&queue);
                app.listen(queue::EVENT_CHANGED, move |_| {
                    tray::update_counts(
                        &handle,
                        &language,
                        queue.active_count(),
                        queue.queued_count(),
                    );
                });
            }

            // Tool discovery runs a subprocess per tool, so it happens off the
            // startup path; the UI renders a "checking" state until it lands.
            // `get_tools` waits for this same pass, so an interface that asks
            // before it finishes cannot be handed the placeholder.
            {
                let handle = handle.clone();
                let settings = loaded.clone();
                tauri::async_runtime::spawn(async move {
                    let state = tools::discovered(&settings).await;
                    log_info!(
                        "app",
                        "engine={} ffmpeg={}",
                        state.engine.available,
                        state.ffmpeg.available
                    );
                    let _ = handle.emit(commands::EVENT_TOOLS_CHANGED, state);
                });
            }

            // The browser link, which is registered on every launch rather than
            // at install time: a browser update, a cleaner or a second copy of
            // the app can all quietly take the registry value, and rewriting it
            // here is what heals that without the user knowing it broke.
            #[cfg(windows)]
            if loaded.browser_link_enabled {
                if let Err(err) = bridge::register() {
                    log_warn!("app", "browser link unavailable: {err}");
                }
            }
            // A lease that outlived its engine process is never resumable
            // state, unlike a partial download, so it goes unconditionally.
            bridge::sweep_leases();

            match paths::sweep_temp(TEMP_SWEEP_AGE_SECS) {
                Ok(count) if count > 0 => log_info!("app", "swept {count} stale temp files"),
                Err(err) => log_warn!("app", "temp sweep failed: {err}"),
                _ => {}
            }
            cache::enforce_limit(loaded.cache_limit_mb);

            // The installer that produced this build has done its job. Until
            // the update happened it was kept, so that a restart in between
            // did not mean downloading it again.
            #[cfg(windows)]
            if let Ok(dir) = paths::updates_dir() {
                updater::sweep_applied(&dir);
            }

            // Launched by the autostart entry: stay in the tray rather than
            // stealing focus during sign-in.
            #[cfg(desktop)]
            let start_hidden = std::env::args().any(|arg| arg == "--minimized");
            #[cfg(mobile)]
            let start_hidden = false;
            if !start_hidden {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                }
            }

            Ok(())
        });

    // Minimising and closing to the tray are desktop ideas; a phone manages the
    // window's lifetime itself.
    #[cfg(desktop)]
    let builder = builder.on_window_event(handle_window_event);

    builder
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::reset_settings,
            commands::bridge_status,
            commands::bridge_repair,
            commands::bridge_disconnect,
            commands::bridge_diagnostics,
            commands::get_tools,
            commands::refresh_tools,
            commands::install_tool,
            commands::check_tool_update,
            commands::detect_platform,
            commands::analyze_url,
            commands::get_thumbnail,
            commands::summarize_plan,
            commands::list_downloads,
            commands::enqueue_download,
            commands::enqueue_gallery,
            commands::pause_download,
            commands::resume_download,
            commands::cancel_download,
            commands::retry_download,
            commands::remove_download,
            commands::pause_all_downloads,
            commands::resume_all_downloads,
            commands::clear_finished_downloads,
            commands::reorder_download,
            commands::set_download_order,
            commands::convert_formats,
            commands::export_state,
            commands::start_export,
            commands::cancel_export,
            commands::export_default_dir,
            commands::media_keyframes,
            commands::fetch_state,
            commands::start_range_fetch,
            commands::cancel_range_fetch,
            commands::timeline_state,
            commands::request_timeline,
            commands::cancel_timeline,
            commands::frame_at,
            commands::allow_media_preview,
            commands::probe_media,
            commands::list_conversions,
            commands::enqueue_conversions,
            commands::cancel_conversion,
            commands::retry_conversion,
            commands::remove_conversion,
            commands::clear_finished_conversions,
            commands::list_history,
            commands::count_history,
            commands::delete_history_entry,
            commands::clear_history,
            commands::path_exists,
            commands::preview_filename,
            commands::cache_stats,
            commands::clear_cache,
            commands::get_diagnostics,
            commands::get_log_dir,
            commands::get_licenses,
            commands::get_app_version,
            commands::sweep_temp_files,
            commands::provisional_download_label,
            commands::new_task_id,
            commands::platform_open_file,
            commands::platform_open_downloads,
            commands::platform_open_app_settings,
            commands::platform_pick_media_files,
            commands::platform_set_system_bars,
            commands::platform_take_shared_text,
            commands::check_app_update,
            commands::get_build_commit,
            commands::install_app_update,
            commands::apply_app_update,
        ])
        .run(tauri::generate_context!())
        .expect("the application failed to start");
}

#[cfg(desktop)]
fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            let close_to_tray = window
                .app_handle()
                .try_state::<AppState>()
                .map(|state| state.settings().close_to_tray)
                .unwrap_or(false);

            if close_to_tray {
                api.prevent_close();
                let _ = window.hide();
            } else if let Some(state) = window.app_handle().try_state::<AppState>() {
                state.queue.shutdown();
                state.converter.shutdown();
                state.exporter.shutdown();
                state.timeline.shutdown();
                state.fetcher.shutdown();
            }
        }
        WindowEvent::Resized(_) => {
            let Some(state) = window.app_handle().try_state::<AppState>() else {
                return;
            };
            if state.settings().minimize_to_tray && window.is_minimized().unwrap_or(false) {
                let _ = window.hide();
            }
        }
        _ => {}
    }
}
