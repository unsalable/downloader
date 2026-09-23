//! Discovery and installation of the external tools.
//!
//! None of them is bundled in the installer: yt-dlp changes weekly (a bundled
//! copy would be stale the month after release), FFmpeg is GPL, which is
//! cleanest to keep as a separate process the user opts into, and the
//! JavaScript runtime is 40 MB that plenty of installs never need. All are
//! fetched once, on request, into the app's own data directory -- and a copy
//! the user already has on PATH is preferred over downloading anything at all.
//!
//! Android differs in two respects: nothing an app downloads may be executed
//! there, so FFmpeg ships inside the APK and is always present, and the
//! JavaScript runtime is not a tool at all -- the APK carries QuickJS, which
//! `android.rs` hands the engine directly. yt-dlp is still fetched on request
//! -- it is a Python program, and the interpreter that runs it is the part the
//! APK carries.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::time::Duration;

use futures_util::StreamExt;
use once_cell::sync::Lazy;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::{AppError, AppResult};
use crate::model::{ToolKind, ToolSource, ToolStatus, ToolUpdateCheck, ToolsState, VideoCodec};
use crate::settings::Settings;
use crate::{log_info, log_warn, paths, process};

#[cfg(not(target_os = "android"))]
const ENGINE_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";

/// The platform-independent zipapp, run by the Python the APK bundles.
#[cfg(target_os = "android")]
const ENGINE_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp";

/// The shared build is 77 MB against 170 MB for the static one, and the extra
/// DLLs land in the same directory as the executable.
#[cfg(not(target_os = "android"))]
const FFMPEG_URL: &str =
    "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl-shared.zip";

/// The file `FFMPEG_URL` fetches, by the name its release lists it under.
#[cfg(not(target_os = "android"))]
const FFMPEG_ASSET: &str = "ffmpeg-master-latest-win64-gpl-shared.zip";

#[cfg(not(target_os = "android"))]
const ENGINE_EXE: &str = "yt-dlp.exe";
#[cfg(target_os = "android")]
const ENGINE_EXE: &str = "yt-dlp";
#[cfg(not(target_os = "android"))]
const FFMPEG_EXE: &str = "ffmpeg.exe";

/// Deno rather than Node or Bun: it is the one runtime yt-dlp looks for by
/// itself, so it is the best-supported path and keeps working even if this app
/// stops naming it. The Windows release is a single executable in a zip.
#[cfg(not(target_os = "android"))]
const JS_RUNTIME_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";

#[cfg(not(target_os = "android"))]
const JS_RUNTIME_EXE: &str = "deno.exe";

/// The managed runtime's own directory. Not `runtime`, which on Android is
/// where the libraries unpacked from the APK go.
#[cfg(not(target_os = "android"))]
const JS_RUNTIME_DIR: &str = "js";

static STATE: Lazy<RwLock<ToolsState>> = Lazy::new(|| RwLock::new(blank_state()));

fn blank_state() -> ToolsState {
    ToolsState {
        engine: ToolStatus::missing(ToolKind::Engine),
        ffmpeg: ToolStatus::missing(ToolKind::Ffmpeg),
        #[cfg(not(target_os = "android"))]
        js_runtime: ToolStatus::missing(ToolKind::JsRuntime),
    }
}

/// Serialises discovery. A pass runs a subprocess per tool and then publishes
/// what it saw, so two overlapping passes let the slower one publish a view from
/// before the faster one's install finished -- a tool the UI had just shown as
/// installed would then read as missing to everything else until a restart.
static DISCOVERY: Lazy<AsyncMutex<()>> = Lazy::new(|| AsyncMutex::new(()));

/// Set once the first pass has published, so the placeholder "missing" state
/// above is never handed out as if it were an answer.
static DISCOVERED: AtomicBool = AtomicBool::new(false);

/// A version check is normally well under a second, but the first run of a
/// freshly downloaded yt-dlp unpacks itself and is scanned on the way, which on
/// a slow machine can take tens of seconds. This only bounds a tool that hangs.
const VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(90);

/// Pauses between attempts to start a tool that has just been written to disk.
/// Real-time antivirus scanning commonly holds a new executable for a few
/// seconds, and giving up on the first refusal left a good install reading as
/// missing until the app was restarted and discovery ran again.
const VERIFY_BACKOFF_SECS: [u64; 6] = [1, 2, 3, 5, 8, 13];

/// Marks a previous copy that was renamed aside during an update.
const REPLACED_MARKER: &str = ".replaced-";

pub fn snapshot() -> ToolsState {
    STATE
        .read()
        .map(|state| state.clone())
        .unwrap_or_else(|_| blank_state())
}

pub fn engine_path() -> Option<PathBuf> {
    let state = STATE.read().ok()?;
    state
        .engine
        .available
        .then(|| state.engine.path.as_ref().map(PathBuf::from))
        .flatten()
}

pub fn ffmpeg_path() -> Option<PathBuf> {
    let state = STATE.read().ok()?;
    state
        .ffmpeg
        .available
        .then(|| state.ffmpeg.path.as_ref().map(PathBuf::from))
        .flatten()
}

/// The JavaScript runtime discovery settled on, if there is one.
///
/// There is deliberately no `require_js_runtime`: every download that worked
/// before this tool existed still works without it, so a missing runtime is
/// never a reason to refuse a link -- it only costs the formats that have to be
/// deciphered, which is the signed-in half of YouTube.
#[cfg(not(target_os = "android"))]
pub fn js_runtime_path() -> Option<PathBuf> {
    let state = STATE.read().ok()?;
    state
        .js_runtime
        .available
        .then(|| state.js_runtime.path.as_ref().map(PathBuf::from))
        .flatten()
}

pub fn require_engine() -> AppResult<PathBuf> {
    engine_path().ok_or(AppError::EngineMissing)
}

pub fn require_ffmpeg() -> AppResult<PathBuf> {
    ffmpeg_path().ok_or(AppError::FfmpegMissing)
}

/// What yt-dlp is given as `--ffmpeg-location`, so it uses the same FFmpeg the
/// app does rather than whatever is on PATH.
///
/// Normally that is the containing directory. On Android the binaries are
/// named `libffmpeg.so` and `libffprobe.so`, which yt-dlp would not find by
/// looking in a directory; handed the file itself, it takes the name as given
/// and derives the ffprobe beside it by substituting the word.
pub fn ffmpeg_location() -> Option<String> {
    let ffmpeg = ffmpeg_path()?;
    #[cfg(target_os = "android")]
    let location = Some(ffmpeg.as_path());
    #[cfg(not(target_os = "android"))]
    let location = ffmpeg.parent();
    location.map(|path| path.to_string_lossy().into_owned())
}

fn managed_engine() -> AppResult<PathBuf> {
    Ok(paths::tools_dir()?.join(ENGINE_EXE))
}

#[cfg(not(target_os = "android"))]
fn managed_ffmpeg() -> AppResult<PathBuf> {
    Ok(paths::tools_dir()?.join("ffmpeg").join(FFMPEG_EXE))
}

#[cfg(not(target_os = "android"))]
fn managed_js_runtime() -> AppResult<PathBuf> {
    Ok(paths::tools_dir()?.join(JS_RUNTIME_DIR).join(JS_RUNTIME_EXE))
}

/// Re-detect every tool and publish the result. Called after a custom path
/// setting changes and whenever the user asks for a fresh check.
pub async fn refresh(settings: &Settings) -> ToolsState {
    let _pass = DISCOVERY.lock().await;
    discover_all(settings).await
}

