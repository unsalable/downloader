//! A stream handed over by the browser extension, downloaded end to end.
//!
//! Hermetic on the network side -- a local server plays the part of a video
//! host -- but it runs the real engine and FFmpeg, so it is `#[ignore]`d like
//! the live tests and needs both installed (the app installs them on first
//! run, or `cargo test --test pipeline -- --ignored` does):
//!
//! ```text
//! cargo test --test handoff -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! The server answers only a request that names the player page in its
//! Referer, on the playlist and on every segment alike, which is what the
//! hosts behind most embedded players do and what made the same address
//! download from the browser and fail from the app.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use universal_downloader_lib::downloader::control::TaskControl;
use universal_downloader_lib::downloader;
use universal_downloader_lib::model::{
    DownloadMode, DownloadProgress, DownloadRequest, QualityPreference, SourceContext,
    WatermarkPreference,
};
use universal_downloader_lib::settings::Settings;
use universal_downloader_lib::tools;

const PLAYER: &str = "https://player.example/";
const TITLE: &str = "Uzun Yol — 5. Bölüm";

/// Six seconds of picture and tone cut into a bare media playlist: no master,
/// so no resolution and no codecs -- the shape yt-dlp reports for most
/// players' own playlists.
fn make_stream(dir: &Path) {
    let ffmpeg = tools::ffmpeg_path().expect("FFmpeg should be installed");
    let status = std::process::Command::new(ffmpeg)
        .current_dir(dir)
        .args([
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc=duration=6:size=640x360:rate=25",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=6",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest",
            "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", "seg%d.ts", "index.m3u8",
        ])
        .status()
        .expect("FFmpeg should run");
    assert!(status.success(), "FFmpeg could not cut the test stream");
}

/// Serve `dir` on a free port, refusing whatever does not name the player.
/// Returns the base address and the number of refusals so far.
fn serve(dir: PathBuf) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let refused = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&refused);

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let dir = dir.clone();
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let mut referer = None;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.eq_ignore_ascii_case("referer") {
                            referer = Some(value.trim().to_string());
                        }
                    }
                }

                let mut out = stream;
                let file = dir.join(path.trim_start_matches('/').split('?').next().unwrap_or(""));
                if referer.as_deref() != Some(PLAYER) {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let _ = out.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    return;
                }
                let Ok(body) = std::fs::read(&file) else {
                    let _ = out.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    return;
                };
                let kind = if path.contains(".m3u8") { "application/vnd.apple.mpegurl" } else { "video/mp2t" };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = out.write_all(head.as_bytes());
                let _ = out.write_all(&body);
            });
        }
    });

    (base, refused)
}

fn request(url: String, output_dir: &Path, source: Option<SourceContext>) -> DownloadRequest {
    DownloadRequest {
        url,
        mode: DownloadMode::Video,
        quality: QualityPreference::Best,
        video_format_id: None,
        audio_format_id: None,
        container: Some("mp4".into()),
        watermark: WatermarkPreference::Any,
        output_dir: Some(output_dir.to_string_lossy().into_owned()),
        title: Some(TITLE.into()),
        thumbnail_url: None,
        platform: None,
        entry: None,
        audio_language: None,
        source,
    }
}

async fn ready() -> Settings {
    let settings = Settings {
        network_timeout_sec: 20,
        ..Settings::default()
    };
    let state = tools::refresh(&settings).await;
    assert!(state.engine.available, "the engine should be installed");
    settings
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ud-handoff-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
#[ignore = "runs the engine and FFmpeg"]
async fn a_handed_over_stream_downloads_with_the_pages_headers() {
    let settings = ready().await;
    let stream_dir = scratch("stream");
    let out_dir = scratch("out");
    make_stream(&stream_dir);
    let (base, refused) = serve(stream_dir.clone());

    let source = SourceContext {
        page_url: Some("https://tv.example/watch/5".into()),
        referer: Some(PLAYER.into()),
        origin: Some("https://player.example".into()),
        user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/154.0 Safari/537.36".into()),
    };
    let request = request(format!("{base}/index.m3u8"), &out_dir, Some(source));

    let mut sink = |progress: DownloadProgress| eprintln!("  {:?}", progress.stage);
    let outcome = downloader::execute("handoff-test", &request, &settings, Arc::new(TaskControl::new()), &mut sink)
        .await
        .expect("the stream should download with the page's headers");

    eprintln!("{} ({} bytes, {})", outcome.output_path.display(), outcome.file_size, outcome.quality_label);
    assert!(outcome.output_path.is_file());
    assert!(outcome.file_size > 20_000, "suspiciously small: {}", outcome.file_size);
    let name = outcome.output_path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("Uzun Yol"), "named after the playlist, not the tab: {name}");
    assert_eq!(refused.load(Ordering::SeqCst), 0, "some request went out without the Referer");

    let _ = std::fs::remove_dir_all(&stream_dir);
    let _ = std::fs::remove_dir_all(&out_dir);
}

#[tokio::test]
#[ignore = "runs the engine and FFmpeg"]
async fn the_same_stream_without_the_pages_headers_is_refused() {
    let settings = ready().await;
    let stream_dir = scratch("stream-bare");
    let out_dir = scratch("out-bare");
    make_stream(&stream_dir);
    let (base, refused) = serve(stream_dir.clone());

    let request = request(format!("{base}/index.m3u8"), &out_dir, None);
    let mut sink = |_: DownloadProgress| {};
    let result = downloader::execute("handoff-bare", &request, &settings, Arc::new(TaskControl::new()), &mut sink).await;

    if let Err(err) = &result {
        eprintln!("refused as expected: {err}");
    }
    assert!(result.is_err(), "the host should have refused a request with no Referer");
    assert!(refused.load(Ordering::SeqCst) > 0);

    let _ = std::fs::remove_dir_all(&stream_dir);
    let _ = std::fs::remove_dir_all(&out_dir);
}
