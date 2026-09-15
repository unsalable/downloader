//! TikTok photo posts.
//!
//! A photo-mode post is a set of pictures over a soundtrack. The engine only
//! knows TikTok videos: it refuses a `/photo/` address outright, and reads the
//! same post under `/video/` as its soundtrack alone. The pictures are listed
//! in the data TikTok's own post page carries, which is read here.
//!
//! That page first answers with a proof-of-work check: the browser finds a
//! number whose hash matches the one given, and gets the post once a cookie
//! carries the answer. The same work is done here; it takes a fraction of a
//! second and is the check doing what it is for, not a way around it.

use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::{alphabet, Engine as _};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::{ACCEPT, COOKIE};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::model::{FormatKind, MediaFormat, MediaMetadata, PlatformId};
use crate::providers::{detect, image_format};
use crate::settings::Settings;

use super::{count, cookie_header, response_cookies, title_from_text, upload_date, Post};

const PAGE_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

/// The engine's title length for TikTok, so photo and video posts read alike.
const TITLE_CHARS: usize = 72;

/// The largest number the check asks for.
const MAX_ANSWER: u32 = 1_000_000;

/// TikTok's check tolerates base64 with or without padding.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

static UNIVERSAL_DATA: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<script[^>]+\bid="__UNIVERSAL_DATA_FOR_REHYDRATION__"[^>]*>(.*?)</script>"#)
        .expect("universal data pattern is valid")
});

/// Read a link when it leads to a photo post by its address: a `/photo/` page,
/// or a short link that redirects to one.
pub async fn analyze_photo_link(url: &str, settings: &Settings) -> AppResult<Option<MediaMetadata>> {
    let target = if is_short_link(url) {
        let client = crate::net::client(settings)?;
        // Only the redirect is needed; the body is never read.
        let response = client.get(url).header(ACCEPT, PAGE_ACCEPT).send().await?;
        response.url().to_string()
    } else {
        url.to_string()
    };

    match post_address(&target) {
        Some(address) if address.photo => {
            let mut metadata = analyze(&target, settings).await?;
            if let Some(metadata) = metadata.as_mut() {
                metadata.url = url.to_string();
            }
            Ok(metadata)
        }
        _ => Ok(None),
    }
}

/// Read a post as photos. `None` when it is a video after all.
pub async fn analyze(url: &str, settings: &Settings) -> AppResult<Option<MediaMetadata>> {
    let Some(address) = post_address(url) else {
        return Ok(None);
    };

    // The `/video/` page carries the post's data whatever kind it is; the
    // `/photo/` page leaves it to be loaded by script.
    let page_url = format!("https://www.tiktok.com/@{}/video/{}", address.user.as_deref().unwrap_or("_"), address.id);
    let page = fetch_post_page(&page_url, settings).await?;
    parse_page(&page, url)
}

struct PostAddress {
    id: String,
    user: Option<String>,
    photo: bool,
}

fn is_short_link(url: &str) -> bool {
    detect::classify(url).is_some_and(|info| {
        matches!(info.host.as_str(), "vm.tiktok.com" | "vt.tiktok.com")
            || (info.host.ends_with("tiktok.com") && info.path.starts_with("/t/"))
    })
}

/// The post a full TikTok address points at.
fn post_address(url: &str) -> Option<PostAddress> {
    let info = detect::classify(url)?;
    if !(info.host == "tiktok.com" || info.host.ends_with(".tiktok.com")) {
        return None;
    }
    let path = info.path.split(['?', '#']).next().unwrap_or_default();
    let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();

    let marker = segments.iter().position(|segment| *segment == "photo" || *segment == "video")?;
    let id = *segments.get(marker + 1)?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let user = marker
        .checked_sub(1)
        .and_then(|index| segments.get(index))
        .and_then(|segment| segment.strip_prefix('@'))
        .filter(|user| !user.is_empty() && *user != "_")
        .map(str::to_string);

    Some(PostAddress {
        id: id.to_string(),
        user,
        photo: segments[marker] == "photo",
    })
}

