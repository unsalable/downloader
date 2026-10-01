//! URL classification.
//!
//! This is the single source of truth for "which platform is this?" -- the
//! frontend asks over IPC rather than keeping a second copy of these patterns.
//!
//! Note that platform detection and provider selection are separate concerns.
//! The platform is a display label; which provider handles the link is decided
//! in `registry.rs` and mostly does not depend on it.

use crate::model::PlatformId;

/// Host suffixes, matched against the registrable part of the hostname so that
/// `m.`, `www.` and country subdomains all fall through to the same platform.
const HOSTS: &[(&str, PlatformId)] = &[
    ("youtube.com", PlatformId::Youtube),
    ("youtu.be", PlatformId::Youtube),
    ("youtube-nocookie.com", PlatformId::Youtube),
    ("tiktok.com", PlatformId::Tiktok),
    ("instagram.com", PlatformId::Instagram),
    ("instagr.am", PlatformId::Instagram),
    ("twitter.com", PlatformId::Twitter),
    ("x.com", PlatformId::Twitter),
    ("t.co", PlatformId::Twitter),
    ("reddit.com", PlatformId::Reddit),
    ("redd.it", PlatformId::Reddit),
    ("facebook.com", PlatformId::Facebook),
    ("fb.watch", PlatformId::Facebook),
    ("fb.com", PlatformId::Facebook),
    ("twitch.tv", PlatformId::Twitch),
    ("pinterest.com", PlatformId::Pinterest),
    ("pin.it", PlatformId::Pinterest),
    ("vimeo.com", PlatformId::Vimeo),
    ("dailymotion.com", PlatformId::Dailymotion),
    ("dai.ly", PlatformId::Dailymotion),
    ("soundcloud.com", PlatformId::Soundcloud),
    ("snd.sc", PlatformId::Soundcloud),
    ("spotify.com", PlatformId::Spotify),
];

/// Services that encrypt what they stream. The app cannot download from them
/// and does not try: saying so at once beats a long wait for a vague refusal.
/// Named by the host suffix, with the name the user knows them by.
const PROTECTED: &[(&str, &str)] = &[
    ("netflix.com", "Netflix"),
    ("disneyplus.com", "Disney+"),
    ("primevideo.com", "Prime Video"),
    ("max.com", "Max"),
    ("hbomax.com", "HBO Max"),
    ("hulu.com", "Hulu"),
    ("paramountplus.com", "Paramount+"),
    ("peacocktv.com", "Peacock"),
    ("tv.apple.com", "Apple TV+"),
    ("music.apple.com", "Apple Music"),
    ("crunchyroll.com", "Crunchyroll"),
    ("blutv.com", "BluTV"),
    ("exxen.com", "Exxen"),
    ("gain.tv", "Gain"),
    ("tod.tv", "TOD"),
    ("tvplus.com.tr", "TV+"),
    ("deezer.com", "Deezer"),
    ("tidal.com", "TIDAL"),
];

/// The protected service a link belongs to, if it belongs to one.
///
/// A song on Spotify is not on this list: its recording is found elsewhere,
/// and only the description is read from Spotify.
pub fn protected_service(raw: &str) -> Option<&'static str> {
    let info = classify(raw)?;
    let host = info.host.as_str();
    let on = |suffix: &str| host == suffix || host.ends_with(&format!(".{suffix}"));

    // Amazon's videos live under its shop's many country domains.
    if host.split('.').any(|label| label == "amazon") && info.path.starts_with("/gp/video") {
        return Some("Prime Video");
    }

    PROTECTED
        .iter()
        .find(|(suffix, _)| on(suffix))
        .map(|(_, name)| *name)
}

/// Pinterest and a few others use per-country domains; matching the leading
/// label covers `pinterest.co.uk`, `pinterest.com.au` and friends.
const HOST_PREFIXES: &[(&str, PlatformId)] = &[("pinterest.", PlatformId::Pinterest)];

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "webm", "mkv", "mov", "m4v", "avi", "flv", "ts", "mpg", "mpeg", "3gp", "ogv",
];
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "aac", "wav", "opus", "ogg", "flac", "wma"];
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "avif", "bmp", "tiff", "heic", "heif",
];