/// The discovered state, waiting for the first pass rather than returning the
/// placeholder. Startup discovery runs in the background, so the interface can
/// ask before it has finished; this is what keeps that race from showing an
/// installed tool as missing.
pub async fn discovered(settings: &Settings) -> ToolsState {
    if DISCOVERED.load(Ordering::SeqCst) {
        return snapshot();
    }
    let _pass = DISCOVERY.lock().await;
    if DISCOVERED.load(Ordering::SeqCst) {
        return snapshot();
    }
    discover_all(settings).await
}

/// The caller holds `DISCOVERY`.
async fn discover_all(settings: &Settings) -> ToolsState {
    // Neither tool can start until the libraries they link against have been
    // unpacked from the APK. That is a no-op on every launch but the first
    // after an install or update.
    #[cfg(target_os = "android")]
    prepare_android_runtime(false).await;

    let engine = detect_kind(ToolKind::Engine, settings).await;
    let ffmpeg = detect_kind(ToolKind::Ffmpeg, settings).await;
    #[cfg(not(target_os = "android"))]
    let js_runtime = detect_kind(ToolKind::JsRuntime, settings).await;

    // What the encoders and filters answered belongs to the FFmpeg that
    // answered it. A custom path, an update or a repair is a different binary
    // with a different set of answers, and this is every path that reaches one.
    CAPABILITIES.lock().await.clear();

    let state = publish(|state| {
        state.engine = engine;
        state.ffmpeg = ffmpeg;
        #[cfg(not(target_os = "android"))]
        {
            state.js_runtime = js_runtime;
        }
    });
    DISCOVERED.store(true, Ordering::SeqCst);
    state
}

// -- what this FFmpeg will actually do --------------------------------------

/// Encoders and filters that have been asked, keyed by name.
///
/// Cleared with every discovery pass, because the answers describe one
/// particular binary and nothing else.
static CAPABILITIES: Lazy<AsyncMutex<HashMap<String, bool>>> =
    Lazy::new(|| AsyncMutex::new(HashMap::new()));

/// A probe encodes fifteen frames of a test pattern and is normally over in
/// well under a second. This only bounds a driver that hangs instead of
/// refusing, which is a thing graphics drivers do.
const CAPABILITY_TIMEOUT: Duration = Duration::from_secs(20);

/// Ask once and remember, with the lock held across the probe so that two
/// callers asking about the same encoder do not both start one.
async fn remembered<F>(key: String, probe: F) -> bool
where
    F: std::future::Future<Output = bool>,
{
    let mut cache = CAPABILITIES.lock().await;
    if let Some(known) = cache.get(&key) {
        return *known;
    }
    let answer = probe.await;
    cache.insert(key, answer);
    answer
}

/// Whether an encoder will actually encode.
///
/// `-encoders` is not an availability test, only a list of what this build was
/// compiled with. Measured on this machine: `h264_amf` is advertised, and the
/// moment it is handed a frame it exits 171 with "DLL amfrt64.dll failed to
/// open", because the card it wants is not in the machine. The only honest
/// question is whether a frame comes out the other side, so that is the
/// question this asks.
pub async fn encoder_runs(name: &str) -> bool {
    let encoder = name.to_owned();
    remembered(format!("encoder:{name}"), async move {
        let Ok(binary) = require_ffmpeg() else {
            return false;
        };
        let mut args: Vec<String> = vec![
            "-nostdin".into(),
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "testsrc=size=320x240:rate=30:duration=0.5".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-c:v".into(),
            encoder.clone(),
        ];
        // Asked at the speed an export would ask for, so that the answer
        // covers the settings as well as the encoder. It also keeps the
        // question short: libaom left at its own default is several times
        // slower than an export runs it, and on a phone fifteen frames of
        // that can outlast the timeout below.
        args.extend(crate::ffmpeg::speed_args(&encoder));
        // Nothing is kept. The question is whether the encoder opens and takes
        // frames, and a file on disk would not answer it any better.
        args.extend(["-f".into(), "null".into(), "-".into()]);
        match process::run_with_timeout(&binary, &args, CAPABILITY_TIMEOUT).await {
            Ok(output) if output.success() => true,
            Ok(output) => {
                log_info!(
                    "tools",
                    "{encoder} is listed but will not run: {}",
                    output.stderr.lines().next().unwrap_or("no detail").trim()
                );
                false
            }
            Err(err) => {
                log_warn!("tools", "{encoder} could not be probed: {err}");
                false
            }
        }
    })
    .await
}

/// Whether a filter is compiled into this build.
///
/// A listed filter, unlike a listed encoder, is one that exists: filters have
/// no separate runtime to be missing. Listing is therefore the whole test.
pub async fn filter_available(name: &str) -> bool {
    let filter = name.to_owned();
    remembered(format!("filter:{name}"), async move {
        let Ok(binary) = require_ffmpeg() else {
            return false;
        };
        let args: Vec<String> = vec!["-hide_banner".into(), "-filters".into()];
        match process::run_with_timeout(&binary, &args, CAPABILITY_TIMEOUT).await {
            Ok(output) => output
                .stdout
                .lines()
                .any(|line| line.split_whitespace().nth(1) == Some(filter.as_str())),
            Err(err) => {
                log_warn!("tools", "the filter list could not be read: {err}");
                false
            }
        }
    })
    .await
}

/// The encoder to use for a codec: the first hardware one that answers, or the
/// processor.
///
/// Order is by how much work each takes off the processor, and every one of
/// them is tried for real before it is chosen. Falling back to the CPU encoder
/// is not a failure -- it is the encoder most machines were always going to
/// use, and it is the one that guarantees a result.
pub async fn preferred_encoder(codec: VideoCodec, hardware: bool) -> String {
    if hardware {
        for candidate in hardware_encoders(codec) {
            if encoder_runs(candidate).await {
                return (*candidate).to_owned();
            }
        }
    }
    processor_encoder(codec).await.to_owned()
}

/// The encoder that needs nothing but the processor, as this FFmpeg has it.
///
/// Only AV1 has a choice to make. SVT-AV1 is by far the fastest of the three
/// and is asked first. Both FFmpeg builds this app runs were configured with
/// it -- the phone's as well, whose runtime carries `libSvtAv1Enc.so` -- but a
/// listed encoder is only a claim (see [`encoder_runs`]), and the phone's one
/// build goes to every kind of processor, which is where that claim is most
/// likely to be wrong. libaom and rav1e are in both builds and are asked next.
/// All three are asked the same way a graphics card is, by running them. When
/// none of them answers the first is returned anyway, so that the failure the
/// user sees names the encoder they would have expected rather than the least
/// likely one.
///
/// A codec with one candidate is not asked at all: there is no other answer to
/// fall back to, and the probe would cost a process for nothing.
pub async fn processor_encoder(codec: VideoCodec) -> &'static str {
    let candidates = processor_encoders(codec);
    if let [only] = candidates {
        return only;
    }
    for candidate in candidates {
        if encoder_runs(candidate).await {
            return candidate;
        }
    }
    candidates[0]
}

fn hardware_encoders(codec: VideoCodec) -> &'static [&'static str] {
    match codec {
        VideoCodec::H264 => &["h264_nvenc", "h264_qsv", "h264_amf"],
        VideoCodec::H265 => &["hevc_nvenc", "hevc_qsv", "hevc_amf"],
        VideoCodec::Av1 => &["av1_nvenc", "av1_qsv"],
        // VP9 on a GPU exists on very few machines, and where it does it looks
        // worse at the same size than libvpx does. It is never offered.
        VideoCodec::Vp9 => &[],
    }
}

