import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  AppErrorInfo,
  AppUpdate,
  BridgeStatus,
  CacheStats,
  ConvertFormatInfo,
  ConvertJob,
  ConvertRequest,
  ConvertStatus,
  DiagnosticsSnapshot,
  DownloadRequest,
  DownloadTask,
  HistoryEntry,
  LicenseEntry,
  MediaMetadata,
  MediaProbe,
  PlatformId,
  Settings,
  ToolInstallProgress,
  ToolKind,
  ToolUpdateCheck,
  ToolsState,
  UpdateProgress,
  ExportRequest,
  ExportState,
  FetchState,
  RangeFetchRequest,
  TimelineRequest,
  TimelineState,
} from '@/types';

/**
 * Typed wrappers around the Rust commands.
 *
 * Nothing else in the app calls `invoke` directly: keeping the boundary in one
 * file means a renamed command breaks the build here rather than at runtime in
 * a component.
 */

/** Rust returns a structured error object; anything else is a real crash. */
export function isAppError(value: unknown): value is AppErrorInfo {
  return (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    'title' in value &&
    'message' in value
  );
}

export function toAppError(value: unknown): AppErrorInfo {
  if (isAppError(value)) return value;
  return {
    code: 'unknown',
    title: 'Something went wrong',
    message: 'The operation could not be completed.',
    technical: value instanceof Error ? value.message : String(value),
    retryable: true,
  };
}

// -- settings --------------------------------------------------------------

export const getSettings = () => invoke<Settings>('get_settings');
export const saveSettings = (settings: Settings) =>
  invoke<Settings>('save_settings', { settings });
export const resetSettings = () => invoke<Settings>('reset_settings');

// -- tools -----------------------------------------------------------------

export const getTools = () => invoke<ToolsState>('get_tools');
export const refreshTools = () => invoke<ToolsState>('refresh_tools');
export const installTool = (tool: ToolKind) => invoke<ToolsState>('install_tool', { tool });
/** Asks the tool's release page; downloads nothing. */
export const checkToolUpdate = (tool: ToolKind) =>
  invoke<ToolUpdateCheck>('check_tool_update', { tool });

// -- browser link ----------------------------------------------------------
//
// Windows only, like the bridge itself. Reading the status is cheap enough to
// poll -- Rust reads one small JSON file and stats another -- which is what the
// Connection section does while it is on screen.

export const bridgeStatus = () => invoke<BridgeStatus>('bridge_status');
export const bridgeRepair = () => invoke<BridgeStatus>('bridge_repair');
export const bridgeDisconnect = () => invoke<BridgeStatus>('bridge_disconnect');
/** A support paste. Carries cookie names, never their values. */
export const bridgeDiagnostics = () => invoke<string>('bridge_diagnostics');

// -- analysis --------------------------------------------------------------

export const detectPlatform = (url: string) => invoke<PlatformId>('detect_platform', { url });
export const analyzeUrl = (url: string) => invoke<MediaMetadata>('analyze_url', { url });
export const getThumbnail = (url: string) => invoke<string>('get_thumbnail', { url });

/**
 * What the current options would actually download.
 *
 * The metadata the UI already holds is sent back so Rust can run its real
 * selection logic; that keeps the displayed quality, container and size in
 * lockstep with the download itself.
 */
export interface PlanSummary {
  label: string;
  qualityLabel: string;
  container: string;
  needsMerge: boolean;
  needsFfmpeg: boolean;
  estimatedBytes: number | null;
  videoFormatId: string | null;
  audioFormatId: string | null;
  stageCount: number;
}

export const summarizePlan = (metadata: MediaMetadata, request: DownloadRequest) =>
  invoke<PlanSummary>('summarize_plan', { metadata, request });

// -- queue -----------------------------------------------------------------

export const listDownloads = () => invoke<DownloadTask[]>('list_downloads');
export const enqueueDownload = (request: DownloadRequest) =>
  invoke<DownloadTask>('enqueue_download', { request });
export const enqueueGallery = (request: DownloadRequest) =>
  invoke<DownloadTask[]>('enqueue_gallery', { request });
export const pauseDownload = (id: string) => invoke<void>('pause_download', { id });
export const resumeDownload = (id: string) => invoke<void>('resume_download', { id });
export const cancelDownload = (id: string) => invoke<void>('cancel_download', { id });
export const retryDownload = (id: string) => invoke<void>('retry_download', { id });
export const removeDownload = (id: string) => invoke<void>('remove_download', { id });
export const pauseAllDownloads = () => invoke<void>('pause_all_downloads');
export const resumeAllDownloads = () => invoke<void>('resume_all_downloads');
export const clearFinishedDownloads = () => invoke<void>('clear_finished_downloads');
export const reorderDownload = (id: string, delta: number) =>
  invoke<void>('reorder_download', { id, delta });
