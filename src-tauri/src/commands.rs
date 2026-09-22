//! The IPC surface.
//!
//! Every command is thin: validate, delegate, return a typed value. Errors are
//! `AppError`, which serialises into the structured shape the UI knows how to
//! present, so no Rust error text ever reaches a user.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, State};

use crate::converter::ConvertManager;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::model::{
    BridgeStatus, CacheStats, ConvertFormatInfo, ConvertJob, ConvertRequest, DiagnosticsSnapshot,
    DownloadRequest, DownloadTask, HistoryEntry, MediaMetadata, MediaProbe, PlatformId,
    ToolInstallProgress, ToolKind, ToolsState, TrimRequest, TrimState,
};
use crate::queue::QueueManager;
use crate::trim::TrimManager;
use crate::settings::Settings;
use crate::{
    bridge, cache, converter, downloader, filename, log_warn, logging, net, paths, providers, tools,
    updater, util,
};

pub const EVENT_TOOL_PROGRESS: &str = "tools://progress";
pub const EVENT_TOOLS_CHANGED: &str = "tools://changed";
pub const EVENT_SETTINGS_CHANGED: &str = "settings://changed";
pub const EVENT_BRIDGE_CHANGED: &str = "bridge://changed";

pub struct AppState {
    pub db: Arc<Database>,
    pub settings: Arc<Mutex<Settings>>,
    pub queue: Arc<QueueManager>,
    pub converter: Arc<ConvertManager>,
    pub trimmer: Arc<TrimManager>,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

// -- settings --------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: Settings,
) -> AppResult<Settings> {
    settings.sanitize();

    let previous = state.settings();
    {
        let mut guard = state
            .settings
            .lock()
            .map_err(|_| AppError::Other("settings lock was poisoned".into()))?;
        *guard = settings.clone();
    }
    state.db.save_settings(&settings)?;

    logging::set_debug_enabled(settings.debug_logging);

    // Anything that changes how requests are made invalidates the pooled client.
    if previous.proxy_url != settings.proxy_url
        || previous.custom_user_agent != settings.custom_user_agent
        || previous.network_timeout_sec != settings.network_timeout_sec
    {
        net::invalidate();
    }

    // A changed tool path means the cached discovery result is stale, and the
    // interface has to hear about the new one: nothing else would tell it.
    if previous.engine_path != settings.engine_path || previous.ffmpeg_path != settings.ffmpeg_path {
        let _ = app.emit(EVENT_TOOLS_CHANGED, tools::refresh(&settings).await);
    }

    if previous.start_with_windows != settings.start_with_windows {
        apply_autostart(&app, settings.start_with_windows);
    }

    // Turning the browser link off has to reach the disk, not just this
    // process: the bridge host runs while the app is closed and reads the same
    // state to decide whether to accept a push. Unregistering as well means a
    // browser cannot even start the helper afterwards.
    if previous.browser_link_enabled != settings.browser_link_enabled {
        apply_browser_link(&app, settings.browser_link_enabled);
    }

    let _ = app.emit(EVENT_SETTINGS_CHANGED, settings.clone());
    Ok(settings)
}

#[tauri::command]
pub async fn reset_settings(app: AppHandle, state: State<'_, AppState>) -> AppResult<Settings> {
    let defaults = Settings {
        // Onboarding is a one-time thing; resetting preferences should not
        // replay it.
        onboarding_complete: state.settings().onboarding_complete,
        ..Settings::default()
    };
    save_settings(app, state, defaults).await
}

fn apply_autostart(app: &AppHandle, enabled: bool) {
    #[cfg(windows)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let manager = app.autolaunch();
        let _ = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
    }
    #[cfg(not(windows))]
    {
        let _ = (app, enabled);
    }
}

fn apply_browser_link(app: &AppHandle, enabled: bool) {
    if let Err(err) = bridge::set_enabled(enabled) {
        log_warn!("bridge", "could not record the link state: {err}");
    }

    let result = if enabled {
        bridge::register()
    } else {
        bridge::unregister()
    };
    if let Err(err) = result {
        log_warn!("bridge", "could not update the browser registration: {err}");
    }

    let _ = app.emit(EVENT_BRIDGE_CHANGED, ());
}

// -- browser link ----------------------------------------------------------

/// The Connection section polls this while it is on screen. It reads one small
/// file and stats another, and never decrypts the session, so polling is
/// cheaper than watching a file two processes write.
#[tauri::command]
pub fn bridge_status(app: AppHandle, state: State<'_, AppState>) -> BridgeStatus {
    bridge::status(&state.settings(), &app.package_info().version.to_string())
}

