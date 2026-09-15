//! Photos on Reddit.
//!
//! The engine reads a Reddit post and then fetches an image post's picture as
//! if it were a web page, which Reddit's image host answers with a redirect
//! that ends in a refusal. Galleries fare the same. The post's own JSON --
//! what Reddit's website loads -- names every picture directly, so image posts
//! and galleries are read from it here. Video and link posts are left to the
//! engine, which handles those.
//!
//! Reddit serves that JSON to a signed-out visitor who has the anonymous
//! session cookie its website hands out, so that visit is made first, once.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use reqwest::header::{ACCEPT, COOKIE};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::model::{MediaMetadata, PlatformId};
use crate::providers::{detect, image_format};
use crate::settings::Settings;

use super::{cookie_header, response_cookies, upload_date, Post};

/// The anonymous session lasts far longer than this; renewing it now and then
/// costs one request and keeps a stale one from failing every read.
const SESSION_TTL: Duration = Duration::from_secs(6 * 60 * 60);

struct Session {
    cookies: String,
    /// The network settings it was made under.
    signature: String,
    at: Instant,
}

static SESSION: Lazy<Mutex<Option<Session>>> = Lazy::new(|| Mutex::new(None));

pub async fn analyze(url: &str, settings: &Settings) -> AppResult<Option<MediaMetadata>> {
    let Some(address) = post_address(url) else {
        return Ok(None);
    };
    let client = crate::net::client(settings)?;

    let id = match address {
        PostAddress::Id(id) => id,
        // A share link names the post only through where it redirects.
        PostAddress::Share => {
            let response = client.get(url).send().await?;
            match post_address(response.url().as_str()) {
                Some(PostAddress::Id(id)) => id,
                _ => return Ok(None),
            }
        }
    };

    let mut fresh = false;
    loop {
        let cookies = session(&client, settings, fresh).await?;
        let response = client
            .get(format!("https://www.reddit.com/comments/{id}/.json?raw_json=1"))
            .header(ACCEPT, "application/json")
            .header(COOKIE, format!("{cookies}; over18=1"))
            .send()
            .await?;

        let status = response.status();
        if status.as_u16() == 403 && !fresh {
            // Possibly a session Reddit no longer honours; one new one is tried.
            fresh = true;
            continue;
        }
        if !status.is_success() {
            return Err(AppError::from_status(status.as_u16(), "the post could not be read"));
        }

        let body = response.text().await?;
        let listing: Value = serde_json::from_str(&body)
            .map_err(|err| AppError::Parse(format!("Reddit answered with something other than the post: {err}")))?;
        return Ok(parse_listing(&listing, url));
    }
}

enum PostAddress {
    Id(String),
    Share,
}

/// The post a Reddit address points at, when it points at one.
fn post_address(url: &str) -> Option<PostAddress> {
    let info = detect::classify(url)?;
    let path = info.path.split(['?', '#']).next().unwrap_or_default();
    let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();

    // redd.it/<id>
    if info.host == "redd.it" {
        return segments.first().filter(|id| is_post_id(id)).map(|id| PostAddress::Id(id.to_string()));
    }
    if !(info.host == "reddit.com" || info.host.ends_with(".reddit.com")) {
        return None;
    }

    // .../comments/<id>/... and /gallery/<id>
    if let Some(marker) = segments.iter().position(|segment| *segment == "comments" || *segment == "gallery") {
        return segments
            .get(marker + 1)
            .filter(|id| is_post_id(id))
            .map(|id| PostAddress::Id(id.to_string()));
    }
    // /r/<subreddit>/s/<code>, what the apps' Share button produces.
    if segments.len() >= 4 && segments[0] == "r" && segments[2] == "s" {
        return Some(PostAddress::Share);
    }
    None
}

fn is_post_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

async fn session(client: &reqwest::Client, settings: &Settings, renew: bool) -> AppResult<String> {
    let signature = format!("{:?}|{:?}", settings.proxy_url, settings.custom_user_agent);
    if !renew {
        let guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = guard.as_ref() {
            if session.signature == signature && session.at.elapsed() < SESSION_TTL {
                return Ok(session.cookies.clone());
            }
        }
    }

    let response = client.get("https://old.reddit.com/").send().await?;
    let status = response.status();
    let cookies = cookie_header(&response_cookies(response.headers()));
    if !status.is_success() || cookies.is_empty() {
        return Err(AppError::from_status(
            if status.is_success() { 403 } else { status.as_u16() },
            "Reddit did not start a session",
        ));
    }

    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
        cookies: cookies.clone(),
        signature,
        at: Instant::now(),
    });
    Ok(cookies)
}

