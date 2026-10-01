//! Anime from its licensors' own YouTube channels.
//!
//! Only channels that post whole episodes with the original Japanese track and
//! the subtitles as tracks of their own are searched. On those, a download
//! without subtitles is simply the video: nothing has to be taken out of the
//! picture, because nothing was ever put into it. Channels that burn their
//! subtitles in (Ani-One, Muse, the Gundam channel) or post only dubs (TMS)
//! are left out, and so is every site that streams anime it has no licence to.
//!
//! Episodes found here download like any other YouTube video, in its original
//! language unless another is picked, and are named the way media servers
//! file a series: `Series/Season 01/Series - S01E03.mp4`.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::error::AppResult;
use crate::model::MediaMetadata;
use crate::providers::engine;
use crate::providers::words::{contains_words, normalize};
use crate::settings::Settings;
use crate::log_debug;

/// The channels searched, by id, with the name they upload under.
const CHANNELS: &[(&str, &str)] = &[
    ("UC6pGDc4bFGD1_36IKv3FnYg", "Crunchyroll"),
    ("UCsj_CYajUSQ2ca8bYCMan9g", "It's Anime powered by REMOW"),
];

/// Shorter than this is a clip, a trailer or an opening, not an episode.
const MIN_EPISODE_SEC: f64 = 600.0;

/// How many of each channel's results are looked at.
const PER_CHANNEL: u32 = 30;

/// One episode, or one season posted as a single video.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimeEpisode {
    pub url: String,
    pub title: String,
    pub channel: String,
    pub duration_sec: Option<f64>,
    pub thumbnail_url: Option<String>,
}

/// Search every channel for a series, and list what it has posted in full.
///
/// A channel whose search fails is left out of the answer; only every one of
/// them failing is an error.
pub async fn search(query: &str, settings: &Settings) -> AppResult<Vec<AnimeEpisode>> {
    let query = query.trim();
    if normalize(query).is_empty() {
        return Ok(Vec::new());
    }

    let searches = CHANNELS.iter().map(|(id, _)| {
        let address = format!(
            "https://www.youtube.com/channel/{id}/search?query={}",
            urlencoding::encode(query)
        );
        async move { engine::search(&address, PER_CHANNEL, settings).await }
    });
    let answers = futures_util::future::join_all(searches).await;

    let mut hits = Vec::new();
    let mut first_error = None;
    for ((_, name), answer) in CHANNELS.iter().zip(answers) {
        match answer {
            Ok(found) => hits.extend(found.into_iter().map(|hit| (*name, hit))),
            Err(err) => {
                log_debug!("anime", "searching {name} failed: {err}");
                first_error.get_or_insert(err);
            }
        }
    }
    if hits.is_empty() {
        if let Some(err) = first_error {
            return Err(err);
        }
    }
    Ok(rank(query, hits))
}

/// The episodes among the results, most relevant first and in episode order
/// among equals.
fn rank(query: &str, hits: Vec<(&str, engine::SearchHit)>) -> Vec<AnimeEpisode> {
    let mut found: Vec<(usize, Option<Episode>, AnimeEpisode)> = hits
        .into_iter()
        .filter(|(_, hit)| hit.duration_sec.is_some_and(|seconds| seconds >= MIN_EPISODE_SEC))
        .filter_map(|(channel, hit)| {
            let score = relevance(query, &hit.title)?;
            Some((
                score,
                parse_episode(&hit.title),
                AnimeEpisode {
                    url: format!("https://www.youtube.com/watch?v={}", hit.id),
                    channel: channel.to_string(),
                    title: hit.title,
                    duration_sec: hit.duration_sec,
                    thumbnail_url: hit.thumbnail_url,
                },
            ))
        })
        .collect();

    found.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            let key = |episode: &Option<Episode>| episode.as_ref().map(|e| (e.season, e.number));
            match (key(&a.1), key(&b.1)) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        })
    });
    let mut seen = std::collections::HashSet::new();
    found
        .into_iter()
        .map(|(_, _, episode)| episode)
        .filter(|episode| seen.insert(episode.url.clone()))
        .collect()
}

/// How many of the searched-for words a title has, when it has enough of
/// them to be the series at all. A channel's own search answers with its
/// popular uploads when nothing matches, and those are not what was asked for.
fn relevance(query: &str, title: &str) -> Option<usize> {
    let title = normalize(title);
    let wanted: Vec<String> = normalize(query)
        .split(' ')
        .filter(|word| word.chars().count() > 1)
        .map(str::to_string)
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let present = wanted.iter().filter(|word| contains_words(&title, word)).count();
    (present * 3 >= wanted.len() * 2).then_some(present)
}

/// Where an episode sits in its series, read from its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    pub series: String,
    pub season: u32,
    pub number: u32,
}

static EPISODE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:episode|ep\.?)\s*#?\s*(\d{1,4})\b").expect("episode pattern is valid")
});

static SEASON: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\bseason\s*(\d{1,2})\b").expect("season pattern is valid"));