pub struct UrlInfo {
    pub host: String,
    pub path: String,
    pub platform: PlatformId,
    /// Set when the path ends in a recognised media extension.
    pub direct_extension: Option<String>,
}

/// Parse and classify. Returns `None` for anything that is not an http(s) URL.
pub fn classify(raw: &str) -> Option<UrlInfo> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let (scheme, rest) = with_scheme.split_once("://")?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return None;
    }

    // Split the authority off first: a path may legitimately contain '@'
    // (TikTok profiles are /@user/...), and stripping credentials before this
    // point would swallow the host.
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    let authority = authority.rsplit_once('@').map(|(_, a)| a).unwrap_or(authority);

    let host = authority
        .rsplit_once(':')
        // Only treat the trailing segment as a port when it is numeric, so an
        // IPv6 literal is not mangled.
        .filter(|(_, port)| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
        .map(|(h, _)| h)
        .unwrap_or(authority)
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();

    if host.is_empty() || !host.contains('.') {
        return None;
    }

    Some(UrlInfo {
        platform: platform_for_host(&host),
        direct_extension: direct_extension(path),
        host,
        path: path.to_string(),
    })
}

fn platform_for_host(host: &str) -> PlatformId {
    for (suffix, platform) in HOSTS {
        if host == *suffix || host.ends_with(&format!(".{suffix}")) {
            return *platform;
        }
    }
    for (prefix, platform) in HOST_PREFIXES {
        // Also match `www.pinterest.co.uk`, not just the bare host.
        let bare = host.strip_prefix("www.").unwrap_or(host);
        if bare.starts_with(prefix) {
            return *platform;
        }
    }
    PlatformId::Generic
}

/// The extension of the last path segment, when it is a known media type.
fn direct_extension(path: &str) -> Option<String> {
    let without_query = path.split(['?', '#']).next().unwrap_or(path);
    let segment = without_query.rsplit('/').next()?;
    let ext = segment.rsplit_once('.')?.1.to_ascii_lowercase();

    if ext.is_empty() || ext.len() > 5 {
        return None;
    }
    let known = VIDEO_EXTENSIONS.contains(&ext.as_str())
        || AUDIO_EXTENSIONS.contains(&ext.as_str())
        || IMAGE_EXTENSIONS.contains(&ext.as_str());

    known.then_some(ext)
}