async fn fetch_post_page(page_url: &str, settings: &Settings) -> AppResult<String> {
    let client = crate::net::client(settings)?;

    let first = client.get(page_url).header(ACCEPT, PAGE_ACCEPT).send().await?;
    check_page(&first)?;
    let mut cookies = response_cookies(first.headers());
    let body = first.text().await?;
    if UNIVERSAL_DATA.is_match(&body) {
        return Ok(body);
    }

    let challenge = Challenge::from_page(&body).ok_or_else(|| {
        AppError::Parse("TikTok answered with a page that is neither the post nor its check".into())
    })?;
    // Hashing up to a million times is CPU work; it does not belong on the
    // thread that is serving everything else.
    let answered = tokio::task::spawn_blocking(move || challenge.answer())
        .await
        .map_err(|err| AppError::Other(format!("the page check was interrupted: {err}")))?
        .ok_or_else(|| AppError::Parse("TikTok's page check had no answer in range".into()))?;
    cookies.extend(answered);

    let second = client
        .get(page_url)
        .header(ACCEPT, PAGE_ACCEPT)
        .header(COOKIE, cookie_header(&cookies))
        .send()
        .await?;
    check_page(&second)?;
    Ok(second.text().await?)
}

fn check_page(response: &reqwest::Response) -> AppResult<()> {
    if response.url().path() == "/login" {
        return Err(AppError::Forbidden {
            status: 403,
            detail: "TikTok requires signing in to see this post".into(),
        });
    }
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::from_status(status.as_u16(), "the post page could not be read"));
    }
    Ok(())
}

/// The check a TikTok page answers with before the post.
#[derive(Debug)]
struct Challenge {
    /// The check as sent, decoded: a JSON object the answer is added to.
    payload: String,
    prefix: Vec<u8>,
    expected: Vec<u8>,
    cookie_name: String,
    /// A second cookie some checks ask to be echoed back.
    echo: Option<(String, String)>,
}

impl Challenge {
    fn from_page(page: &str) -> Option<Self> {
        let encoded = class_of(page, "cs")?;
        let payload = String::from_utf8(BASE64.decode(encoded.trim_end_matches('=')).ok()?).ok()?;
        let parsed: Value = serde_json::from_str(&payload).ok()?;
        let field = |key: &str| {
            parsed
                .pointer(&format!("/v/{key}"))
                .and_then(Value::as_str)
                .and_then(|value| BASE64.decode(value.trim_end_matches('=')).ok())
        };

        let echo = match (class_of(page, "rci"), class_of(page, "rs")) {
            (Some(name), Some(value)) if !name.is_empty() && !value.is_empty() => Some((name, value)),
            _ => None,
        };

        Some(Self {
            prefix: field("a")?,
            expected: field("c")?,
            cookie_name: class_of(page, "wci").filter(|name| !name.is_empty())?,
            echo,
            payload,
        })
    }

    /// The cookies that carry the answer, when there is one.
    fn answer(self) -> Option<Vec<(String, String)>> {
        let base = Sha256::new_with_prefix(&self.prefix);
        let number = (0..=MAX_ANSWER).find(|number| {
            base.clone().chain_update(number.to_string().as_bytes()).finalize().as_slice() == self.expected.as_slice()
        })?;

        // The answer joins the check's own fields, which are otherwise sent
        // back exactly as they came.
        let body = self.payload.trim_end().strip_suffix('}')?;
        let answered = format!("{body},\"d\":\"{}\"}}", BASE64.encode(number.to_string()));

        let mut cookies = vec![(self.cookie_name, BASE64.encode(answered))];
        cookies.extend(self.echo);
        Some(cookies)
    }
}

/// The `class` attribute of the element with the given id.
fn class_of(page: &str, id: &str) -> Option<String> {
    let element = Regex::new(&format!(r#"<[a-zA-Z]+\s[^>]*\bid="{}"[^>]*>"#, regex::escape(id))).ok()?;
    let tag = element.find(page)?.as_str();
    static CLASS: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\bclass="([^"]*)""#).expect("class pattern is valid"));
    CLASS.captures(tag).and_then(|found| found.get(1)).map(|value| value.as_str().to_string())
}