/// The post as photos, or `None` when it is not an image post or a gallery.
fn parse_listing(listing: &Value, requested_url: &str) -> Option<MediaMetadata> {
    let data = listing.get(0)?.pointer("/data/children/0/data")?;
    // A crosspost carries its pictures on the post it shares.
    let source = data
        .pointer("/crosspost_parent_list/0")
        .filter(|parent| parent.get("is_gallery").is_some() || parent.get("post_hint").is_some())
        .unwrap_or(data);

    let text = |node: &Value, key: &str| node.get(key).and_then(Value::as_str).map(str::to_string);
    let post = Post {
        platform: PlatformId::Reddit,
        url: requested_url.to_string(),
        canonical_url: text(data, "permalink")
            .map(|link| format!("https://www.reddit.com{link}"))
            .unwrap_or_else(|| requested_url.to_string()),
        title: text(data, "title").filter(|title| !title.trim().is_empty()).unwrap_or_else(|| "Reddit post".into()),
        creator: text(data, "author").filter(|author| author != "[deleted]"),
        description: text(data, "selftext").filter(|body| !body.trim().is_empty()),
        upload_date: data.get("created_utc").and_then(Value::as_f64).and_then(|time| upload_date(time as i64)),
        view_count: None,
        like_count: data.get("ups").and_then(Value::as_u64),
    };

    if source.get("is_gallery").and_then(Value::as_bool) == Some(true) {
        let metadata = source.get("media_metadata")?;
        let items: Vec<MediaMetadata> = source
            .pointer("/gallery_data/items")
            .and_then(Value::as_array)?
            .iter()
            .filter_map(|item| {
                let media_id = item.get("media_id").and_then(Value::as_str)?;
                let media = metadata.get(media_id)?;
                gallery_picture(media_id, media).map(|(photo, preview)| post.item(photo, preview))
            })
            .collect();
        return post.assemble(items);
    }

    let address = source.get("url_overridden_by_dest").or_else(|| source.get("url")).and_then(Value::as_str)?;
    let is_image = source.get("post_hint").and_then(Value::as_str) == Some("image")
        || detect::classify(address)
            .and_then(|info| info.direct_extension)
            .is_some_and(|extension| detect::is_image_extension(&extension));
    if !is_image {
        return None;
    }

    let preview = source.pointer("/preview/images/0");
    let source_size = |key: &str| {
        preview
            .and_then(|image| image.pointer(&format!("/source/{key}")))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    };
    let photo = image_format("image", address, source_size("width"), source_size("height"), Vec::new());
    let item = post.item(photo, preview.and_then(preview_address));
    post.assemble(vec![item])
}

