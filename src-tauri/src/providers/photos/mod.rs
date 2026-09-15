//! Photo posts the engine does not return.
//!
//! The engine is built around video. On X it skips a post's photos, on Reddit
//! it fails to fetch an image post's picture, and a TikTok photo-mode post is
//! either refused or read as its soundtrack alone. Each module here reads one
//! platform's photo posts from the same public data its own website loads for
//! a signed-out visitor, and nothing else: a post that turns out to be a
//! video, or one that cannot be read this way, is left to the engine exactly
//! as before.
//!
//! Nothing here signs in or gets around a restriction. A post the platform
//! only shows to signed-in accounts is not readable here either.

pub mod reddit;
pub mod tiktok;
pub mod twitter;

use reqwest::header::{HeaderMap, SET_COOKIE};

use crate::error::AppResult;
use crate::model::{FormatKind, MediaFormat, MediaKind, MediaMetadata, PlatformId, WatermarkSupport};
use crate::settings::Settings;
use crate::log_warn;

pub const PROVIDER_ID: &str = "photos";

/// What a photo reader made of a link.
pub enum Reading {
    /// A post of photos, read in full.
    Photos(Box<MediaMetadata>),
    /// The photos of a post that also has videos, waiting for the engine's
    /// reading of those to be put in order around them.
    WithVideos(MixedPost),
    /// Not a photo post, or not one read here: the engine's.
    Elsewhere,
}

impl Reading {
    fn photos(metadata: MediaMetadata) -> Self {
        Self::Photos(Box::new(metadata))
    }
}

/// A post of photos and videos, as far as reading its photos goes.
pub struct MixedPost {
    title: String,
    canonical_url: String,
    /// The post's media in order: each photo, and `None` where a video is.
    slots: Vec<Option<MediaMetadata>>,
}

impl MixedPost {
    /// The whole post, with the engine's videos in their places among the
    /// photos. Should the engine count the videos differently, its reading
    /// is kept as it is rather than guessed into place.
    fn merge(self, engine: MediaMetadata) -> MediaMetadata {
        let videos = if engine.entries.is_empty() {
            vec![engine.clone()]
        } else {
            engine.entries.clone()
        };
        if videos.len() != self.slots.iter().filter(|slot| slot.is_none()).count() {
            log_warn!("photos", "the engine read a different number of videos; leaving out the photos");
            return engine;
        }

        let mut videos = videos.into_iter();
        let items: Vec<MediaMetadata> = self
            .slots
            .into_iter()
            .filter_map(|slot| slot.or_else(|| videos.next()))
            .collect();
        super::gallery(Some(self.title), Some(self.canonical_url), items).unwrap_or(engine)
    }
}

/// Read `url` as a photo post, when it is the kind of link read here.
pub async fn analyze(url: &str, platform: PlatformId, settings: &Settings) -> AppResult<Reading> {
    let whole = |found: Option<MediaMetadata>| found.map_or(Reading::Elsewhere, Reading::photos);
    match platform {
        PlatformId::Twitter => twitter::analyze(url, settings).await,
        PlatformId::Reddit => reddit::analyze(url, settings).await.map(whole),
        PlatformId::Tiktok => tiktok::analyze_photo_link(url, settings).await.map(whole),
        _ => Ok(Reading::Elsewhere),
    }
}

/// Fill in what the engine missed about a post it did read.
///
/// The photos of a post that also has videos go in among the videos. And a
/// TikTok photo post reached through a video address comes back from the
/// engine as its soundtrack alone -- TikTok has no audio-only posts, so a
/// result without a picture or a video is exactly that case.
pub async fn complete(metadata: MediaMetadata, mixed: Option<MixedPost>, settings: &Settings) -> MediaMetadata {
    if let Some(post) = mixed {
        return post.merge(metadata);
    }

    let soundtrack_only = metadata.platform == PlatformId::Tiktok
        && metadata.entries.is_empty()
        && !metadata
            .formats
            .iter()
            .any(|format| format.has_video || format.kind == FormatKind::Image);
    if !soundtrack_only {
        return metadata;
    }

    match tiktok::analyze(&metadata.canonical_url, settings).await {
        Ok(Some(mut photos)) => {
            photos.url = metadata.url;
            photos
        }
        Ok(None) => metadata,
        Err(err) => {
            log_warn!("photos", "could not read the photos of a TikTok post: {err}");
            metadata
        }
    }
}

/// What a post says about itself, shared by each of its items.
struct Post {
    platform: PlatformId,
    url: String,
    canonical_url: String,
    title: String,
    creator: Option<String>,
    description: Option<String>,
    upload_date: Option<String>,
    view_count: Option<u64>,
    like_count: Option<u64>,
}

impl Post {
    /// One picture of the post, as a downloadable item.
    fn item(&self, photo: MediaFormat, thumbnail_url: Option<String>) -> MediaMetadata {
        MediaMetadata {
            url: self.url.clone(),
            canonical_url: self.canonical_url.clone(),
            platform: self.platform,
            platform_label: self.platform.label().to_string(),
            provider_id: PROVIDER_ID.to_string(),
            media_kind: MediaKind::Image,
            title: self.title.clone(),
            creator: self.creator.clone(),
            description: self.description.clone(),
            thumbnail_url: thumbnail_url.or_else(|| photo.url.clone()),
            duration_sec: None,
            view_count: self.view_count,
            like_count: self.like_count,
            upload_date: self.upload_date.clone(),
            is_live: false,
            formats: vec![photo],
            entry_count: None,
            watermark_support: WatermarkSupport::NotApplicable,
            warnings: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// The whole post from its pictures: a gallery, or the one photo.
    fn assemble(&self, items: Vec<MediaMetadata>) -> Option<MediaMetadata> {
        super::gallery(Some(self.title.clone()), Some(self.canonical_url.clone()), items)
    }
}

/// `name=value` of every cookie a response sets. Only what is needed to make
/// the follow-up request a browser would make; nothing is kept beyond it.
fn response_cookies(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|cookie| {
            let (name, value) = cookie.split(';').next()?.split_once('=')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_string(), value.trim().to_string()))
        })
        .collect()
}

