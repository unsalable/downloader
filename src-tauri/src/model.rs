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
    /// Whether this link can hand over just a slice of itself, decided from the
    /// protocol of the streams that would actually be fetched. Sent to the
    /// interface so the editor can promise a ranged fetch without keeping its
    /// own list of which sites allow one -- a list that would start lying the
    /// day a site changed how it delivers.
    pub range_fetchable: bool,
    pub warnings: Vec<String>,
    /// Every item of a carousel, gallery or album, in the source's order. The
    /// fields above describe the first one, which is what the link downloads
    /// as a whole; a request names any other by its position. Not sent to the
    /// UI, for the same reason stream addresses are not.
    #[serde(skip)]
    pub entries: Vec<MediaMetadata>,
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
    /// 1-based position of one item of a carousel or gallery. `None` means the
    /// link as a whole, which for a gallery is its first item.
    #[serde(default)]
    pub entry: Option<u32>,
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
    /// A JavaScript engine, which the download engine needs to read the formats
    /// a signed-in YouTube request is answered with.
    ///
    /// It does not exist on Android: the APK carries QuickJS, and nothing an
    /// app downloads there would be allowed to run. The variant is compiled out
    /// rather than merely ignored so that build cannot name it at all.
    #[cfg(not(target_os = "android"))]
    JsRuntime,
}

impl ToolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Ffmpeg => "ffmpeg",
            #[cfg(not(target_os = "android"))]
            Self::JsRuntime => "jsRuntime",
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
    #[cfg(not(target_os = "android"))]
    pub js_runtime: ToolStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInstallProgress {
    pub tool: ToolKind,
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
    pub stage: String,
}

/// What asking for a newer release of a tool found. Asking downloads nothing;
/// the interface installs only when the answer is that one exists.
///
/// `up_to_date` is true only when the app knows it. A version it cannot read,
/// or a copy it cannot compare with the release -- a Node found on PATH where
/// the release is Deno's -- answers false, which leads to the same install the
/// button always started.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolUpdateCheck {
    pub tool: ToolKind,
    pub installed: Option<String>,
    /// The newest release's version, or for FFmpeg, which publishes no version
    /// numbers, the day its current build went up.
    pub latest: Option<String>,
    pub up_to_date: bool,
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
    #[cfg(not(target_os = "android"))]
    pub js_runtime: ToolStatus,
    pub download_dir: String,
    pub db_path: String,
    pub log_path: String,
    pub active_downloads: u32,
    pub queued_downloads: u32,
}

/// What the Connection section shows, and everything the app knows about the
/// browser link without decrypting anything.
///
/// `supported` and `storeListed` are separate on purpose: the first says this
/// build can host a link at all, the second that there is a published extension
/// to point the user at. The section hides itself unless both hold, so it never
/// offers a button that leads to a listing that does not exist yet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    pub supported: bool,
    pub store_listed: bool,
    pub enabled: bool,
    /// Whether the browsers on this machine can still start our helper. False
    /// means the registry values are gone or name something else, which calls
    /// for Repair rather than for installing the extension again.
    pub registered: bool,
    pub connected: bool,
    pub browser: Option<String>,
    pub profile_label: Option<String>,
    pub account_hint: Option<String>,
    pub extension_version: Option<String>,
    pub last_push_at: Option<i64>,
    pub session: crate::bridge::SessionState,
    pub host_path: Option<String>,
    pub app_version: String,
    pub extension_id: String,
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
    /// The shape of one stored pixel. A frame stored 720x576 with a 64:45 pixel
    /// is a 16:9 picture, so anything that measures the picture rather than the
    /// file has to square the pixels first. `None` when the probe could not say,
    /// which is read as square.
    pub pixel_aspect: Option<f64>,
    /// The video stream's own duration, which is not always the container's: a
    /// file can carry 26 seconds of audio over 20 seconds of picture, and a
    /// filmstrip cut to the container's length ends in black cells.
    pub video_duration_sec: Option<f64>,
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

// -- trimming ---------------------------------------------------------------

/// The root of every other export decision, and the one thing that cannot be
/// had both ways.
///
/// A stream copy is seconds rather than minutes and leaves the picture
/// bit-for-bit the original, but a copied video stream can only begin at a
/// keyframe, and nothing about the frame -- its shape, its rate, its size, its
/// codec -- can change along the way. Everything the editor offers beyond
/// where the cuts fall and whether the sound comes along is therefore a reason
/// to leave this mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportMode {
    #[default]
    Lossless,
    Reencode,
}