/// The encoders that need nothing but the processor, fastest first. Never
/// empty: this list is what guarantees an export a result.
pub(crate) fn processor_encoders(codec: VideoCodec) -> &'static [&'static str] {
    match codec {
        VideoCodec::H264 => &["libx264"],
        VideoCodec::H265 => &["libx265"],
        VideoCodec::Vp9 => &["libvpx-vp9"],
        VideoCodec::Av1 => &["libsvtav1", "libaom-av1", "librav1e"],
    }
}

fn publish(update: impl FnOnce(&mut ToolsState)) -> ToolsState {
    match STATE.write() {
        Ok(mut guard) => {
            update(&mut guard);
            guard.clone()
        }
        Err(_) => snapshot(),
    }
}

fn slot(state: &mut ToolsState, kind: ToolKind) -> &mut ToolStatus {
    match kind {
        ToolKind::Engine => &mut state.engine,
        ToolKind::Ffmpeg => &mut state.ffmpeg,
        #[cfg(not(target_os = "android"))]
        ToolKind::JsRuntime => &mut state.js_runtime,
    }
}

async fn detect_kind(kind: ToolKind, settings: &Settings) -> ToolStatus {
    match kind {
        ToolKind::Engine => {
            detect(kind, engine_candidates(settings), &["--version".to_string()]).await
        }
        ToolKind::Ffmpeg => {
            detect(kind, ffmpeg_candidates(settings), &["-version".to_string()]).await
        }
        #[cfg(not(target_os = "android"))]
        ToolKind::JsRuntime => {
            detect(kind, js_runtime_candidates(), &["--version".to_string()]).await
        }
    }
}

/// Order matters: an explicit choice wins, then the copy this app manages,
/// then whatever the system already provides.
#[cfg(not(target_os = "android"))]
fn engine_candidates(settings: &Settings) -> Vec<(PathBuf, ToolSource)> {
    ordered_candidates(settings.engine_path.as_deref(), managed_engine().ok(), "yt-dlp")
}

#[cfg(not(target_os = "android"))]
fn ffmpeg_candidates(settings: &Settings) -> Vec<(PathBuf, ToolSource)> {
    ordered_candidates(settings.ffmpeg_path.as_deref(), managed_ffmpeg().ok(), "ffmpeg")
}

/// The copy this app installed, then any runtime the machine already has.
///
/// There is no custom-path setting to honour here, so the order is only those
/// two. Node and Bun are looked for beside Deno because the engine is happy
/// with any of them once it is told where one is, and a machine that has one
/// should not be asked to fetch 40 MB to learn nothing new.
#[cfg(not(target_os = "android"))]
fn js_runtime_candidates() -> Vec<(PathBuf, ToolSource)> {
    managed_js_runtime()
        .ok()
        .map(|path| (path, ToolSource::Managed))
        .into_iter()
        .chain(
            ["deno", "node", "bun"]
                .iter()
                .filter_map(|name| Some((process::which(name)?, ToolSource::System))),
        )
        .collect()
}

