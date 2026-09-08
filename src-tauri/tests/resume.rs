//! Chunking and resume semantics, tested against a purpose-built local server.
//!
//! Neither can be verified reliably against a public host: CDNs differ in
//! whether they honour `Range`, so an online test silently exercises whichever
//! branch that host happens to take and proves nothing. This server implements
//! ranged requests properly (and, on request, ignores them entirely), so both
//! paths are covered deterministically and without network access.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use universal_downloader_lib::downloader::control::TaskControl;
use universal_downloader_lib::downloader::http;
use universal_downloader_lib::net;
use universal_downloader_lib::settings::Settings;

/// Three low-resource chunks' worth, so a clean run has to issue more than one
/// ranged request.
const BODY_LEN: usize = 6 * 1024 * 1024;
const LOW_RESOURCE_CHUNK: usize = 2 * 1024 * 1024;
const WRITE_CHUNK: usize = 128 * 1024;
/// Paces the response so a transfer lasts long enough to be interrupted.
const WRITE_DELAY: Duration = Duration::from_millis(18);

fn body() -> Vec<u8> {
    // A position-dependent pattern, so a file whose pieces were stitched
    // together at the wrong offsets fails the comparison.
    (0..BODY_LEN).map(|index| (index % 251) as u8).collect()
}

struct TestServer {
    port: u16,
    requests: Arc<AtomicUsize>,
}

impl TestServer {
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/media.bin", self.port)
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

fn start_server(honour_ranges: bool) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&requests);

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            counter.fetch_add(1, Ordering::SeqCst);
            // One connection at a time is plenty: the client is sequential.
            let _ = serve(stream, honour_ranges);
        }
    });

    TestServer { port, requests }
}

fn serve(mut stream: TcpStream, honour_ranges: bool) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut range: Option<(usize, Option<usize>)> = None;
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range:") {
            if let Some(spec) = value.trim().strip_prefix("bytes=") {
                let mut parts = spec.trim().split('-');
                let start = parts.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                let end = parts.next().and_then(|v| v.trim().parse::<usize>().ok());
                range = Some((start, end));
            }
        }
    }

    let payload = body();
    let serving_range = honour_ranges && range.is_some();

    let slice: &[u8] = match (serving_range, range) {
        (true, Some((start, end))) => {
            if start >= payload.len() {
                stream.write_all(
                    b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )?;
                return Ok(());
            }
            // An end past the resource is clamped, exactly as RFC 9110 requires.
            let stop = end.map_or(payload.len(), |e| (e + 1).min(payload.len()));
            &payload[start..stop]
        }
        _ => &payload,
    };

    let header = match (serving_range, range) {
        (true, Some((start, _))) => format!(
            "HTTP/1.1 206 Partial Content\r\n\
             Content-Type: application/octet-stream\r\n\
             Accept-Ranges: bytes\r\n\
             Content-Range: bytes {}-{}/{}\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n",
            start,
            start + slice.len() - 1,
            payload.len(),
            slice.len()
        ),
        _ => format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/octet-stream\r\n\
             {}\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n",
            if honour_ranges { "Accept-Ranges: bytes\r\n" } else { "" },
            slice.len()
        ),
    };

    stream.write_all(header.as_bytes())?;

    for chunk in slice.chunks(WRITE_CHUNK) {
        if stream.write_all(chunk).is_err() {
            // The client hung up mid-transfer, which is what an interruption
            // looks like from this side.
            return Ok(());
        }
        stream.flush()?;
        thread::sleep(WRITE_DELAY);
    }
    Ok(())
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = universal_downloader_lib::paths::temp_dir().expect("temp dir");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(http::part_path(&path));
    path
}

async fn fetch<F>(
    url: &str,
    target: &std::path::Path,
    control: Arc<TaskControl>,
    mut sink: F,
) -> Result<u64, universal_downloader_lib::error::AppError>
where
    F: FnMut(http::ProgressSample) + Send,
{
    let client = net::client(&Settings::default()).unwrap();
    http::fetch(
        &client,
        http::FetchOptions {
            url,
            headers: &[],
            target,
            // Smaller chunks keep the fixture small while still exercising the
            // multi-request path.
            low_resource: true,
        },
        control,
        &mut sink,
    )
    .await
}

/// Cancels after a fixed delay. More reliable than cancelling from the progress
/// callback: the transfer loop checks the flag on every chunk, whereas progress
/// is only emitted a couple of times a second.
fn cancel_after(control: &Arc<TaskControl>, delay: Duration) {
    let control = Arc::clone(control);
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        control.cancel();
    });
}