/// Repair: point every supported browser back at this installation's helper.
///
/// One button for every broken shape of the link -- a browser update that
/// cleared the value, a second copy of the app that claimed it, an uninstall
/// that took it -- because a user cannot tell those apart and does not have to.
#[tauri::command]
pub fn bridge_repair(app: AppHandle, state: State<'_, AppState>) -> AppResult<BridgeStatus> {
    bridge::register()?;
    let _ = app.emit(EVENT_BRIDGE_CHANGED, ());
    Ok(bridge::status(
        &state.settings(),
        &app.package_info().version.to_string(),
    ))
}

/// Forget the browser and drop the stored session. The registration stays, so
/// reconnecting is one press in the extension rather than a repair.
#[tauri::command]
pub fn bridge_disconnect(app: AppHandle, state: State<'_, AppState>) -> AppResult<BridgeStatus> {
    bridge::disconnect()?;
    let _ = app.emit(EVENT_BRIDGE_CHANGED, ());
    Ok(bridge::status(
        &state.settings(),
        &app.package_info().version.to_string(),
    ))
}

/// A support paste. Carries cookie names but never values: a jar without `SID`
/// is a signed-out jar, and that distinction is most of field diagnosis.
#[tauri::command]
pub fn bridge_diagnostics(app: AppHandle, state: State<'_, AppState>) -> String {
    bridge::diagnostics(&state.settings(), &app.package_info().version.to_string())
}

// -- tools -----------------------------------------------------------------

#[tauri::command]
pub async fn get_tools(state: State<'_, AppState>) -> AppResult<ToolsState> {
    Ok(tools::discovered(&state.settings()).await)
}

#[tauri::command]
pub async fn refresh_tools(app: AppHandle, state: State<'_, AppState>) -> AppResult<ToolsState> {
    let tools = tools::refresh(&state.settings()).await;
    let _ = app.emit(EVENT_TOOLS_CHANGED, tools.clone());
    Ok(tools)
}

#[tauri::command]
pub async fn install_tool(
    app: AppHandle,
    state: State<'_, AppState>,
    tool: ToolKind,
) -> AppResult<ToolsState> {
    let settings = state.settings();
    let emitter = app.clone();

    let on_progress = move |received: u64, total: Option<u64>, stage: &str| {
        let _ = emitter.emit(
            EVENT_TOOL_PROGRESS,
            ToolInstallProgress {
                tool,
                received_bytes: received,
                total_bytes: total,
                stage: stage.to_string(),
            },
        );
    };

    let result = tools::install(tool, &settings, &on_progress).await;

    // Published on failure too. Every screen that shows a tool reads it from
    // this event, and a failed update can still have changed what is on disk.
    let tools = tools::snapshot();
    let _ = app.emit(EVENT_TOOLS_CHANGED, tools.clone());

    result?;
    Ok(tools)
}

// -- analysis --------------------------------------------------------------

/// Cheap, synchronous, and called on every keystroke in the URL field, so the
/// platform indicator updates without a round trip to any network.
#[tauri::command]
pub fn detect_platform(url: String) -> PlatformId {
    providers::detect::detect_platform(&url)
}

#[tauri::command]
pub async fn analyze_url(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> AppResult<MediaMetadata> {
    let settings = state.settings();
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidUrl("no address was given".into()));
    }
    let metadata = match providers::analyze(trimmed, &settings).await {
        Ok(metadata) => metadata,
        Err(err) => return Err(net::explain_failure(&app, err).await),
    };
    providers::remember_analysis(trimmed, &settings, &metadata);
    Ok(metadata)
}

#[tauri::command]
pub async fn get_thumbnail(state: State<'_, AppState>, url: String) -> AppResult<String> {
    cache::thumbnail_data_url(&url, &state.settings()).await
}

/// What the current option set would actually download.
///
/// The UI sends the metadata it already holds back for this: running the real
/// `plan::build` is the only way for the displayed quality, container and size
/// to be guaranteed to match what the download will do. A second copy of the
/// selection rules in TypeScript would drift the first time either changed.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub label: String,
    pub quality_label: String,
    pub container: String,
    pub needs_merge: bool,
    pub needs_ffmpeg: bool,
    pub estimated_bytes: Option<u64>,
    pub video_format_id: Option<String>,
    pub audio_format_id: Option<String>,
    pub stage_count: u32,
}

