//! End-to-end tests against the real network.
//!
//! These are `#[ignore]`d so an ordinary `cargo test` stays offline and fast.
//! Run them deliberately:
//!
//! ```text
//! cargo test --test pipeline -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! They exercise what unit tests cannot: that the engine really installs, that
//! a live source parses into the model, and that the downloader writes a
//! byte-correct file. Resume is covered separately and hermetically in
//! `tests/resume.rs`, where the server's behaviour is not a variable.

use std::sync::Arc;

use universal_downloader_lib::downloader::control::TaskControl;
use universal_downloader_lib::downloader::{http, plan};
use universal_downloader_lib::model::{DownloadMode, QualityPreference, WatermarkPreference};
use universal_downloader_lib::settings::Settings;
use universal_downloader_lib::{net, paths, providers, tools};

/// 2.8 MB, `video/mp4`. Small enough to fetch twice in a test run.
const SAMPLE_MEDIA: &str = "https://download.samplelib.com/mp4/sample-5s.mp4";

fn settings() -> Settings {
    Settings {
        network_timeout_sec: 45,
        ..Settings::default()
    }
}

async fn ensure_engine(settings: &Settings) {
    let state = tools::refresh(settings).await;
    if state.engine.available {
        eprintln!("engine already present: {:?}", state.engine.version);
        return;
    }

    eprintln!("installing the engine...");
    let progress = |received: u64, total: Option<u64>, stage: &str| {
        if stage == "downloading" && received % (4 * 1024 * 1024) < 200_000 {
            eprintln!("  {received} / {total:?}");
        }
    };
    let status = tools::install(
        universal_downloader_lib::model::ToolKind::Engine,
        settings,
        &progress,
    )
    .await
    .expect("the engine should install");

    assert!(status.available, "engine reported unavailable after install");
    eprintln!("installed engine {:?}", status.version);
}

#[tokio::test]
#[ignore = "downloads the engine binary"]
async fn installs_the_engine_and_reports_a_version() {
    let settings = settings();
    ensure_engine(&settings).await;

    let state = tools::snapshot();
    assert!(state.engine.available);
    let version = state.engine.version.clone().unwrap_or_default();
    assert!(!version.is_empty(), "engine did not report a version");
    assert!(
        version.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "unexpected version string: {version}"
    );
}

#[tokio::test]
#[ignore = "contacts a live media host"]
async fn reads_a_direct_media_file_without_the_engine() {
    let settings = settings();
    let metadata = providers::analyze(SAMPLE_MEDIA, &settings)
        .await
        .expect("the direct provider should handle a .mp4 URL");

    assert_eq!(metadata.provider_id, "direct");
    assert_eq!(metadata.platform, universal_downloader_lib::model::PlatformId::Direct);
    assert_eq!(metadata.formats.len(), 1);

    let format = &metadata.formats[0];
    assert_eq!(format.container, "mp4");
    assert!(!format.needs_engine_download);
    assert!(
        format.filesize.is_some_and(|size| size > 1_000_000),
        "expected a real content length, got {:?}",
        format.filesize
    );
}

#[tokio::test]
#[ignore = "downloads a real file"]
async fn downloads_a_direct_file_and_writes_it_whole() {
    let settings = settings();
    let metadata = providers::analyze(SAMPLE_MEDIA, &settings).await.unwrap();

    let built = plan::build(
        &metadata,
        DownloadMode::Video,
        QualityPreference::Best,
        None,
        None,
        None,
        WatermarkPreference::Any,
    )
    .expect("a plan should be produced");

    let format = built.video.expect("the plan should carry a video stream");
    let expected = format.filesize.expect("the source reported a size");

    let target = paths::temp_dir().unwrap().join("test-direct.mp4");
    let _ = std::fs::remove_file(&target);
    let _ = std::fs::remove_file(http::part_path(&target));

    let client = net::client(&settings).unwrap();
    let mut samples = 0usize;
    let mut last_percent = 0.0f64;
    let mut sink = |sample: http::ProgressSample| {
        samples += 1;
        if let Some(percent) = sample.percent {
            // Progress must never move backwards.
            assert!(
                percent + 0.001 >= last_percent,
                "progress went backwards: {last_percent} -> {percent}"
            );
            last_percent = percent;
        }
    };

    let written = http::fetch(
        &client,
        http::FetchOptions {
            url: format.url.as_deref().unwrap(),
            headers: &format.http_headers,
            target: &target,
            low_resource: false,
        },
        TaskControl::shared(),
        &mut sink,
    )
    .await
    .expect("the download should succeed");

    assert_eq!(written, expected, "wrote a different number of bytes than announced");
    assert!(samples > 0, "no progress was reported");

    let on_disk = std::fs::metadata(&target).unwrap().len();
    assert_eq!(on_disk, expected);
    assert!(
        !http::part_path(&target).exists(),
        "the .part file should be renamed away on success"
    );

    // An MP4 begins with a box-size field followed by the 'ftyp' box type.
    let head = std::fs::read(&target).unwrap();
    assert_eq!(&head[4..8], b"ftyp", "downloaded bytes are not an MP4");

    std::fs::remove_file(&target).unwrap();
}

