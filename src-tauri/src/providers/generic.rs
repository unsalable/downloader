//! Last-resort provider: read the page and see what it advertises.
//!
//! Runs only after the engine has said it does not know the site. It looks at
//! Open Graph and Twitter Card tags and at plain `<video>`/`<source>` elements
//! -- all of which are the page telling anyone who asks where its media is.
//! Nothing here defeats access control; a page that does not publish a media
//! URL simply yields no formats.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::error::{AppError, AppResult};
use crate::model::{FormatKind, MediaFormat, MediaKind, MediaMetadata, WatermarkSupport};
use crate::providers::detect;
use crate::settings::Settings;

pub const PROVIDER_ID: &str = "generic";

/// Pages are only read far enough to reach `<head>`; a full page body is not
/// needed and could be many megabytes.
const MAX_HTML_BYTES: usize = 512 * 1024;

static META_TAG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<meta\s+[^>]*?(?:property|name)\s*=\s*["']([^"']+)["'][^>]*?content\s*=\s*["']([^"']*)["'][^>]*>"#)
        .expect("meta tag pattern is valid")
});

static META_TAG_REVERSED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<meta\s+[^>]*?content\s*=\s*["']([^"']*)["'][^>]*?(?:property|name)\s*=\s*["']([^"']+)["'][^>]*>"#)
        .expect("reversed meta tag pattern is valid")
});

static SOURCE_TAG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<(?:video|audio|source)\s+[^>]*?src\s*=\s*["']([^"']+)["']"#)
        .expect("source tag pattern is valid")
});

static TITLE_TAG: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title pattern is valid"));

pub struct GenericProvider;

impl GenericProvider {
    pub fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    pub fn can_handle(&self, url: &str) -> bool {
        detect::classify(url).is_some()
    }

    pub async fn analyze(&self, url: &str, settings: &Settings) -> AppResult<MediaMetadata> {
        let client = crate::net::client(settings)?;
        let response = client
            .get(url)
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(AppError::from_status(status.as_u16(), "the page could not be read"));
        }

        let final_url = response.url().to_string();
        let body = response.text().await?;
        let html = &body[..body.len().min(MAX_HTML_BYTES)];

        let meta = collect_meta(html);
        let mut formats = Vec::new();

        // Video first: a page with both a video and a poster image means video.
        for key in ["og:video:secure_url", "og:video:url", "og:video", "twitter:player:stream"] {
            if let Some(candidate) = meta.get(key) {
                push_format(&mut formats, candidate, &final_url, FormatKind::Muxed);
            }
        }

        if formats.is_empty() {
            for capture in SOURCE_TAG.captures_iter(html).take(6) {
                if let Some(src) = capture.get(1) {
                    push_format(&mut formats, src.as_str(), &final_url, FormatKind::Muxed);
                }
            }
        }

        if formats.is_empty() {
            for key in ["og:audio:secure_url", "og:audio", "twitter:audio:stream"] {
                if let Some(candidate) = meta.get(key) {
                    push_format(&mut formats, candidate, &final_url, FormatKind::Audio);
                }
            }
        }

        let image = ["og:image:secure_url", "og:image", "twitter:image"]
            .iter()
            .find_map(|key| meta.get(*key))
            .and_then(|value| absolutize(value, &final_url));

        if formats.is_empty() {
            if let Some(image_url) = image.as_deref() {
                push_format(&mut formats, image_url, &final_url, FormatKind::Image);
            }
        }

        if formats.is_empty() {
            return Err(AppError::Unsupported(
                "the page does not publish a media file".into(),
            ));
        }

        let media_kind = match formats[0].kind {
            FormatKind::Image => MediaKind::Image,
            FormatKind::Audio => MediaKind::Audio,
            _ => MediaKind::Video,
        };

