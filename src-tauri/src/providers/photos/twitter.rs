//! Photos on X.
//!
//! The engine reads an X post's videos and skips its photos, so a post of
//! photos has, to it, nothing in it at all. The post is read here instead from
//! the endpoint X's embedded-post widget uses, which serves any public post to
//! a signed-out visitor. Videos and GIFs stay the engine's, which reads them
//! better than this endpoint describes them; in a post that has both, the
//! photos read here are put in order around the videos the engine reads.

use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::model::{MediaMetadata, PlatformId};
use crate::providers::{detect, image_format};
use crate::settings::Settings;

use super::{title_from_text, MixedPost, Post, Reading};

const ENDPOINT: &str = "https://cdn.syndication.twimg.com/tweet-result";

/// The engine's title length for X posts, so photo and video titles read alike.
const TITLE_CHARS: usize = 72;

pub async fn analyze(url: &str, settings: &Settings) -> AppResult<Reading> {
    let Some(address) = status_address(url) else {
        return Ok(Reading::Elsewhere);
    };
    let Some(token) = syndication_token(&address.id) else {
        return Ok(Reading::Elsewhere);
    };

    let client = crate::net::client(settings)?;
    let response = client
        .get(format!("{ENDPOINT}?id={}&token={token}&lang=en", address.id))
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await?;

    let status = response.status();
    // A post the widget cannot show -- removed, protected, age-restricted --
    // is the engine's to explain in its own words.
    if status.as_u16() == 404 {
        return Ok(Reading::Elsewhere);
    }
    if !status.is_success() {
        return Err(AppError::from_status(status.as_u16(), "the post could not be read"));
    }

    let body = response.text().await?;
    let post: Value = serde_json::from_str(&body)
        .map_err(|err| AppError::Parse(format!("the post data was unreadable: {err}")))?;
    Ok(parse_post(&post, url, address.photo))
}

struct StatusAddress {
    id: String,
    /// 1-based, when the address points at one photo of the post.
    photo: Option<usize>,
}

/// The post an X or Twitter address points at.
fn status_address(url: &str) -> Option<StatusAddress> {
    let info = detect::classify(url)?;
    let host = info.host.trim_start_matches("www.").trim_start_matches("mobile.");
    if host != "x.com" && host != "twitter.com" {
        return None;
    }

    let path = info.path.split(['?', '#']).next().unwrap_or_default();
    let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
    let marker = segments
        .iter()
        .position(|segment| *segment == "status" || *segment == "statuses")?;
    let id = *segments.get(marker + 1)?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let photo = match (segments.get(marker + 2).copied(), segments.get(marker + 3)) {
        (Some("photo"), Some(number)) => number.parse::<usize>().ok().filter(|n| *n >= 1),
        // A video number is the engine's to follow.
        (Some("video"), _) => return None,
        _ => None,
    };

    Some(StatusAddress {
        id: id.to_string(),
        photo,
    })
}

