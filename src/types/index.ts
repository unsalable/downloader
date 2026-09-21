/**
 * The contract between the React layer and the Rust core.
 *
 * Every shape here has a `#[serde(rename_all = "camelCase")]` counterpart in
 * `src-tauri/src/model.rs`. Changing one side without the other is a bug; the
 * type names are deliberately identical to make the pairing easy to check.
 */

export type PlatformId =
  | 'youtube'
  | 'tiktok'
  | 'instagram'
  | 'twitter'
  | 'reddit'
  | 'facebook'
  | 'twitch'
  | 'pinterest'
  | 'vimeo'
  | 'dailymotion'
  | 'soundcloud'
  | 'direct'
  | 'generic'
  | 'unknown';

export type MediaKind = 'video' | 'audio' | 'image' | 'gallery';

export type FormatKind = 'muxed' | 'video' | 'audio' | 'image';

/**
 * Whether a watermark-free download is actually obtainable for this media.
 * The app never strips a watermark itself -- it only reports whether the
 * source offers a clean rendition.
 */
export type WatermarkSupport = 'notApplicable' | 'cleanAvailable' | 'watermarkedOnly';

export interface MediaFormat {
  id: string;
  kind: FormatKind;
  container: string;
  protocol: string;
  hasVideo: boolean;
  hasAudio: boolean;
  width: number | null;
  height: number | null;
  fps: number | null;
  vcodec: string | null;
  acodec: string | null;
  /** kbps, as reported by the source. */
  tbr: number | null;
  vbr: number | null;
  abr: number | null;
  /** Exact size when the source reports one. */
  filesize: number | null;
  /** Source's own estimate, used only when `filesize` is absent. */
  filesizeApprox: number | null;
  /** Short human label: "1080p", "320 kbps", "2160x2160". */
  qualityLabel: string;
  /** True/false when known; null when the source does not say. */
  watermarked: boolean | null;
  note: string | null;
  /** Set when the stream must be fetched through the external engine. */
  needsEngineDownload: boolean;
}

export interface MediaMetadata {
  url: string;
  canonicalUrl: string;
  platform: PlatformId;
  platformLabel: string;
  providerId: string;
  mediaKind: MediaKind;
  title: string;
  creator: string | null;
  description: string | null;
  thumbnailUrl: string | null;
  durationSec: number | null;
  viewCount: number | null;
  likeCount: number | null;
  uploadDate: string | null;
  isLive: boolean;
  formats: MediaFormat[];
  /** Populated for carousels, galleries and playlists. */
  entryCount: number | null;
  watermarkSupport: WatermarkSupport;
  /** Non-fatal notes worth surfacing, already localised keys where possible. */
  warnings: string[];
}

export type DownloadMode = 'video' | 'audio' | 'image';

export type QualityPreference =
  | { type: 'best' }
  | { type: 'auto' }
  | { type: 'maxHeight'; height: number }
  | { type: 'audioBitrate'; kbps: number };

export type WatermarkPreference = 'any' | 'cleanOnly';

export interface DownloadRequest {
  url: string;
  mode: DownloadMode;
  quality: QualityPreference;
  /** Explicit stream ids, set by the advanced format selector. */
  videoFormatId: string | null;
  audioFormatId: string | null;
  /** Target container. Null means "keep whatever the source gives". */
  container: string | null;
  watermark: WatermarkPreference;
  outputDir: string | null;
  /** Carried through so the queue can render before metadata is re-fetched. */
  title: string | null;
  thumbnailUrl: string | null;
  platform: PlatformId | null;
  /**
   * 1-based position of one item of a carousel or gallery; null for the link
   * as a whole. Set by the backend when it queues a gallery item by item.
   */
  entry?: number | null;
}

export type DownloadStatus =
  | 'queued'
  | 'preparing'
  | 'downloading'
  | 'processing'
  | 'paused'
  | 'completed'
  | 'failed'
  | 'canceled';

export type DownloadStage =
  | 'waiting'
  | 'resolving'
  | 'video'
  | 'audio'
  | 'image'
  | 'merging'
  | 'converting'
  | 'finalizing'
  | 'done';