/// Resume *semantics* are covered hermetically in `tests/resume.rs`, against a
/// server this repository controls. Public CDNs vary in whether they honour
/// `Range` -- this sample host answers 200 and ignores it -- so asserting a
/// resume here would be asserting the host's behaviour, not the app's. What
/// this test checks is the property that must hold either way: after an
/// interruption, running the fetch again yields a complete, correct file.
#[tokio::test]
#[ignore = "downloads a real file twice"]
async fn recovers_from_an_interruption_against_a_live_host() {
    let settings = settings();
    let metadata = providers::analyze(SAMPLE_MEDIA, &settings).await.unwrap();
    let format = metadata.formats.into_iter().next().unwrap();
    let expected = format.filesize.unwrap();

    let target = paths::temp_dir().unwrap().join("test-resume.mp4");
    let part = http::part_path(&target);
    let _ = std::fs::remove_file(&target);
    let _ = std::fs::remove_file(&part);

    let client = net::client(&settings).unwrap();

    // First pass: stop as soon as a meaningful slice has arrived.
    let control = TaskControl::shared();
    {
        let control_for_sink = Arc::clone(&control);
        let mut sink = move |sample: http::ProgressSample| {
            if sample.received > expected / 4 {
                control_for_sink.cancel();
            }
        };

        let result = http::fetch(
            &client,
            http::FetchOptions {
                url: format.url.as_deref().unwrap(),
                headers: &format.http_headers,
                target: &target,
                low_resource: false,
            },
            Arc::clone(&control),
            &mut sink,
        )
        .await;

        assert!(result.is_err(), "the interrupted fetch should not report success");
        assert!(!target.exists(), "an interrupted download must not be finalized");
    }

    let partial = std::fs::metadata(&part)
        .expect("a .part file should remain after an interruption")
        .len();
    assert!(partial > 0 && partial < expected, "unexpected partial size {partial}");
    eprintln!("interrupted at {partial} / {expected} bytes");

    // Second pass: a fresh control resumes from the partial file.
    let mut first_report: Option<u64> = None;
    let mut sink = |sample: http::ProgressSample| {
        first_report.get_or_insert(sample.received);
    };

    let written = http::fetch(
        &client,
        http::FetchOptions {
            url: format.url.as_deref().unwrap(),
            headers: &format.http_headers,
            target: &target,
            low_resource: false,
        },
        TaskControl::shared(),
        &mut sink,
    )
    .await
    .expect("the resumed download should succeed");

    assert_eq!(written, expected);

    // Either the host honoured the range and picked up where we stopped, or it
    // ignored it and the transfer restarted. Both are correct; a count that is
    // neither would mean the partial file was appended to wrongly.
    let resumed = first_report == Some(partial);
    assert!(
        resumed || first_report == Some(0),
        "unexpected starting count {first_report:?} (partial was {partial})"
    );
    eprintln!(
        "host {} the range request",
        if resumed { "honoured" } else { "ignored" }
    );

    let head = std::fs::read(&target).unwrap();
    assert_eq!(&head[4..8], b"ftyp", "the resumed file is not a valid MP4");
    std::fs::remove_file(&target).unwrap();
}