#[tauri::command]
pub fn summarize_plan(
    metadata: crate::model::MediaMetadata,
    request: DownloadRequest,
) -> AppResult<PlanSummary> {
    let plan = crate::downloader::plan::build(
        &metadata,
        request.mode,
        request.quality,
        request.video_format_id.as_deref(),
        request.audio_format_id.as_deref(),
        request.container.as_deref(),
        request.watermark,
    )?;

    // The engine merges for itself, so only a native two-stream download or a
    // conversion actually requires FFmpeg to be present.
    let needs_ffmpeg =
        (plan.needs_merge && !plan.needs_engine) || plan.convert_to.is_some();

    Ok(PlanSummary {
        label: plan.label.clone(),
        quality_label: plan.quality_label.clone(),
        container: plan.container.clone(),
        needs_merge: plan.needs_merge,
        needs_ffmpeg,
        estimated_bytes: plan.estimated_bytes,
        video_format_id: plan.video.as_ref().map(|f| f.id.clone()),
        audio_format_id: plan.audio.as_ref().map(|f| f.id.clone()),
        stage_count: plan.stage_count(),
    })
}

// -- queue -----------------------------------------------------------------

#[tauri::command]
pub fn list_downloads(state: State<'_, AppState>) -> Vec<DownloadTask> {
    state.queue.list()
}

#[tauri::command]
pub fn enqueue_download(state: State<'_, AppState>, request: DownloadRequest) -> DownloadTask {
    state.queue.enqueue(request)
}

/// Queue every item of a carousel, gallery or album as its own task.
///
/// Items are addressed by position within the link: a carousel's items have
/// no addresses of their own. The analysis the user is looking at already
/// lists them, so it is what the tasks are made from, and what they download.
#[tauri::command]
pub async fn enqueue_gallery(
    state: State<'_, AppState>,
    request: DownloadRequest,
) -> AppResult<Vec<DownloadTask>> {
    let settings = state.settings();
    let metadata = match providers::recent_analysis(&request.url, &settings) {
        Some(metadata) => metadata,
        None => {
            let metadata = providers::analyze(&request.url, &settings).await?;
            providers::remember_analysis(&request.url, &settings, &metadata);
            metadata
        }
    };

    if metadata.entries.len() < 2 {
        return Ok(vec![state.queue.enqueue(request)]);
    }

    // One creation time for the whole album: the Downloads screen lists newest
    // first, so entries stamped one by one would read backwards there.
    let created_at = util::now_ms();
    Ok(metadata
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let mut item = request.clone();
            item.entry = Some(index as u32 + 1);
            item.title = Some(entry.title.clone());
            item.thumbnail_url = entry.thumbnail_url.clone().or_else(|| request.thumbnail_url.clone());
            // Streams picked by hand belong to the item they were picked on.
            if index > 0 {
                item.video_format_id = None;
                item.audio_format_id = None;
            }
            state.queue.enqueue_at(item, created_at)
        })
        .collect())
}

#[tauri::command]
pub fn pause_download(state: State<'_, AppState>, id: String) {
    state.queue.pause(&id);
}

#[tauri::command]
pub fn resume_download(state: State<'_, AppState>, id: String) {
    state.queue.resume(&id);
}

#[tauri::command]
pub fn cancel_download(state: State<'_, AppState>, id: String) {
    state.queue.cancel(&id);
}

#[tauri::command]
pub fn retry_download(state: State<'_, AppState>, id: String) {
    state.queue.retry(&id);
}

#[tauri::command]
pub fn remove_download(state: State<'_, AppState>, id: String) {
    state.queue.remove(&id);
}

#[tauri::command]
pub fn pause_all_downloads(state: State<'_, AppState>) {
    state.queue.pause_all();
}

#[tauri::command]
pub fn resume_all_downloads(state: State<'_, AppState>) {
    state.queue.resume_all();
}

#[tauri::command]
pub fn clear_finished_downloads(state: State<'_, AppState>) {
    state.queue.clear_finished();
}

#[tauri::command]
pub fn reorder_download(state: State<'_, AppState>, id: String, delta: i32) {
    state.queue.reorder(&id, delta);
}

#[tauri::command]
pub fn set_download_order(state: State<'_, AppState>, ids: Vec<String>) {
    state.queue.set_order(&ids);
}

// -- conversion ------------------------------------------------------------

