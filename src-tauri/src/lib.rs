//! Application wiring.
//!
//! Everything that has to happen once -- opening the database, discovering the
//! external tools, restoring the queue, building the tray -- happens here, and
//! nothing else in the crate reaches for global state.

#[cfg(target_os = "android")]
pub mod android;
pub mod cache;
pub mod commands;
pub mod converter;
pub mod db;
pub mod downloader;
pub mod error;
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
pub mod settings;
pub mod tools;
#[cfg(desktop)]
pub mod tray;
pub mod util;

use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager};
#[cfg(desktop)]
use tauri::{Listener, WindowEvent};

use commands::AppState;
use converter::ConvertManager;
use db::Database;
use queue::QueueManager;

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

            app.manage(AppState {
                db: Arc::clone(&database),
                settings: Arc::clone(&settings),
                queue: Arc::clone(&queue),
                converter: Arc::clone(&converter),
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

            match paths::sweep_temp(TEMP_SWEEP_AGE_SECS) {
                Ok(count) if count > 0 => log_info!("app", "swept {count} stale temp files"),
                Err(err) => log_warn!("app", "temp sweep failed: {err}"),
                _ => {}
            }
            cache::enforce_limit(loaded.cache_limit_mb);

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
            commands::get_tools,
            commands::refresh_tools,
            commands::install_tool,
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
            commands::platform_pick_media_files,
            commands::platform_set_system_bars,
            commands::platform_take_shared_text,
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
