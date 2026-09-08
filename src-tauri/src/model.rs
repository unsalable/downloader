//! Serialised types shared with the React layer.
//!
//! Every type here has a hand-written counterpart in `src/types/index.ts`.
//! They are kept structurally identical on purpose: `camelCase` on the wire,
//! `snake_case` in Rust, same names on both sides.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlatformId {
    Youtube,
    Tiktok,
    Instagram,
    Twitter,
    Reddit,
    Facebook,
    Twitch,
    Pinterest,
    Vimeo,
    Dailymotion,
    Soundcloud,
    Direct,
    Generic,
    Unknown,
}

impl PlatformId {
    pub fn label(self) -> &'static str {
        match self {
            Self::Youtube => "YouTube",
            Self::Tiktok => "TikTok",
            Self::Instagram => "Instagram",
            Self::Twitter => "X",
            Self::Reddit => "Reddit",
            Self::Facebook => "Facebook",
            Self::Twitch => "Twitch",
            Self::Pinterest => "Pinterest",
            Self::Vimeo => "Vimeo",
            Self::Dailymotion => "Dailymotion",
            Self::Soundcloud => "SoundCloud",
            Self::Direct => "Direct file",
            Self::Generic => "Web page",
            Self::Unknown => "Unknown",
        }
    }

    /// Short, filesystem-safe token used in file name templates.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Youtube => "youtube",
            Self::Tiktok => "tiktok",
            Self::Instagram => "instagram",
            Self::Twitter => "x",
            Self::Reddit => "reddit",
            Self::Facebook => "facebook",
            Self::Twitch => "twitch",
            Self::Pinterest => "pinterest",
            Self::Vimeo => "vimeo",
            Self::Dailymotion => "dailymotion",
            Self::Soundcloud => "soundcloud",
            Self::Direct => "direct",
            Self::Generic => "web",
            Self::Unknown => "media",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    Video,
    Audio,
    Image,
    Gallery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormatKind {
    /// Video and audio already interleaved in one stream.
    Muxed,
    Video,
    Audio,
    Image,
}

/// Whether a watermark-free rendition is genuinely obtainable.
///
/// The app never removes a watermark; it only reports what the source offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatermarkSupport {
    NotApplicable,
    CleanAvailable,
    WatermarkedOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFormat {
    pub id: String,
    pub kind: FormatKind,
    pub container: String,
    pub protocol: String,
    pub has_video: bool,
    pub has_audio: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub tbr: Option<f64>,
    pub vbr: Option<f64>,
    pub abr: Option<f64>,
    pub filesize: Option<u64>,
    pub filesize_approx: Option<u64>,
    pub quality_label: String,
    pub watermarked: Option<bool>,
    pub note: Option<String>,
    /// Segmented protocols (HLS/DASH) cannot be fetched with a plain ranged
    /// GET, so they are handed back to the external engine.
    pub needs_engine_download: bool,
    /// Not sent to the UI: the direct stream URL and the headers required to
    /// fetch it. Kept server-side so URLs never round-trip through the webview.
    #[serde(skip)]
    pub url: Option<String>,
    #[serde(skip)]
    pub http_headers: Vec<(String, String)>,
}

impl MediaFormat {
    pub fn best_known_size(&self) -> Option<u64> {
        self.filesize.or(self.filesize_approx)
    }