/// What the post's photos make of it.
fn parse_post(post: &Value, requested_url: &str, photo: Option<usize>) -> Reading {
    let Some(id) = post.get("id_str").and_then(Value::as_str) else {
        return Reading::Elsewhere;
    };

    // The post's own media first, then that of a post it quotes -- the same
    // order the engine lists the videos in.
    let media: Vec<&Value> = [Some(post), post.get("quoted_tweet")]
        .into_iter()
        .flatten()
        .filter_map(|tweet| tweet.get("mediaDetails").and_then(Value::as_array))
        .flatten()
        .collect();
    let is_photo = |item: &Value| item.get("type").and_then(Value::as_str) == Some("photo");
    if !media.iter().any(|item| is_photo(item)) {
        return Reading::Elsewhere;
    }

    let user = post.get("user");
    let name = user.and_then(|user| user.get("name")).and_then(Value::as_str);
    let handle = user.and_then(|user| user.get("screen_name")).and_then(Value::as_str);

    let text = post
        .get("text")
        .and_then(Value::as_str)
        .map(strip_trailing_links)
        .unwrap_or_default();
    // The account is the creator, shown beside the title and put in front of
    // it in file names; repeating it in the title would name it twice.
    let title = title_from_text(&text, TITLE_CHARS)
        .or_else(|| name.or(handle).map(|name| format!("Photo by {name}")))
        .unwrap_or_else(|| format!("X post {id}"));

    let status_url = match handle {
        Some(handle) => format!("https://x.com/{handle}/status/{id}"),
        None => format!("https://x.com/i/status/{id}"),
    };

    // An address naming one photo downloads that photo, and stays pointed at
    // it. X numbers a post's media together, photos and videos alike.
    let chosen = photo.filter(|number| media.get(number - 1).is_some_and(|item| is_photo(item)));
    let canonical_url = match chosen {
        Some(number) => format!("{status_url}/photo/{number}"),
        None => status_url,
    };

    let post_info = Post {
        platform: PlatformId::Twitter,
        url: requested_url.to_string(),
        canonical_url,
        title,
        creator: name.or(handle).map(str::to_string),
        description: (!text.is_empty()).then_some(text),
        upload_date: post
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.format("%Y%m%d").to_string()),
        view_count: None,
        like_count: post.get("favorite_count").and_then(Value::as_u64),
    };

    if let Some(number) = chosen {
        return post_info
            .assemble(photo_item(&post_info, number - 1, media[number - 1]).into_iter().collect())
            .map_or(Reading::Elsewhere, Reading::photos);
    }

    if media.iter().all(|item| is_photo(item)) {
        let items = media
            .iter()
            .enumerate()
            .filter_map(|(index, item)| photo_item(&post_info, index, item))
            .collect();
        return post_info.assemble(items).map_or(Reading::Elsewhere, Reading::photos);
    }

    // Photos and videos: the photos are kept, and the engine's videos will
    // fill the places in between.
    let mut slots = Vec::with_capacity(media.len());
    for (index, item) in media.iter().enumerate() {
        if is_photo(item) {
            match photo_item(&post_info, index, item) {
                Some(photo) => slots.push(Some(photo)),
                None => return Reading::Elsewhere,
            }
        } else {
            slots.push(None);
        }
    }
    Reading::WithVideos(MixedPost {
        title: post_info.title,
        canonical_url: post_info.canonical_url,
        slots,
    })
}

/// One photo of a post, at its original size.
fn photo_item(post: &Post, index: usize, item: &Value) -> Option<MediaMetadata> {
    let address = item.get("media_url_https").and_then(Value::as_str)?;
    let (stem, extension) = address.rsplit_once('.')?;
    let dimension = |key: &str| {
        item.get("original_info")
            .and_then(|info| info.get(key))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    };

    let mut photo = image_format(
        format!("photo-{}", index + 1),
        format!("{stem}?format={extension}&name=orig"),
        dimension("width"),
        dimension("height"),
        Vec::new(),
    );
    // The address has no extension of its own; the query names it.
    photo.container = if extension.eq_ignore_ascii_case("jpeg") {
        "jpg".to_string()
    } else {
        extension.to_ascii_lowercase()
    };
    Some(post.item(photo, Some(format!("{stem}?format={extension}&name=medium"))))
}

/// Post text ends with the short link to its own media; it is not part of what
/// anyone wrote.
fn strip_trailing_links(text: &str) -> String {
    let mut words: Vec<&str> = text.split_whitespace().collect();
    while words.last().is_some_and(|word| word.starts_with("https://t.co/") || word.starts_with("http://t.co/")) {
        words.pop();
    }
    words.join(" ")
}

/// The token the embed widget sends with a post id: in JavaScript,
/// `((id / 1e15) * Math.PI).toString(36).replace(/(0+|\.)/g, '')`.
fn syndication_token(id: &str) -> Option<String> {
    let id: f64 = id.parse().ok()?;
    let token = to_radix_string(id / 1e15 * std::f64::consts::PI, 36).replace(['0', '.'], "");
    (!token.is_empty()).then_some(token)
}