        let title = meta
            .get("og:title")
            .or_else(|| meta.get("twitter:title"))
            .cloned()
            .or_else(|| {
                TITLE_TAG
                    .captures(html)
                    .and_then(|c| c.get(1))
                    .map(|m| decode_entities(m.as_str().trim()))
            })
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                detect::classify(&final_url)
                    .map(|info| info.host)
                    .unwrap_or_else(|| "Untitled".to_string())
            });

        let platform = detect::detect_platform(&final_url);

        Ok(MediaMetadata {
            url: url.to_string(),
            canonical_url: meta
                .get("og:url")
                .cloned()
                .unwrap_or_else(|| final_url.clone()),
            platform,
            platform_label: platform.label().to_string(),
            provider_id: PROVIDER_ID.to_string(),
            media_kind,
            title: decode_entities(&title),
            creator: meta
                .get("og:site_name")
                .cloned()
                .or_else(|| detect::classify(&final_url).map(|info| info.host)),
            description: meta
                .get("og:description")
                .or_else(|| meta.get("description"))
                .map(|value| decode_entities(value).chars().take(400).collect()),
            thumbnail_url: image,
            duration_sec: meta
                .get("og:video:duration")
                .or_else(|| meta.get("video:duration"))
                .and_then(|value| value.parse::<f64>().ok()),
            view_count: None,
            like_count: None,
            upload_date: None,
            is_live: false,
            formats,
            entry_count: None,
            watermark_support: WatermarkSupport::NotApplicable,
            warnings: vec!["generic".to_string()],
            entries: Vec::new(),
        })
    }
}

fn collect_meta(html: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for capture in META_TAG.captures_iter(html) {
        if let (Some(key), Some(value)) = (capture.get(1), capture.get(2)) {
            map.entry(key.as_str().to_ascii_lowercase())
                .or_insert_with(|| decode_entities(value.as_str()));
        }
    }
    // Attribute order is not fixed in HTML, so `content` may precede `property`.
    for capture in META_TAG_REVERSED.captures_iter(html) {
        if let (Some(value), Some(key)) = (capture.get(1), capture.get(2)) {
            map.entry(key.as_str().to_ascii_lowercase())
                .or_insert_with(|| decode_entities(value.as_str()));
        }
    }
    map
}

fn push_format(formats: &mut Vec<MediaFormat>, candidate: &str, base: &str, kind: FormatKind) {
    let Some(absolute) = absolutize(candidate, base) else {
        return;
    };
    if formats.iter().any(|existing| existing.url.as_deref() == Some(absolute.as_str())) {
        return;
    }

    let extension = detect::classify(&absolute)
        .and_then(|info| info.direct_extension)
        .unwrap_or_else(|| match kind {
            FormatKind::Image => "jpg".to_string(),
            FormatKind::Audio => "mp3".to_string(),
            _ => "mp4".to_string(),
        });

    // A manifest advertised in a meta tag still needs a segmented downloader.
    let segmented = absolute.contains(".m3u8") || absolute.contains(".mpd");

    formats.push(MediaFormat {
        id: format!("generic-{}", formats.len()),
        kind,
        container: extension,
        protocol: if segmented { "m3u8".to_string() } else { "https".to_string() },
        has_video: matches!(kind, FormatKind::Muxed),
        has_audio: matches!(kind, FormatKind::Muxed | FormatKind::Audio),
        width: None,
        height: None,
        fps: None,
        vcodec: None,
        acodec: None,
        tbr: None,
        vbr: None,
        abr: None,
        filesize: None,
        filesize_approx: None,
        quality_label: "Original".to_string(),
        watermarked: None,
        note: None,
        needs_engine_download: segmented,
        url: Some(absolute),
        http_headers: Vec::new(),
    });
}

