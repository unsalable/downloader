//! Error taxonomy.
//!
//! Nothing here ever reaches the user as a raw string. Every variant maps to a
//! stable `code` that the React layer looks up in its dictionary, plus an
//! English fallback so the app still reads sensibly if a key is missing. The
//! underlying technical text is preserved separately and only shown behind
//! "View technical details".

use crate::model::AppErrorInfo;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid url: {0}")]
    InvalidUrl(String),

    #[error("unsupported source: {0}")]
    Unsupported(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("access denied ({status})")]
    Forbidden { status: u16, detail: String },

    #[error("not found ({status})")]
    NotFound { status: u16, detail: String },

    #[error("download engine is not installed")]
    EngineMissing,

    #[error("ffmpeg is not installed")]
    FfmpegMissing,

    #[error("the engine failed: {0}")]
    Engine(String),

    #[error("could not read the response: {0}")]
    Parse(String),

    #[error("not enough disk space")]
    DiskFull,

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("io error: {0}")]
    Io(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("canceled")]
    Canceled,

    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl(_) => "invalidUrl",
            Self::Unsupported(_) => "unsupported",
            Self::Network(_) => "network",
            Self::Forbidden { .. } => "forbidden",
            Self::NotFound { .. } => "notFound",
            Self::EngineMissing => "engineMissing",
            Self::FfmpegMissing => "ffmpegMissing",
            Self::Engine(_) => "unknown",
            Self::Parse(_) => "unknown",
            Self::DiskFull => "diskFull",
            Self::Permission(_) => "permission",
            Self::Io(_) => "unknown",
            Self::Database(_) => "unknown",
            Self::Canceled => "canceled",
            Self::Other(_) => "unknown",
        }
    }

    /// Whether offering "Try again" makes sense. A 404 or a missing tool will
    /// not fix itself by retrying, so the button is hidden for those.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Network(_)
                | Self::Forbidden { .. }
                | Self::Engine(_)
                | Self::Io(_)
                | Self::DiskFull
                | Self::Other(_)
        )
    }

    fn english(&self) -> (&'static str, &'static str) {
        match self.code() {
            "invalidUrl" => ("That link doesn't look right", "Check the address and try again."),
            "unsupported" => (
                "This link is not supported",
                "No provider could read media from this address, and no direct media file was found.",
            ),
            "network" => (
                "We couldn't reach the source",
                "Check your connection and try again.",
            ),
            "forbidden" => (
                "We couldn't access this media",
                "The source may require login or may not currently support public downloads.",
            ),
            "notFound" => (
                "This media no longer exists",
                "The post may have been deleted or made private.",
            ),
            "engineMissing" => (
                "The download engine is missing",
                "Install it from Settings to analyze and download media.",
            ),
            "ffmpegMissing" => (
                "FFmpeg is required for this download",
                "This media has separate video and audio streams that need merging. Install FFmpeg from Settings, or pick a single-stream quality.",
            ),
            "diskFull" => (
                "Not enough space",
                "Free up some room in the download folder and try again.",
            ),
            "permission" => (
                "We couldn't write the file",
                "Choose a different download folder and try again.",
            ),
            "canceled" => ("Canceled", "The download was stopped."),
            _ => ("Something went wrong", "The download could not be completed."),
        }
    }

    /// Full technical text, kept out of the default UI.
    pub fn technical(&self) -> Option<String> {
        match self {
            Self::Canceled => None,
            Self::Forbidden { status, detail } | Self::NotFound { status, detail } => {
                Some(format!("HTTP {status}: {detail}"))
            }
            other => Some(other.to_string()),
        }
    }

    pub fn to_info(&self) -> AppErrorInfo {
        let (title, message) = self.english();
        AppErrorInfo {
            code: self.code().to_string(),
            title: title.to_string(),
            message: message.to_string(),
            technical: self.technical(),
            retryable: self.retryable(),
        }
    }

    /// Classify an HTTP status into the taxonomy above.
    pub fn from_status(status: u16, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        match status {
            401 | 403 | 429 => Self::Forbidden { status, detail },
            404 | 410 => Self::NotFound { status, detail },
            _ => Self::Network(format!("HTTP {status}: {detail}")),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        use std::io::ErrorKind;
        match value.kind() {
            ErrorKind::PermissionDenied => Self::Permission(value.to_string()),
            ErrorKind::NotFound => Self::Io(value.to_string()),
            // `StorageFull` is nightly-only; the raw OS codes are the portable
            // way to detect a full volume on Windows (ERROR_DISK_FULL = 112,
            // ERROR_HANDLE_DISK_FULL = 39).
            _ if matches!(value.raw_os_error(), Some(112) | Some(39) | Some(28)) => Self::DiskFull,
            _ => Self::Io(value.to_string()),
        }
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        if value.is_timeout() {
            Self::Network(format!("timed out: {value}"))
        } else if let Some(status) = value.status() {
            Self::from_status(status.as_u16(), value.to_string())
        } else {
            Self::Network(value.to_string())
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Parse(value.to_string())
    }
}

/// Tauri commands return this so the UI always receives a structured error
/// object rather than a stringified Rust error.
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_info().serialize(serializer)
    }
}
