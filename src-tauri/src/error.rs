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

    /// The phone has no network to use.
    #[error("offline: {0}")]
    Offline(String),

    /// The phone has a network, but this app cannot get through it: Android,
    /// a VPN or a firewall is not letting it, or its DNS is not answering.
    #[error("no internet access for this app: {0}")]
    NetworkBlocked(String),

    #[error("access denied ({status})")]
    Forbidden { status: u16, detail: String },

    /// A video published to a channel's members. Kept apart from `Forbidden`
    /// because it is the one refusal the app has something to offer about: a
    /// browser the user has linked may already be signed in to an account that
    /// holds the membership.
    #[error("limited to channel members: {detail}")]
    MembershipRequired { detail: String },

    /// A TikTok post shown only to a signed-in viewer: one its creator limited
    /// to adults with audience controls, a private post or account, or one
    /// TikTok put behind its login page. Kept apart from `Forbidden` because,
    /// like a membership, it is a refusal the desktop has an answer to -- a
    /// browser signed in to TikTok can lend its session. `session_tried` says
    /// that answer was already given and TikTok still said no, which is a
    /// different sentence for the user, and not one worth repeating.
    #[error("TikTok shows this post only to a signed-in viewer: {detail}")]
    TiktokSignIn { detail: String, session_tried: bool },

    #[error("not found ({status})")]
    NotFound { status: u16, detail: String },

    /// A finished file that is no longer where the app saved it. Kept apart from
    /// `Io` so the interface can say exactly that, and only that: any other
    /// refusal to open a file is not evidence that it is gone.
    #[error("file not found: {0}")]
    FileMissing(String),

    /// A song shared from a music service whose recording could not be found
    /// anywhere the app can download from.
    #[error("no matching recording: {0}")]
    NoMatch(String),

    /// A service that encrypts what it streams. Nothing the app does can
    /// download from it, and it says so before trying.
    #[error("protected by the service: {0}")]
    Protected(String),

    /// A link that will hand over all of itself and no part of it.
    ///
    /// Kept apart from `Forbidden`, which it would otherwise arrive as, because
    /// it is not the same refusal and does not have the same answer. Measured:
    /// a partial fetch is carried out by an FFmpeg the engine starts, and the
    /// session the engine was given never reaches it -- only the user agent and
    /// the accept headers are passed on. A host that gates the media request
    /// itself on a cookie therefore refuses the piece while still serving the
    /// whole, and telling the user to sign in again would send them after
    /// something that cannot help.
    #[error("only the whole of this link can be fetched: {0}")]
    RangeUnavailable(String),

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
            Self::Offline(_) => "offline",
            Self::NetworkBlocked(_) => "networkBlocked",
            Self::Forbidden { .. } => "forbidden",
            Self::MembershipRequired { .. } => "membershipRequired",
            Self::TiktokSignIn {
                session_tried: false,
                ..
            } => "tiktokSignIn",
            Self::TiktokSignIn {
                session_tried: true,
                ..
            } => "tiktokNotForAccount",
            Self::NotFound { .. } => "notFound",
            Self::FileMissing(_) => "fileMissing",
            Self::NoMatch(_) => "noMatch",
            Self::Protected(_) => "protected",
            Self::RangeUnavailable(_) => "rangeUnavailable",
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
    ///
    /// A membership wall counts, even though the same run repeated changes
    /// nothing on its own: the message tells the user to connect their browser,
    /// and "Try again" is the button they reach for once they have.
    pub fn retryable(&self) -> bool {
        match self {
            // Like a membership on the desktop: the user turns the browser's
            // TikTok session on, then presses Try again. Never once the session
            // was tried -- the queue repeats a retryable failure on its own
            // (`queue.rs`), and each repeat would spend the account on a post
            // TikTok will not show it -- and never on a phone, which has no
            // session to lend.
            Self::TiktokSignIn { session_tried, .. } => !*session_tried && cfg!(windows),
            other => matches!(
                other,
                Self::Network(_)
                    | Self::Offline(_)
                    | Self::NetworkBlocked(_)
                    | Self::Forbidden { .. }
                    | Self::MembershipRequired { .. }
                    | Self::Engine(_)
                    | Self::Io(_)
                    | Self::DiskFull
                    | Self::Other(_)
            ),
        }
    }

    /// The same refusal, seen on the run the stored session was lent for.
    ///
    /// Only TikTok's wall changes: it is the one refusal whose sentence to the
    /// user depends on whether a session was already behind the request. Every
    /// other error passes through as it came.
    pub fn after_session(self) -> Self {
        match self {
            Self::TiktokSignIn { detail, .. } => Self::TiktokSignIn {
                detail,
                session_tried: true,
            },
            other => other,
        }
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
            "offline" => ("You're offline", "Connect to Wi-Fi or mobile data and try again."),
            "networkBlocked" => (
                "Universal Downloader can't get online",
                "Your phone is connected, but this app can't use the connection. Allow Universal Downloader to use Wi-Fi and mobile data in its settings, check any VPN, firewall or ad blocker, then try again.",
            ),
            "forbidden" => (
                "We couldn't access this media",
                "The source may require login or may not currently support public downloads.",
            ),
            // Deliberately a description of what the app will do, not a promise
            // about what YouTube will allow. A linked browser signed in to the
            // right account is what the app can offer; whether the video is
            // then served is YouTube's decision and not always a yes.
            "membershipRequired" => (
                "This video is for channel members",
                "YouTube only serves it to an account that holds the channel's membership. Connect your browser in Settings and Universal Downloader will offer that sign-in the next time you try this link.",
            ),
            // Covers audience controls, a private post and TikTok's own login
            // page alike, so it says what TikTok does rather than guessing
            // which of the three the creator chose.
            "tiktokSignIn" => (
                "This post needs a TikTok sign-in",
                "TikTok shows it only to signed-in viewers. Turn on TikTok session in the browser extension, in the browser where you're signed in to TikTok, then try again.",
            ),
            "tiktokNotForAccount" => (
                "TikTok won't show this post to the linked account either",
                "The account may look under 18 to TikTok, or the post may be for followers only.",
            ),
            "notFound" => (
                "This media no longer exists",
                "The post may have been deleted or made private.",
            ),
            "fileMissing" => ("File moved or deleted", "It's no longer where it was saved."),
            "noMatch" => (
                "This song couldn't be found",
                "No recording of it turned up on YouTube.",
            ),
            "protected" => (
                "This service can't be downloaded from",
                "It encrypts what it streams, so there is nothing a downloader can save.",
            ),
            // Deliberately not phrased as something to fix. There is no
            // setting and no sign-in that changes this answer, and the one
            // thing that does work is the button beside it.
            "rangeUnavailable" => (
                "This source only hands over the whole video",
                "Fetching part of a video is done by a separate step that this source will not answer. Fetch all of it and trim it here instead.",
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
            Self::Network(format!("timed out: {}", with_causes(&value)))
        } else if let Some(status) = value.status() {
            Self::from_status(status.as_u16(), value.to_string())
        } else {
            Self::Network(with_causes(&value))
        }
    }
}

