//! Photo posts, end to end against the real platforms.
//!
//! `#[ignore]`d like the rest of the online suite, and run the same way:
//!
//! ```text
//! cargo test --test photos -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! Each test reads a long-standing public post and downloads a picture from it
//! through the same path a queued download takes, then checks that what landed
//! on disk is a real image of the size the post described. The posts are
//! among the most-viewed on their platforms, which is what keeps them around;
//! if one is ever removed, swap in another of the same kind.

use std::path::PathBuf;

use universal_downloader_lib::downloader::{self, control::TaskControl, DownloadOutcome};
use universal_downloader_lib::model::{
    DownloadMode, DownloadProgress, DownloadRequest, FormatKind, MediaKind, MediaMetadata, PlatformId,
    QualityPreference, WatermarkPreference,
};
use universal_downloader_lib::settings::Settings;
use universal_downloader_lib::{paths, providers, tools};

fn settings() -> Settings {
    Settings {
        network_timeout_sec: 45,
        ..Settings::default()
    }
}

async fn require_engine(settings: &Settings) {
    let state = tools::refresh(settings).await;
    assert!(
        state.engine.available,
        "these tests read Instagram and Pinterest through the engine; install it first (see tests/pipeline.rs)"
    );
}

async fn analyze(url: &str) -> MediaMetadata {
    let metadata = providers::analyze(url, &settings())
        .await
        .unwrap_or_else(|err| panic!("{url} could not be read: {err}"));
    eprintln!(
        "{url}\n  -> {:?} via {}: {:?} ({} items)",
        metadata.media_kind,
        metadata.provider_id,
        metadata.title,
        metadata.entry_count.unwrap_or(1)
    );
    metadata
}

/// Download one item the way the queue does, into a folder of its own.
async fn download(url: &str, entry: Option<u32>, mode: DownloadMode) -> DownloadOutcome {
    let folder = paths::temp_dir().unwrap().join("photo-tests");
    std::fs::create_dir_all(&folder).unwrap();

    let request = DownloadRequest {
        url: url.to_string(),
        mode,
        quality: QualityPreference::Best,
        video_format_id: None,
        audio_format_id: None,
        container: None,
        watermark: WatermarkPreference::Any,
        output_dir: Some(folder.to_string_lossy().into_owned()),
        title: None,
        thumbnail_url: None,
        platform: None,
        entry,
    };

    let task = format!("photo-test-{}", std::process::id());
    let mut sink = |_: DownloadProgress| {};
    let outcome = downloader::execute(&task, &request, &settings(), TaskControl::shared(), &mut sink)
        .await
        .unwrap_or_else(|err| panic!("{url} item {entry:?} did not download: {err}"));
    eprintln!("  saved {} ({} bytes)", outcome.output_path.display(), outcome.file_size);
    outcome
}

/// Width and height from a JPEG's frame header or a PNG's header chunk.
fn image_size(path: &PathBuf) -> (&'static str, u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        return ("png", width, height);
    }
    assert!(bytes.starts_with(&[0xFF, 0xD8, 0xFF]), "{} is neither a JPEG nor a PNG", path.display());

    let mut index = 2;
    while index + 9 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        if (0xC0..=0xC2).contains(&marker) {
            let height = u32::from(u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]));
            let width = u32::from(u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]));
            return ("jpeg", width, height);
        }
        let length = usize::from(u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]));
        index += 2 + length;
    }
    panic!("{} has no JPEG frame header", path.display());
}

fn cleanup(outcome: &DownloadOutcome) {
    let _ = std::fs::remove_file(&outcome.output_path);
}

#[tokio::test]
#[ignore = "reads a live Instagram post through the engine"]
async fn an_instagram_photo_downloads_at_full_size() {
    require_engine(&settings()).await;
    let url = "https://www.instagram.com/p/BsOGulcndj-/";

    let metadata = analyze(url).await;
    assert_eq!(metadata.media_kind, MediaKind::Image);
    assert!(metadata.title.starts_with("Photo by"), "{}", metadata.title);

    let outcome = download(url, None, DownloadMode::Image).await;
    let (kind, width, height) = image_size(&outcome.output_path);
    assert_eq!(kind, "jpeg");
    // This photo's original is 584 px square; every other rendition Instagram
    // offers of it is capped at 480.
    assert!(width > 480 && height > 480, "got a {width}x{height} rendition, not the original");
    cleanup(&outcome);
}

#[tokio::test]
#[ignore = "reads a live Instagram carousel through the engine"]
async fn each_item_of_an_instagram_carousel_is_its_own_download() {
    require_engine(&settings()).await;
    // Five photos and one video.
    let url = "https://www.instagram.com/p/DQ3zR6-DPGm/";

    let metadata = analyze(url).await;
    assert_eq!(metadata.media_kind, MediaKind::Gallery);
    let kinds: Vec<MediaKind> = metadata.entries.iter().map(|item| item.media_kind).collect();
    assert!(kinds.len() >= 2, "{kinds:?}");
    assert!(kinds.contains(&MediaKind::Image), "{kinds:?}");

    // Two different photos must be two different files -- the bug this
    // replaced downloaded the first item once per item.
    let photos: Vec<u32> = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == MediaKind::Image)
        .map(|(index, _)| index as u32 + 1)
        .take(2)
        .collect();
    let first = download(url, Some(photos[0]), DownloadMode::Image).await;
    let second = download(url, Some(photos[1]), DownloadMode::Image).await;
    assert_ne!(
        std::fs::read(&first.output_path).unwrap(),
        std::fs::read(&second.output_path).unwrap(),
        "two items of the carousel downloaded as the same picture"
    );
    image_size(&first.output_path);
    cleanup(&first);
    cleanup(&second);

    // The video among the photos is planned as a video even in image mode.
    if let Some(position) = kinds.iter().position(|kind| *kind == MediaKind::Video) {
        let item = providers::select_entry(metadata.clone(), Some(position as u32 + 1)).unwrap();
        let plan = downloader::plan::build(
            &item,
            DownloadMode::Image,
            QualityPreference::Best,
            None,
            None,
            None,
            WatermarkPreference::Any,
        )
        .unwrap();
        assert!(plan.video.is_some() && plan.image.is_none());
    }
}