/// Resolve a possibly-relative URL against the page it came from.
fn absolutize(candidate: &str, base: &str) -> Option<String> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return None;
    }

    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        return Some(candidate.to_string());
    }
    // Anything else with a scheme (data:, blob:, javascript:) is not fetchable.
    if candidate.contains("://") || candidate.starts_with("data:") || candidate.starts_with("javascript:") {
        return None;
    }

    let (scheme, rest) = base.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;

    if let Some(stripped) = candidate.strip_prefix("//") {
        return Some(format!("{scheme}://{stripped}"));
    }
    if candidate.starts_with('/') {
        return Some(format!("{scheme}://{authority}{candidate}"));
    }

    let base_path = rest
        .get(authority.len()..)
        .unwrap_or("/")
        .split(['?', '#'])
        .next()
        .unwrap_or("/");
    let directory = base_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    Some(format!("{scheme}://{authority}{directory}/{candidate}"))
}

/// Only the entities that actually show up in meta tags. A full HTML entity
/// table would be dead weight for the handful that matter here.
fn decode_entities(input: &str) -> String {
    let mut out = input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");

    // Numeric references, e.g. &#8217;
    while let Some(start) = out.find("&#") {
        let Some(end) = out[start..].find(';').map(|index| start + index) else {
            break;
        };
        let body = &out[start + 2..end];
        let parsed = if let Some(hex) = body.strip_prefix('x').or_else(|| body.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()
        } else {
            body.parse::<u32>().ok()
        };
        match parsed.and_then(char::from_u32) {
            Some(ch) => out.replace_range(start..=end, &ch.to_string()),
            // Leave an unparseable reference alone, but stop scanning so this
            // cannot loop forever on the same position.
            None => break,
        }
    }

    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_open_graph_tags_in_either_attribute_order() {
        let html = r#"
            <meta property="og:title" content="Hello">
            <meta content="https://cdn.test/v.mp4" property="og:video">
        "#;
        let meta = collect_meta(html);
        assert_eq!(meta.get("og:title").map(String::as_str), Some("Hello"));
        assert_eq!(
            meta.get("og:video").map(String::as_str),
            Some("https://cdn.test/v.mp4")
        );
    }

    #[test]
    fn resolves_relative_urls_against_the_page() {
        let base = "https://host.test/a/b/page.html";
        assert_eq!(
            absolutize("/x/v.mp4", base).as_deref(),
            Some("https://host.test/x/v.mp4")
        );
        assert_eq!(
            absolutize("v.mp4", base).as_deref(),
            Some("https://host.test/a/b/v.mp4")
        );
        assert_eq!(
            absolutize("//cdn.test/v.mp4", base).as_deref(),
            Some("https://cdn.test/v.mp4")
        );
        assert_eq!(
            absolutize("https://other.test/v.mp4", base).as_deref(),
            Some("https://other.test/v.mp4")
        );
    }

    #[test]
    fn rejects_non_fetchable_schemes() {
        let base = "https://host.test/page";
        assert!(absolutize("data:video/mp4;base64,AAAA", base).is_none());
        assert!(absolutize("javascript:void(0)", base).is_none());
        assert!(absolutize("", base).is_none());
    }

    #[test]
    fn decodes_the_entities_that_appear_in_meta_tags() {
        assert_eq!(decode_entities("a &amp; b &#39;c&#39;"), "a & b 'c'");
        assert_eq!(decode_entities("&#x2019;"), "\u{2019}");
    }

    #[test]
    fn an_undecodable_entity_does_not_loop() {
        assert_eq!(decode_entities("&#zz; tail"), "&#zz; tail");
    }

    #[test]
    fn a_manifest_url_is_marked_for_the_engine() {
        let mut formats = Vec::new();
        push_format(
            &mut formats,
            "https://cdn.test/stream.m3u8",
            "https://host.test/",
            FormatKind::Muxed,
        );
        assert!(formats[0].needs_engine_download);
    }

    #[test]
    fn duplicate_sources_are_collapsed() {
        let mut formats = Vec::new();
        push_format(&mut formats, "https://cdn.test/v.mp4", "https://h/", FormatKind::Muxed);
        push_format(&mut formats, "https://cdn.test/v.mp4", "https://h/", FormatKind::Muxed);
        assert_eq!(formats.len(), 1);
    }
}