/// The post from its page, as photos; `None` when it is a video.
fn parse_page(page: &str, requested_url: &str) -> AppResult<Option<MediaMetadata>> {
    let data = UNIVERSAL_DATA
        .captures(page)
        .and_then(|found| found.get(1))
        .ok_or_else(|| AppError::Parse("the post page carried no post data".into()))?;
    let data: Value = serde_json::from_str(data.as_str())?;
    let detail = data
        .pointer("/__DEFAULT_SCOPE__/webapp.video-detail")
        .ok_or_else(|| AppError::Parse("the post page carried no post".into()))?;

    match detail.get("statusCode").and_then(Value::as_i64).unwrap_or(0) {
        0 => {}
        // A private post, or a private account.
        10216 | 10222 => {
            return Err(AppError::Forbidden {
                status: 403,
                detail: "this TikTok post is private".into(),
            })
        }
        10204 => {
            return Err(AppError::Forbidden {
                status: 403,
                detail: "TikTok does not show this post in this region".into(),
            })
        }
        code => {
            return Err(AppError::NotFound {
                status: 404,
                detail: format!("TikTok reported the post unavailable (status {code})"),
            })
        }
    }

    let Some(item) = detail.pointer("/itemInfo/itemStruct") else {
        return Ok(None);
    };
    Ok(parse_item(item, requested_url))
}

fn parse_item(item: &Value, requested_url: &str) -> Option<MediaMetadata> {
    let images = item.pointer("/imagePost/images").and_then(Value::as_array)?;
    let id = item.get("id").and_then(Value::as_str)?;

    let author = item.get("author");
    let handle = author.and_then(|author| author.get("uniqueId")).and_then(Value::as_str);
    let nickname = author.and_then(|author| author.get("nickname")).and_then(Value::as_str);
    let caption = item.get("desc").and_then(Value::as_str).unwrap_or_default();

    let title = title_from_text(caption, TITLE_CHARS)
        .or_else(|| {
            item.pointer("/imagePost/title")
                .and_then(Value::as_str)
                .and_then(|title| title_from_text(title, TITLE_CHARS))
        })
        .or_else(|| nickname.or(handle).map(|name| format!("Photo by {name}")))
        .unwrap_or_else(|| format!("TikTok post {id}"));

    let post = Post {
        platform: PlatformId::Tiktok,
        url: requested_url.to_string(),
        canonical_url: format!("https://www.tiktok.com/@{}/photo/{id}", handle.unwrap_or("_")),
        title,
        creator: handle.or(nickname).map(str::to_string),
        description: (!caption.trim().is_empty()).then(|| caption.to_string()),
        upload_date: item
            .get("createTime")
            .and_then(|time| time.as_i64().or_else(|| time.as_str()?.parse().ok()))
            .and_then(upload_date),
        view_count: count(item.pointer("/stats/playCount")),
        like_count: count(item.pointer("/stats/diggCount")),
    };

    let items: Vec<MediaMetadata> = images
        .iter()
        .enumerate()
        .filter_map(|(index, image)| {
            let address = image.pointer("/imageURL/urlList/0").and_then(Value::as_str)?;
            let dimension = |key: &str| {
                image
                    .get(key)
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|value| *value > 0)
            };
            let photo = image_format(
                format!("image-{}", index + 1),
                address,
                dimension("imageWidth"),
                dimension("imageHeight"),
                vec![("Referer".to_string(), "https://www.tiktok.com/".to_string())],
            );
            Some(post.item(photo, None))
        })
        .collect();

    let mut metadata = post.assemble(items)?;

    // The soundtrack belongs to the post, not to any one picture: it is offered
    // with the post as a whole, and the pictures download without it.
    if let Some(sound) = item.pointer("/music/playUrl").and_then(Value::as_str).filter(|url| url.starts_with("http")) {
        metadata.formats.push(soundtrack(sound));
    }
    if let Some(cover) = item.pointer("/imagePost/cover/imageURL/urlList/0").and_then(Value::as_str) {
        metadata.thumbnail_url = Some(cover.to_string());
    }
    Some(metadata)
}