#[cfg(not(target_os = "android"))]
fn ordered_candidates(
    custom: Option<&str>,
    managed: Option<PathBuf>,
    path_name: &str,
) -> Vec<(PathBuf, ToolSource)> {
    [
        custom.map(|value| (PathBuf::from(value), ToolSource::Custom)),
        managed.map(|value| (value, ToolSource::Managed)),
        process::which(path_name).map(|value| (value, ToolSource::System)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A custom path cannot be honoured on Android: a file the user points at
/// would not be allowed to run. Only the managed copy counts.
#[cfg(target_os = "android")]
fn engine_candidates(_settings: &Settings) -> Vec<(PathBuf, ToolSource)> {
    managed_engine()
        .ok()
        .map(|path| (path, ToolSource::Managed))
        .into_iter()
        .collect()
}

#[cfg(target_os = "android")]
fn ffmpeg_candidates(_settings: &Settings) -> Vec<(PathBuf, ToolSource)> {
    vec![(crate::android::ffmpeg_binary(), ToolSource::Bundled)]
}

#[cfg(target_os = "android")]
async fn prepare_android_runtime(force: bool) {
    match tokio::task::spawn_blocking(move || crate::android::prepare_runtime(force)).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => log_warn!("tools", "the bundled runtime could not be unpacked: {err}"),
        Err(err) => log_warn!("tools", "unpacking the bundled runtime panicked: {err}"),
    }
}

// -- remembering what a version check answered ------------------------------

/// What a version check saw, recorded against the file it saw it in.
///
/// Only the engine is remembered, and it is the reason this exists: yt-dlp on
/// Windows is a 17.8 MB PyInstaller bundle that unpacks itself to a temporary
/// directory every time it starts, so asking it its version costs the better
/// part of a second. That second used to be spent on every launch, with the
/// link field greyed out for the whole of it, to be told what the last launch
/// was already told. FFmpeg is not cached: it answers in tens of milliseconds,
/// and the shared build depends on DLLs beside it that an unchanged
/// `ffmpeg.exe` would not notice the loss of.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
struct ProbedVersion {
    size: u64,
    modified_ms: i64,
    version: String,
}

fn probe_cache_path() -> AppResult<PathBuf> {
    Ok(paths::cache_dir()?.join("engine-version.json"))
}

/// Size and modification time, which together are what make a file the same
/// file. A path whose metadata will not be read is simply never remembered.
fn fingerprint(path: &Path) -> Option<(u64, i64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((metadata.len(), modified))
}

static PROBE_CACHE: Lazy<RwLock<HashMap<String, ProbedVersion>>> =
    Lazy::new(|| RwLock::new(load_probe_cache()));

fn load_probe_cache() -> HashMap<String, ProbedVersion> {
    probe_cache_path()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The version remembered for `path`, if that path still holds the file it was
/// read from. An install, an update, or the user pointing the setting somewhere
/// else all move the size or the modification time, and so all re-run the check.
fn remembered_version(path: &Path) -> Option<String> {
    let (size, modified_ms) = fingerprint(path)?;
    let cache = PROBE_CACHE.read().ok()?;
    let entry = cache.get(path.to_string_lossy().as_ref())?;
    (entry.size == size && entry.modified_ms == modified_ms).then(|| entry.version.clone())
}

/// Remember what this file answered, on disk, so the next launch starts knowing
/// it. A cache that cannot be written costs the check again and nothing else.
fn remember_version(path: &Path, version: &str) {
    let Some((size, modified_ms)) = fingerprint(path) else {
        return;
    };
    let entry = ProbedVersion {
        size,
        modified_ms,
        version: version.to_owned(),
    };
    let key = path.to_string_lossy().into_owned();

    let snapshot = {
        let Ok(mut cache) = PROBE_CACHE.write() else {
            return;
        };
        if cache.get(&key) == Some(&entry) {
            return;
        }
        // One tool, one entry: a path that moved leaves nothing behind.
        cache.clear();
        cache.insert(key, entry);
        cache.clone()
    };

    if let (Ok(path), Ok(bytes)) = (probe_cache_path(), serde_json::to_vec(&snapshot)) {
        let _ = std::fs::write(path, bytes);
    }
}

async fn detect(
    kind: ToolKind,
    candidates: Vec<(PathBuf, ToolSource)>,
    version_args: &[String],
) -> ToolStatus {
    for (path, source) in candidates {
        if !path.is_file() {
            continue;
        }
        // A binary that has not changed answers what it answered last time, and
        // for the engine that answer is a second of the user's launch.
        if kind == ToolKind::Engine {
            if let Some(version) = remembered_version(&path) {
                return ToolStatus {
                    name: kind,
                    available: true,
                    path: Some(path.to_string_lossy().into_owned()),
                    version: Some(version),
                    source,
                };
            }
        }
        match process::run_with_timeout(&path, version_args, VERSION_CHECK_TIMEOUT).await {
            Ok(output) if output.success() => {
                let version = parse_version(kind, &output.stdout);
                if kind == ToolKind::Engine {
                    remember_version(&path, &version);
                }
                return ToolStatus {
                    name: kind,
                    available: true,
                    path: Some(path.to_string_lossy().into_owned()),
                    version: Some(version),
                    source,
                };
            }
            Ok(output) => {
                log_warn!(
                    "tools",
                    "{} at {} exited with {:?}: {}",
                    kind.as_str(),
                    path.display(),
                    output.status,
                    output.stderr.lines().next().unwrap_or("")
                );
            }
            Err(err) => {
                log_warn!("tools", "{} at {} is not runnable: {err}", kind.as_str(), path.display());
            }
        }
    }

    ToolStatus::missing(kind)
}

fn parse_version(kind: ToolKind, stdout: &str) -> String {
    let first = stdout.lines().next().unwrap_or("").trim();
    match kind {
        // yt-dlp prints just the version, e.g. "2026.08.24".
        ToolKind::Engine => first.to_string(),
        // ffmpeg prints "ffmpeg version n7.1-... Copyright ...".
        ToolKind::Ffmpeg => first
            .split_whitespace()
            .nth(2)
            .unwrap_or(first)
            .trim_start_matches('n')
            .to_string(),
        // Three runtimes, three shapes: "deno 2.9.7 (stable, ...)", Node's
        // bare "v22.11.0", Bun's bare "1.1.0". The first word that starts
        // with a digit is the version in all of them.
        #[cfg(not(target_os = "android"))]
        ToolKind::JsRuntime => first
            .split_whitespace()
            .map(|word| word.trim_start_matches('v'))
            .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
            .unwrap_or(first)
            .to_string(),
    }
}

// -- whether a newer release exists -----------------------------------------

/// Where the engine is released. One repository on every platform: the
/// Windows executable and the zipapp the phone runs are two files of the same
/// release.
const ENGINE_REPOSITORY: &str = "yt-dlp/yt-dlp";

#[cfg(not(target_os = "android"))]
const JS_RUNTIME_REPOSITORY: &str = "denoland/deno";

/// FFmpeg's builds are the files of a single release that is rebuilt in place,
/// so there is no version to compare -- only the release's own record of which
/// file is up now.
#[cfg(not(target_os = "android"))]
const FFMPEG_RELEASE_API: &str =
    "https://api.github.com/repos/yt-dlp/FFmpeg-Builds/releases/tags/latest";

/// Kept inside the managed FFmpeg's directory: which file of the release it
/// was unpacked from.
#[cfg(not(target_os = "android"))]
const FFMPEG_BUILD_MARKER: &str = "build.json";

/// Each request a check makes. Someone is watching a spinner for the length of
/// it, so a stalled network is given up on long before the transport would.
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether a newer release of `kind` than the copy in use has been published.
///
/// Nothing is downloaded. The copy compared is the one discovery settled on,
/// which is the one the row shows, so the answer is about what the user sees.
/// The engine and the JavaScript runtime are released by version and compared
/// exactly. FFmpeg is rebuilt every day, so for it "newer" means more than a
/// month newer; see [`FFMPEG_CURRENT_FOR_DAYS`].
pub async fn check_update(kind: ToolKind, settings: &Settings) -> AppResult<ToolUpdateCheck> {
    let mut state = discovered(settings).await;
    let status = slot(&mut state, kind).clone();
    let installed = status.available.then(|| status.version.clone()).flatten();

    let (latest, up_to_date) = match kind {
        ToolKind::Engine => {
            let client = crate::net::client(settings)?;
            let tag = latest_tag(&client, ENGINE_REPOSITORY).await?;
            let current = installed
                .as_deref()
                .is_some_and(|version| version_at_least(version, &tag));
            (Some(tag), current)
        }
        // Part of the APK, so exactly as new as the app. There is no other
        // copy to fetch, and its row has nothing to press.
        #[cfg(target_os = "android")]
        ToolKind::Ffmpeg => (installed.clone(), status.available),
        #[cfg(not(target_os = "android"))]
        ToolKind::Ffmpeg => {
            let client = crate::net::client(settings)?;
            let remote = ffmpeg_release(&client).await?;
            // The record describes the managed copy and nothing else.
            let marker = if status.source == ToolSource::Managed {
                managed_ffmpeg()
                    .ok()
                    .and_then(|exe| read_ffmpeg_marker(exe.parent()?))
            } else {
                None
            };
            let current = status.available
                && ffmpeg_is_current(marker.as_ref(), installed.as_deref(), &remote);
            let published = published_day(&remote.updated_at).map(|day| day.to_string());
            (published, current)
        }
        #[cfg(not(target_os = "android"))]
        ToolKind::JsRuntime => {
            let client = crate::net::client(settings)?;
            let tag = latest_tag(&client, JS_RUNTIME_REPOSITORY).await?;
            // A Node or Bun on PATH serves as well, but its numbers are not
            // Deno's, and comparing them would answer nonsense either way.
            let is_deno = status
                .path
                .as_deref()
                .and_then(|path| Path::new(path).file_stem())
                .is_some_and(|stem| stem.eq_ignore_ascii_case("deno"));
            let current = is_deno
                && installed
                    .as_deref()
                    .is_some_and(|version| version_at_least(version, &tag));
            (Some(tag.trim_start_matches('v').to_owned()), current)
        }
    };

    log_info!(
        "tools",
        "{} {:?} against the release's {:?}: {}",
        kind.as_str(),
        installed,
        latest,
        if up_to_date { "current" } else { "not current" }
    );
    Ok(ToolUpdateCheck {
        tool: kind,
        installed,
        latest,
        up_to_date,
    })
}

/// The tag of a repository's newest release.
///
/// GitHub answers `/releases/latest` with a redirect to `/releases/tag/<tag>`,
/// so the address the request ends at names it. The shared client follows
/// redirects, so it is read off the end of the chain rather than out of the
/// first `Location`; either way it costs nothing against the API's sixty
/// requests an hour, which a few presses on a shared connection would
/// otherwise spend. The API is asked only if the page stops leading there.
async fn latest_tag(client: &reqwest::Client, repository: &str) -> AppResult<String> {
    let page = format!("https://github.com/{repository}/releases/latest");
    // Where it leads is the whole answer, so no page is asked for.
    let response = client
        .head(&page)
        .timeout(UPDATE_CHECK_TIMEOUT)
        .send()
        .await?;
    if let Some(tag) = tag_from_release_url(response.url().as_str()) {
        return Ok(tag);
    }
    log_warn!(
        "tools",
        "{page} answered {} without leading to a release",
        response.status()
    );

    #[derive(serde::Deserialize)]
    struct LatestRelease {
        tag_name: String,
    }
    let api = format!("https://api.github.com/repos/{repository}/releases/latest");
    let release: LatestRelease = github_json(client, &api).await?;
    Ok(release.tag_name)
}

async fn github_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> AppResult<T> {
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(UPDATE_CHECK_TIMEOUT)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(AppError::from_status(response.status().as_u16(), url.to_owned()));
    }
    Ok(response.json::<T>().await?)
}

/// The tag in a release page's address: `.../releases/tag/v2.9.7` is `v2.9.7`.
fn tag_from_release_url(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("/releases/tag/")?;
    let tag = rest.split(['?', '#', '/']).next()?;
    (!tag.is_empty()).then(|| tag.to_owned())
}

/// A version as the numbers it is made of.
///
/// Compared as numbers, "2026.9.5" and "2026.09.05" are one release, "2.10.0"
/// comes after "2.9.7", and a fourth number -- a hotfix, or the time of day a
/// nightly was built -- comes after the release it follows. "2.9" and "2.9.0"
/// are the same. Anything after the numbers ("-rc.1", " (stable, ...)") is not
/// part of it, and `None` means there were no numbers to read.
fn version_numbers(version: &str) -> Option<Vec<u64>> {
    let text = version.trim().trim_start_matches(['v', 'V']);
    let run = text
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .next()?;
    let mut numbers = run
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    while numbers.last() == Some(&0) {
        numbers.pop();
    }
    (!numbers.is_empty()).then_some(numbers)
}

/// Whether `installed` is `latest` or comes after it. A version that cannot be
/// read is never claimed to be current.
fn version_at_least(installed: &str, latest: &str) -> bool {
    match (version_numbers(installed), version_numbers(latest)) {
        (Some(installed), Some(latest)) => installed >= latest,
        _ => false,
    }
}

/// One build of the FFmpeg release, as the release lists its file. This is
/// also what is kept beside a managed copy, to say which build it is.
#[cfg(not(target_os = "android"))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct FfmpegBuild {
    id: u64,
    size: u64,
    updated_at: String,
}

#[cfg(not(target_os = "android"))]
#[derive(serde::Deserialize)]
struct FfmpegRelease {
    #[serde(default)]
    assets: Vec<FfmpegReleaseAsset>,
}

#[cfg(not(target_os = "android"))]
#[derive(serde::Deserialize)]
struct FfmpegReleaseAsset {
    name: String,
    id: u64,
    size: u64,
    updated_at: String,
}

#[cfg(not(target_os = "android"))]
async fn ffmpeg_release(client: &reqwest::Client) -> AppResult<FfmpegBuild> {
    let release: FfmpegRelease = github_json(client, FFMPEG_RELEASE_API).await?;
    ffmpeg_build_in(release).ok_or_else(|| {
        AppError::Other(format!("the FFmpeg release no longer carries {FFMPEG_ASSET}"))
    })
}

#[cfg(not(target_os = "android"))]
fn ffmpeg_build_in(release: FfmpegRelease) -> Option<FfmpegBuild> {
    release
        .assets
        .into_iter()
        .find(|asset| asset.name == FFMPEG_ASSET)
        .map(|asset| FfmpegBuild {
            id: asset.id,
            size: asset.size,
            updated_at: asset.updated_at,
        })
}

#[cfg(not(target_os = "android"))]
fn read_ffmpeg_marker(dir: &Path) -> Option<FfmpegBuild> {
    let bytes = std::fs::read(dir.join(FFMPEG_BUILD_MARKER)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// How far behind the release's newest FFmpeg a copy may be and still be the
/// current one.
///
/// The release is FFmpeg's development branch, rebuilt about once a day, so
/// "not the very latest build" is true of every copy a day after it was
/// installed. Answering that with "a newer one is available" had the button
/// fetch 86 MB again for a few days of commits nobody downloading a video
/// would notice -- which from the outside looks like an app that cannot tell
/// it is up to date. A month behind is where what has changed starts to be
/// worth a download that size.
#[cfg(not(target_os = "android"))]
const FFMPEG_CURRENT_FOR_DAYS: i64 = 30;

/// Whether the FFmpeg in use counts as current: at most
/// [`FFMPEG_CURRENT_FOR_DAYS`] older than the release's newest build.
///
/// A record settles which build this is: it names the very file this copy was
/// unpacked from, and the day that file went up is the copy's age, whatever
/// its version says. Without one -- a copy installed before records were kept,
/// or one found on PATH or chosen by hand -- the day the build was made, which
/// this release's builds carry at the end of their version, stands in for it;
/// a build goes up some hours after it is made, which the month more than
/// covers. A version with no day in it is not claimed to be current, and a copy
/// newer than the release is never behind it.
#[cfg(not(target_os = "android"))]
fn ffmpeg_is_current(
    marker: Option<&FfmpegBuild>,
    version: Option<&str>,
    remote: &FfmpegBuild,
) -> bool {
    if marker.is_some_and(|marker| marker.id == remote.id && marker.updated_at == remote.updated_at)
    {
        return true;
    }
    let installed = match marker {
        Some(marker) => published_day(&marker.updated_at),
        None => version.and_then(ffmpeg_build_day),
    };
    match (installed, published_day(&remote.updated_at)) {
        (Some(installed), Some(latest)) => {
            latest.signed_duration_since(installed).num_days() <= FFMPEG_CURRENT_FOR_DAYS
        }
        _ => false,
    }
}

/// The day at the end of a build's version: "N-126767-g7499a8ba58-20260922".
#[cfg(not(target_os = "android"))]
fn ffmpeg_build_day(version: &str) -> Option<chrono::NaiveDate> {
    let last = version.trim().rsplit('-').next()?;
    if last.len() != 8 || !last.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    chrono::NaiveDate::parse_from_str(last, "%Y%m%d").ok()
}

/// The day, in UTC, of a GitHub timestamp such as "2026-09-22T18:21:33Z".
#[cfg(not(target_os = "android"))]
fn published_day(timestamp: &str) -> Option<chrono::NaiveDate> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|time| time.naive_utc().date())
}

/// Progress callback: `(received, total_or_none, stage)`.
pub type InstallProgress<'a> = &'a (dyn Fn(u64, Option<u64>, &str) + Send + Sync);

pub async fn install(
    kind: ToolKind,
    settings: &Settings,
    on_progress: InstallProgress<'_>,
) -> AppResult<ToolStatus> {
    sweep_replaced();

    match kind {
        ToolKind::Engine => install_engine(settings, on_progress).await?,
        ToolKind::Ffmpeg => install_ffmpeg(settings, on_progress).await?,
        #[cfg(not(target_os = "android"))]
        ToolKind::JsRuntime => install_js_runtime(settings, on_progress).await?,
    }

    on_progress(0, None, "verifying");
    let status = verify(kind, settings).await;

    if !status.available {
        return Err(AppError::Other(format!(
            "{} was downloaded but could not be started",
            kind.as_str()
        )));
    }

    on_progress(0, None, "done");
    log_info!("tools", "installed {} {:?}", kind.as_str(), status.version);
    Ok(status)
}

/// Detect one tool that has just been installed, allowing for the delay before
/// a new executable is allowed to run.
///
/// Only this tool's entry is published. Re-detecting the other one here would
/// race that tool's own install when both run at once, which is how a finished
/// install used to be overwritten with a stale "missing".
async fn verify(kind: ToolKind, settings: &Settings) -> ToolStatus {
    let mut delays = VERIFY_BACKOFF_SECS.iter();
    loop {
        let status = {
            let _pass = DISCOVERY.lock().await;
            detect_kind(kind, settings).await
        };

        if status.available {
            publish(|state| *slot(state, kind) = status.clone());
            return status;
        }

        let Some(secs) = delays.next() else {
            publish(|state| *slot(state, kind) = status.clone());
            return status;
        };
        log_warn!(
            "tools",
            "{} is not runnable yet, checking again in {secs}s",
            kind.as_str()
        );
        tokio::time::sleep(Duration::from_secs(*secs)).await;
    }
}

async fn install_engine(settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    let target = managed_engine()?;
    let temp = target.with_extension("download");

    download_to_file(ENGINE_URL, &temp, settings, on_progress).await?;
    replace_path(&temp, &target)
}

/// FFmpeg is part of the APK, so "installing" it can only mean unpacking its
/// libraries again -- which is also the repair for a damaged copy.
#[cfg(target_os = "android")]
async fn install_ffmpeg(_settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    on_progress(0, None, "extracting");
    tokio::task::spawn_blocking(|| crate::android::prepare_runtime(true))
        .await
        .map_err(|err| AppError::Other(format!("unpacking FFmpeg failed: {err}")))?
}

#[cfg(not(target_os = "android"))]
async fn install_ffmpeg(settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    let tools_dir = paths::tools_dir()?;
    let dir = tools_dir.join("ffmpeg");
    let staging = tools_dir.join("ffmpeg.staging");
    let archive = tools_dir.join("ffmpeg-download.zip");

    // Which build is about to be fetched, so a later check can tell whether the
    // release has moved on. Asked before the download rather than after: a
    // build published in between then leaves the record older than the file,
    // never newer, and the worst that does is offer one update too many. Not
    // being able to ask is no reason to refuse the install.
    let build = match crate::net::client(settings) {
        Ok(client) => match ffmpeg_release(&client).await {
            Ok(build) => Some(build),
            Err(err) => {
                log_warn!("tools", "the FFmpeg build could not be identified: {err}");
                None
            }
        },
        Err(_) => None,
    };

    download_to_file(FFMPEG_URL, &archive, settings, on_progress).await?;
    let downloaded = std::fs::metadata(&archive).map(|meta| meta.len()).ok();

    on_progress(0, None, "extracting");
    // Extracted beside the live copy and swapped in whole, so discovery never
    // sees an ffmpeg.exe whose DLLs have not landed yet, and a failed
    // extraction leaves a working install untouched.
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let extracted = extract_ffmpeg(&archive, &staging);
    let _ = std::fs::remove_file(&archive);
    if let Err(err) = extracted {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }

    // Into the staged copy, so the record arrives with the files it describes.
    // The size is compared first: a build published between asking and
    // fetching would otherwise be recorded against a file it is not.
    if let Some(build) = build.filter(|build| Some(build.size) == downloaded) {
        let written = serde_json::to_vec(&build)
            .map_err(AppError::from)
            .and_then(|bytes| Ok(std::fs::write(staging.join(FFMPEG_BUILD_MARKER), bytes)?));
        if let Err(err) = written {
            log_warn!("tools", "the FFmpeg build could not be recorded: {err}");
        }
    }

    replace_path(&staging, &dir)
}

/// The same shape as the FFmpeg install: fetch the zip, unpack it beside the
/// live copy, swap the directory in whole. One executable rather than a
/// directory's worth would fit in a plain file swap, but a release that one day
/// ships a DLL beside it would then land half-installed.
#[cfg(not(target_os = "android"))]
async fn install_js_runtime(settings: &Settings, on_progress: InstallProgress<'_>) -> AppResult<()> {
    let tools_dir = paths::tools_dir()?;
    let dir = tools_dir.join(JS_RUNTIME_DIR);
    let staging = tools_dir.join(format!("{JS_RUNTIME_DIR}.staging"));
    let archive = tools_dir.join("js-runtime-download.zip");

    download_to_file(JS_RUNTIME_URL, &archive, settings, on_progress).await?;

    on_progress(0, None, "extracting");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let extracted = extract_js_runtime(&archive, &staging);
    let _ = std::fs::remove_file(&archive);
    if let Err(err) = extracted {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }

    replace_path(&staging, &dir)
}

/// Move `fresh` into place at `target`, keeping the previous copy until the
/// move has succeeded.
///
/// The old copy is renamed aside rather than deleted: Windows lets a running
/// executable be renamed but not removed, so an update does not fail just
/// because a download happens to be using the tool.
fn replace_path(fresh: &Path, target: &Path) -> AppResult<()> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tool");
    let previous = target.with_file_name(format!(
        "{name}{REPLACED_MARKER}{}",
        chrono::Utc::now().timestamp_millis()
    ));

    let had_previous = target.exists();
    if had_previous {
        std::fs::rename(target, &previous).map_err(|err| {
            AppError::Other(format!(
                "{} is in use and could not be replaced: {err}",
                target.display()
            ))
        })?;
    }

    if let Err(err) = std::fs::rename(fresh, target) {
        if had_previous {
            let _ = std::fs::rename(&previous, target);
        }
        return Err(err.into());
    }

    if had_previous {
        remove_path(&previous);
    }
    Ok(())
}