fn cookie_header(cookies: &[(String, String)]) -> String {
    cookies
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// `YYYYMMDD`, the form upload dates travel in, from a Unix time.
fn upload_date(unix_seconds: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(unix_seconds, 0).map(|time| time.format("%Y%m%d").to_string())
}

/// A caption shortened to title length, on a character boundary.
fn title_from_text(text: &str, max_chars: usize) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(if line.chars().count() > max_chars {
        format!("{}...", line.chars().take(max_chars.saturating_sub(3)).collect::<String>().trim_end())
    } else {
        line.to_string()
    })
}

/// A count that a platform may send as a number or as a numeric string.
fn count(value: Option<&serde_json::Value>) -> Option<u64> {
    let value = value?;
    value.as_u64().or_else(|| value.as_str()?.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::image_format;
    use reqwest::header::HeaderValue;

    fn post() -> Post {
        Post {
            platform: PlatformId::Twitter,
            url: "https://x.com/a/status/1".into(),
            canonical_url: "https://x.com/a/status/1".into(),
            title: "A post".into(),
            creator: Some("a".into()),
            description: None,
            upload_date: None,
            view_count: None,
            like_count: None,
        }
    }

    fn photo(name: &str) -> MediaMetadata {
        post().item(image_format("image", format!("https://pbs.test/{name}"), None, None, Vec::new()), None)
    }

    fn video(name: &str) -> MediaMetadata {
        let mut item = photo(name);
        item.provider_id = "engine".into();
        item.media_kind = MediaKind::Video;
        let stream = &mut item.formats[0];
        stream.kind = FormatKind::Muxed;
        stream.has_video = true;
        stream.has_audio = true;
        stream.url = Some(format!("https://video.test/{name}.mp4"));
        item
    }

    fn mixed(slots: Vec<Option<MediaMetadata>>) -> MixedPost {
        MixedPost {
            title: "A post".into(),
            canonical_url: "https://x.com/a/status/1".into(),
            slots,
        }
    }

    fn addresses(post: &MediaMetadata) -> Vec<String> {
        post.entries.iter().map(|item| item.formats[0].url.clone().unwrap()).collect()
    }

    #[test]
    fn the_engines_videos_go_between_the_photos_in_order() {
        let engine = super::super::gallery(Some("x".into()), None, vec![video("v1"), video("v2")]).unwrap();
        let post = mixed(vec![None, Some(photo("p1")), None]).merge(engine);

        assert_eq!(post.entry_count, Some(3));
        assert_eq!(
            addresses(&post),
            ["https://video.test/v1.mp4", "https://pbs.test/p1", "https://video.test/v2.mp4"]
        );
        assert_eq!(post.entries[1].provider_id, PROVIDER_ID);
        assert_eq!(post.entries[2].provider_id, "engine");
        assert_eq!(post.title, "A post");
    }

    #[test]
    fn a_single_video_from_the_engine_takes_its_one_place() {
        let post = mixed(vec![Some(photo("p1")), None]).merge(video("v1"));
        assert_eq!(addresses(&post), ["https://pbs.test/p1", "https://video.test/v1.mp4"]);
    }

    #[test]
    fn a_different_video_count_keeps_the_engines_reading() {
        let post = mixed(vec![Some(photo("p1")), None, None]).merge(video("v1"));
        assert!(post.entries.is_empty());
        assert_eq!(post.formats[0].url.as_deref(), Some("https://video.test/v1.mp4"));
    }

    #[test]
    fn cookies_are_read_as_name_and_value_only() {
        let mut headers = HeaderMap::new();
        headers.append(SET_COOKIE, HeaderValue::from_static("loid=abc.def; Domain=.reddit.com; Path=/; Secure"));
        headers.append(SET_COOKIE, HeaderValue::from_static("csv=2; Max-Age=63072000"));
        headers.append(SET_COOKIE, HeaderValue::from_static("=junk"));

        let cookies = response_cookies(&headers);
        assert_eq!(
            cookies,
            [("loid".to_string(), "abc.def".to_string()), ("csv".to_string(), "2".to_string())]
        );
        assert_eq!(cookie_header(&cookies), "loid=abc.def; csv=2");
    }

    #[test]
    fn a_long_caption_is_shortened_to_a_title() {
        assert_eq!(title_from_text("\n  Short one  \nsecond", 72).as_deref(), Some("Short one"));
        let long = "word ".repeat(40);
        let title = title_from_text(&long, 20).unwrap();
        assert!(title.ends_with("...") && title.chars().count() <= 20, "{title}");
        assert_eq!(title_from_text("   \n ", 72), None);
    }

    #[test]
    fn counts_arrive_as_numbers_or_strings() {
        assert_eq!(count(Some(&serde_json::json!(372))), Some(372));
        assert_eq!(count(Some(&serde_json::json!("21"))), Some(21));
        assert_eq!(count(Some(&serde_json::json!("n/a"))), None);
        assert_eq!(count(None), None);
    }

    #[test]
    fn an_upload_date_is_written_as_the_engine_writes_one() {
        assert_eq!(upload_date(1_546_621_545).as_deref(), Some("20190104"));
    }
}
