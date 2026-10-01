//! Songs shared from Spotify.
//!
//! Spotify's own audio is encrypted, and nothing here goes near it. What a
//! Spotify link gives this app is a description: the song, who made it, the
//! album it is on and how long it runs. The sound is the same recording as
//! published on YouTube, found by that description and downloaded like any
//! other YouTube audio; the description then names the file and becomes its
//! tags.
//!
//! The description comes from pages Spotify serves to anyone, with no account
//! and no key: the embedded player for an album, playlist or artist, which
//! lists their songs, and a song's own page, whose tags name its album. The
//! embedded player lists at most a hundred songs, so a longer playlist
//! arrives as its first hundred.
//!
//! Finding the recording is the expensive part -- a search and a full read of
//! the result, several seconds each -- so an album is listed without it and
//! each song is looked for only when its own download starts.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::model::{
    FormatKind, MediaFormat, MediaKind, MediaMetadata, MusicTags, PlatformId, TrackSummary,
    WatermarkSupport,
};
use crate::providers::words::{contains_words, normalize};
use crate::providers::{detect, engine, generic};
use crate::settings::Settings;
use crate::{log_debug, log_info};

pub const PROVIDER_ID: &str = "spotify";

/// The format a song is listed with before its recording has been found. It
/// stands for the AAC stream every YouTube upload carries, which is what the
/// download will take unless another format is asked for.
const PENDING_FORMAT_ID: &str = "spotify-pending";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Track,
    Album,
    Playlist,
    Artist,
}

impl Kind {
    fn path(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Playlist => "playlist",
            Self::Artist => "artist",
        }
    }
}

#[derive(Debug, PartialEq)]
struct Link {
    kind: Kind,
    id: String,
}

/// An `open.spotify.com` address of a song, album, playlist or artist, in any
/// of the forms the app shares: with a language prefix, a tracking query, or
/// both.
fn parse_link(url: &str) -> Option<Link> {
    let info = detect::classify(url)?;
    if info.host != "open.spotify.com" && info.host != "play.spotify.com" {
        return None;
    }
    let path = info.path.split(['?', '#']).next().unwrap_or_default();
    let mut segments = path.split('/').filter(|segment| !segment.is_empty()).peekable();
    if segments.peek().is_some_and(|segment| segment.starts_with("intl-")) {
        segments.next();
    }
    let kind = match segments.next()? {
        "track" => Kind::Track,
        "album" => Kind::Album,
        "playlist" => Kind::Playlist,
        "artist" => Kind::Artist,
        _ => return None,
    };
    let id = segments.next()?;
    if id.len() < 10 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    Some(Link {
        kind,
        id: id.to_string(),
    })
}

fn page_url(kind: Kind, id: &str) -> String {
    format!("https://open.spotify.com/{}/{id}", kind.path())
}

/// Read a Spotify link, or `None` when it is not one.
///
/// A song is looked for at once, so it previews with the streams it will
/// download. An album, playlist or artist is only listed.
pub async fn analyze(url: &str, settings: &Settings) -> AppResult<Option<MediaMetadata>> {
    let Some(link) = parse_link(url) else {
        return Ok(None);
    };

    if link.kind == Kind::Track {
        let tags = read_song(&link.id, settings).await?;
        return resolve(pending_song(tags), settings).await.map(Some);
    }

    let entity = read_embed(link.kind, &link.id, settings).await?;
    let collection = parse_collection(&entity, link.kind, &link.id).ok_or_else(|| {
        AppError::Unsupported("Spotify listed no songs for this link".into())
    })?;
    Ok(Some(collection))
}

/// Whether a song still has to be looked for before it can be downloaded.
pub fn is_pending(metadata: &MediaMetadata) -> bool {
    metadata
        .music
        .as_ref()
        .is_some_and(|music| music.stream_page.is_none())
}

// -- reading Spotify ----------------------------------------------------------

/// How the app introduces itself to Spotify. A browser's name is answered
/// with the web player, an empty shell its script fills in; a program that
/// says what it is gets the page with the song's tags in its head, the one
/// link previews are made from.
const AGENT: &str = concat!("Mozilla/5.0 (compatible; UniversalDownloader/", env!("CARGO_PKG_VERSION"), ")");

async fn fetch_page(url: &str, settings: &Settings) -> AppResult<String> {
    let client = crate::net::client(settings)?;
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, AGENT)
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
        .header(reqwest::header::ACCEPT_LANGUAGE, "en")
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::from_status(status.as_u16(), "Spotify did not serve the page"));
    }
    Ok(response.text().await?)
}

static NEXT_DATA: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<script id="__NEXT_DATA__" type="application/json">(.*?)</script>"#)
        .expect("next data pattern is valid")
});

async fn read_embed(kind: Kind, id: &str, settings: &Settings) -> AppResult<Value> {
    let html = fetch_page(&format!("https://open.spotify.com/embed/{}/{id}", kind.path()), settings).await?;
    embed_entity(&html).ok_or_else(|| AppError::Parse("the Spotify player page had no song list".into()))
}