#[tokio::test]
#[ignore = "reads a live X post"]
async fn an_x_photo_post_is_read_without_the_engine() {
    let url = "https://x.com/TheEllenShow/status/440322224407314432";

    let metadata = analyze(url).await;
    assert_eq!(metadata.provider_id, "photos");
    assert_eq!(metadata.platform, PlatformId::Twitter);
    assert_eq!(metadata.media_kind, MediaKind::Image);

    let outcome = download(url, None, DownloadMode::Image).await;
    assert_eq!(image_size(&outcome.output_path), ("jpeg", 1920, 1080));
    cleanup(&outcome);
}

#[tokio::test]
#[ignore = "reads a live X post through the engine"]
async fn an_x_post_of_a_video_and_photos_keeps_all_three_in_order() {
    require_engine(&settings()).await;
    // A video, then two photos.
    let url = "https://x.com/liberdalau/status/1623739803874349067";

    let metadata = analyze(url).await;
    assert_eq!(metadata.media_kind, MediaKind::Gallery);
    let kinds: Vec<MediaKind> = metadata.entries.iter().map(|item| item.media_kind).collect();
    assert_eq!(kinds, [MediaKind::Video, MediaKind::Image, MediaKind::Image]);

    let outcome = download(url, Some(3), DownloadMode::Video).await;
    image_size(&outcome.output_path);
    cleanup(&outcome);
}

#[tokio::test]
#[ignore = "reads a live TikTok photo post"]
async fn a_tiktok_photo_post_downloads_its_pictures() {
    let url = "https://www.tiktok.com/@natgeo/photo/7680195684426927373";

    let metadata = analyze(url).await;
    assert_eq!(metadata.provider_id, "photos");
    assert_eq!(metadata.media_kind, MediaKind::Gallery);
    assert!(metadata.entries.len() >= 2);
    assert!(
        metadata.formats.iter().any(|format| format.kind == FormatKind::Audio),
        "the soundtrack should be offered with the post"
    );

    let outcome = download(url, Some(2), DownloadMode::Image).await;
    let (_, width, height) = image_size(&outcome.output_path);
    assert!(width >= 720 && height >= 720, "{width}x{height}");
    cleanup(&outcome);
}

#[tokio::test]
#[ignore = "reads a live TikTok photo post through the engine"]
async fn a_tiktok_photo_post_under_a_video_address_is_still_read_as_photos() {
    require_engine(&settings()).await;
    let url = "https://www.tiktok.com/@natgeo/video/7680195684426927373";

    let metadata = analyze(url).await;
    assert_eq!(metadata.media_kind, MediaKind::Gallery, "the engine alone reads only the soundtrack");
    assert!(metadata.entries.iter().all(|item| item.media_kind == MediaKind::Image));
}

#[tokio::test]
#[ignore = "reads live Reddit posts"]
async fn reddit_image_posts_and_galleries_download_their_originals() {
    let image = "https://www.reddit.com/r/pics/comments/1wbx1wo/there_is_a_crease/";
    let metadata = analyze(image).await;
    assert_eq!(metadata.provider_id, "photos");
    assert_eq!(metadata.media_kind, MediaKind::Image);
    let outcome = download(image, None, DownloadMode::Image).await;
    let (_, width, _) = image_size(&outcome.output_path);
    assert!(width >= 2000, "{width}");
    cleanup(&outcome);

    let gallery = "https://www.reddit.com/r/pics/comments/1wcv3i8/2977_drones_recreated_the_twin_towers_over_ny/";
    let metadata = analyze(gallery).await;
    assert_eq!(metadata.media_kind, MediaKind::Gallery);
    let outcome = download(gallery, Some(2), DownloadMode::Image).await;
    image_size(&outcome.output_path);
    cleanup(&outcome);
}

/// Photo support must not change how videos on the same platforms are read:
/// the photo readers look first, and have to step aside for these.
#[tokio::test]
#[ignore = "reads live video posts through the engine"]
async fn videos_on_photo_platforms_are_still_the_engines() {
    require_engine(&settings()).await;
    let videos = [
        "https://x.com/historyinmemes/status/1790637656616943991",
        "https://www.tiktok.com/@natgeo/video/7685375766586019086",
        "https://www.reddit.com/r/aww/comments/1wfk6od/took_in_a_small_pregnant_girl_she_gave_birth_to/",
    ];
    for url in videos {
        let metadata = analyze(url).await;
        assert_eq!(metadata.provider_id, "engine", "{url}");
        assert!(metadata.formats.iter().any(|format| format.has_video), "{url} lost its video");
    }
}

#[tokio::test]
#[ignore = "reads a live Pinterest pin through the engine"]
async fn a_pinterest_image_pin_downloads_its_original() {
    require_engine(&settings()).await;
    let url = "https://www.pinterest.com/pin/15692298698497364/";

    let metadata = analyze(url).await;
    assert_eq!(metadata.media_kind, MediaKind::Image);

    let outcome = download(url, None, DownloadMode::Image).await;
    let (_, width, height) = image_size(&outcome.output_path);
    assert!(width >= 1000, "{width}x{height}");
    cleanup(&outcome);
}