/// The formats the converter can write. Served rather than hard-coded in the
/// UI so the list on screen is the list the backend will actually accept.
#[tauri::command]
pub fn convert_formats() -> Vec<ConvertFormatInfo> {
    converter::catalogue()
}

/// Read a local file so the UI can show what it is before offering to convert
/// it. Also the point at which an unreadable or non-media file is rejected.
#[tauri::command]
pub async fn probe_media(path: String) -> AppResult<MediaProbe> {
    converter::probe(path.trim()).await
}

#[tauri::command]
pub fn list_conversions(state: State<'_, AppState>) -> Vec<ConvertJob> {
    state.converter.list()
}

#[tauri::command]
pub async fn enqueue_conversions(
    state: State<'_, AppState>,
    request: ConvertRequest,
) -> AppResult<Vec<ConvertJob>> {
    state.converter.enqueue(request).await
}

#[tauri::command]
pub fn cancel_conversion(state: State<'_, AppState>, id: String) {
    state.converter.cancel(&id);
}

#[tauri::command]
pub fn retry_conversion(state: State<'_, AppState>, id: String) {
    state.converter.retry(&id);
}

#[tauri::command]
pub fn remove_conversion(state: State<'_, AppState>, id: String) {
    state.converter.remove(&id);
}

// -- trimming --------------------------------------------------------------

#[tauri::command]
pub fn trim_state(state: State<'_, AppState>) -> TrimState {
    state.trimmer.state()
}

/// Cut the marked range out of the open file. Replaces a cut already running.
#[tauri::command]
pub async fn start_trim(state: State<'_, AppState>, request: TrimRequest) -> AppResult<()> {
    state.trimmer.start(request).await
}

#[tauri::command]
pub fn cancel_trim(state: State<'_, AppState>) {
    state.trimmer.cancel();
}

/// Let the webview read one file, so a `<video>` element can play it.
///
/// The asset protocol is enabled with an empty scope: nothing on disk is
/// readable until something here says so, and what says so is the user having
/// picked that file in a dialog or dropped it on the window. Granting the
/// whole file system once at build time would have been one line of config and
/// a standing offer to every page the webview ever loads.
#[cfg(not(target_os = "android"))]
#[tauri::command]
pub fn allow_media_preview(app: AppHandle, path: String) -> AppResult<()> {
    use tauri::Manager;

    let file = std::path::PathBuf::from(path.trim());
    if !file.is_file() {
        return Err(AppError::Io(format!("{} is not a file", file.display())));
    }
    app.asset_protocol_scope()
        .allow_file(&file)
        .map_err(|err| AppError::Other(format!("that file could not be opened for preview: {err}")))
}

#[tauri::command]
pub fn clear_finished_conversions(state: State<'_, AppState>) {
    state.converter.clear_finished();
}

// -- history ---------------------------------------------------------------

#[tauri::command]
pub fn list_history(
    state: State<'_, AppState>,
    query: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> AppResult<Vec<HistoryEntry>> {
    let query = query.filter(|value| !value.trim().is_empty());
    state
        .db
        .history_list(query.as_deref(), limit.unwrap_or(100).min(500), offset.unwrap_or(0))
}

#[tauri::command]
pub fn count_history(state: State<'_, AppState>) -> AppResult<u32> {
    state.db.history_count()
}

#[tauri::command]
pub fn delete_history_entry(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.db.history_delete(id)
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> AppResult<()> {
    state.db.history_clear()?;
    state.db.vacuum()
}

// -- filesystem ------------------------------------------------------------

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

/// Live preview for the file-name template field in Settings.
#[tauri::command]
pub fn preview_filename(template: String) -> String {
    let rendered = filename::render_template(
        &template,
        &filename::NameContext {
            title: "How It Works",
            creator: Some("Creator"),
            quality: "1080p",
            platform: "youtube",
            date: &chrono::Local::now().format("%Y-%m-%d").to_string(),
            ext: "mp4",
        },
    );
    format!("{rendered}.mp4")
}

// -- mobile platform -------------------------------------------------------
//
// What the opener, dialog and drag-and-drop APIs do on the desktop has to be
// asked of the OS on Android. The interface only calls these there; elsewhere
// they refuse rather than pretend.

#[cfg(not(target_os = "android"))]
fn android_only<T>() -> AppResult<T> {
    Err(AppError::Other("this is only available on Android".into()))
}

#[tauri::command]
pub async fn platform_open_file(app: AppHandle, path: String) -> AppResult<()> {
    #[cfg(target_os = "android")]
    return crate::android::open_file(app, path).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, path);
        android_only()
    }
}