fn embed_entity(html: &str) -> Option<Value> {
    let json = NEXT_DATA.captures(html)?.get(1)?.as_str();
    let root: Value = serde_json::from_str(json).ok()?;
    root.pointer("/props/pageProps/state/data/entity").cloned()
}

static META: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<meta\s+(?:property|name)="([^"]+)"\s+content="([^"]*)""#).expect("meta pattern is valid")
});

/// A song's own page, read for the tags it carries in its head: the album
/// and position the embedded player does not give. When the page has none of
/// them the embedded player still names the song, its artists and its length.
async fn read_song(id: &str, settings: &Settings) -> AppResult<MusicTags> {
    let url = page_url(Kind::Track, id);
    match fetch_page(&url, settings).await.map(|html| song_from_page(&html, &url)) {
        Ok(Some(song)) => return Ok(song),
        Ok(None) => log_debug!("spotify", "the song page carried no tags; reading the player instead"),
        Err(err) => log_debug!("spotify", "the song page could not be read ({err}); reading the player instead"),
    }
    let entity = read_embed(Kind::Track, id, settings).await?;
    song_from_embed(&entity, &url).ok_or_else(|| AppError::Parse("Spotify named no song for this link".into()))
}

fn song_from_embed(entity: &Value, page_url: &str) -> Option<MusicTags> {
    let title = entity
        .get("name")
        .or_else(|| entity.get("title"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    let artists: Vec<String> = entity
        .get("artists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artist| artist.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    Some(MusicTags {
        title,
        album_artist: artists.first().cloned(),
        artists,
        release_date: entity
            .pointer("/releaseDate/isoString")
            .and_then(Value::as_str)
            .and_then(|iso| iso.get(..10))
            .map(str::to_string),
        duration_sec: entity
            .get("duration")
            .and_then(Value::as_f64)
            .filter(|ms| *ms > 0.0)
            .map(|ms| ms / 1000.0),
        cover_url: largest_image(entity),
        page_url: page_url.to_string(),
        ..MusicTags::default()
    })
}

fn song_from_page(html: &str, page_url: &str) -> Option<MusicTags> {
    let head = &html[..head_end(html)];
    let meta: Vec<(String, String)> = META
        .captures_iter(head)
        .filter_map(|capture| {
            Some((
                capture.get(1)?.as_str().to_string(),
                generic::decode_entities(capture.get(2)?.as_str()),
            ))
        })
        .collect();
    let first = |key: &str| {
        meta.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };

    let title = first("og:title")?;
    // "Artist, Artist · Album · Song · 2020"
    let description = first("og:description").unwrap_or_default();
    let parts: Vec<&str> = description.split(" · ").map(str::trim).collect();

    let mut artists: Vec<String> = meta
        .iter()
        .filter(|(name, _)| name == "music:musician_description")
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if artists.is_empty() {
        artists = parts.first().map(|line| split_artists(line)).unwrap_or_default();
    }

    Some(MusicTags {
        title,
        album_artist: artists.first().cloned(),
        artists,
        album: (parts.len() >= 3).then(|| parts[1].to_string()).filter(|album| !album.is_empty()),
        track_number: first("music:album:track").and_then(|value| value.parse().ok()),
        release_date: first("music:release_date"),
        duration_sec: first("music:duration").and_then(|value| value.parse().ok()),
        cover_url: first("og:image"),
        page_url: page_url.to_string(),
        collection: None,
        stream_page: None,
    })
}

/// Where `<head>` ends, or the whole page when that cannot be found. Keeps
/// the tag search off the hundreds of kilobytes of script that follow.
fn head_end(html: &str) -> usize {
    html.find("</head>").unwrap_or(html.len())
}

fn split_artists(line: &str) -> Vec<String> {
    line.split(", ")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// The largest picture the embedded player lists.
fn largest_image(entity: &Value) -> Option<String> {
    let from_identity = entity
        .pointer("/visualIdentity/image")
        .and_then(Value::as_array)
        .and_then(|images| {
            images
                .iter()
                .max_by_key(|image| image.get("maxWidth").and_then(Value::as_u64).unwrap_or(0))
        })
        .and_then(|image| image.get("url").and_then(Value::as_str));
    let from_cover = entity
        .pointer("/coverArt/sources")
        .and_then(Value::as_array)
        .and_then(|sources| sources.first())
        .and_then(|source| source.get("url").and_then(Value::as_str));
    from_identity.or(from_cover).map(str::to_string)
}

fn parse_collection(entity: &Value, kind: Kind, id: &str) -> Option<MediaMetadata> {
    let text = |key: &str| {
        entity
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let name = text("name").or_else(|| text("title"))?;
    let subtitle = text("subtitle");
    let cover = largest_image(entity);
    let url = page_url(kind, id);

    let songs: Vec<MusicTags> = entity
        .get("trackList")
        .and_then(Value::as_array)?
        .iter()
        .enumerate()
        .filter_map(|(index, track)| {
            let title = track.get("title").and_then(Value::as_str)?.trim().to_string();
            let song_id = track
                .get("uri")
                .and_then(Value::as_str)?
                .strip_prefix("spotify:track:")?
                .to_string();
            let artists = track
                .get("subtitle")
                .and_then(Value::as_str)
                .map(split_artists)
                .unwrap_or_default();
            // Only an album says what album its songs are on; a playlist's
            // and an artist's songs each have their own, read from the song's
            // page when it is downloaded.
            let on_album = kind == Kind::Album;
            Some(MusicTags {
                title,
                artists,
                album: on_album.then(|| name.clone()),
                album_artist: if on_album { subtitle.clone() } else { None },
                track_number: on_album.then_some(index as u32 + 1),
                release_date: None,
                duration_sec: track
                    .get("duration")
                    .and_then(Value::as_f64)
                    .filter(|ms| *ms > 0.0)
                    .map(|ms| ms / 1000.0),
                cover_url: if on_album { cover.clone() } else { None },
                page_url: page_url(Kind::Track, &song_id),
                collection: Some(name.clone()),
                stream_page: None,
            })
        })
        .filter(|song| !song.title.is_empty())
        .collect();

    if songs.is_empty() {
        return None;
    }

    let tracks = songs
        .iter()
        .enumerate()
        .map(|(index, song)| TrackSummary {
            position: index as u32 + 1,
            title: song.title.clone(),
            artists: song.artist_line(),
            duration_sec: song.duration_sec,
        })
        .collect();
    let entries: Vec<MediaMetadata> = songs.into_iter().map(pending_song).collect();

    // An artist's "subtitle" is the heading of the list, not a person.
    let creator = match kind {
        Kind::Artist => Some(name.clone()),
        _ => subtitle,
    };

    Some(MediaMetadata {
        url: url.clone(),
        canonical_url: url,
        media_kind: MediaKind::Gallery,
        title: name,
        creator,
        thumbnail_url: cover,
        duration_sec: None,
        entry_count: Some(entries.len() as u32),
        tracks,
        music: None,
        entries,
        ..pending_song(MusicTags::default())
    })
}

/// A song as listed, before its recording has been looked for.
fn pending_song(tags: MusicTags) -> MediaMetadata {
    MediaMetadata {
        url: tags.page_url.clone(),
        canonical_url: tags.page_url.clone(),
        platform: PlatformId::Spotify,
        platform_label: PlatformId::Spotify.label().to_string(),
        provider_id: PROVIDER_ID.to_string(),
        media_kind: MediaKind::Audio,
        title: tags.title.clone(),
        creator: (!tags.artists.is_empty()).then(|| tags.artist_line()),
        description: None,
        thumbnail_url: tags.cover_url.clone(),
        duration_sec: tags.duration_sec,
        view_count: None,
        like_count: None,
        upload_date: upload_date(tags.release_date.as_deref()),
        is_live: false,
        formats: vec![pending_format()],
        entry_count: None,
        watermark_support: WatermarkSupport::NotApplicable,
        range_fetchable: false,
        warnings: Vec::new(),
        entries: Vec::new(),
        tracks: Vec::new(),
        music: Some(tags),
    }
}

fn pending_format() -> MediaFormat {
    MediaFormat {
        id: PENDING_FORMAT_ID.to_string(),
        kind: FormatKind::Audio,
        container: "m4a".to_string(),
        protocol: "https".to_string(),
        has_video: false,
        has_audio: true,
        width: None,
        height: None,
        fps: None,
        vcodec: None,
        acodec: Some("mp4a.40.2".to_string()),
        tbr: None,
        vbr: None,
        abr: None,
        filesize: None,
        filesize_approx: None,
        quality_label: "AAC".to_string(),
        watermarked: None,
        note: None,
        needs_engine_download: false,
        language: None,
        language_preference: None,
        url: None,
        http_headers: Vec::new(),
    }
}

/// `2020-03-20` as the `20200320` the rest of the app keeps dates in. A bare
/// year is not a day, and is left out rather than pinned to the first of
/// January.
fn upload_date(release: Option<&str>) -> Option<String> {
    let release = release?;
    let digits: String = release.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == 8).then_some(digits)
}

// -- finding the recording ----------------------------------------------------

/// Find a pending song's recording and read it, so it can be downloaded.
///
/// A song listed from a playlist or an artist does not yet know its album,
/// so its own page is read first; that failing costs the tags, not the song.
pub async fn resolve(item: MediaMetadata, settings: &Settings) -> AppResult<MediaMetadata> {
    let Some(mut tags) = item.music.clone() else {
        return Ok(item);
    };
    if tags.stream_page.is_some() {
        return Ok(item);
    }

    // Only a song listed from a playlist or an artist is missing what its page
    // says; a song read from its page already has whatever that page had.
    if tags.album.is_none() && tags.collection.is_some() {
        if let Some(id) = parse_link(&tags.page_url).map(|link| link.id) {
            match read_song(&id, settings).await {
                Ok(page) => tags = complete(tags, page),
                Err(err) => log_debug!("spotify", "the song page could not be read: {err}"),
            }
        }
    }

    let found = find_recording(&tags, settings).await?;
    log_info!("spotify", "matched \"{}\" to {}", tags.title, found.canonical_url);
    Ok(merge(tags, found))
}

/// What a song's own page adds to a song listed from a playlist.
fn complete(listed: MusicTags, page: MusicTags) -> MusicTags {
    MusicTags {
        album: page.album,
        album_artist: page.album_artist.or(listed.album_artist),
        track_number: page.track_number,
        release_date: page.release_date.or(listed.release_date),
        duration_sec: listed.duration_sec.or(page.duration_sec),
        cover_url: page.cover_url.or(listed.cover_url),
        artists: if listed.artists.is_empty() { page.artists } else { listed.artists },
        ..listed
    }
}

/// The recording's streams under the song's own description.
///
/// Only its sound is kept, and of that the AAC renditions when there are any:
/// they go into an M4A file as they are, where Opus would have to be
/// re-encoded to become one. A rendition with its loudness compressed is
/// dropped when the original is also offered.
fn merge(mut tags: MusicTags, found: MediaMetadata) -> MediaMetadata {
    let audio: Vec<MediaFormat> = found
        .formats
        .iter()
        .filter(|format| format.kind == FormatKind::Audio)
        .cloned()
        .collect();
    let uncompressed: Vec<MediaFormat> = audio.iter().filter(|format| !is_drc(format)).cloned().collect();
    let audio = if uncompressed.is_empty() { audio } else { uncompressed };
    let aac: Vec<MediaFormat> = audio
        .iter()
        .filter(|format| format.container == "m4a" || format.acodec.as_deref().is_some_and(|codec| codec.starts_with("mp4a")))
        .cloned()
        .collect();
    let formats = if !aac.is_empty() {
        aac
    } else if !audio.is_empty() {
        audio
    } else {
        found.formats.clone()
    };

    tags.stream_page = Some(found.canonical_url.clone());
    if tags.duration_sec.is_none() {
        tags.duration_sec = found.duration_sec;
    }
    if tags.cover_url.is_none() {
        tags.cover_url = found.thumbnail_url.clone();
    }

    let mut song = pending_song(tags);
    song.provider_id = found.provider_id;
    song.formats = formats;
    song.duration_sec = song.duration_sec.or(found.duration_sec);
    song
}

fn is_drc(format: &MediaFormat) -> bool {
    format.id.to_ascii_lowercase().ends_with("-drc")
        || format.note.as_deref().is_some_and(|note| note.to_ascii_lowercase().contains("drc"))
}

/// Look on YouTube Music first, whose song results are the recordings
/// themselves, and then in YouTube's search, whose results say how long they
/// run and who posted them.
async fn find_recording(song: &MusicTags, settings: &Settings) -> AppResult<MediaMetadata> {
    let query = search_words(song);

    let songs_page = format!(
        "https://music.youtube.com/search?q={}#songs",
        urlencoding::encode(&query)
    );
    match engine::search(&songs_page, 3, settings).await {
        Ok(hits) => {
            for hit in hits.iter().take(2) {
                match read_candidate(&hit.url, settings).await {
                    Ok(found) if is_same_song(song, &found) => return Ok(found),
                    Ok(found) => log_debug!(
                        "spotify",
                        "\"{}\" ({:?}s) is not \"{}\"",
                        found.title,
                        found.duration_sec,
                        song.title
                    ),
                    Err(err) => log_debug!("spotify", "a YouTube Music result could not be read: {err}"),
                }
            }
        }
        Err(err) => log_debug!("spotify", "YouTube Music search failed: {err}"),
    }

    let hits = engine::search(&format!("ytsearch10:{query}"), 10, settings).await?;
    let mut ranked: Vec<(f64, &engine::SearchHit)> = hits
        .iter()
        .filter_map(|hit| score(song, hit).map(|value| (value, hit)))
        .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    for (_, hit) in ranked.into_iter().take(2) {
        match read_candidate(&hit.url, settings).await {
            Ok(found) => return Ok(found),
            Err(err) => log_debug!("spotify", "a YouTube result could not be read: {err}"),
        }
    }

    Err(AppError::NoMatch(format!("no recording of \"{query}\" was found on YouTube")))
}

async fn read_candidate(url: &str, settings: &Settings) -> AppResult<MediaMetadata> {
    engine::EngineProvider.analyze(url, settings).await
}

/// The words a song is searched for by: who made it, then its name without
/// the version notes Spotify appends.
fn search_words(song: &MusicTags) -> String {
    let artists = song.artists.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    format!("{artists} {}", base_title(&song.title)).trim().to_string()
}

/// A song's name without what is written after it: " - Remastered 2011",
/// "(feat. Someone)", "[Live]". Used to recognise the song, never to decide
/// which version of it is wanted.
fn base_title(title: &str) -> String {
    let before_dash = title.split(" - ").next().unwrap_or(title);
    let mut out = String::with_capacity(before_dash.len());
    let mut depth = 0usize;
    for ch in before_dash.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        before_dash.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Versions that are not the recording unless the song's own name says so.
const OTHER_VERSIONS: &[&str] = &[
    "live", "remix", "cover", "karaoke", "instrumental", "sped up", "slowed", "reverb", "8d",
    "nightcore", "acoustic", "edit", "extended", "mashup", "bass boosted", "trailer", "reaction",
    "tutorial", "lesson", "1 hour", "loop", "choreography", "dance practice", "behind the scenes",
    "concert", "teaser", "snippet", "unplugged", "piano version", "guitar", "drum cover",
];

fn other_version(song_title: &str, candidate_title: &str) -> bool {
    let wanted = normalize(song_title);
    let candidate = normalize(candidate_title);
    OTHER_VERSIONS
        .iter()
        .any(|marker| contains_words(&candidate, marker) && !contains_words(&wanted, marker))
}

fn names_the_song(song: &MusicTags, candidate_title: &str) -> bool {
    let name = normalize(&base_title(&song.title));
    let candidate = normalize(candidate_title);
    if contains_words(&candidate, &name) {
        return true;
    }
    // Word by word, for a name the upload spells a little differently.
    let words: Vec<&str> = name.split(' ').filter(|word| !word.is_empty()).collect();
    if words.is_empty() {
        return false;
    }
    let present = words.iter().filter(|word| contains_words(&candidate, word)).count();
    present * 4 >= words.len() * 3
}

fn names_an_artist(song: &MusicTags, text: &str) -> bool {
    let text = normalize(text);
    song.artists
        .iter()
        .map(|artist| normalize(artist))
        .any(|artist| !artist.is_empty() && contains_words(&text, &artist))
}

fn duration_gap(song: &MusicTags, duration: Option<f64>) -> Option<f64> {
    Some((song.duration_sec? - duration?).abs())
}

/// Whether a YouTube Music song result is the song: the same name, by one of
/// its artists, running within a couple of seconds of the same time.
fn is_same_song(song: &MusicTags, found: &MediaMetadata) -> bool {
    let by = format!("{} {}", found.creator.as_deref().unwrap_or(""), found.title);
    let close = match (song.duration_sec, duration_gap(song, found.duration_sec)) {
        (Some(length), Some(gap)) => gap <= (length * 0.02).max(3.0),
        // Nothing to measure against: the name and the artist have to do.
        _ => true,
    };
    close
        && names_the_song(song, &found.title)
        && names_an_artist(song, &by)
        && !other_version(&song.title, &found.title)
}

/// How good a search result looks as the song. `None` rules it out.
fn score(song: &MusicTags, hit: &engine::SearchHit) -> Option<f64> {
    if !names_the_song(song, &hit.title) || other_version(&song.title, &hit.title) {
        return None;
    }

    let mut value = 100.0;
    if let (Some(length), Some(gap)) = (song.duration_sec, duration_gap(song, hit.duration_sec)) {
        if gap > (length * 0.1).max(15.0) {
            return None;
        }
        value -= gap * 2.0;
    }

    let channel = hit.channel.as_deref().unwrap_or("");
    if names_an_artist(song, channel) {
        value += 25.0;
    } else if names_an_artist(song, &hit.title) {
        value += 10.0;
    } else {
        value -= 20.0;
    }
    if normalize(channel).ends_with(" topic") {
        value += 10.0;
    }

    let title = normalize(&hit.title);
    if contains_words(&title, "official audio") {
        value += 8.0;
    } else if contains_words(&title, "official video") || contains_words(&title, "music video") {
        // Often the song with a scene before or after it.
        value -= 4.0;
    }

    if let Some(views) = hit.view_count.filter(|views| *views > 0) {
        value += (views as f64).log10();
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_addresses_spotify_shares() {
        let link = |url: &str| parse_link(url).map(|link| (link.kind, link.id));
        assert_eq!(
            link("https://open.spotify.com/track/0VjIjW4GlUZAMYd2vXMi3b?si=abc123"),
            Some((Kind::Track, "0VjIjW4GlUZAMYd2vXMi3b".into()))
        );
        assert_eq!(
            link("https://open.spotify.com/intl-tr/album/4yP0hdKOZPNshxUOjY0cZj"),
            Some((Kind::Album, "4yP0hdKOZPNshxUOjY0cZj".into()))
        );
        assert_eq!(
            link("open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M"),
            Some((Kind::Playlist, "37i9dQZF1DXcBWIGoYBM5M".into()))
        );
        assert_eq!(
            link("https://open.spotify.com/artist/1Xyo4u8uXC1ZmMpatF05PJ"),
            Some((Kind::Artist, "1Xyo4u8uXC1ZmMpatF05PJ".into()))
        );
        assert_eq!(link("https://open.spotify.com/episode/0VjIjW4GlUZAMYd2vXMi3b"), None);
        assert_eq!(link("https://open.spotify.com/track/"), None);
        assert_eq!(link("https://notspotify.com/track/0VjIjW4GlUZAMYd2vXMi3b"), None);
    }

    /// The head of a song page as Spotify served it on 2026-10-01, cut down to
    /// the tags this reads.
    const SONG_PAGE: &str = r#"<html><head>
        <meta property="og:title" content="Blinding Lights"/>
        <meta property="og:description" content="The Weeknd · After Hours · Song · 2020"/>
        <meta property="og:image" content="https://i.scdn.co/image/ab67616d0000b2738863bc11d2aa12b54f5aeb36"/>
        <meta name="music:duration" content="200"/>
        <meta name="music:album:track" content="9"/>
        <meta name="music:release_date" content="2020-03-20"/>
        <meta name="music:musician_description" content="The Weeknd"/>
        </head><body><meta property="og:title" content="Something else"/></body></html>"#;

    #[test]
    fn the_player_still_names_a_song_whose_page_has_no_tags() {
        let entity = embed_entity(&embed(serde_json::json!({
            "type": "track",
            "name": "Blinding Lights",
            "artists": [{ "name": "The Weeknd", "uri": "spotify:artist:1Xyo4u8uXC1ZmMpatF05PJ" }],
            "releaseDate": { "isoString": "2020-03-20T00:00:00Z" },
            "duration": 200040,
            "visualIdentity": { "image": [{ "url": "https://image-cdn-fa.spotifycdn.com/image/ab67616d0000b273aa", "maxWidth": 640 }] },
        })))
        .unwrap();
        let song = song_from_embed(&entity, "https://open.spotify.com/track/0VjIjW4GlUZAMYd2vXMi3b").unwrap();
        assert_eq!(song.title, "Blinding Lights");
        assert_eq!(song.artists, ["The Weeknd"]);
        assert_eq!(song.release_date.as_deref(), Some("2020-03-20"));
        assert_eq!(song.duration_sec, Some(200.04));
        assert_eq!(song.album, None);

        // The web player's shell: no song to read.
        assert!(song_from_page("<html><head><title>Spotify – Web Player</title></head></html>", "x").is_none());
    }

    #[test]
    fn a_song_page_gives_the_album_and_its_place_on_it() {
        let song = song_from_page(SONG_PAGE, "https://open.spotify.com/track/0VjIjW4GlUZAMYd2vXMi3b").unwrap();
        assert_eq!(song.title, "Blinding Lights");
        assert_eq!(song.artists, ["The Weeknd"]);
        assert_eq!(song.album.as_deref(), Some("After Hours"));
        assert_eq!(song.track_number, Some(9));
        assert_eq!(song.release_date.as_deref(), Some("2020-03-20"));
        assert_eq!(song.duration_sec, Some(200.0));
        assert!(song.cover_url.as_deref().is_some_and(|url| url.contains("ab67616d0000b273")));
        assert_eq!(upload_date(song.release_date.as_deref()).as_deref(), Some("20200320"));
        assert_eq!(upload_date(Some("2020")), None);
    }

    fn embed(entity: Value) -> String {
        format!(
            r#"<html><script id="__NEXT_DATA__" type="application/json">{}</script></html>"#,
            serde_json::json!({ "props": { "pageProps": { "state": { "data": { "entity": entity } } } } })
        )
    }

    fn album_entity() -> Value {
        serde_json::json!({
            "type": "album",
            "name": "Starboy",
            "subtitle": "The Weeknd",
            "visualIdentity": { "image": [
                { "url": "https://image-cdn-fa.spotifycdn.com/image/ab67616d00001e02aa", "maxWidth": 300 },
                { "url": "https://image-cdn-fa.spotifycdn.com/image/ab67616d0000b273aa", "maxWidth": 640 },
                { "url": "https://image-cdn-fa.spotifycdn.com/image/ab67616d00004851aa", "maxWidth": 64 },
            ]},
            "trackList": [
                { "uri": "spotify:track:7MXVkk9YMctZqd1Srtv4MB", "title": "Starboy", "subtitle": "The Weeknd, Daft Punk", "duration": 230453 },
                { "uri": "spotify:track:1U6hk5hXyEKM8jJBCuWYWb", "title": "Party Monster", "subtitle": "The Weeknd", "duration": 249213 },
            ],
        })
    }

    #[test]
    fn an_album_lists_its_songs_with_their_album_and_places() {
        let entity = embed_entity(&embed(album_entity())).unwrap();
        let album = parse_collection(&entity, Kind::Album, "2ODvWsOgouMbaA5xf0RkJe").unwrap();

        assert_eq!(album.media_kind, MediaKind::Gallery);
        assert_eq!(album.platform, PlatformId::Spotify);
        assert_eq!(album.title, "Starboy");
        assert_eq!(album.creator.as_deref(), Some("The Weeknd"));
        assert_eq!(album.entry_count, Some(2));
        assert_eq!(album.thumbnail_url.as_deref(), Some("https://image-cdn-fa.spotifycdn.com/image/ab67616d0000b273aa"));
        assert_eq!(album.tracks[0].artists, "The Weeknd, Daft Punk");
        assert_eq!(album.tracks[1].position, 2);

        let second = album.entries[1].music.as_ref().unwrap();
        assert_eq!(second.album.as_deref(), Some("Starboy"));
        assert_eq!(second.track_number, Some(2));
        assert_eq!(second.collection.as_deref(), Some("Starboy"));
        assert_eq!(second.page_url, "https://open.spotify.com/track/1U6hk5hXyEKM8jJBCuWYWb");
        assert!(is_pending(&album.entries[1]));
        assert_eq!(album.entries[0].music.as_ref().unwrap().artists, ["The Weeknd", "Daft Punk"]);
    }

    #[test]
    fn a_playlists_songs_wait_for_their_own_pages_to_name_their_albums() {
        let mut entity = album_entity();
        entity["type"] = "playlist".into();
        entity["subtitle"] = "Spotify".into();
        let playlist = parse_collection(&entity, Kind::Playlist, "37i9dQZF1DXcBWIGoYBM5M").unwrap();
        let first = playlist.entries[0].music.as_ref().unwrap();
        assert_eq!(first.album, None);
        assert_eq!(first.track_number, None);
        assert_eq!(first.collection.as_deref(), Some("Starboy"));

        let page = song_from_page(SONG_PAGE, &first.page_url).unwrap();
        let completed = complete(first.clone(), page);
        assert_eq!(completed.album.as_deref(), Some("After Hours"));
        assert_eq!(completed.track_number, Some(9));
        // The listing's own description of the song stands.
        assert_eq!(completed.title, "Starboy");
        assert_eq!(completed.collection.as_deref(), Some("Starboy"));
    }

    fn song(title: &str, artists: &[&str], seconds: f64) -> MusicTags {
        MusicTags {
            title: title.into(),
            artists: artists.iter().map(|name| name.to_string()).collect(),
            duration_sec: Some(seconds),
            ..MusicTags::default()
        }
    }

    fn hit(title: &str, channel: &str, seconds: f64, views: u64) -> engine::SearchHit {
        engine::SearchHit {
            id: title.into(),
            url: format!("https://www.youtube.com/watch?v={}", title.len()),
            title: title.into(),
            duration_sec: Some(seconds),
            channel: Some(channel.into()),
            channel_id: None,
            view_count: Some(views),
            thumbnail_url: None,
        }
    }

    fn best<'a>(wanted: &MusicTags, hits: &'a [engine::SearchHit]) -> Option<&'a str> {
        hits.iter()
            .filter_map(|candidate| score(wanted, candidate).map(|value| (value, candidate)))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .map(|(_, candidate)| candidate.title.as_str())
    }

    #[test]
    fn the_studio_recording_wins_over_a_live_one_that_runs_closer() {
        // YouTube's own results for this song on 2026-10-01.
        let hits = [
            hit("The Weeknd - Blinding Lights (Official Video)", "The Weeknd", 263.0, 1_068_439_933),
            hit("The Weeknd - Blinding Lights (Official Audio)", "The Weeknd", 204.0, 880_644_506),
            hit("The Weeknd - Blinding Lights (Lyrics)", "7clouds", 200.0, 157_512_794),
            hit("The Weeknd - Blinding Lights (Live On The 2020 MTV VMAs)", "The Weeknd", 199.0, 50_604_657),
            hit("The Weeknd - Blinding Lights (Fideles Edit)", "Fideles Music", 302.0, 10_981),
        ];
        let wanted = song("Blinding Lights", &["The Weeknd"], 200.04);
        assert_eq!(best(&wanted, &hits), Some("The Weeknd - Blinding Lights (Official Audio)"));
    }

    #[test]
    fn a_name_spelled_without_its_accents_is_the_same_name() {
        let hits = [
            hit("TARKAN - Şımarık (Official Music Video)", "Tarkan", 192.0, 105_866_079),
            hit("Simarik", "Tarkan", 193.0, 18_261_962),
            hit("TARKAN : THE WORLD MUSIC AWARDS IN MONACO 1999", "F6FGrumman", 209.0, 18_313_981),
        ];
        let wanted = song("Şımarık", &["Tarkan"], 193.0);
        assert_eq!(best(&wanted, &hits), Some("Simarik"));
    }

    #[test]
    fn a_version_the_song_itself_names_is_not_held_against_it() {
        let wanted = song("Blinding Lights - Live", &["The Weeknd"], 199.0);
        assert!(score(&wanted, &hit("The Weeknd - Blinding Lights (Live On The 2020 MTV VMAs)", "The Weeknd", 199.0, 1)).is_some());

        let studio = song("Blinding Lights", &["The Weeknd"], 200.0);
        assert!(score(&studio, &hit("The Weeknd - Blinding Lights (Live)", "The Weeknd", 200.0, 1)).is_none());
        assert!(score(&studio, &hit("Alive", "The Weeknd", 200.0, 1)).is_none(), "not the song at all");
    }

    #[test]
    fn a_result_far_from_the_songs_length_is_ruled_out() {
        let wanted = song("Nicole Kidman", &["ADÈLA"], 181.27);
        assert!(score(&wanted, &hit("ADÈLA - \"Nicole Kidman\" (Official Trailer)", "ADÈLA", 8.0, 557_328)).is_none());
        assert!(score(&wanted, &hit("Nicole Kidman", "Adela", 182.0, 1_201_577)).is_some());
    }

    #[test]
    fn version_notes_are_left_out_of_the_name_that_is_searched_for() {
        assert_eq!(base_title("Here Comes The Sun - Remastered 2019"), "Here Comes The Sun");
        assert_eq!(base_title("One Of The Girls (with JENNIE, Lily Rose Depp)"), "One Of The Girls");
        assert_eq!(base_title("(I Can't Get No) Satisfaction"), "Satisfaction");
        assert_eq!(base_title("(untitled)"), "(untitled)");
        assert_eq!(
            search_words(&song("Starboy (feat. Daft Punk)", &["The Weeknd", "Daft Punk"], 230.0)),
            "The Weeknd Daft Punk Starboy"
        );
    }

    fn found(title: &str, creator: &str, seconds: f64) -> MediaMetadata {
        MediaMetadata {
            title: title.into(),
            creator: Some(creator.into()),
            duration_sec: Some(seconds),
            canonical_url: "https://music.youtube.com/watch?v=J7p4bzqLvCw".into(),
            provider_id: engine::PROVIDER_ID.into(),
            ..pending_song(MusicTags::default())
        }
    }

    #[test]
    fn a_youtube_music_result_has_to_be_the_same_length_and_artist() {
        let wanted = song("Blinding Lights", &["The Weeknd"], 200.04);
        assert!(is_same_song(&wanted, &found("Blinding Lights", "The Weeknd - Topic", 201.0)));
        assert!(!is_same_song(&wanted, &found("Blinding Lights", "The Weeknd - Topic", 262.0)));
        assert!(!is_same_song(&wanted, &found("Blinding Lights", "Some Cover Band", 200.0)));
        assert!(!is_same_song(&wanted, &found("Save Your Tears", "The Weeknd - Topic", 200.0)));
    }

    fn audio(id: &str, container: &str, codec: &str, abr: f64) -> MediaFormat {
        MediaFormat {
            id: id.into(),
            container: container.into(),
            acodec: Some(codec.into()),
            abr: Some(abr),
            quality_label: format!("{abr} kbps"),
            url: Some(format!("https://rr.googlevideo.test/{id}")),
            ..pending_format()
        }
    }

    #[test]
    fn a_found_recording_keeps_its_aac_sound_under_the_songs_description() {
        let mut recording = found("Blinding Lights", "The Weeknd - Topic", 201.0);
        recording.formats = vec![
            audio("251", "webm", "opus", 135.0),
            audio("140", "m4a", "mp4a.40.2", 129.5),
            audio("140-drc", "m4a", "mp4a.40.2", 129.5),
            MediaFormat {
                id: "137".into(),
                kind: FormatKind::Video,
                has_video: true,
                has_audio: false,
                ..pending_format()
            },
        ];
        let mut wanted = song("Blinding Lights", &["The Weeknd"], 200.04);
        wanted.page_url = "https://open.spotify.com/track/0VjIjW4GlUZAMYd2vXMi3b".into();

        let song = merge(wanted, recording);
        let ids: Vec<&str> = song.formats.iter().map(|format| format.id.as_str()).collect();
        assert_eq!(ids, ["140"]);
        assert_eq!(song.title, "Blinding Lights");
        assert_eq!(song.creator.as_deref(), Some("The Weeknd"));
        assert_eq!(song.platform, PlatformId::Spotify);
        assert_eq!(song.canonical_url, "https://open.spotify.com/track/0VjIjW4GlUZAMYd2vXMi3b");
        assert_eq!(song.stream_page(), "https://music.youtube.com/watch?v=J7p4bzqLvCw");
        assert_eq!(song.provider_id, engine::PROVIDER_ID);
        assert!(!is_pending(&song));
    }

    #[test]
    fn a_pending_song_plans_as_an_m4a_file() {
        let pending = pending_song(song("Blinding Lights", &["The Weeknd"], 200.0));
        let plan = crate::downloader::plan::build(
            &pending,
            crate::model::DownloadMode::Audio,
            crate::model::QualityPreference::Best,
            None,
            None,
            None,
            crate::model::WatermarkPreference::Any,
        )
        .unwrap();
        assert_eq!(plan.container, "m4a");
        assert!(plan.convert_to.is_none());
    }
}