#[tokio::test]
#[ignore = "contacts a live platform through the engine"]
async fn reads_platform_metadata_through_the_engine() {
    let settings = settings();
    ensure_engine(&settings).await;

    // A long-standing public Creative Commons upload. Metadata only -- no media
    // bytes are fetched by this test.
    let url = "https://www.youtube.com/watch?v=aqz-KE-bpKQ";
    let metadata = providers::analyze(url, &settings)
        .await
        .expect("the engine should read this public video");

    assert_eq!(metadata.provider_id, "engine");
    assert_eq!(metadata.platform, universal_downloader_lib::model::PlatformId::Youtube);
    assert!(!metadata.title.is_empty());
    assert!(metadata.creator.is_some());
    assert!(metadata.duration_sec.is_some_and(|d| d > 0.0));
    assert!(metadata.thumbnail_url.is_some());
    assert!(metadata.formats.len() > 3, "expected several renditions");

    // The catalogue must contain both muxed and split streams for a plan to be
    // able to choose between merging and not.
    assert!(metadata.formats.iter().any(|f| f.has_video));
    assert!(metadata.formats.iter().any(|f| f.has_audio));

    let best = plan::build(
        &metadata,
        DownloadMode::Video,
        QualityPreference::Best,
        None,
        None,
        None,
        WatermarkPreference::Any,
    )
    .expect("a best-quality plan should be produced");
    assert!(best.video.is_some());
    eprintln!(
        "{} - best plan: {} (merge={}, engine={})",
        metadata.title, best.label, best.needs_merge, best.needs_engine
    );

    let audio = plan::build(
        &metadata,
        DownloadMode::Audio,
        QualityPreference::Best,
        None,
        None,
        None,
        WatermarkPreference::Any,
    )
    .expect("an audio-only plan should be produced");
    assert!(audio.audio.is_some());
    assert!(audio.video.is_none());
}

/// A song shared from Spotify: its description read from Spotify, its
/// recording found on YouTube, and the file named and tagged as the song.
#[tokio::test]
#[ignore = "contacts Spotify and YouTube and downloads a song"]
async fn downloads_a_spotify_song_as_that_song() {
    use universal_downloader_lib::downloader;
    use universal_downloader_lib::model::{DownloadRequest, PlatformId};

    let settings = settings();
    ensure_engine(&settings).await;

    let url = "https://open.spotify.com/track/0VjIjW4GlUZAMYd2vXMi3b";
    let song = providers::analyze(url, &settings)
        .await
        .expect("the song should be found");
    assert_eq!(song.platform, PlatformId::Spotify);
    assert_eq!(song.title, "Blinding Lights");
    assert_eq!(song.creator.as_deref(), Some("The Weeknd"));
    let music = song.music.clone().expect("a song carries its tags");
    assert_eq!(music.album.as_deref(), Some("After Hours"));
    let found = music.stream_page.clone().expect("a recording was found");
    eprintln!("matched {found}; formats {:?}", song.formats.iter().map(|f| &f.id).collect::<Vec<_>>());
    assert!(found.contains("youtube.com/watch?v="));
    assert!(song.formats.iter().all(|format| format.kind == universal_downloader_lib::model::FormatKind::Audio));

    let album = providers::analyze("https://open.spotify.com/album/4yP0hdKOZPNshxUOjY0cZj", &settings)
        .await
        .expect("the album should be listed");
    assert_eq!(album.title, "After Hours");
    assert_eq!(album.tracks.len(), 14);
    eprintln!("album tracks: {:?}", album.tracks.iter().map(|t| &t.title).collect::<Vec<_>>());

    let output = std::env::temp_dir().join(format!("ud-spotify-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    let request = DownloadRequest {
        url: album.canonical_url.clone(),
        mode: DownloadMode::Audio,
        quality: QualityPreference::Best,
        video_format_id: None,
        audio_format_id: None,
        container: None,
        watermark: WatermarkPreference::Any,
        output_dir: Some(output.to_string_lossy().into_owned()),
        title: None,
        thumbnail_url: None,
        platform: None,
        entry: Some(2),
        audio_language: None,
        source: None,
    };
    providers::remember_analysis(&request.url, &settings, &album);
    let mut sink = |_progress| {};
    let outcome = downloader::execute("spotifytest", &request, &settings, Arc::new(TaskControl::new()), &mut sink)
        .await
        .expect("the second song of the album should download");

    eprintln!("wrote {}", outcome.output_path.display());
    assert_eq!(outcome.output_path.parent().unwrap(), output.join("After Hours"));
    assert_eq!(outcome.output_path.extension().unwrap(), "m4a");

    let ffprobe = tools::ffmpeg_path().unwrap().with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    let probe = std::process::Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "format_tags:stream=codec_type,codec_name:stream_disposition=attached_pic", "-of", "json"])
        .arg(&outcome.output_path)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&probe.stdout);
    eprintln!("{report}");
    let report: serde_json::Value = serde_json::from_str(&report).unwrap();
    let tags = &report["format"]["tags"];
    assert_eq!(tags["album"], "After Hours");
    assert_eq!(tags["artist"], "The Weeknd");
    assert_eq!(tags["track"], "2");
    assert!(report["streams"].as_array().unwrap().iter().any(|s| s["disposition"]["attached_pic"] == 1), "no cover");

    std::fs::remove_dir_all(&output).unwrap();
}