#[tauri::command]
pub async fn platform_open_downloads(app: AppHandle) -> AppResult<()> {
    #[cfg(target_os = "android")]
    return crate::android::open_downloads(app).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        android_only()
    }
}

#[tauri::command]
pub async fn platform_open_app_settings(app: AppHandle) -> AppResult<()> {
    #[cfg(target_os = "android")]
    return crate::android::open_app_settings(app).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        android_only()
    }
}

#[tauri::command]
pub async fn platform_pick_media_files(app: AppHandle) -> AppResult<Vec<String>> {
    #[cfg(target_os = "android")]
    return crate::android::pick_media_files(app).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        android_only()
    }
}

#[tauri::command]
pub async fn platform_set_system_bars(app: AppHandle, dark: bool) -> AppResult<()> {
    #[cfg(target_os = "android")]
    return crate::android::set_system_bars(app, dark).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, dark);
        Ok(())
    }
}

#[tauri::command]
pub async fn platform_take_shared_text(app: AppHandle) -> AppResult<Option<String>> {
    #[cfg(target_os = "android")]
    return crate::android::take_shared_text(app).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(None)
    }
}

// -- app updates -----------------------------------------------------------

/// Whether a newer build has been released. `None` when this one is current,
/// and always for a copy that no installer made.
#[tauri::command]
pub async fn check_app_update(state: State<'_, AppState>) -> AppResult<Option<updater::AppUpdate>> {
    let settings = state.settings();
    // Offline or on a stalled network this runs while the app is in use, so it
    // is bounded rather than left to the transport timeouts.
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        updater::check(&settings),
    )
    .await
    .unwrap_or_else(|_| Err(AppError::Network("the update check timed out".into())));

    // The automatic check says nothing on screen when it fails -- being
    // offline is not worth interrupting anyone over -- so the log is the only
    // place a check that never succeeds can be seen at all.
    if let Err(err) = &outcome {
        log_warn!("updater", "the update check failed: {err}");
    }
    outcome
}

/// The commit this build was made from. Releases carry no version number, so
/// this is what the About page names a build by; empty when it is unknown.
#[tauri::command]
pub fn get_build_commit() -> &'static str {
    updater::build_commit()
}

/// Download a newer build. The phone then opens the system installer on it;
/// the desktop only stages the file, verified, for `apply_app_update` to run
/// once nothing would be interrupted by it.
#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    state: State<'_, AppState>,
    update: updater::AppUpdate,
) -> AppResult<()> {
    #[cfg(any(target_os = "android", windows))]
    {
        let settings = state.settings();
        let emitter = app.clone();
        let on_progress = move |received: u64, total: Option<u64>, _stage: &str| {
            let _ = emitter.emit(
                updater::EVENT_UPDATE_PROGRESS,
                updater::UpdateProgress {
                    received_bytes: received,
                    total_bytes: total,
                },
            );
        };

        #[cfg(target_os = "android")]
        {
            let apk =
                updater::download(&update, &crate::android::update_dir(), &settings, &on_progress)
                    .await?;
            crate::android::install_apk(app, apk.to_string_lossy().into_owned()).await
        }
        #[cfg(windows)]
        {
            updater::download(&update, &paths::updates_dir()?, &settings, &on_progress).await?;
            Ok(())
        }
    }
    #[cfg(not(any(target_os = "android", windows)))]
    {
        let _ = (app, state, update);
        no_self_update()
    }
}

/// Replace the running app with the build `install_app_update` staged: start
/// its installer, then leave so the files are free to be replaced. Does not
/// return when it works -- the installer starts the new build when it is done.
#[tauri::command]
pub async fn apply_app_update(
    app: AppHandle,
    state: State<'_, AppState>,
    update: updater::AppUpdate,
) -> AppResult<()> {
    #[cfg(windows)]
    {
        use tauri::Manager;

        let installer = updater::staged(&update, &paths::updates_dir()?)?;

        // A window the user has put away in the tray comes back put away.
        let minimized = app
            .get_webview_window("main")
            .is_some_and(|window| !window.is_visible().unwrap_or(true));
        updater::launch_installer(&installer, minimized)?;
        crate::log_info!("updater", "handing over to {}", installer.display());

        // The same way out as Quit in the tray menu. Closing the window would
        // not do: with close-to-tray on that only hides it, and the process
        // the installer needs gone would still be holding its files.
        state.queue.shutdown();
        state.converter.shutdown();
        state.trimmer.shutdown();
        app.exit(0);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, state, update);
        no_self_update()
    }
}