export interface DownloadProgress {
  receivedBytes: number;
  totalBytes: number | null;
  /** 0..100. Null while the total is still unknown. */
  percent: number | null;
  /** Smoothed bytes/sec -- see `speed.rs`; not a raw per-tick delta. */
  speedBps: number;
  etaSec: number | null;
  stage: DownloadStage;
  /** 1-based, for "step 2 of 3" style copy on multi-stream downloads. */
  stageIndex: number;
  stageCount: number;
  resumable: boolean;
}

/** A user-facing failure. `technical` is only shown behind "View details". */
export interface AppErrorInfo {
  code: string;
  title: string;
  message: string;
  technical: string | null;
  retryable: boolean;
}

export interface DownloadTask {
  id: string;
  url: string;
  title: string;
  platform: PlatformId;
  thumbnailUrl: string | null;
  status: DownloadStatus;
  progress: DownloadProgress;
  /** Short description of the chosen output, e.g. "1080p - MP4". */
  formatLabel: string;
  outputPath: string | null;
  error: AppErrorInfo | null;
  createdAt: number;
  startedAt: number | null;
  completedAt: number | null;
  attempt: number;
  request: DownloadRequest;
}

export interface HistoryEntry {
  id: number;
  url: string;
  title: string;
  platform: PlatformId;
  thumbnailUrl: string | null;
  filePath: string;
  fileExists: boolean;
  container: string;
  qualityLabel: string;
  fileSize: number | null;
  createdAt: number;
  status: 'completed' | 'failed';
  request: DownloadRequest | null;
}

// -- conversion ------------------------------------------------------------

/** Whether a target format produces a video file or an audio-only one. */
export type ConvertKind = 'video' | 'audio';

export type ConvertStatus = 'queued' | 'running' | 'completed' | 'failed' | 'canceled';

/** One entry in the catalogue of targets the backend will accept. */
export interface ConvertFormatInfo {
  id: string;
  kind: ConvertKind;
}

/** What a local file actually contains, read before anything is offered for it. */
export interface MediaProbe {
  path: string;
  fileName: string;
  container: string;
  sizeBytes: number;
  durationSec: number | null;
  width: number | null;
  height: number | null;
  fps: number | null;
  videoCodec: string | null;
  audioCodec: string | null;
  audioBitrateKbps: number | null;
  hasVideo: boolean;
  hasAudio: boolean;
}

/** How hard FFmpeg should work when a re-encode cannot be avoided. */
export type ConvertQuality = 'high' | 'balanced' | 'small';

export interface ConvertOptions {
  targetFormat: string;
  /** Null means "beside the source file". */
  outputDir: string | null;
  quality: ConvertQuality;
  /** Downscale cap in pixels of height. Null keeps the source resolution. */
  maxHeight: number | null;
  audioBitrateKbps: number | null;
  /** Repackage instead of re-encoding when the container allows it. */
  allowStreamCopy: boolean;
}

export interface ConvertRequest {
  inputPaths: string[];
  options: ConvertOptions;
}

export interface ConvertJob {
  id: string;
  inputPath: string;
  inputName: string;
  inputSizeBytes: number;
  outputPath: string | null;
  outputSizeBytes: number | null;
  kind: ConvertKind;
  status: ConvertStatus;
  /** 0..100 while running; null when the duration could not be read. */
  percent: number | null;
  durationSec: number | null;
  /** True when the file was repackaged rather than re-encoded. */
  streamCopied: boolean;
  error: AppErrorInfo | null;
  createdAt: number;
  startedAt: number | null;
  completedAt: number | null;
  options: ConvertOptions;
}

export type ThemePreference = 'dark' | 'light' | 'system';
export type LanguageCode = 'en' | 'tr';

export interface Settings {
  // General
  startWithWindows: boolean;
  minimizeToTray: boolean;
  closeToTray: boolean;
  clipboardMonitoring: boolean;
  notificationsEnabled: boolean;
  notifyOnComplete: boolean;
  notifyOnError: boolean;