/// Anime searched for on the channels that license it, and a dubbed episode
/// downloaded in its original language.
#[tokio::test]
#[ignore = "searches YouTube"]
async fn finds_an_official_episode_and_keeps_its_japanese_track() {
    let settings = settings();
    ensure_engine(&settings).await;

    let episodes = providers::anime::search("dress up darling", &settings)
        .await
        .expect("the channels should answer");
    for episode in &episodes {
        eprintln!("{} | {} | {:?}", episode.channel, episode.title, episode.duration_sec);
    }
    assert!(!episodes.is_empty(), "no episodes found");
    assert!(episodes.iter().all(|episode| episode.duration_sec.is_some_and(|s| s >= 600.0)));

    let metadata = providers::analyze(&episodes[0].url, &settings)
        .await
        .expect("the episode should be readable");
    let plan = plan::build(
        &metadata,
        DownloadMode::Video,
        QualityPreference::Best,
        None,
        None,
        None,
        WatermarkPreference::Any,
    )
    .unwrap();
    let audio = plan.audio.expect("a separate sound track");
    eprintln!("{} + {} ({:?})", plan.video.unwrap().quality_label, audio.id, audio.language);
    assert_eq!(audio.language.as_deref(), Some("ja"));
    assert!(providers::anime::episode_of(&metadata).is_some());
}

/// An X video keeps its sound. X names no codec on its progressive files and
/// none on its HLS sound track, which once read as a silent picture and as
/// nothing at all.
#[tokio::test]
#[ignore = "contacts X through the engine"]
async fn an_x_video_downloads_with_its_sound() {
    let settings = settings();
    ensure_engine(&settings).await;

    let metadata = providers::analyze("https://x.com/i/web/status/910031516746514432", &settings)
        .await
        .expect("the post should be readable");
    for quality in [QualityPreference::Auto, QualityPreference::Best] {
        let chosen = plan::build(&metadata, DownloadMode::Video, quality, None, None, None, WatermarkPreference::Any)
            .unwrap();
        let video = chosen.video.as_ref().expect("a picture");
        eprintln!("{quality:?}: {} (sound in it: {}, separate: {:?})", video.id, video.has_audio, chosen.audio.as_ref().map(|a| &a.id));
        assert!(video.has_audio || chosen.audio.is_some(), "{quality:?} would download no sound");
    }
    let sound = plan::build(&metadata, DownloadMode::Audio, QualityPreference::Best, None, None, None, WatermarkPreference::Any)
        .expect("the sound alone should be offered too");
    assert!(sound.audio.is_some());
}