/// One kept piece of the source, in seconds from its start.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSegment {
    pub start_sec: f64,
    pub end_sec: f64,
}

impl EditSegment {
    pub fn length(&self) -> f64 {
        self.end_sec - self.start_sec
    }
}

/// The shape of the exported frame.
///
/// `Source` is not a ratio but the absence of one: the frame is left exactly as
/// it was found, which is the only value a lossless export can carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectRatio {
    #[default]
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "16:9")]
    Widescreen,
    #[serde(rename = "9:16")]
    Portrait,
    #[serde(rename = "16:10")]
    Tall,
    #[serde(rename = "4:3")]
    Classic,
    #[serde(rename = "1:1")]
    Square,
}

impl AspectRatio {
    /// Width and height as whole numbers, so the ratio the button names is the
    /// ratio the file gets. Deriving the size inside a crop expression instead
    /// gives an even but inexact frame -- 202x360 is 0.5611, not 0.5625.
    pub fn parts(self) -> Option<(u32, u32)> {
        match self {
            AspectRatio::Source => None,
            AspectRatio::Widescreen => Some((16, 9)),
            AspectRatio::Portrait => Some((9, 16)),
            AspectRatio::Tall => Some((16, 10)),
            AspectRatio::Classic => Some((4, 3)),
            AspectRatio::Square => Some((1, 1)),
        }
    }
}

/// What happens to a frame that is not the shape it is being poured into.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FrameFit {
    /// Fill the frame and lose the edges.
    #[default]
    Fill,
    /// Keep all of it and put black where it does not reach.
    Fit,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoCodec {
    #[default]
    H264,
    H265,
    Vp9,
    Av1,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioCodec {
    #[default]
    Aac,
    Opus,
    Mp3,
    Flac,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportQuality {
    Maximum,
    High,
    #[default]
    Balanced,
    Small,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub mode: ExportMode,
    /// Target container, lowercased and without the dot.
    pub container: String,
    pub video_codec: VideoCodec,
    pub quality: ExportQuality,
    /// An average to aim for, in kilobits a second, in place of `quality`.
    ///
    /// `None` is the ordinary case and asks the encoder for a constant quality,
    /// which spends bits where the picture needs them and makes no promise
    /// about size. A number is that promise instead: the file comes out near
    /// `length x rate` whatever is in it. A copy re-encodes nothing and so has
    /// no rate to set; it ignores this. Absent from what an older interface
    /// sends, which is why it has a default.
    #[serde(default)]
    pub video_bitrate_kbps: Option<u32>,
    /// `None` keeps the source's rate.
    pub fps: Option<f64>,
    /// Cap in pixels of height. `None` keeps the source's resolution.
    pub max_height: Option<u32>,
    pub aspect: AspectRatio,
    pub fit: FrameFit,
    /// Drop the audio entirely rather than encode it -- or copy it, which
    /// makes this the one audio setting a lossless export can honour.
    pub mute: bool,
    pub audio_codec: AudioCodec,
    pub audio_bitrate_kbps: Option<u32>,
    /// Linear gain on the sound: 1 leaves it alone, 0 keeps a silent track --
    /// which is not the same as `mute`, since a player that expects sound still
    /// finds some -- and 2 is the most on offer. Defaults to 1 for an interface
    /// that does not send it.
    #[serde(default = "unity_gain")]
    pub volume: f64,
    /// Map an HDR source down to Rec. 709 instead of letting it wash out.
    pub tone_map_sdr: bool,
    pub hardware: bool,
}

fn unity_gain() -> f64 {
    1.0
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            mode: ExportMode::Lossless,
            container: "mp4".into(),
            video_codec: VideoCodec::H264,
            quality: ExportQuality::Balanced,
            video_bitrate_kbps: None,
            fps: None,
            max_height: None,
            aspect: AspectRatio::Source,
            fit: FrameFit::Fill,
            mute: false,
            audio_codec: AudioCodec::Aac,
            audio_bitrate_kbps: Some(192),
            volume: unity_gain(),
            tone_map_sdr: false,
            hardware: false,
        }
    }
}