#[tokio::test]
async fn a_completed_download_is_byte_exact_and_leaves_no_part_file() {
    let server = start_server(true);
    let target = scratch("resume-plain.bin");

    let written = fetch(&server.url(), &target, TaskControl::shared(), |_| {})
        .await
        .expect("download should succeed");

    assert_eq!(written as usize, BODY_LEN);
    assert_eq!(std::fs::read(&target).unwrap(), body());
    assert!(!http::part_path(&target).exists());
    std::fs::remove_file(&target).unwrap();
}

#[tokio::test]
async fn a_large_transfer_is_split_into_bounded_ranged_requests() {
    let server = start_server(true);
    let target = scratch("resume-chunked.bin");

    fetch(&server.url(), &target, TaskControl::shared(), |_| {})
        .await
        .expect("download should succeed");

    // Requesting one long open-ended stream is what several media hosts
    // throttle; the fetcher must ask for bounded ranges instead.
    let expected_requests = BODY_LEN.div_ceil(LOW_RESOURCE_CHUNK);
    assert_eq!(
        server.request_count(),
        expected_requests,
        "expected {expected_requests} chunk requests for a {BODY_LEN}-byte body"
    );
    assert_eq!(std::fs::read(&target).unwrap(), body());
    std::fs::remove_file(&target).unwrap();
}

#[tokio::test]
async fn an_interrupted_download_keeps_its_partial_file_and_does_not_finalize() {
    let server = start_server(true);
    let target = scratch("resume-interrupt.bin");

    let control = TaskControl::shared();
    cancel_after(&control, Duration::from_millis(250));
    let result = fetch(&server.url(), &target, Arc::clone(&control), |_| {}).await;

    assert!(result.is_err(), "an interrupted fetch must not report success");
    assert!(!target.exists(), "the final file must not appear");

    let partial = std::fs::metadata(http::part_path(&target)).unwrap().len() as usize;
    assert!(partial > 0 && partial < BODY_LEN, "unexpected partial size {partial}");

    // Whatever landed on disk must be a correct prefix of the real body.
    let bytes = std::fs::read(http::part_path(&target)).unwrap();
    assert_eq!(bytes, body()[..partial], "the partial file is corrupt");

    let _ = std::fs::remove_file(http::part_path(&target));
}

#[tokio::test]
async fn resuming_continues_from_the_partial_file_when_the_server_supports_ranges() {
    let server = start_server(true);
    let target = scratch("resume-continue.bin");

    let control = TaskControl::shared();
    cancel_after(&control, Duration::from_millis(250));
    let _ = fetch(&server.url(), &target, Arc::clone(&control), |_| {}).await;

    let partial = std::fs::metadata(http::part_path(&target)).unwrap().len();
    assert!(partial > 0 && (partial as usize) < BODY_LEN, "partial was {partial}");

    let mut first_received = None;
    let written = fetch(&server.url(), &target, TaskControl::shared(), |sample| {
        first_received.get_or_insert(sample.received);
    })
    .await
    .expect("the resumed download should succeed");

    assert_eq!(written as usize, BODY_LEN);
    assert_eq!(
        first_received,
        Some(partial),
        "a resumed transfer must count the bytes already on disk"
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        body(),
        "the stitched file does not match the original"
    );

    std::fs::remove_file(&target).unwrap();
}

#[tokio::test]
async fn a_server_that_ignores_ranges_restarts_cleanly_instead_of_corrupting() {
    let server = start_server(false);
    let target = scratch("resume-norange.bin");

    let control = TaskControl::shared();
    cancel_after(&control, Duration::from_millis(250));
    let _ = fetch(&server.url(), &target, Arc::clone(&control), |_| {}).await;

    let partial = std::fs::metadata(http::part_path(&target)).unwrap().len();
    assert!(partial > 0);

    let mut resumable_flags = Vec::new();
    let written = fetch(&server.url(), &target, TaskControl::shared(), |sample| {
        resumable_flags.push(sample.resumable);
    })
    .await
    .expect("the restarted download should still succeed");

    assert_eq!(written as usize, BODY_LEN);
    assert!(
        resumable_flags.iter().all(|flag| !flag),
        "a server without range support must not be reported as resumable"
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        body(),
        "restarting must produce the correct file, not a concatenation"
    );

    std::fs::remove_file(&target).unwrap();
}

#[tokio::test]
async fn a_range_past_the_end_is_treated_as_a_finished_file() {
    let server = start_server(true);
    let target = scratch("resume-complete.bin");

    // Simulate a `.part` that already holds the whole resource, which is what a
    // download interrupted at the very last moment leaves behind.
    std::fs::write(http::part_path(&target), body()).unwrap();

    let written = fetch(&server.url(), &target, TaskControl::shared(), |_| {})
        .await
        .expect("an already-complete part file should finalize, not error");

    assert_eq!(written as usize, BODY_LEN);
    assert_eq!(std::fs::read(&target).unwrap(), body());
    assert_eq!(server.request_count(), 1, "only the confirming request was needed");
    std::fs::remove_file(&target).unwrap();
}