/// An error with the chain of errors behind it. A request error on its own
/// only says which address failed ("error sending request for url (...)");
/// why -- a lookup that found nothing, a refused connection -- is in its
/// sources.
fn with_causes(err: &(dyn std::error::Error + 'static)) -> String {
    let mut text = err.to_string();
    let mut cause = err.source();
    while let Some(source) = cause {
        let part = source.to_string();
        // Wrappers often repeat what they wrap.
        if !text.ends_with(&part) {
            text.push_str(": ");
            text.push_str(&part);
        }
        cause = source.source();
    }
    text
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wall() -> AppError {
        AppError::TiktokSignIn {
            detail: "ERROR: [TikTok] 1: This post may not be comfortable for some audiences. Log in for access.".into(),
            session_tried: false,
        }
    }

    /// A refusal seen with the session behind it is told apart, and never
    /// offered again: the queue would otherwise repeat it on its own, each
    /// time spending the user's TikTok account on a post it will not be shown.
    #[test]
    fn after_session_turns_a_tiktok_wall_into_the_account_refusal() {
        let first = wall();
        assert_eq!(first.code(), "tiktokSignIn");
        assert_eq!(first.retryable(), cfg!(windows));

        let second = first.after_session();
        assert_eq!(second.code(), "tiktokNotForAccount");
        assert!(!second.retryable());
        assert!(second
            .technical()
            .is_some_and(|text| text.contains("comfortable for some audiences")));

        // Every other failure is what it was, session or not.
        let forbidden = AppError::Forbidden {
            status: 403,
            detail: "x".into(),
        }
        .after_session();
        assert_eq!(forbidden.code(), "forbidden");
        assert!(forbidden.retryable());
        let membership = AppError::MembershipRequired { detail: "x".into() }.after_session();
        assert_eq!(membership.code(), "membershipRequired");

        // Both have sentences of their own, not the "something went wrong"
        // every unknown code falls back to.
        let fallback = AppError::Other("x".into()).to_info();
        for err in [wall(), wall().after_session()] {
            let info = err.to_info();
            assert!(
                !info.title.is_empty() && !info.message.is_empty(),
                "{info:?}"
            );
            assert_ne!(info.title, fallback.title);
            assert_ne!(info.message, fallback.message);
        }
    }
}