  // Downloads
  downloadDir: string;
  defaultMode: DownloadMode;
  defaultQuality: QualityPreference;
  defaultContainer: string | null;
  maxConcurrentDownloads: number;
  autoRetryCount: number;
  filenameTemplate: string;

  // Appearance
  theme: ThemePreference;
  language: LanguageCode;
  reduceMotion: boolean;
  showAnimatedBackground: boolean;

  // Performance
  lowResourceMode: boolean;
  hardwareAcceleration: boolean;
  cacheLimitMb: number;

  // Advanced
  ffmpegPath: string | null;
  enginePath: string | null;
  networkTimeoutSec: number;
  proxyUrl: string | null;
  customUserAgent: string | null;
  debugLogging: boolean;

  // Connection. Desktop only: the phone has no browser to link to, and the
  // Connection section is hidden there rather than showing a dead toggle.
  browserLinkEnabled: boolean;

  // Hotkeys, stored as accelerator strings ("Ctrl+Shift+D").
  hotkeys: Record<HotkeyAction, string>;

  /** Set once the welcome screen has been dismissed. */
  onboardingComplete: boolean;
}

export type HotkeyAction =
  | 'pasteUrl'
  | 'download'
  | 'openDownloads'
  | 'openHistory'
  | 'openSettings';

export type ToolKind = 'engine' | 'ffmpeg' | 'jsRuntime';

/** State of the external tools the app shells out to. */
export interface ToolStatus {
  name: ToolKind;
  available: boolean;
  path: string | null;
  version: string | null;
  source: 'bundled' | 'managed' | 'system' | 'custom' | 'missing';
}

export interface ToolsState {
  engine: ToolStatus;
  ffmpeg: ToolStatus;
  /**
   * Desktop only. Android carries its own JavaScript engine inside the APK, so
   * there is nothing to install there and the field arrives unavailable.
   */
  jsRuntime: ToolStatus;
}

export interface ToolInstallProgress {
  tool: ToolKind;
  receivedBytes: number;
  totalBytes: number | null;
  stage: 'downloading' | 'extracting' | 'verifying' | 'done';
}

/** A newer build of the phone app, found on the release page. */
export interface AppUpdate {
  commit: string;
  assetUrl: string;
  assetSize: number;
  digest: string | null;
  publishedAt: string | null;
}

export interface UpdateProgress {
  receivedBytes: number;
  totalBytes: number | null;
}

export interface CacheStats {
  thumbnailCount: number;
  thumbnailBytes: number;
  metadataCount: number;
  totalBytes: number;
}

export interface DiagnosticsSnapshot {
  appVersion: string;
  os: string;
  engine: ToolStatus;
  ffmpeg: ToolStatus;
  downloadDir: string;
  dbPath: string;
  logPath: string;
  activeDownloads: number;
  queuedDownloads: number;
}

/**
 * How old the session the browser lent us is. `stale` is not an error: the
 * session is still stored, it is simply too old to be trusted, and saying so
 * separately is what lets the interface explain a quiet connection instead of
 * failing at the next members-only link with nothing to show for it.
 */
export type BridgeSessionState = 'none' | 'fresh' | 'stale';

/**
 * Everything the app knows about the browser link without decrypting anything.
 * Nothing here describes a cookie -- see the header of `bridge/protocol.rs`.
 */
export interface BridgeStatus {
  supported: boolean;
  /** Whether there is a published extension to send the user to. */
  storeListed: boolean;
  enabled: boolean;
  /** False when the browsers on this machine can no longer start our helper. */
  registered: boolean;
  connected: boolean;
  browser: string | null;
  profileLabel: string | null;
  accountHint: string | null;
  extensionVersion: string | null;
  /** Seconds since the epoch, not milliseconds: it comes from the host. */
  lastPushAt: number | null;
  session: BridgeSessionState;
  hostPath: string | null;
  appVersion: string;
  extensionId: string;
}

export interface LicenseEntry {
  name: string;
  version: string;
  license: string;
  kind: 'npm' | 'cargo' | 'font' | 'asset' | 'external-tool';
  url: string;
  note?: string;
}