export const setDownloadOrder = (ids: string[]) => invoke<void>('set_download_order', { ids });

// -- conversion ------------------------------------------------------------

export const convertFormats = () => invoke<ConvertFormatInfo[]>('convert_formats');
export const probeMedia = (path: string) => invoke<MediaProbe>('probe_media', { path });
export const listConversions = () => invoke<ConvertJob[]>('list_conversions');
export const enqueueConversions = (request: ConvertRequest) =>
  invoke<ConvertJob[]>('enqueue_conversions', { request });
export const cancelConversion = (id: string) => invoke<void>('cancel_conversion', { id });
export const retryConversion = (id: string) => invoke<void>('retry_conversion', { id });
export const removeConversion = (id: string) => invoke<void>('remove_conversion', { id });
export const clearFinishedConversions = () => invoke<void>('clear_finished_conversions');

// -- the editor ------------------------------------------------------------
//
// Three managers rather than one, because the three things the editor waits for
// are unrelated: a fetch and an export can be asked for in either order, and a
// timeline redraw happens constantly while both of them are idle. One shared
// state would have had cancelling either one clear the other.

export const exportState = () => invoke<ExportState>('export_state');
export const startExport = (request: ExportRequest) => invoke<void>('start_export', { request });
export const cancelExport = () => invoke<void>('cancel_export');
/**
 * The folder an export of `path` lands in when none is chosen, or null when
 * that is beside it. Only the backend knows which folders are the app's own.
 */
export const exportDefaultDir = (path: string) =>
  invoke<string | null>('export_default_dir', { path });

/**
 * Where a copied stream is actually allowed to begin, in seconds.
 *
 * Read once per opened file and only when it matters -- a lossless export is
 * the one thing that cannot land between two of these, and the timeline shows
 * the user where the cut will really fall rather than letting them find out
 * afterwards.
 */
export const mediaKeyframes = (path: string) => invoke<number[]>('media_keyframes', { path });

export const fetchState = () => invoke<FetchState>('fetch_state');
export const startRangeFetch = (request: RangeFetchRequest) =>
  invoke<void>('start_range_fetch', { request });
export const cancelRangeFetch = () => invoke<void>('cancel_range_fetch');

export const timelineState = () => invoke<TimelineState>('timeline_state');
export const requestTimeline = (request: TimelineRequest) =>
  invoke<void>('request_timeline', { request });
export const cancelTimeline = () => invoke<void>('cancel_timeline');

/**
 * One frame, as a data URI. For the files the window cannot decode: the strip
 * and the marks still work, so the editor loses the moving picture and nothing
 * else rather than becoming a lesser screen.
 */
export const frameAt = (path: string, seconds: number, height: number) =>
  invoke<string>('frame_at', { path, seconds, height });

/**
 * Let the webview read one file so a `<video>` element can play it. The asset
 * protocol starts with an empty scope, so nothing is readable until a file the
 * user picked is named here.
 */
export const allowMediaPreview = (path: string) =>
  invoke<void>('allow_media_preview', { path });

// -- history ---------------------------------------------------------------

export const listHistory = (query?: string, limit?: number, offset?: number) =>
  invoke<HistoryEntry[]>('list_history', { query, limit, offset });
export const countHistory = () => invoke<number>('count_history');
export const deleteHistoryEntry = (id: number) => invoke<void>('delete_history_entry', { id });
export const clearHistory = () => invoke<void>('clear_history');

// -- misc ------------------------------------------------------------------

export const pathExists = (path: string) => invoke<boolean>('path_exists', { path });
export const previewFilename = (template: string) =>
  invoke<string>('preview_filename', { template });
export const cacheStats = () => invoke<CacheStats>('cache_stats');
export const clearCache = () => invoke<void>('clear_cache');
export const getDiagnostics = () => invoke<DiagnosticsSnapshot>('get_diagnostics');
export const getLogDir = () => invoke<string>('get_log_dir');
export const getLicenses = () => invoke<{ packages: LicenseEntry[] }>('get_licenses');
export const sweepTempFiles = () => invoke<number>('sweep_temp_files');

// -- mobile platform -------------------------------------------------------
//
// Only called on Android (see `IS_MOBILE`); the desktop has plugin APIs for
// the same jobs.

export const platformOpenFile = (path: string) => invoke<void>('platform_open_file', { path });
export const platformOpenDownloads = () => invoke<void>('platform_open_downloads');
/** The app's page in the system settings, where its network access is allowed. */
export const platformOpenAppSettings = () => invoke<void>('platform_open_app_settings');
export const platformPickMediaFiles = () => invoke<string[]>('platform_pick_media_files');
export const platformSetSystemBars = (dark: boolean) =>
  invoke<void>('platform_set_system_bars', { dark });
