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