    /// Frame area, used to rank formats. Sources frequently report only one
    /// dimension; treating that as zero would sort such a format below every
    /// other one, so the missing side is estimated at 16:9.
    pub fn pixels(&self) -> u64 {
        match (self.width, self.height) {
            (Some(w), Some(h)) => u64::from(w) * u64::from(h),
            (None, Some(h)) => u64::from(h) * u64::from(h) * 16 / 9,
            (Some(w), None) => u64::from(w) * u64::from(w) * 9 / 16,
            (None, None) => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadata {
    pub url: String,
    pub canonical_url: String,
    pub platform: PlatformId,
    pub platform_label: String,
    pub provider_id: String,
    pub media_kind: MediaKind,
    pub title: String,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub duration_sec: Option<f64>,
    pub view_count: Option<u64>,
    pub like_count: Option<u64>,
    pub upload_date: Option<String>,
    pub is_live: bool,
    pub formats: Vec<MediaFormat>,
    pub entry_count: Option<u32>,
    pub watermark_support: WatermarkSupport,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadMode {
    Video,
    Audio,
    Image,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum QualityPreference {
    /// Highest resolution available, merging separate streams if needed.
    #[default]
    Best,
    /// Highest quality that does not require a merge, when one exists.
    Auto,
    MaxHeight { height: u32 },
    AudioBitrate { kbps: u32 },
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatermarkPreference {
    Any,
    CleanOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub url: String,
    pub mode: DownloadMode,
    pub quality: QualityPreference,
    pub video_format_id: Option<String>,
    pub audio_format_id: Option<String>,
    pub container: Option<String>,
    pub watermark: WatermarkPreference,
    pub output_dir: Option<String>,
    pub title: Option<String>,
    pub thumbnail_url: Option<String>,
    pub platform: Option<PlatformId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    Queued,
    Preparing,
    Downloading,
    /// Merging or converting -- work is happening but no bytes are arriving.
    Processing,
    Paused,
    Completed,
    Failed,
    Canceled,
}

impl DownloadStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Preparing | Self::Downloading | Self::Processing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStage {
    Waiting,
    Resolving,
    Video,
    Audio,
    Image,
    Merging,
    Converting,
    Finalizing,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<f64>,
    pub speed_bps: f64,
    pub eta_sec: Option<f64>,
    pub stage: DownloadStage,
    pub stage_index: u32,
    pub stage_count: u32,
    pub resumable: bool,
}

impl Default for DownloadProgress {
    fn default() -> Self {
        Self {
            received_bytes: 0,
            total_bytes: None,
            percent: None,
            speed_bps: 0.0,
            eta_sec: None,
            stage: DownloadStage::Waiting,
            stage_index: 1,
            stage_count: 1,
            resumable: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorInfo {
    pub code: String,
    pub title: String,
    pub message: String,
    pub technical: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub title: String,
    pub platform: PlatformId,
    pub thumbnail_url: Option<String>,
    pub status: DownloadStatus,
    pub progress: DownloadProgress,
    pub format_label: String,
    pub output_path: Option<String>,
    pub error: Option<AppErrorInfo>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub attempt: u32,
    pub request: DownloadRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub platform: PlatformId,
    pub thumbnail_url: Option<String>,
    pub file_path: String,
    pub file_exists: bool,
    pub container: String,
    pub quality_label: String,
    pub file_size: Option<u64>,
    pub created_at: i64,
    pub status: String,
    pub request: Option<DownloadRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    Engine,
    Ffmpeg,
}

impl ToolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Ffmpeg => "ffmpeg",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolSource {
    Bundled,
    Managed,
    System,
    Custom,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub name: ToolKind,
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub source: ToolSource,
}

impl ToolStatus {
    pub fn missing(name: ToolKind) -> Self {
        Self {
            name,
            available: false,
            path: None,
            version: None,
            source: ToolSource::Missing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsState {
    pub engine: ToolStatus,
    pub ffmpeg: ToolStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInstallProgress {
    pub tool: ToolKind,
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
    pub stage: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub thumbnail_count: u64,
    pub thumbnail_bytes: u64,
    pub metadata_count: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    pub app_version: String,
    pub os: String,
    pub engine: ToolStatus,
    pub ffmpeg: ToolStatus,
    pub download_dir: String,
    pub db_path: String,
    pub log_path: String,
    pub active_downloads: u32,
    pub queued_downloads: u32,
}

/// Payload for the `download://progress` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub id: String,
    pub status: DownloadStatus,
    pub progress: DownloadProgress,
    pub output_path: Option<String>,
    pub error: Option<AppErrorInfo>,
}

// -- conversion ------------------------------------------------------------

/// Whether a conversion target produces a video file or an audio-only one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConvertKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConvertStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Canceled,
}

impl ConvertStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }
}

/// One entry in the list of formats the converter accepts as a target. The
/// label is not here on purpose: the UI names formats from its own dictionary,
/// and this list exists so the two sides cannot disagree about what is offered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertFormatInfo {
    pub id: String,
    pub kind: ConvertKind,
}

/// What a local file actually contains, read before anything is offered for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaProbe {
    pub path: String,
    pub file_name: String,
    /// Extension of the source file, lowercased.
    pub container: String,
    pub size_bytes: u64,
    pub duration_sec: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub audio_bitrate_kbps: Option<f64>,
    pub has_video: bool,
    pub has_audio: bool,
}

/// How hard FFmpeg should work when a re-encode cannot be avoided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConvertQuality {
    High,
    #[default]
    Balanced,
    Small,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertOptions {
    pub target_format: String,
    /// Where the result is written. `None` means "beside the source file".
    pub output_dir: Option<String>,
    pub quality: ConvertQuality,
    /// Downscale cap in pixels of height. `None` keeps the source resolution.
    pub max_height: Option<u32>,
    /// Audio bitrate in kbps. `None` derives one from the source.
    pub audio_bitrate_kbps: Option<u32>,
    /// Repackage rather than re-encode when the target container already
    /// accepts the source streams.
    pub allow_stream_copy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertRequest {
    pub input_paths: Vec<String>,
    pub options: ConvertOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertJob {
    pub id: String,
    pub input_path: String,
    pub input_name: String,
    pub input_size_bytes: u64,
    pub output_path: Option<String>,
    pub output_size_bytes: Option<u64>,
    pub kind: ConvertKind,
    pub status: ConvertStatus,
    /// 0..100 while running. `None` when the duration could not be read, which
    /// is what the UI turns into an indeterminate bar.
    pub percent: Option<f64>,
    pub duration_sec: Option<f64>,
    /// True when the file was repackaged rather than re-encoded.
    pub stream_copied: bool,
    pub error: Option<AppErrorInfo>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub options: ConvertOptions,
}

/// Payload for the `convert://progress` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertProgressEvent {
    pub id: String,
    pub status: ConvertStatus,
    pub percent: Option<f64>,
    pub output_path: Option<String>,
    pub output_size_bytes: Option<u64>,
    pub stream_copied: bool,
    pub error: Option<AppErrorInfo>,
}