export const platformTakeSharedText = () => invoke<string | null>('platform_take_shared_text');

/** Dispatched by the Android side when a link is shared into a running app. */
export const SHARED_TEXT_EVENT = 'ud-shared-text';

// -- app updates -----------------------------------------------------------

/** A newer build, or null when this one is current or never updates itself. */
export const checkAppUpdate = () => invoke<AppUpdate | null>('check_app_update');
/**
 * Downloads the build. On the phone this resolves once the system installer
 * has been opened on it; on the desktop, once the installer is staged and
 * verified, which changes nothing until `applyAppUpdate`.
 */
export const installAppUpdate = (update: AppUpdate) =>
  invoke<void>('install_app_update', { update });
/** Desktop only. Starts the staged installer and exits the app. */
export const applyAppUpdate = (update: AppUpdate) =>
  invoke<void>('apply_app_update', { update });
/** The commit this build was made from; empty when it was built outside git. */
export const getBuildCommit = () => invoke<string>('get_build_commit');

// -- events ----------------------------------------------------------------

export const EVENTS = {
  progress: 'download://progress',
  queueChanged: 'download://changed',
  toolProgress: 'tools://progress',
  toolsChanged: 'tools://changed',
  settingsChanged: 'settings://changed',
  bridgeChanged: 'bridge://changed',
  convertChanged: 'convert://changed',
  convertProgress: 'convert://progress',
  exportChanged: 'editor://export',
  fetchChanged: 'editor://fetch',
  timelineChanged: 'editor://timeline',
  updateProgress: 'update://progress',
  navigate: 'navigate',
} as const;

export interface ProgressEvent {
  id: string;
  status: DownloadTask['status'];
  progress: DownloadTask['progress'];
  outputPath: string | null;
  error: AppErrorInfo | null;
}

export function onProgress(handler: (event: ProgressEvent) => void): Promise<UnlistenFn> {
  return listen<ProgressEvent>(EVENTS.progress, (event) => handler(event.payload));
}

export function onQueueChanged(handler: (tasks: DownloadTask[]) => void): Promise<UnlistenFn> {
  return listen<DownloadTask[]>(EVENTS.queueChanged, (event) => handler(event.payload));
}

export function onToolProgress(
  handler: (progress: ToolInstallProgress) => void,
): Promise<UnlistenFn> {
  return listen<ToolInstallProgress>(EVENTS.toolProgress, (event) => handler(event.payload));
}

export function onToolsChanged(handler: (tools: ToolsState) => void): Promise<UnlistenFn> {
  return listen<ToolsState>(EVENTS.toolsChanged, (event) => handler(event.payload));
}

export function onSettingsChanged(handler: (settings: Settings) => void): Promise<UnlistenFn> {
  return listen<Settings>(EVENTS.settingsChanged, (event) => handler(event.payload));
}

/**
 * The browser link changed. The event carries nothing on purpose: the state it
 * announces is written by two processes, so the only trustworthy version is the
 * one a fresh `bridgeStatus()` reads back.
 */
export function onBridgeChanged(handler: () => void): Promise<UnlistenFn> {
  return listen(EVENTS.bridgeChanged, () => handler());
}

export interface ConvertProgressEvent {
  id: string;
  status: ConvertStatus;
  percent: number | null;
  outputPath: string | null;
  outputSizeBytes: number | null;
  streamCopied: boolean;
  error: AppErrorInfo | null;
}

export function onConvertChanged(handler: (jobs: ConvertJob[]) => void): Promise<UnlistenFn> {
  return listen<ConvertJob[]>(EVENTS.convertChanged, (event) => handler(event.payload));
}

export function onExportChanged(handler: (state: ExportState) => void): Promise<UnlistenFn> {
  return listen<ExportState>(EVENTS.exportChanged, (event) => handler(event.payload));
}

export function onFetchChanged(handler: (state: FetchState) => void): Promise<UnlistenFn> {
  return listen<FetchState>(EVENTS.fetchChanged, (event) => handler(event.payload));
}

export function onTimelineChanged(
  handler: (state: TimelineState) => void,
): Promise<UnlistenFn> {
  return listen<TimelineState>(EVENTS.timelineChanged, (event) => handler(event.payload));
}

export function onConvertProgress(
  handler: (event: ConvertProgressEvent) => void,
): Promise<UnlistenFn> {
  return listen<ConvertProgressEvent>(EVENTS.convertProgress, (event) => handler(event.payload));
}

export function onUpdateProgress(
  handler: (progress: UpdateProgress) => void,
): Promise<UnlistenFn> {
  return listen<UpdateProgress>(EVENTS.updateProgress, (event) => handler(event.payload));
}

export function onNavigate(handler: (route: string) => void): Promise<UnlistenFn> {
  return listen<string>(EVENTS.navigate, (event) => handler(event.payload));
}
