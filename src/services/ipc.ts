import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  AppErrorInfo,
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
  ToolsState,
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
export const installTool = (tool: 'engine' | 'ffmpeg') =>
  invoke<ToolsState>('install_tool', { tool });

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
export const getAppVersion = () => invoke<string>('get_app_version');
export const sweepTempFiles = () => invoke<number>('sweep_temp_files');

// -- mobile platform -------------------------------------------------------
//
// Only called on Android (see `IS_MOBILE`); the desktop has plugin APIs for
// the same jobs.

export const platformOpenFile = (path: string) => invoke<void>('platform_open_file', { path });
export const platformOpenDownloads = () => invoke<void>('platform_open_downloads');
export const platformPickMediaFiles = () => invoke<string[]>('platform_pick_media_files');
export const platformSetSystemBars = (dark: boolean) =>
  invoke<void>('platform_set_system_bars', { dark });
export const platformTakeSharedText = () => invoke<string | null>('platform_take_shared_text');

/** Dispatched by the Android side when a link is shared into a running app. */
export const SHARED_TEXT_EVENT = 'ud-shared-text';

// -- events ----------------------------------------------------------------

export const EVENTS = {
  progress: 'download://progress',
  queueChanged: 'download://changed',
  toolProgress: 'tools://progress',
  toolsChanged: 'tools://changed',
  settingsChanged: 'settings://changed',
  convertChanged: 'convert://changed',
  convertProgress: 'convert://progress',
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

export function onConvertProgress(
  handler: (event: ConvertProgressEvent) => void,
): Promise<UnlistenFn> {
  return listen<ConvertProgressEvent>(EVENTS.convertProgress, (event) => handler(event.payload));
}

export function onNavigate(handler: (route: string) => void): Promise<UnlistenFn> {
  return listen<string>(EVENTS.navigate, (event) => handler(event.payload));
}
