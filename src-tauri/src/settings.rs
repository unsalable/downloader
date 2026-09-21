//! User settings: shape, defaults and validation.
//!
//! Persisted as a single JSON blob in the `settings` table rather than one row
//! per key. The whole object is small, always read and written together, and
//! this keeps adding a field to a one-line change.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{DownloadMode, QualityPreference};
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemePreference {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyAction {
    PasteUrl,
    Download,
    OpenDownloads,
    OpenHistory,
    OpenSettings,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 5] = [
        Self::PasteUrl,
        Self::Download,
        Self::OpenDownloads,
        Self::OpenHistory,
        Self::OpenSettings,
    ];

    fn default_accelerator(self) -> &'static str {
        match self {
            Self::PasteUrl => "Ctrl+V",
            Self::Download => "Ctrl+Enter",
            Self::OpenDownloads => "Ctrl+Shift+D",
            Self::OpenHistory => "Ctrl+Shift+H",
            Self::OpenSettings => "Ctrl+,",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    // General
    pub start_with_windows: bool,
    pub minimize_to_tray: bool,
    pub close_to_tray: bool,
    pub clipboard_monitoring: bool,
    pub notifications_enabled: bool,
    pub notify_on_complete: bool,
    pub notify_on_error: bool,

    // Downloads
    pub download_dir: String,
    pub default_mode: DownloadMode,
    pub default_quality: QualityPreference,
    pub default_container: Option<String>,
    pub max_concurrent_downloads: u32,
    pub auto_retry_count: u32,
    pub filename_template: String,

    // Appearance
    pub theme: ThemePreference,
    pub language: String,
    pub reduce_motion: bool,
    pub show_animated_background: bool,

    // Performance
    pub low_resource_mode: bool,
    pub hardware_acceleration: bool,
    pub cache_limit_mb: u64,

    // Advanced
    pub ffmpeg_path: Option<String>,
    pub engine_path: Option<String>,
    pub network_timeout_sec: u64,
    pub proxy_url: Option<String>,
    pub custom_user_agent: Option<String>,
    pub debug_logging: bool,

    /// Whether a signed-in browser may lend the app its YouTube session, for
    /// content the user pays for. Desktop only; see `bridge`.
    ///
    /// On by default, because nothing happens until the user deliberately
    /// installs the extension and presses Connect -- making them find a toggle
    /// first would only add a step to the flow they are most likely to abandon.
    ///
    /// `serde(default)` is not decoration here. This blob has no migration:
    /// `load_settings` parses it with `unwrap_or_default()`, so a field missing
    /// from an older install makes the whole object fail to parse and silently
    /// resets every preference the user ever changed.
    #[serde(default = "default_browser_link")]
    pub browser_link_enabled: bool,

    pub hotkeys: BTreeMap<HotkeyAction, String>,

    pub onboarding_complete: bool,
}

pub const DEFAULT_FILENAME_TEMPLATE: &str = "{creator} - {title} [{quality}]";

impl Default for Settings {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            minimize_to_tray: true,
            close_to_tray: false,
            // Android announces every clipboard read with a banner, and its share
            // sheet is the natural way to hand a link over anyway.
            clipboard_monitoring: !cfg!(target_os = "android"),
            notifications_enabled: true,
            notify_on_complete: true,
            notify_on_error: true,

            download_dir: paths::default_download_dir().to_string_lossy().into_owned(),
            default_mode: DownloadMode::Video,
            default_quality: QualityPreference::Best,
            default_container: None,
            max_concurrent_downloads: 3,
            auto_retry_count: 2,
            filename_template: DEFAULT_FILENAME_TEMPLATE.to_string(),

            theme: ThemePreference::Dark,
            language: "en".to_string(),
            reduce_motion: false,
            show_animated_background: true,

            low_resource_mode: false,
            // The GPU pass is an NVIDIA encoder; a phone would only ever fail it.
            hardware_acceleration: !cfg!(target_os = "android"),
            cache_limit_mb: 256,

            ffmpeg_path: None,
            engine_path: None,
            network_timeout_sec: 30,
            proxy_url: None,
            custom_user_agent: None,
            debug_logging: false,

            browser_link_enabled: default_browser_link(),

            hotkeys: HotkeyAction::ALL
                .iter()
                .map(|action| (*action, action.default_accelerator().to_string()))
                .collect(),

            onboarding_complete: false,
        }
    }
}

impl Settings {
    /// Clamp anything that could destabilise the app if a stale or hand-edited
    /// config carried an out-of-range value.
    pub fn sanitize(&mut self) {
        self.max_concurrent_downloads = self.max_concurrent_downloads.clamp(1, 10);
        self.auto_retry_count = self.auto_retry_count.min(10);
        self.network_timeout_sec = self.network_timeout_sec.clamp(5, 600);
        self.cache_limit_mb = self.cache_limit_mb.clamp(32, 8192);

        if self.filename_template.trim().is_empty() {
            self.filename_template = DEFAULT_FILENAME_TEMPLATE.to_string();
        }
        if self.download_dir.trim().is_empty() {
            self.download_dir = paths::default_download_dir().to_string_lossy().into_owned();
        }
        if !matches!(self.language.as_str(), "en" | "tr") {
            self.language = "en".to_string();
        }

        // Empty strings from cleared text fields mean "unset", not "use ''".
        normalize_optional(&mut self.proxy_url);
        normalize_optional(&mut self.custom_user_agent);
        normalize_optional(&mut self.ffmpeg_path);
        normalize_optional(&mut self.engine_path);

        for action in HotkeyAction::ALL {
            self.hotkeys
                .entry(action)
                .or_insert_with(|| action.default_accelerator().to_string());
        }
    }
}

/// Armed wherever the browser link can exist at all, which is the desktop
/// build. A phone has no desktop browser to link to, so it stores `false`
/// rather than a preference that could never take effect.
fn default_browser_link() -> bool {
    cfg!(windows)
}

fn normalize_optional(value: &mut Option<String>) {
    if let Some(inner) = value {
        if inner.trim().is_empty() {
            *value = None;
        } else {
            *inner = inner.trim().to_string();
        }
    }
}