impl ExportOptions {
    /// The gain the sound actually gets, or `None` when it is left alone.
    ///
    /// Asked in one place so that the two sides of the same question cannot
    /// drift apart: a copy refuses exactly the volumes a re-encode would apply.
    /// Anything within half a percent of unity is unity -- a slider that comes
    /// to rest at 0.999 has not been asked to change anything, and a pass
    /// through the audio filter for it would be a re-encode for nothing. A gain
    /// that is not a number is treated the same way rather than handed to
    /// FFmpeg, which would refuse the whole graph over it.
    pub fn gain(&self) -> Option<f64> {
        let volume = self.volume;
        if !volume.is_finite() || (volume - 1.0).abs() <= 0.005 {
            return None;
        }
        Some(volume.clamp(0.0, 2.0))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub input_path: String,
    /// In order and non-overlapping. They are joined into one file.
    pub segments: Vec<EditSegment>,
    pub options: ExportOptions,
    /// Where the result is written. `None` means beside the source file --
    /// unless the source is one of the app's own files, which is every source
    /// on a phone (private copies) and, on the desktop, a clip fetched from a
    /// link into the app's temporary folder. Those go to the download folder
    /// instead; see `export::export_dir` and the `export_default_dir` command.
    pub output_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportStatus {
    #[default]
    Idle,
    Running,
    Completed,
    Failed,
    Canceled,
}

impl ExportStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running)
    }
}

/// Everything the editor knows about the export it last asked for. There is
/// only ever one, so this is a state rather than a list.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportState {
    pub status: ExportStatus,
    /// 0..100 while running. `None` once it is not.
    pub percent: Option<f64>,
    pub output_path: Option<String>,
    pub error: Option<AppErrorInfo>,
}

/// Bringing a link into the editor.
///
/// The marks are optional because the two answers this carries are different
/// requests: a source that can hand over a slice is asked for one, and a source
/// that cannot is fetched whole and cut afterwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeFetchRequest {
    pub url: String,
    /// Both are set together or not at all; neither alone means anything.
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    /// Cap the rendition's height, so a 4K source does not land as a 4K file
    /// on a screen that is about to re-encode it anyway.
    pub max_height: Option<u32>,
    /// Re-encode at the cuts so the fetch begins on the frame that was asked
    /// for. Roughly twice the time; without it the fetch starts at the keyframe
    /// at or before the mark, which the editor tidies up afterwards anyway.
    pub exact: bool,
    /// `None` means the app's own temporary folder, which the editor opens.
    pub output_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FetchStatus {
    #[default]
    Idle,
    /// Reading the link. On a site that hides its streams behind a challenge
    /// this is most of the wait, and it reports no progress at all.
    Resolving,
    Fetching,
    Completed,
    Failed,
    Canceled,
}

impl FetchStatus {
    /// Reading the link counts: it is a network wait that Android would
    /// freeze in the background exactly as it would the transfer after it.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Resolving | Self::Fetching)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchState {
    pub status: FetchStatus,
    /// `None` is not zero. A ranged fetch goes through FFmpeg, which reports
    /// nothing the engine passes on until it is over, and a bar sitting at zero
    /// for a minute is a lie that a plain spinner would not tell.
    pub percent: Option<f64>,
    pub received_bytes: u64,
    pub title: Option<String>,
    pub output_path: Option<String>,
    pub error: Option<AppErrorInfo>,
}

/// What the timeline needs drawn for it. Both kinds are read by FFmpeg rather
/// than by the window: the picture element cannot be asked for a frame it is
/// not showing, and it cannot be asked for the audio at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimelineKind {
    Waveform,
    Filmstrip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRequest {
    pub path: String,
    pub kind: TimelineKind,
    /// The window to read. `None` for both means the whole file.
    pub start_sec: Option<f64>,
    pub length_sec: Option<f64>,
    /// Buckets for a waveform, cells for a filmstrip.
    pub count: u32,
    /// Filmstrip only: the height of one cell, in device pixels.
    pub cell_height: Option<u32>,
    /// Echoed back untouched, so a slower answer cannot paint over a newer one.
    pub token: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveformData {
    pub buckets: u32,
    pub start_sec: f64,
    pub length_sec: f64,
    /// Base64 of `2 * buckets` bytes: the lowest and highest sample of each
    /// bucket, interleaved, with 128 as silence. The same figures as a JSON
    /// array of floats cost nine times as much to send.
    pub peaks: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmstripData {
    /// Cells in the whole strip, which is `chunk_frames * chunks.len()`.
    pub frames: u32,
    pub chunk_frames: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub start_sec: f64,
    pub length_sec: f64,
    /// Tiled sprites, in order, each holding `chunk_frames` cells. They are
    /// published as they land so the strip fills from the left, which says
    /// more about how far the work has got than a percentage would.
    pub chunks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineState {
    pub path: Option<String>,
    pub token: u64,
    pub working: bool,
    pub waveform: Option<WaveformData>,
    pub filmstrip: Option<FilmstripData>,
    pub error: Option<AppErrorInfo>,
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