/// Previous copies that were still running when they were replaced.
fn sweep_replaced() {
    let Ok(dir) = paths::tools_dir() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().contains(REPLACED_MARKER) {
            remove_path(&entry.path());
        }
    }
}

fn remove_path(path: &Path) {
    let _ = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
}

/// Pull just the executables and their DLLs out of the release archive.
/// `ffplay` is skipped: nothing in this app plays media back.
#[cfg(not(target_os = "android"))]
fn extract_ffmpeg(archive: &Path, dest: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|err| AppError::Other(format!("the FFmpeg archive could not be read: {err}")))?;

    let mut extracted = 0usize;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|err| AppError::Other(format!("the FFmpeg archive is damaged: {err}")))?;

        if entry.is_dir() {
            continue;
        }
        // `enclosed_name` rejects entries that would escape the destination
        // directory -- zip-slip protection, since this archive is remote input.
        let Some(entry_path) = entry.enclosed_name() else {
            continue;
        };
        let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let in_bin = entry_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("bin");

        if !in_bin || name.eq_ignore_ascii_case("ffplay.exe") {
            continue;
        }

        let out_path = dest.join(name);
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        out.flush()?;
        extracted += 1;
    }

    if extracted == 0 {
        return Err(AppError::Other(
            "the FFmpeg archive did not contain the expected files".into(),
        ));
    }
    Ok(())
}