#[cfg(not(windows))]
fn no_self_update<T>() -> AppResult<T> {
    Err(AppError::Other("this build does not install its own updates".into()))
}

// -- cache and diagnostics -------------------------------------------------

#[tauri::command]
pub fn cache_stats() -> AppResult<CacheStats> {
    cache::stats()
}

#[tauri::command]
pub fn clear_cache() -> AppResult<()> {
    cache::clear()
}

#[tauri::command]
pub fn get_diagnostics(app: AppHandle, state: State<'_, AppState>) -> AppResult<DiagnosticsSnapshot> {
    let settings = state.settings();
    let snapshot = tools::snapshot();

    Ok(DiagnosticsSnapshot {
        app_version: app.package_info().version.to_string(),
        os: os_label(),
        engine: snapshot.engine,
        ffmpeg: snapshot.ffmpeg,
        // A support paste that does not say whether a JavaScript runtime was
        // present cannot explain the one failure this tool exists for.
        #[cfg(not(target_os = "android"))]
        js_runtime: snapshot.js_runtime,
        download_dir: settings.download_dir,
        db_path: paths::database_path()?.to_string_lossy().into_owned(),
        log_path: paths::logs_dir()?.to_string_lossy().into_owned(),
        active_downloads: state.queue.active_count(),
        queued_downloads: state.queue.queued_count(),
    })
}

/// How the operating system calls itself, the way a person would write it.
///
/// `std::env::consts::OS` beside the raw version reads "windows 10.0.26200",
/// which is wrong twice over: Windows 11 still reports major version 10, so
/// that string names the wrong Windows, and nobody says their system in
/// lowercase with a three-part number. `os_info` has already done both pieces
/// of work -- it reads the edition out of the registry and checks the build
/// against the 22000 that separates 11 from 10 -- so the name comes from
/// there. The build number is kept, because it is the part of that string a
/// support paste is actually for.
fn os_label() -> String {
    let info = os_info::get();
    let name = info
        .edition()
        .map(str::to_owned)
        .unwrap_or_else(|| info.os_type().to_string());

    match info.version() {
        os_info::Version::Unknown => name,
        // The edition already names the release on Windows; all the version
        // adds that the name does not is the build.
        os_info::Version::Semantic(_, _, build) if cfg!(windows) => {
            format!("{name} (build {build})")
        }
        // Everywhere else the version is the release. Android reports "16",
        // which arrives here as 16.0.0, and trailing zeroes nobody typed are
        // not information.
        os_info::Version::Semantic(major, minor, patch) => {
            let mut number = major.to_string();
            if *minor != 0 || *patch != 0 {
                number.push_str(&format!(".{minor}"));
            }
            if *patch != 0 {
                number.push_str(&format!(".{patch}"));
            }
            format!("{name} {number}")
        }
        version => format!("{name} {version}"),
    }
}

#[tauri::command]
pub fn get_log_dir() -> AppResult<String> {
    Ok(paths::logs_dir()?.to_string_lossy().into_owned())
}

/// Third-party licence list, read from the bundled resource.
///
/// Android keeps bundled resources inside the APK, where no file path reaches
/// them, so that build carries the list compiled in instead.
#[cfg(target_os = "android")]
#[tauri::command]
pub fn get_licenses(app: AppHandle) -> AppResult<serde_json::Value> {
    let _ = app;
    Ok(serde_json::from_str(include_str!("../resources/licenses.json"))?)
}

/// Third-party licence list, read from the bundled resource.
#[cfg(not(target_os = "android"))]
#[tauri::command]
pub fn get_licenses(app: AppHandle) -> AppResult<serde_json::Value> {
    use tauri::Manager;

    let path = app
        .path()
        .resolve("resources/licenses.json", tauri::path::BaseDirectory::Resource)
        .map_err(|err| AppError::Other(format!("could not locate the licence list: {err}")))?;
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

#[tauri::command]
pub fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Used by the developer panel to confirm what the temp sweep reclaimed.
#[tauri::command]
pub fn sweep_temp_files() -> AppResult<u64> {
    paths::sweep_temp(0)
}

#[tauri::command]
pub fn provisional_download_label(request: DownloadRequest) -> String {
    downloader::provisional_label(&request)
}

#[tauri::command]
pub fn new_task_id() -> String {
    util::new_id("ui")
}