/// "My Dress-Up Darling Episode 3 SUB/DUB | Then Why Don't We?" is episode 3
/// of the first season of "My Dress-Up Darling". A title that numbers no
/// episode -- a whole season in one video, a clip -- is not one.
pub fn parse_episode(title: &str) -> Option<Episode> {
    let episode = EPISODE.captures(title)?;
    let number: u32 = episode.get(1)?.as_str().parse().ok()?;
    let episode_at = episode.get(0)?.start();

    let season = SEASON.captures(title);
    let season_number = season
        .as_ref()
        .and_then(|found| found.get(1)?.as_str().parse().ok())
        .unwrap_or(1);
    // The series is what comes before the first of the two.
    let cut = season
        .and_then(|found| found.get(0).map(|whole| whole.start()))
        .filter(|start| *start < episode_at)
        .unwrap_or(episode_at);
    let series = title[..cut]
        .trim()
        .trim_end_matches(['-', '|', ':', '–', '—', ','])
        .trim();
    if series.is_empty() || number == 0 {
        return None;
    }
    Some(Episode {
        series: series.to_string(),
        season: season_number,
        number,
    })
}

/// The episode a download is, when it comes from one of the channels searched
/// here.
pub fn episode_of(metadata: &MediaMetadata) -> Option<Episode> {
    let creator = metadata.creator.as_deref()?;
    CHANNELS
        .iter()
        .any(|(_, name)| *name == creator)
        .then(|| parse_episode(&metadata.title))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_episode_is_read_from_its_title() {
        assert_eq!(
            parse_episode("My Dress-Up Darling Episode 3 SUB/DUB | Then Why Don't We?"),
            Some(Episode { series: "My Dress-Up Darling".into(), season: 1, number: 3 })
        );
        assert_eq!(
            parse_episode("Kaiju No. 8 Season 2 Episode 1 | English Sub"),
            Some(Episode { series: "Kaiju No. 8".into(), season: 2, number: 1 })
        );
        assert_eq!(
            parse_episode("VINLAND SAGA Episode 1 | Somewhere not here"),
            Some(Episode { series: "VINLAND SAGA".into(), season: 1, number: 1 })
        );
        assert_eq!(parse_episode("TOUGEN ANKI: Season 1 Complete (2025) | 6 Audio | MULTI-SUB"), None);
        assert_eq!(parse_episode("Gojo Locks In | My Dress-Up Darling Season 2"), None);
        assert_eq!(parse_episode("Episode 4"), None, "a number is not a series");
    }

    fn hit(id: &str, title: &str, seconds: f64) -> engine::SearchHit {
        engine::SearchHit {
            id: id.into(),
            url: format!("https://www.youtube.com/watch?v={id}"),
            title: title.into(),
            duration_sec: Some(seconds),
            channel: None,
            channel_id: None,
            view_count: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn only_whole_episodes_of_the_series_are_listed_in_order() {
        // What the two channels answered for "dress up darling" on 2026-10-01,
        // shortened.
        let hits = vec![
            ("Crunchyroll", hit("cUtNbkuJbmI", "My Dress-Up Darling Episode 3 SUB/DUB | Then Why Don't We?", 1431.0)),
            ("Crunchyroll", hit("8iwNABl5uQ0", "Gojo Locks In | My Dress-Up Darling Season 2", 129.0)),
            ("Crunchyroll", hit("L76gVFxtfi8", "My Dress-Up Darling Episode 1 SUB/DUB | Someone Who Lives in the Exact Opposite World as Me", 1430.0)),
            ("Crunchyroll", hit("8oveGY6h6T8", "My Dress-Up Darling | Official Trailer | Crunchyroll", 79.0)),
            ("It's Anime powered by REMOW", hit("vJMhT9bZlMU", "My Deer Friend Nokotan: Season 1 Complete (12 Episodes) | MULTI-SUB", 16992.0)),
        ];
        let listed = rank("dress up darling", hits);
        let ids: Vec<&str> = listed.iter().map(|episode| episode.url.rsplit('=').next().unwrap()).collect();
        assert_eq!(ids, ["L76gVFxtfi8", "cUtNbkuJbmI"]);
        assert_eq!(listed[0].channel, "Crunchyroll");
    }

    #[test]
    fn a_title_needs_most_of_the_words_that_were_asked_for() {
        assert!(relevance("frieren", "Frieren: Beyond Journey's End Episode 1").is_some());
        assert!(relevance("dress-up darling", "My Dress Up Darling Episode 2").is_some());
        assert!(relevance("dress up darling", "My Deer Friend Nokotan").is_none());
        assert!(relevance("?", "anything").is_none());
    }

    #[test]
    fn only_the_searched_channels_name_their_downloads_as_episodes() {
        let metadata = |creator: &str| MediaMetadata {
            creator: Some(creator.into()),
            title: "VINLAND SAGA Episode 1 | Somewhere not here".into(),
            ..crate::providers::tests_support::blank()
        };
        assert_eq!(episode_of(&metadata("Crunchyroll")).map(|episode| episode.number), Some(1));
        assert_eq!(episode_of(&metadata("Someone Else")), None);
    }
}