/// One picture of a gallery: the original on Reddit's image host, and a
/// smaller rendition to preview.
fn gallery_picture(media_id: &str, media: &Value) -> Option<(crate::model::MediaFormat, Option<String>)> {
    if media.get("status").and_then(Value::as_str) != Some("valid") {
        return None;
    }
    let kind = media.get("e").and_then(Value::as_str)?;
    if kind != "Image" && kind != "AnimatedImage" {
        return None;
    }

    let extension = match media.get("m").and_then(Value::as_str).unwrap_or("image/jpg") {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "jpg",
    };
    let size = |key: &str| {
        media
            .pointer(&format!("/s/{key}"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    };
    let photo = image_format(
        "image",
        format!("https://i.redd.it/{media_id}.{extension}"),
        size("x"),
        size("y"),
        Vec::new(),
    );

    // The largest rendition up to about screen width previews well and stays
    // well under the size a thumbnail may be.
    let preview = media
        .get("p")
        .and_then(Value::as_array)
        .and_then(|renditions| {
            renditions
                .iter()
                .rfind(|rendition| rendition.get("x").and_then(Value::as_u64).is_some_and(|x| x <= 1080))
        })
        .and_then(|rendition| rendition.get("u").and_then(Value::as_str))
        .map(str::to_string);
    Some((photo, preview))
}

fn preview_address(image: &Value) -> Option<String> {
    image
        .get("resolutions")
        .and_then(Value::as_array)
        .and_then(|renditions| {
            renditions
                .iter()
                .rfind(|rendition| rendition.get("width").and_then(Value::as_u64).is_some_and(|w| w <= 1080))
        })
        .and_then(|rendition| rendition.get("url").and_then(Value::as_str))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaKind;

    #[test]
    fn recognises_the_addresses_a_post_is_shared_under() {
        let id = |url: &str| match post_address(url) {
            Some(PostAddress::Id(id)) => Some(id),
            _ => None,
        };
        assert_eq!(id("https://www.reddit.com/r/pics/comments/1wcv3i8/2977_drones/").as_deref(), Some("1wcv3i8"));
        assert_eq!(id("https://old.reddit.com/comments/haucpf/").as_deref(), Some("haucpf"));
        assert_eq!(id("https://www.reddit.com/gallery/1wcv3i8").as_deref(), Some("1wcv3i8"));
        assert_eq!(id("https://redd.it/haucpf").as_deref(), Some("haucpf"));
        assert!(matches!(post_address("https://www.reddit.com/r/pics/s/AbCdEf123"), Some(PostAddress::Share)));
        assert!(post_address("https://www.reddit.com/r/pics/").is_none());
        assert!(post_address("https://v.redd.it/abc123").is_none());
        assert!(post_address("https://notreddit.com/comments/abc/").is_none());
    }

    fn listing(post: Value) -> Value {
        serde_json::json!([{ "data": { "children": [{ "data": post }] } }])
    }

    #[test]
    fn a_gallery_is_read_from_its_originals_in_order() {
        let post = serde_json::json!({
            "title": "2,977 drones recreated the Twin Towers",
            "author": "someone",
            "permalink": "/r/pics/comments/1wcv3i8/2977_drones/",
            "created_utc": 1789400000.0,
            "ups": 51000,
            "is_gallery": true,
            "gallery_data": { "items": [
                { "media_id": "h4ig7w2j7roh1" },
                { "media_id": "6xyjmv2j7roh1" },
                { "media_id": "gone" },
            ]},
            "media_metadata": {
                "6xyjmv2j7roh1": { "status": "valid", "e": "Image", "m": "image/jpg", "s": { "x": 960, "y": 600, "u": "https://preview.redd.it/6x.jpg?width=960" },
                    "p": [{ "x": 108, "u": "https://preview.redd.it/6x.jpg?width=108" }, { "x": 640, "u": "https://preview.redd.it/6x.jpg?width=640" }] },
                "h4ig7w2j7roh1": { "status": "valid", "e": "Image", "m": "image/png", "s": { "x": 2000, "y": 1500 } },
                "gone": { "status": "failed" },
            },
        });

        let gallery = parse_listing(&listing(post), "https://www.reddit.com/r/pics/comments/1wcv3i8/").unwrap();
        assert_eq!(gallery.media_kind, MediaKind::Gallery);
        assert_eq!(gallery.entry_count, Some(2));
        assert_eq!(gallery.canonical_url, "https://www.reddit.com/r/pics/comments/1wcv3i8/2977_drones/");
        assert_eq!(gallery.creator.as_deref(), Some("someone"));

        let first = &gallery.entries[0].formats[0];
        assert_eq!(first.url.as_deref(), Some("https://i.redd.it/h4ig7w2j7roh1.png"));
        assert_eq!(first.container, "png");
        assert_eq!(first.quality_label, "2000x1500");
        assert_eq!(
            gallery.entries[1].thumbnail_url.as_deref(),
            Some("https://preview.redd.it/6x.jpg?width=640")
        );
    }

    #[test]
    fn an_image_post_is_its_picture() {
        let post = serde_json::json!({
            "title": "There is a crease",
            "author": "someone",
            "permalink": "/r/pics/comments/1wbx1wo/there_is_a_crease/",
            "post_hint": "image",
            "url": "https://i.redd.it/wbj4adhczjoh1.jpeg",
            "preview": { "images": [{
                "source": { "url": "https://preview.redd.it/wbj.jpeg?auto=webp", "width": 2732, "height": 1958 },
                "resolutions": [{ "url": "https://preview.redd.it/wbj.jpeg?width=640", "width": 640 }, { "url": "https://preview.redd.it/wbj.jpeg?width=1080", "width": 1080 }],
            }]},
        });

        let photo = parse_listing(&listing(post), "https://redd.it/1wbx1wo").unwrap();
        assert_eq!(photo.media_kind, MediaKind::Image);
        assert_eq!(photo.title, "There is a crease");
        assert_eq!(photo.formats[0].url.as_deref(), Some("https://i.redd.it/wbj4adhczjoh1.jpeg"));
        assert_eq!(photo.formats[0].container, "jpg");
        assert_eq!(photo.formats[0].quality_label, "2732x1958");
        assert_eq!(photo.thumbnail_url.as_deref(), Some("https://preview.redd.it/wbj.jpeg?width=1080"));
    }

    #[test]
    fn video_and_link_posts_are_left_to_the_engine() {
        let video = serde_json::json!({ "title": "v", "is_video": true, "post_hint": "hosted:video", "url": "https://v.redd.it/abc" });
        assert!(parse_listing(&listing(video), "https://redd.it/x").is_none());

        let link = serde_json::json!({ "title": "l", "post_hint": "link", "url": "https://example.com/article" });
        assert!(parse_listing(&listing(link), "https://redd.it/x").is_none());
    }
}
