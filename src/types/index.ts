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
  /**
   * Whether this link can hand over just a slice of itself. Decided by the
   * backend from the protocol of the streams it would actually fetch, never by
   * a list of site names written down a second time over here.
   */
  rangeFetchable: boolean;
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
  /**
   * The video stream's own duration, which is not always the container's: a
   * file can carry 26 seconds of audio over 20 seconds of picture, and a
   * filmstrip cut to the container's length ends in black cells.
   */
  videoDurationSec: number | null;
  /**
   * The pixel's own shape, when the source does not use square ones. A 720x576
   * frame with a 64:45 pixel is a 16:9 picture, and anything that measures it
   * by its stored size alone is measuring the wrong rectangle.
   */
  pixelAspect: number | null;
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

// -- the editor ------------------------------------------------------------

/** One kept piece of the source, in seconds from its start. */
export interface EditSegment {
  startSec: number;
  endSec: number;
}

/**
 * The shape of the exported frame. `source` is not a ratio but the absence of
 * one: the frame is left exactly as it was found, which is the only setting a
 * lossless export can hold.
 */
export type AspectRatio = 'source' | '16:9' | '9:16' | '16:10' | '4:3' | '1:1';

/** What happens to a frame that is not the shape it is being poured into. */
export type FrameFit = 'fill' | 'fit';

/**
 * The root of every other export decision.
 *
 * `lossless` copies the streams through untouched -- seconds rather than
 * minutes, and the picture bit-for-bit the original -- but a copied video
 * stream can only begin at a keyframe, and nothing about the frame can change.
 * `reencode` decodes and encodes again, which is what buys a different shape,
 * rate, size or codec, and what makes a cut land on the frame it was asked for.
 */
export type ExportMode = 'lossless' | 'reencode';

export type VideoCodec = 'h264' | 'h265' | 'vp9' | 'av1';
export type AudioCodec = 'aac' | 'opus' | 'mp3' | 'flac';
export type ExportQuality = 'maximum' | 'high' | 'balanced' | 'small';

export interface ExportOptions {
  mode: ExportMode;
  /** Target container, lowercased and without the dot: mp4, mkv, webm, mov. */
  container: string;
  videoCodec: VideoCodec;
  quality: ExportQuality;
  /**
   * Target average video bitrate in kbps. Null encodes to the quality preset
   * (constant quality) instead. Re-encode only; a lossless copy ignores it.
   */
  videoBitrateKbps: number | null;
  /** Null keeps the source's rate. */
  fps: number | null;
  /** Cap in pixels of height. Null keeps the source's resolution. */
  maxHeight: number | null;
  aspect: AspectRatio;
  fit: FrameFit;
  /** Drop the audio entirely rather than encode it. */
  mute: boolean;
  /** Linear gain on the audio: 1 leaves it as it is, 0 to 2 is 0 to 200 %. */
  volume: number;
  audioCodec: AudioCodec;
  audioBitrateKbps: number | null;
  /** Map an HDR source down to Rec. 709 instead of letting it wash out. */
  toneMapSdr: boolean;
  hardware: boolean;
}

export interface ExportRequest {
  inputPath: string;
  /** In order, and non-overlapping. They are joined into one file. */
  segments: EditSegment[];
  options: ExportOptions;
  /**
   * Null means beside the source file -- unless the source is one of the app's
   * own files, which is every source on a phone and, on the desktop, a clip
   * fetched from a link into the app's temporary folder. Those go to the
   * download folder instead; `exportDefaultDir` says which, for the inspector.
   */
  outputDir: string | null;
}

export type ExportStatus = 'idle' | 'running' | 'completed' | 'failed' | 'canceled';

/** There is only ever one export, so this is a state rather than a list. */
export interface ExportState {
  status: ExportStatus;
  /** 0..100 while running; null once it is not. */
  percent: number | null;
  outputPath: string | null;
  error: AppErrorInfo | null;
}

// -- fetching a range off a link -------------------------------------------

export interface RangeFetchRequest {
  url: string;
  /** Null for the whole video; both are set together or not at all. */
  startSec: number | null;
  endSec: number | null;
  /** Cap the rendition's height, so a 4K source does not land as a 4K file. */
  maxHeight: number | null;
  /**
   * Re-encode at the cuts so the fetch begins on the frame that was asked for.
   * Costs roughly twice the time; without it the fetch starts at the keyframe
   * at or before the mark, which the editor can tidy up afterwards anyway.
   */
  exact: boolean;
  /** Null means the app's own temporary folder, which the editor then opens. */
  outputDir: string | null;
}

export type FetchStatus =
  | 'idle'
  | 'resolving'
  | 'fetching'
  | 'completed'
  | 'failed'
  | 'canceled';

export interface FetchState {
  status: FetchStatus;
  /**
   * 0..100 when the source reports enough to say. Null is not zero: a ranged
   * fetch through the engine reports nothing until it is over, and a bar that
   * sits at 0 for a minute is a lie a spinner would not tell.
   */
  percent: number | null;
  receivedBytes: number;
  title: string | null;
  outputPath: string | null;
  error: AppErrorInfo | null;
}

// -- what the timeline draws -----------------------------------------------

export type TimelineKind = 'waveform' | 'filmstrip';

export interface TimelineRequest {
  path: string;
  kind: TimelineKind;
  /** The window to read. Null start and length mean the whole file. */
  startSec: number | null;
  lengthSec: number | null;
  /** Buckets for a waveform, cells for a filmstrip. */
  count: number;
  /** Filmstrip only: the height of one cell, in device pixels. */
  cellHeight: number | null;
  /**
   * Echoed back untouched in the state. A zoom gesture supersedes the one
   * before it, and without this the slower answer would paint over the newer.
   */
  token: number;
}

export interface WaveformData {
  buckets: number;
  startSec: number;
  lengthSec: number;
  /**
   * Base64 of `2 * buckets` bytes: the minimum and maximum sample of each
   * bucket, interleaved, with 128 as silence. A JSON array of floats carries
   * the same information at nine times the size.
   */
  peaks: string;
}

export interface FilmstripData {
  /** Cells in the whole strip, which is `chunkFrames * chunks.length`. */
  frames: number;
  chunkFrames: number;
  cellWidth: number;
  cellHeight: number;
  startSec: number;
  lengthSec: number;
  /**
   * Tiled sprites, in order, each holding `chunkFrames` cells. They arrive one
   * at a time so the strip fills from the left rather than appearing at once.
   */
  chunks: string[];
}

/**
 * What the backend has drawn for the file the editor currently has open.
 *
 * `token` is echoed from the request: a zoom gesture supersedes the one before
 * it, and without this the slower answer would paint over the newer one.
 */
export interface TimelineState {
  path: string | null;
  token: number;
  working: boolean;
  waveform: WaveformData | null;
  filmstrip: FilmstripData | null;
  error: AppErrorInfo | null;
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

/** What asking for a newer release of a tool found. Asking downloads nothing. */
export interface ToolUpdateCheck {
  tool: ToolKind;
  installed: string | null;
  /** The newest release's version; for FFmpeg, the day its build went up. */
  latest: string | null;
  /** True only when the backend knows it. Anything it cannot compare is false. */
  upToDate: boolean;
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