pub fn is_video_extension(ext: &str) -> bool {
    VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn is_audio_extension(ext: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

/// Platform for display next to the input box.
pub fn detect_platform(raw: &str) -> PlatformId {
    match classify(raw) {
        Some(info) => {
            if info.platform == PlatformId::Generic && info.direct_extension.is_some() {
                PlatformId::Direct
            } else {
                info.platform
            }
        }
        None => PlatformId::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_services_are_named_and_nothing_else_is() {
        assert_eq!(protected_service("https://www.netflix.com/watch/80100172"), Some("Netflix"));
        assert_eq!(protected_service("https://www.disneyplus.com/video/abc"), Some("Disney+"));
        assert_eq!(protected_service("https://www.amazon.com.tr/gp/video/detail/B0B"), Some("Prime Video"));
        assert_eq!(protected_service("https://www.crunchyroll.com/watch/GR3VWXP96/"), Some("Crunchyroll"));
        assert_eq!(protected_service("https://www.blutv.com/izle/abc"), Some("BluTV"));

        assert_eq!(protected_service("https://www.amazon.com/dp/B000"), None, "a shop page is not a film");
        assert_eq!(protected_service("https://open.spotify.com/track/0VjIjW4GlUZAMYd2vXMi3b"), None);
        assert_eq!(protected_service("https://www.youtube.com/watch?v=abc"), None);
        assert_eq!(protected_service("https://notnetflix.com/x"), None);
        assert_eq!(protected_service("https://maxcdn.example.com/x"), None);
        assert_eq!(detect_platform("https://open.spotify.com/track/abc"), PlatformId::Spotify);
    }

    #[test]
    fn recognises_each_supported_host() {
        let cases = [
            ("https://www.youtube.com/watch?v=abc", PlatformId::Youtube),
            ("https://youtu.be/abc", PlatformId::Youtube),
            ("https://music.youtube.com/watch?v=abc", PlatformId::Youtube),
            ("https://vm.tiktok.com/ZMabc/", PlatformId::Tiktok),
            ("https://www.tiktok.com/@user/video/123", PlatformId::Tiktok),
            ("https://www.instagram.com/reel/abc/", PlatformId::Instagram),
            ("https://x.com/user/status/123", PlatformId::Twitter),
            ("https://twitter.com/user/status/123", PlatformId::Twitter),
            ("https://www.reddit.com/r/x/comments/abc/", PlatformId::Reddit),
            ("https://v.redd.it/abc", PlatformId::Reddit),
            ("https://fb.watch/abc/", PlatformId::Facebook),
            ("https://clips.twitch.tv/abc", PlatformId::Twitch),
            ("https://www.pinterest.co.uk/pin/123/", PlatformId::Pinterest),
            ("https://pin.it/abc", PlatformId::Pinterest),
            ("https://vimeo.com/123456", PlatformId::Vimeo),
            ("https://dai.ly/abc", PlatformId::Dailymotion),
            ("https://soundcloud.com/user/track", PlatformId::Soundcloud),
        ];
        for (url, expected) in cases {
            assert_eq!(detect_platform(url), expected, "for {url}");
        }
    }

    #[test]
    fn a_lookalike_host_is_not_matched() {
        // `notyoutube.com` must not match the `youtube.com` suffix.
        assert_eq!(detect_platform("https://notyoutube.com/watch"), PlatformId::Generic);
        assert_eq!(detect_platform("https://youtube.com.evil.test/x"), PlatformId::Generic);
    }

    #[test]
    fn direct_media_files_are_flagged() {
        assert_eq!(detect_platform("https://cdn.test/a/b/clip.mp4"), PlatformId::Direct);
        assert_eq!(detect_platform("https://cdn.test/song.mp3?token=1"), PlatformId::Direct);
        assert_eq!(detect_platform("https://cdn.test/pic.webp#x"), PlatformId::Direct);
    }

    #[test]
    fn a_platform_url_ending_in_an_extension_stays_on_its_platform() {
        assert_eq!(
            detect_platform("https://www.reddit.com/r/x/comments/abc/video.mp4"),
            PlatformId::Reddit
        );
    }

    #[test]
    fn non_http_and_malformed_input_is_unknown() {
        assert_eq!(detect_platform("file:///c:/x.mp4"), PlatformId::Unknown);
        assert_eq!(detect_platform("javascript:alert(1)"), PlatformId::Unknown);
        assert_eq!(detect_platform("not a url"), PlatformId::Unknown);
        assert_eq!(detect_platform(""), PlatformId::Unknown);
        assert_eq!(detect_platform("localhost"), PlatformId::Unknown);
    }

    #[test]
    fn scheme_relative_input_is_accepted() {
        assert_eq!(detect_platform("youtube.com/watch?v=abc"), PlatformId::Youtube);
    }

    #[test]
    fn an_at_sign_in_the_path_does_not_confuse_the_host() {
        let info = classify("https://www.tiktok.com/@user/video/123").unwrap();
        assert_eq!(info.host, "www.tiktok.com");
        assert_eq!(info.path, "/@user/video/123");
        assert_eq!(info.platform, PlatformId::Tiktok);
    }

    #[test]
    fn credentials_and_ports_are_stripped_from_the_host() {
        let info = classify("https://user:pw@www.vimeo.com:443/123").unwrap();
        assert_eq!(info.host, "www.vimeo.com");
        assert_eq!(info.platform, PlatformId::Vimeo);
    }

    #[test]
    fn unknown_extensions_are_not_direct() {
        assert!(classify("https://cdn.test/file.exe").unwrap().direct_extension.is_none());
        assert!(classify("https://cdn.test/page").unwrap().direct_extension.is_none());
    }
}