fn soundtrack(url: &str) -> MediaFormat {
    MediaFormat {
        id: "soundtrack".to_string(),
        kind: FormatKind::Audio,
        container: "m4a".to_string(),
        protocol: "https".to_string(),
        has_video: false,
        has_audio: true,
        width: None,
        height: None,
        fps: None,
        vcodec: None,
        acodec: Some("aac".to_string()),
        tbr: None,
        vbr: None,
        abr: None,
        filesize: None,
        filesize_approx: None,
        quality_label: "Audio".to_string(),
        watermarked: None,
        note: None,
        needs_engine_download: false,
        url: Some(url.to_string()),
        http_headers: vec![("Referer".to_string(), "https://www.tiktok.com/".to_string())],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaKind;

    #[test]
    fn recognises_post_and_short_link_addresses() {
        let address = post_address("https://www.tiktok.com/@natgeo/photo/7680195684426927373?is_from_webapp=1").unwrap();
        assert_eq!(address.id, "7680195684426927373");
        assert_eq!(address.user.as_deref(), Some("natgeo"));
        assert!(address.photo);

        let address = post_address("https://www.tiktok.com/@/video/7528822871041379606?_r=1").unwrap();
        assert!(!address.photo);
        assert_eq!(address.user, None);

        assert!(post_address("https://www.tiktok.com/@natgeo").is_none());
        assert!(post_address("https://example.com/@a/photo/1").is_none());

        assert!(is_short_link("https://vm.tiktok.com/ZMSnxVwqV/"));
        assert!(is_short_link("https://vt.tiktok.com/ZS123/"));
        assert!(is_short_link("https://www.tiktok.com/t/ZT8abc/"));
        assert!(!is_short_link("https://www.tiktok.com/@a/video/1"));
    }

    /// A check built the way TikTok builds one, with a known answer.
    fn challenge_page(answer: u32) -> String {
        let prefix = b"0123456789abcdef0123456789abcdef";
        let expected = Sha256::new_with_prefix(prefix).chain_update(answer.to_string()).finalize();
        let payload = format!(
            r#"{{"v":{{"a":"{}","b":1789461982,"c":"{}"}},"s":"c2lnbmF0dXJl"}}"#,
            BASE64.encode(prefix),
            BASE64.encode(expected)
        );
        let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(payload);
        format!(
            r#"<body> Please wait... <p id="wci" class="_wafchallengeid"></p> <p id="cs" class="{encoded}"></p> <p id="rci" class="waforiginalreid"></p> <p id="rs" class=""></p></body>"#
        )
    }

    #[test]
    fn the_page_check_is_answered_and_sent_back_whole() {
        let challenge = Challenge::from_page(&challenge_page(4242)).unwrap();
        let cookies = challenge.answer().unwrap();

        // An empty echo value is not sent.
        assert_eq!(cookies.len(), 1);
        let (name, value) = &cookies[0];
        assert_eq!(name, "_wafchallengeid");

        let returned: Value = serde_json::from_slice(&BASE64.decode(value).unwrap()).unwrap();
        assert_eq!(returned["d"], BASE64.encode("4242"));
        assert_eq!(returned["s"], "c2lnbmF0dXJl");
        assert_eq!(returned["v"]["b"], 1789461982);
    }

    #[test]
    fn a_page_that_is_not_a_check_is_not_mistaken_for_one() {
        assert!(Challenge::from_page("<html><body>Hello</body></html>").is_none());
        assert!(Challenge::from_page(r#"<p id="cs" class="not base64 json"></p>"#).is_none());
    }

    fn page(item: Value, status: i64) -> String {
        let data = serde_json::json!({
            "__DEFAULT_SCOPE__": {
                "webapp.video-detail": { "statusCode": status, "itemInfo": { "itemStruct": item } }
            }
        });
        format!(r#"<html><script id="__UNIVERSAL_DATA_FOR_REHYDRATION__" type="application/json">{data}</script></html>"#)
    }

    fn photo_post(count: usize) -> Value {
        let images: Vec<Value> = (0..count)
            .map(|index| {
                serde_json::json!({
                    "imageWidth": 1080,
                    "imageHeight": 1350,
                    "imageURL": { "urlList": [
                        format!("https://p16-sign.tiktokcdn.test/photomode/{index}~tplv-photomode-image.jpeg?x-expires=1"),
                        format!("https://p19-sign.tiktokcdn.test/photomode/{index}~tplv-photomode-image.jpeg?x-expires=1"),
                    ]},
                })
            })
            .collect();
        serde_json::json!({
            "id": "7680195684426927373",
            "desc": "In 19th-century England, medical students could only legally dissect the bodies of murderers.",
            "createTime": 1788188400,
            "author": { "uniqueId": "natgeo", "nickname": "National Geographic" },
            "stats": { "playCount": 14000, "diggCount": 372, "collectCount": "21" },
            "music": { "playUrl": "https://sf16-music.tiktokcdn.test/obj/sound" },
            "imagePost": {
                "images": images,
                "cover": { "imageURL": { "urlList": ["https://p16-sign.tiktokcdn.test/photomode/cover.jpeg"] } },
                "title": "",
            },
            "video": { "duration": 0, "playAddr": "" },
        })
    }

    #[test]
    fn a_photo_post_becomes_a_gallery_with_its_soundtrack_on_the_post() {
        let url = "https://www.tiktok.com/@natgeo/photo/7680195684426927373";
        let post = parse_page(&page(photo_post(3), 0), url).unwrap().unwrap();

        assert_eq!(post.media_kind, MediaKind::Gallery);
        assert_eq!(post.entry_count, Some(3));
        assert_eq!(post.creator.as_deref(), Some("natgeo"));
        assert_eq!(post.canonical_url, url);
        assert_eq!(post.upload_date.as_deref(), Some("20260831"));
        assert!(post.title.ends_with("...") && post.title.chars().count() <= TITLE_CHARS);
        assert_eq!(post.thumbnail_url.as_deref(), Some("https://p16-sign.tiktokcdn.test/photomode/cover.jpeg"));

        // The post as a whole: its first picture, and the sound.
        assert!(post.formats.iter().any(|format| format.kind == FormatKind::Image));
        assert!(post.formats.iter().any(|format| format.kind == FormatKind::Audio));

        // Each picture alone: just the picture.
        let second = &post.entries[1];
        assert_eq!(second.formats.len(), 1);
        assert_eq!(second.formats[0].container, "jpg");
        assert_eq!(second.formats[0].quality_label, "1080x1350");
        assert!(second.formats[0].url.as_deref().unwrap().contains("/photomode/1~"));
    }

    #[test]
    fn a_single_picture_post_is_a_photo_with_its_sound() {
        let post = parse_page(&page(photo_post(1), 0), "https://www.tiktok.com/@natgeo/photo/1").unwrap().unwrap();
        assert_eq!(post.media_kind, MediaKind::Image);
        assert_eq!(post.formats.len(), 2);
    }

    #[test]
    fn a_video_post_is_not_read_as_photos() {
        let video = serde_json::json!({ "id": "1", "desc": "a video", "video": { "duration": 12, "playAddr": "https://v.test/v.mp4" } });
        assert!(parse_page(&page(video, 0), "https://www.tiktok.com/@a/video/1").unwrap().is_none());
    }

    #[test]
    fn a_private_or_missing_post_says_so() {
        assert_eq!(parse_page(&page(serde_json::json!({}), 10222), "u").unwrap_err().code(), "forbidden");
        assert_eq!(parse_page(&page(serde_json::json!({}), 10204), "u").unwrap_err().code(), "forbidden");
        assert_eq!(parse_page(&page(serde_json::json!({}), 10000), "u").unwrap_err().code(), "notFound");
        assert!(parse_page("<html>nothing</html>", "u").is_err());
    }
}