/// `Number.prototype.toString(radix)` as JavaScript engines implement it: the
/// fraction is written only to the precision the value actually carries, so
/// the digits match what a browser produces.
fn to_radix_string(value: f64, radix: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let base = f64::from(radix);

    let negative = value < 0.0;
    let value = value.abs();
    let mut integer = value.floor();
    let mut fraction = value - integer;

    // Half the distance to the next representable value: beyond that, further
    // digits describe the rounding, not the number. (`value` is non-negative
    // and finite here, so the next value up is the next bit pattern.)
    let next_up = |x: f64| f64::from_bits(x.to_bits() + 1);
    let mut delta = (0.5 * (next_up(value) - value)).max(next_up(0.0));
    let mut fraction_digits: Vec<u8> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= base;
            delta *= base;
            let digit = fraction as u32;
            fraction_digits.push(DIGITS[digit as usize]);
            fraction -= f64::from(digit);

            // Round half to even, carrying into earlier digits as needed.
            if (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0 {
                loop {
                    match fraction_digits.pop() {
                        None => {
                            integer += 1.0;
                            break;
                        }
                        Some(last) => {
                            let previous = if last > b'9' { u32::from(last - b'a') + 10 } else { u32::from(last - b'0') };
                            if previous + 1 < radix {
                                fraction_digits.push(DIGITS[(previous + 1) as usize]);
                                break;
                            }
                        }
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }

    // Digits below the precision of a large integer are written as zeros.
    let mut integer_digits: Vec<u8> = Vec::new();
    while integer / base >= 9_007_199_254_740_992.0 {
        integer /= base;
        integer_digits.push(b'0');
    }
    loop {
        let remainder = integer % base;
        integer_digits.push(DIGITS[remainder as usize]);
        integer = (integer - remainder) / base;
        if integer <= 0.0 {
            break;
        }
    }
    integer_digits.reverse();

    let mut out = String::with_capacity(integer_digits.len() + fraction_digits.len() + 2);
    if negative {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(&integer_digits).unwrap_or("0"));
    if !fraction_digits.is_empty() {
        out.push('.');
        out.push_str(std::str::from_utf8(&fraction_digits).unwrap_or(""));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FormatKind, MediaKind};

    #[test]
    fn base_36_matches_what_a_browser_writes() {
        // Reference values from `Number.prototype.toString(36)` in V8.
        let cases = [
            (0.5, "0.i"),
            (1.0 / 3.0, "0.c"),
            (255.5, "73.i"),
            (12345.6789, "9ix.ofuravwu"),
            (0.1, "0.3lllllllllm"),
            (35.99999999999999, "z.zzzzzzzzz"),
            (1e21, "5v1j4f4ds7c000"),
            (2f64.powi(60), "8rc4kbdvss00"),
        ];
        for (value, expected) in cases {
            assert_eq!(to_radix_string(value, 36), expected, "for {value}");
        }
    }

    #[test]
    fn the_token_matches_the_embed_widgets() {
        let cases = [
            ("440322224407314432", "12fb9qdo78ad"),
            ("1799999999999999999", "4d2v7cbm2xj"),
            ("1234567890123456789", "2zqic77uqyk"),
            ("20", "6dq1a2xwd93"),
        ];
        for (id, expected) in cases {
            assert_eq!(syndication_token(id).as_deref(), Some(expected), "for {id}");
        }
    }

    #[test]
    fn recognises_the_addresses_a_post_is_shared_under() {
        let id = |url: &str| status_address(url).map(|address| (address.id, address.photo));
        assert_eq!(id("https://x.com/TheEllenShow/status/440322224407314432"), Some(("440322224407314432".into(), None)));
        assert_eq!(id("https://twitter.com/a/status/123?s=20"), Some(("123".into(), None)));
        assert_eq!(id("https://mobile.x.com/a/status/123/photo/2"), Some(("123".into(), Some(2))));
        assert_eq!(id("https://x.com/i/web/status/123"), Some(("123".into(), None)));
        assert_eq!(id("https://x.com/a/status/123/video/1"), None);
        assert_eq!(id("https://x.com/a"), None);
        assert_eq!(id("https://x.com/a/status/abc"), None);
        assert_eq!(id("https://notx.com/a/status/123"), None);
    }

    fn post(media: Value) -> Value {
        serde_json::json!({
            "__typename": "Tweet",
            "id_str": "440322224407314432",
            "text": "If only Bradley's arm was longer. Best photo ever. #oscars http://t.co/C9U5NOtGap",
            "created_at": "2014-03-03T03:06:13.000Z",
            "favorite_count": 1863479,
            "user": { "name": "The Ellen Show", "screen_name": "TheEllenShow" },
            "mediaDetails": media,
        })
    }

    fn photo(file: &str) -> Value {
        serde_json::json!({
            "type": "photo",
            "media_url_https": format!("https://pbs.twimg.com/media/{file}.jpg"),
            "original_info": { "width": 1920, "height": 1080 },
        })
    }

    fn video() -> Value {
        serde_json::json!({ "type": "video", "media_url_https": "https://pbs.twimg.com/ext_tw_video_thumb/1/pu/img/x.jpg" })
    }

    fn photos(reading: Reading) -> MediaMetadata {
        match reading {
            Reading::Photos(metadata) => *metadata,
            _ => panic!("expected the post to be read as photos"),
        }
    }

    #[test]
    fn a_photo_post_is_read_at_original_size() {
        let meta = photos(parse_post(
            &post(serde_json::json!([photo("BhxWutnCEAAtEQ6")])),
            "https://x.com/TheEllenShow/status/440322224407314432",
            None,
        ));

        assert_eq!(meta.media_kind, MediaKind::Image);
        assert_eq!(meta.title, "If only Bradley's arm was longer. Best photo ever. #oscars");
        assert_eq!(meta.creator.as_deref(), Some("The Ellen Show"));
        assert_eq!(meta.upload_date.as_deref(), Some("20140303"));
        assert_eq!(meta.canonical_url, "https://x.com/TheEllenShow/status/440322224407314432");

        let format = &meta.formats[0];
        assert_eq!(format.kind, FormatKind::Image);
        assert_eq!(format.url.as_deref(), Some("https://pbs.twimg.com/media/BhxWutnCEAAtEQ6?format=jpg&name=orig"));
        assert_eq!(format.container, "jpg");
        assert_eq!(format.quality_label, "1920x1080");
    }

    #[test]
    fn several_photos_make_a_gallery_and_an_address_can_pick_one() {
        let media = serde_json::json!([photo("A"), photo("B"), photo("C")]);
        let gallery = photos(parse_post(&post(media.clone()), "https://x.com/a/status/1", None));
        assert_eq!(gallery.entry_count, Some(3));
        assert_eq!(gallery.entries[2].formats[0].url.as_deref(), Some("https://pbs.twimg.com/media/C?format=jpg&name=orig"));

        let second = photos(parse_post(&post(media), "https://x.com/a/status/1/photo/2", Some(2)));
        assert!(second.entries.is_empty());
        assert_eq!(second.formats[0].url.as_deref(), Some("https://pbs.twimg.com/media/B?format=jpg&name=orig"));
        assert_eq!(second.canonical_url, "https://x.com/TheEllenShow/status/440322224407314432/photo/2");
    }

    #[test]
    fn a_post_of_videos_or_of_nothing_is_left_to_the_engine() {
        let elsewhere = |reading: Reading| matches!(reading, Reading::Elsewhere);
        assert!(elsewhere(parse_post(&post(serde_json::json!([video()])), "https://x.com/a/status/1", None)));
        assert!(elsewhere(parse_post(&post(serde_json::json!([])), "https://x.com/a/status/1", None)));
        assert!(elsewhere(parse_post(&serde_json::json!({ "__typename": "TweetTombstone" }), "https://x.com/a/status/1", None)));
    }

    #[test]
    fn a_post_of_photos_and_videos_keeps_the_photos_in_their_places() {
        let media = serde_json::json!([video(), photo("A"), video(), photo("B")]);
        let Reading::WithVideos(mixed) = parse_post(&post(media.clone()), "https://x.com/a/status/1", None) else {
            panic!("expected photos waiting for videos");
        };
        let places: Vec<bool> = mixed.slots.iter().map(Option::is_some).collect();
        assert_eq!(places, [false, true, false, true]);

        // A photo named by its position is just that photo, even in a mixed post;
        // a video's position is not a photo to read.
        let one = photos(parse_post(&post(media.clone()), "https://x.com/a/status/1/photo/4", Some(4)));
        assert_eq!(one.formats[0].url.as_deref(), Some("https://pbs.twimg.com/media/B?format=jpg&name=orig"));
        assert!(matches!(
            parse_post(&post(media), "https://x.com/a/status/1/photo/1", Some(1)),
            Reading::WithVideos(_)
        ));
    }

    #[test]
    fn the_media_link_is_not_part_of_the_text() {
        assert_eq!(strip_trailing_links("Look at this https://t.co/abc"), "Look at this");
        assert_eq!(strip_trailing_links("https://t.co/abc"), "");
        assert_eq!(strip_trailing_links("keep https://t.co/abc in the middle"), "keep https://t.co/abc in the middle");
    }
}