/// Pull the runtime out of its release archive.
///
/// That zip holds exactly one file today, `deno.exe`. Everything at the root is
/// taken rather than that one name, so a release that grows a library beside
/// the executable still installs whole; a subdirectory is ignored, since
/// nothing the engine is handed would look for one. The install fails outright
/// if the executable itself was not among them -- an empty `js` directory would
/// otherwise read as a runtime that merely refuses to start.
#[cfg(not(target_os = "android"))]
fn extract_js_runtime(archive: &Path, dest: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|err| {
        AppError::Other(format!(
            "the JavaScript runtime archive could not be read: {err}"
        ))
    })?;

    let mut found_runtime = false;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|err| {
            AppError::Other(format!("the JavaScript runtime archive is damaged: {err}"))
        })?;

        if entry.is_dir() {
            continue;
        }
        // Remote input again, so the same zip-slip guard as above.
        let Some(entry_path) = entry.enclosed_name() else {
            continue;
        };
        if !at_archive_root(&entry_path) {
            continue;
        }
        let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let mut out = std::fs::File::create(dest.join(name))?;
        std::io::copy(&mut entry, &mut out)?;
        out.flush()?;
        found_runtime |= name.eq_ignore_ascii_case(JS_RUNTIME_EXE);
    }

    if !found_runtime {
        return Err(AppError::Other(
            "the JavaScript runtime archive did not contain the runtime".into(),
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn at_archive_root(path: &Path) -> bool {
    path.components().count() == 1
}

/// Stream `url` to `target`, removing the partial file if the transfer fails.
pub(crate) async fn download_to_file(
    url: &str,
    target: &Path,
    settings: &Settings,
    on_progress: InstallProgress<'_>,
) -> AppResult<()> {
    let result = stream_to_file(url, target, settings, on_progress).await;
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

async fn stream_to_file(
    url: &str,
    target: &Path,
    settings: &Settings,
    on_progress: InstallProgress<'_>,
) -> AppResult<()> {
    let client = crate::net::client(settings)?;
    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(AppError::from_status(
            response.status().as_u16(),
            format!("could not download from {url}"),
        ));
    }

    let total = response.content_length();
    let mut file = std::fs::File::create(target)?;
    let mut stream = response.bytes_stream();
    let mut received = 0u64;
    let mut last_report = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        received += chunk.len() as u64;

        // Report a few times a second, not once per chunk.
        if last_report.elapsed().as_millis() >= 120 {
            on_progress(received, total, "downloading");
            last_report = std::time::Instant::now();
        }
    }

    file.flush()?;
    on_progress(received, total, "downloading");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("ud-tools-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn replacing_a_file_keeps_only_the_new_copy() {
        let dir = scratch("file");
        let target = dir.join("tool.exe");
        let fresh = dir.join("tool.download");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&fresh, b"new").unwrap();

        replace_path(&fresh, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!fresh.exists());
        let leftovers = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(leftovers, 1, "the previous copy should have been removed");
    }

    #[test]
    fn replacing_a_directory_swaps_it_whole() {
        let dir = scratch("dir");
        let target = dir.join("ffmpeg");
        let staging = dir.join("ffmpeg.staging");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("stale.dll"), b"old").unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("ffmpeg.exe"), b"new").unwrap();

        replace_path(&staging, &target).unwrap();

        assert!(target.join("ffmpeg.exe").is_file());
        assert!(!target.join("stale.dll").exists());
        assert!(!staging.exists());
    }

    #[test]
    fn a_failed_swap_puts_the_previous_copy_back() {
        let dir = scratch("restore");
        let target = dir.join("tool.exe");
        std::fs::write(&target, b"old").unwrap();

        let missing = dir.join("never-downloaded");
        assert!(replace_path(&missing, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }

    /// The name the TypeScript side is written against. Renaming the variant
    /// without renaming it there would leave the runtime installing into a
    /// card that never updates.
    #[test]
    #[cfg(not(target_os = "android"))]
    fn the_js_runtime_is_called_what_the_interface_calls_it() {
        assert_eq!(ToolKind::JsRuntime.as_str(), "jsRuntime");
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn a_js_runtime_reports_a_version_whichever_runtime_it_is() {
        let version = |stdout: &str| parse_version(ToolKind::JsRuntime, stdout);

        assert_eq!(
            version("deno 2.9.7 (stable, release, x86_64-pc-windows-msvc)\nv8 14.2\n"),
            "2.9.7"
        );
        assert_eq!(version("v22.11.0\n"), "22.11.0");
        assert_eq!(version("1.1.0\n"), "1.1.0");

        // Nothing recognisable rather than a panic or an empty card.
        assert_eq!(version("some runtime\n"), "some runtime");
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn only_the_top_of_the_runtime_archive_is_unpacked() {
        assert!(at_archive_root(Path::new("deno.exe")));
        assert!(at_archive_root(Path::new("LICENSE.md")));
        assert!(!at_archive_root(Path::new("bin/deno.exe")));
        assert!(!at_archive_root(Path::new("deno/bin/deno.exe")));
    }

    const CODECS: [VideoCodec; 4] = [
        VideoCodec::H264,
        VideoCodec::H265,
        VideoCodec::Av1,
        VideoCodec::Vp9,
    ];

    /// The processor encoders each FFmpeg this app runs was built with. The
    /// desktop list is the part of the downloaded build this app relies on.
    /// The phone's build lists SVT-AV1 too, but it is left out here: this is
    /// the phone's build on a processor SVT-AV1 will not open on, which is the
    /// case the encoders after it in the AV1 list are there for.
    const DESKTOP_BUILD: [&str; 6] = [
        "libx264",
        "libx265",
        "libvpx-vp9",
        "libsvtav1",
        "libaom-av1",
        "librav1e",
    ];
    const ANDROID_BUILD: [&str; 5] = ["libx264", "libx265", "libvpx-vp9", "libaom-av1", "librav1e"];

    /// The order is by how much work is taken off the processor, and every list
    /// ends somewhere that needs no hardware at all -- on every build this app
    /// ships, not only on the one it was written on.
    #[test]
    fn every_codec_has_a_way_to_be_encoded_without_a_graphics_card() {
        for codec in CODECS {
            let processors = processor_encoders(codec);
            assert!(!processors.is_empty(), "{codec:?}");
            for candidate in hardware_encoders(codec) {
                assert!(!processors.contains(candidate), "{codec:?}: {candidate}");
            }
            for (build, carries) in [
                ("desktop", &DESKTOP_BUILD[..]),
                ("android", &ANDROID_BUILD[..]),
            ] {
                assert!(
                    processors.iter().any(|encoder| carries.contains(encoder)),
                    "{codec:?} has no processor encoder on the {build} build"
                );
            }
        }
        assert_eq!(
            hardware_encoders(VideoCodec::H264),
            ["h264_nvenc", "h264_qsv", "h264_amf"]
        );
        assert!(hardware_encoders(VideoCodec::Vp9).is_empty());
        assert_eq!(processor_encoders(VideoCodec::Vp9), ["libvpx-vp9"]);
    }

    /// SVT-AV1 stays first wherever it runs: it is several times faster than
    /// either of the encoders after it, which are there for where it does not.
    #[test]
    fn av1_is_asked_of_the_fastest_encoder_first() {
        assert_eq!(
            processor_encoders(VideoCodec::Av1),
            ["libsvtav1", "libaom-av1", "librav1e"]
        );
    }

    #[test]
    fn versions_are_compared_as_numbers() {
        assert!(version_at_least("2026.09.15", "2026.09.15"));
        // Leading zeros are not part of a number.
        assert!(version_at_least("2026.9.5", "2026.09.05"));
        assert!(version_at_least("2026.09.05", "2026.9.5"));
        // Nor is a string comparison: 10 comes after 9.
        assert!(version_at_least("2.10.0", "v2.9.7"));
        assert!(!version_at_least("2.9.7", "v2.10.0"));
        assert!(version_at_least("2.9.7", "v2.9.7"));
        assert!(version_at_least("2.9", "2.9.0"));

        assert!(!version_at_least("2026.08.19", "2026.09.15"));
        assert!(version_at_least("2026.10.01", "2026.09.15"));

        // A hotfix comes after the release it fixes, and a nightly -- which
        // carries the time it was built -- after the stable one it follows.
        assert!(version_at_least("2026.09.15.1", "2026.09.15"));
        assert!(!version_at_least("2026.09.15", "2026.09.15.1"));
        assert!(version_at_least("2026.09.16.232624", "2026.09.15"));

        // What follows the numbers is not read.
        assert!(version_at_least("2.9.7 (stable, release)", "v2.9.7"));
        assert!(version_at_least("2026.09.15.dev0", "2026.09.15"));
    }

    #[test]
    fn a_version_that_cannot_be_read_is_never_current() {
        assert!(!version_at_least("", "2026.09.15"));
        assert!(!version_at_least("some runtime", "2.9.7"));
        assert!(!version_at_least("2026.09.15", "nightly"));
        assert_eq!(version_numbers("99999999999999999999999.1"), None);
    }

    #[test]
    fn the_latest_tag_is_read_from_where_the_release_page_leads() {
        let tag = |url: &str| tag_from_release_url(url);

        assert_eq!(
            tag("https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19").as_deref(),
            Some("2026.08.19")
        );
        assert_eq!(
            tag("https://github.com/denoland/deno/releases/tag/v2.9.7").as_deref(),
            Some("v2.9.7")
        );
        assert_eq!(
            tag("https://github.com/denoland/deno/releases/tag/v2.9.7?from=latest#top").as_deref(),
            Some("v2.9.7")
        );
        // A page that did not redirect, or redirected somewhere else.
        assert_eq!(tag("https://github.com/yt-dlp/yt-dlp/releases/latest"), None);
        assert_eq!(tag("https://github.com/yt-dlp/yt-dlp/releases/tag/"), None);
        assert_eq!(tag("https://github.com/login?return_to=/releases"), None);
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn an_ffmpeg_build_is_dated_by_the_end_of_its_version() {
        let day = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d);

        assert_eq!(ffmpeg_build_day("N-126767-g7499a8ba58-20260922"), day(2026, 9, 22));
        // A release-branch build of the same project.
        assert_eq!(ffmpeg_build_day("7.1-153-gaeb8631048-20250918"), day(2025, 9, 18));

        assert_eq!(ffmpeg_build_day("7.1"), None);
        assert_eq!(
            ffmpeg_build_day("2025-09-18-git-7e1ab21e1c-full_build-www.gyan.dev"),
            None
        );
        assert_eq!(ffmpeg_build_day("N-126767-g7499a8ba58-20261340"), None);
        assert_eq!(ffmpeg_build_day("N-126767-g7499a8ba58-2026092"), None);

        assert_eq!(published_day("2026-09-22T18:21:33Z"), day(2026, 9, 22));
        assert_eq!(published_day("yesterday"), None);
    }

    #[cfg(not(target_os = "android"))]
    fn published_build() -> FfmpegBuild {
        FfmpegBuild {
            id: 581977991,
            size: 86326164,
            updated_at: "2026-09-22T18:21:33Z".into(),
        }
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn without_a_record_an_ffmpeg_is_current_within_a_month_of_the_build() {
        // The release went up on 22 September.
        let remote = published_build();
        let current = |version: &str| ffmpeg_is_current(None, Some(version), &remote);

        assert!(current("N-126767-g7499a8ba58-20260922"));
        // Built late the day before it went up.
        assert!(current("N-126767-g7499a8ba58-20260921"));
        // A few days of nightly builds behind is not worth 86 MB, which is
        // the complaint this rule answers: pressing the button two days after
        // installing fetched the whole thing again.
        assert!(current("N-126700-g1111111111-20260920"));
        assert!(current("N-126000-g3333333333-20260901"));
        // Thirty days behind is the last day of current; thirty-one is not.
        assert!(current("N-125900-g4444444444-20260823"));
        assert!(!current("N-125890-g5555555555-20260822"));
        assert!(!current("N-120000-g6666666666-20260301"));
        // Newer than the release is not behind it.
        assert!(current("N-126800-g2222222222-20260925"));

        // Nothing to date it by.
        assert!(!current("7.1"));
        assert!(!ffmpeg_is_current(None, None, &remote));
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn with_a_record_an_ffmpeg_is_as_old_as_the_file_it_came_from() {
        let remote = published_build();
        let installed_on = |updated_at: &str| FfmpegBuild {
            id: 581000001,
            size: 86300000,
            updated_at: updated_at.into(),
        };
        let yesterday = installed_on("2026-09-21T18:20:02Z");
        let last_month = installed_on("2026-08-23T18:20:02Z");
        let too_old = installed_on("2026-08-22T18:20:02Z");
        let reuploaded = FfmpegBuild {
            updated_at: "2026-09-22T20:00:00Z".into(),
            ..published_build()
        };

        // That very build is current, whatever the version says.
        assert!(ffmpeg_is_current(Some(&remote), Some("7.1"), &remote));
        // Yesterday's is current too: the release is rebuilt daily, and one
        // day of commits is not a reason to download it again.
        assert!(ffmpeg_is_current(Some(&yesterday), None, &remote));
        assert!(ffmpeg_is_current(Some(&reuploaded), None, &remote));
        assert!(ffmpeg_is_current(Some(&last_month), None, &remote));
        // The record decides, whatever the version says: a copy installed a
        // month and a day ago is behind, even with a fresh-looking version.
        assert!(!ffmpeg_is_current(
            Some(&too_old),
            Some("N-126767-g7499a8ba58-20260922"),
            &remote
        ));

        // A record that cannot be dated claims nothing.
        let undated = installed_on("sometime");
        assert!(!ffmpeg_is_current(Some(&undated), None, &remote));
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn the_release_record_names_the_file_that_is_downloaded() {
        assert!(FFMPEG_URL.ends_with(&format!("/{FFMPEG_ASSET}")));

        let release: FfmpegRelease = serde_json::from_str(
            r#"{
                "tag_name": "latest",
                "assets": [
                    {"id": 581977978, "name": "ffmpeg-master-latest-linux64-gpl.tar.xz",
                     "size": 151532188, "updated_at": "2026-09-22T18:21:34Z"},
                    {"id": 581977991, "name": "ffmpeg-master-latest-win64-gpl-shared.zip",
                     "size": 86326164, "updated_at": "2026-09-22T18:21:33Z",
                     "created_at": "2026-09-22T18:21:31Z", "download_count": 12}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(ffmpeg_build_in(release), Some(published_build()));

        let empty: FfmpegRelease = serde_json::from_str(r#"{"tag_name": "latest"}"#).unwrap();
        assert_eq!(ffmpeg_build_in(empty), None);
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn the_build_record_reads_back_what_was_written() {
        let dir = scratch("marker");
        assert_eq!(read_ffmpeg_marker(&dir), None);

        std::fs::write(
            dir.join(FFMPEG_BUILD_MARKER),
            serde_json::to_vec(&published_build()).unwrap(),
        )
        .unwrap();
        assert_eq!(read_ffmpeg_marker(&dir), Some(published_build()));

        std::fs::write(dir.join(FFMPEG_BUILD_MARKER), b"{ damaged").unwrap();
        assert_eq!(read_ffmpeg_marker(&dir), None);
    }

    #[test]
    fn first_install_needs_no_previous_copy() {
        let dir = scratch("first");
        let target = dir.join("tool.exe");
        let fresh = dir.join("tool.download");
        std::fs::write(&fresh, b"new").unwrap();

        replace_path(&fresh, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }
}