/// A TikTok video whose sharpest picture has no sound and whose sound is never
/// offered apart. The file keeps the 1080p picture and the sound out of the
/// 720p rendition -- one picture, one sound, nothing else -- and the sound
/// alone still comes out as an M4A.
///
/// TikTok refuses the direct transfer with a 403, so this runs the engine
/// path with `--video-multistreams` and the merger's negative map, which is
/// what a unit test cannot: it depends on how this yt-dlp builds its merge.
/// The app installs the engine's latest release and keeps it updated, so this
/// is worth running after every engine update rather than once: a change
/// there shows up as a stray second picture in the file, not as silence.
#[tokio::test]
#[ignore = "contacts TikTok through the engine and downloads about 110 MB"]
async fn a_tiktok_video_whose_best_picture_is_silent_downloads_with_its_sound() {
    use universal_downloader_lib::downloader;
    use universal_downloader_lib::model::DownloadRequest;

    let settings = settings();
    ensure_engine(&settings).await;

    let url = "https://www.tiktok.com/@edat673243/video/7691247381555268877";
    let metadata = providers::analyze(url, &settings).await.expect("the video should be readable");
    let chosen = plan::build(&metadata, DownloadMode::Video, QualityPreference::Best, None, None, None, WatermarkPreference::Any)
        .unwrap();
    eprintln!("best: {} ({:?})", chosen.selector_for_engine(), chosen.estimated_bytes);
    assert!(chosen.sound_from_muxed(), "the 1080p picture is published without sound");

    let output = std::env::temp_dir().join(format!("ud-tiktok-{}", std::process::id()));
    std::fs::create_dir_all(&output).unwrap();
    let ffprobe = tools::ffmpeg_path().unwrap().with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    let streams = |path: &std::path::Path| -> Vec<(String, Option<u64>)> {
        let probe = std::process::Command::new(&ffprobe)
            .args(["-v", "error", "-show_entries", "stream=codec_type,height", "-of", "json"])
            .arg(path)
            .output()
            .unwrap();
        let report: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        report["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|stream| (stream["codec_type"].as_str().unwrap().to_string(), stream["height"].as_u64()))
            .collect()
    };

    for mode in [DownloadMode::Video, DownloadMode::Audio] {
        let request = DownloadRequest {
            url: url.into(),
            mode,
            quality: QualityPreference::Best,
            video_format_id: None,
            audio_format_id: None,
            container: None,
            watermark: WatermarkPreference::Any,
            output_dir: Some(output.to_string_lossy().into_owned()),
            title: None,
            thumbnail_url: None,
            platform: None,
            entry: None,
            audio_language: None,
            source: None,
        };
        let mut last_received = 0u64;
        let mut sink = |progress: universal_downloader_lib::model::DownloadProgress| {
            if progress.stage == universal_downloader_lib::model::DownloadStage::Video {
                assert!(progress.received_bytes >= last_received, "progress went backwards");
                last_received = progress.received_bytes;
            }
        };
        let outcome = downloader::execute("tiktoktest", &request, &settings, Arc::new(TaskControl::new()), &mut sink)
            .await
            .expect("the download should succeed");
        let found = streams(&outcome.output_path);
        eprintln!("{mode:?}: {} -> {found:?}", outcome.output_path.display());

        let pictures: Vec<_> = found.iter().filter(|(kind, _)| kind == "video").collect();
        let sounds = found.iter().filter(|(kind, _)| kind == "audio").count();
        assert_eq!(sounds, 1, "{mode:?}: exactly one sound track");
        match mode {
            DownloadMode::Video => {
                assert_eq!(pictures.len(), 1, "the 720p picture must not ride along");
                assert_eq!(pictures[0].1, Some(1080));
                assert_eq!(outcome.output_path.extension().unwrap(), "mp4");
            }
            _ => {
                assert!(pictures.is_empty());
                assert_eq!(outcome.output_path.extension().unwrap(), "m4a");
            }
        }
    }

    std::fs::remove_dir_all(&output).unwrap();
}

/// A TikTok post its creator limited to signed-in adults says so, rather than
/// reading as a refusal with nothing to offer.
///
/// With the browser link off: this asks what the app says to someone who has
/// lent it nothing, and the developer's own TikTok session -- if this machine
/// holds one -- would make the answer depend on that account instead.
#[tokio::test]
#[ignore = "contacts a live media host"]
async fn an_age_restricted_tiktok_asks_for_a_tiktok_sign_in() {
    let settings = Settings {
        browser_link_enabled: false,
        ..settings()
    };
    ensure_engine(&settings).await;

    let err = providers::analyze(
        "https://www.tiktok.com/@itzmav_/video/7670756126907960589",
        &settings,
    )
    .await
    .expect_err("the post is shown only to a signed-in viewer");
    eprintln!("{err}");
    assert_eq!(err.code(), "tiktokSignIn");
}
