//! The engine-backed provider.
//!
//! Metadata and stream URLs come from yt-dlp, run as a separate process with an
//! argument vector -- the URL is never interpolated into a command string.
//! Everything the process prints is parsed here into the app's own model, so
//! nothing yt-dlp-shaped leaks into the UI.
//!
//! Stream URLs are deliberately *not* cached and never sent to the webview:
//! they are signed and short-lived, so they are re-resolved immediately before
//! a download starts.
//!
//! The engine is a video tool. A photo post is, to it, a post with no video --
//! an error by default. It is asked to report such posts instead, and on the
//! platforms that publish photos the pictures it lists as thumbnails are then
//! offered as what they are.

use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::model::{
    FormatKind, MediaFormat, MediaKind, MediaMetadata, PlatformId, WatermarkSupport,
};
use crate::providers::{self, detect};
use crate::settings::Settings;
use crate::{bridge, log_debug, logging, net, paths, process, tools};

pub const PROVIDER_ID: &str = "engine";

pub struct EngineProvider;

impl EngineProvider {
    pub fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    /// The engine supports well over a thousand sites, so it claims any http
    /// URL. `registry` still tries the direct-file provider first.
    pub fn can_handle(&self, url: &str) -> bool {
        detect::classify(url).is_some()
    }

    pub async fn analyze(&self, url: &str, settings: &Settings) -> AppResult<MediaMetadata> {
        let engine = tools::require_engine()?;
        let mut args = base_args(settings);
        // A post without a stream is reported rather than refused, so its
        // photos can be offered. Why it has no stream -- a premiere that has
        // not started, DRM -- then arrives as a warning, and warnings are
        // what explain a link that turns out to have nothing to download.
        args.retain(|arg| arg != "--no-warnings");
        args.push("--ignore-no-formats-error".into());
        args.push("--no-playlist".into());
        args.push("-J".into());

        // The first look at any link is taken with no session behind it, which
        // is why the overwhelming majority of downloads -- public video -- never
        // cause the stored cookies to be written to disk at all.
        let output = run_engine(&engine, &args, url, None).await?;
        let mut result = interpret(&output, url);

        if let Err(refusal) = &result {
            if let Some(session) = session_for(refusal, url, settings) {
                let jar = session.path().to_string_lossy().into_owned();
                let retried = run_engine(&engine, &args, url, Some(jar.as_str())).await?;
                result = interpret(&retried, url);

                // Only after the run that worked: folding back a jar the engine
                // rewrote while still being refused would store a signed-out
                // session over a good one.
                if result.is_ok() {
                    session.fold_back();
                }
            }
        }

        result
    }
}

/// What a finished engine run means: the media it described, or the reason
/// there is none.
///
/// The reason has to be worked out here rather than from the exit status,
/// because a refusal reaches this code in two shapes. yt-dlp exits non-zero
/// when it refuses outright, but `--ignore-no-formats-error` -- which is what
/// lets a photo post be reported instead of refused -- downgrades the
/// members-only wall to a warning on a run that exits 0 and prints JSON with an
/// empty format list. That second shape is the one the browser link exists for,
/// so deciding on the exit code alone would leave the session unreachable for
/// exactly the videos it was stored to reach.
fn interpret(output: &process::CapturedOutput, url: &str) -> AppResult<MediaMetadata> {
    if !output.success() {
        return Err(classify_engine_error(&output.stderr));
    }

    let root: Value = serde_json::from_str(output.stdout.trim())
        .map_err(|err| AppError::Parse(format!("the engine returned unreadable JSON: {err}")))?;

    parse_result(&root, url).map_err(|err| explain_missing_streams(err, &output.stderr))
}

/// One engine run: the shared arguments, a cookie jar when the caller has one
/// to lend, and the URL last.
async fn run_engine(
    engine: &Path,
    args: &[String],
    url: &str,
    cookies: Option<&str>,
) -> AppResult<process::CapturedOutput> {
    let mut args = args.to_vec();
    if let Some(jar) = cookies {
        args.push("--cookies".to_string());
        args.push(jar.to_string());
    }
    args.push(url.to_string());
    process::run(engine, &args).await
}

/// The browser session to repeat a failed run with, if repeating it is worth
/// anything.
///
/// Three conditions, all of them cheap and all of them necessary. The failure
/// has to be a wall a signed-in viewer could be past -- a membership, or the
/// broader refusal the engine gives when it cannot tell who is asking. The
/// host has to be one the link was built for, so a session is never sent
/// anywhere it does not belong. And a fresh session has to actually exist,
/// which it does not for the user who never connected a browser.
///
/// The caller runs this once and only once. A second refusal with the session
/// attached is a wall the browser cannot pass either, and grinding at it would
/// spend a real Google login on a video that is not going to be served.
/// Whether a refusal is the kind a stored browser session could answer.
///
/// Kept apart from `session_for` so the decision can be tested on its own: the
/// function around it ends in a lease, and a test run has no session to lease.
fn worth_a_session(err: &AppError) -> bool {
    match err {
        AppError::MembershipRequired { .. } | AppError::Forbidden { .. } => true,
        // A video whose real formats the engine could not reach is reported as
        // having only images, because the storyboards are all that survive the
        // attempt. On YouTube that is another face of the same wall; anywhere
        // else it is a genuine picture post, which is why this is only ever
        // consulted for the hosts `wants_cookies` allows.
        AppError::Engine(detail) => detail
            .to_ascii_lowercase()
            .contains("only images are available"),
        _ => false,
    }
}

pub fn session_for(err: &AppError, url: &str, settings: &Settings) -> Option<bridge::CookieLease> {
    if !worth_a_session(err) || !bridge::wants_cookies(url) {
        return None;
    }
    bridge::lease(settings)
}

/// Arguments shared by every engine invocation.
pub fn base_args(settings: &Settings) -> Vec<String> {
    let mut args = vec![
        // Ignore any yt-dlp config the machine happens to have, so behaviour
        // does not silently differ between installs.
        "--ignore-config".to_string(),
        "--no-warnings".to_string(),
        "--no-progress".to_string(),
        "--no-colors".to_string(),
        "--retries".to_string(),
        "3".to_string(),
        "--extractor-retries".to_string(),
        "2".to_string(),
        "--socket-timeout".to_string(),
        settings.network_timeout_sec.to_string(),
    ];

    if let Ok(cache) = paths::cache_dir() {
        args.push("--cache-dir".to_string());
        args.push(cache.join("engine").to_string_lossy().into_owned());
    }

    if let Some(proxy) = settings.proxy_url.as_deref() {
        args.push("--proxy".to_string());
        args.push(proxy.to_string());
    }

    if let Some(agent) = settings.custom_user_agent.as_deref() {
        args.push("--add-header".to_string());
        args.push(format!("User-Agent:{agent}"));
    }

    // YouTube's player has to be run through JavaScript before most formats
    // are offered. yt-dlp only looks for Deno by default, which a phone does
    // not have; the APK carries QuickJS for this.
    #[cfg(target_os = "android")]
    {
        args.push("--js-runtimes".to_string());
        args.push(format!(
            "quickjs:{}",
            crate::android::quickjs_binary().display()
        ));
    }

    #[cfg(not(target_os = "android"))]
    if let Some(runtime) = js_runtime() {
        args.push("--js-runtimes".to_string());
        args.push(runtime);
    }

    args
}

/// A JavaScript engine for the desktop, if the machine has one.
///
/// This matters far more for a signed-in request than for an anonymous one.
/// Without cookies YouTube is read through a player client whose formats need
/// no deciphering; an authenticated request is answered with formats that do,
/// and with no runtime to decipher them yt-dlp discards every one and leaves
/// only the storyboard images behind. What reaches the user then is "only
/// images are available" -- on precisely the members-only video the browser
/// link was connected for.
///
/// yt-dlp finds Deno by itself and nothing else, so anything else has to be
/// named.
///
/// What discovery settled on wins, and discovery prefers the copy this app
/// installed over anything on PATH. A machine that already had Deno or Node
/// downloads nothing -- its own copy is found, reported as present, and no
/// install is ever offered -- while a machine that had none uses the one the
/// user installed here. The PATH search below only answers before the first
/// discovery pass has published, the one moment the managed copy is invisible.
#[cfg(not(target_os = "android"))]
fn js_runtime() -> Option<String> {
    tools::js_runtime_path()
        .as_deref()
        .and_then(runtime_spec)
        .or_else(path_runtime)
}

/// Which runtime a path holds, in the `kind:path` pair yt-dlp is given. The
/// program names itself: `deno.exe` is a Deno.
#[cfg(not(target_os = "android"))]
fn runtime_spec(path: &Path) -> Option<String> {
    let name = path.file_stem()?.to_str()?;
    Some(format!("{name}:{}", path.display()))
}

/// Whatever the machine already had. Looked up once: this runs for every
/// invocation, and PATH does not change underneath a running app.
#[cfg(not(target_os = "android"))]
fn path_runtime() -> Option<String> {
    static FOUND: once_cell::sync::OnceCell<Option<String>> = once_cell::sync::OnceCell::new();

    FOUND
        .get_or_init(|| {
            ["deno", "node", "bun"].iter().find_map(|name| {
                let path = process::which(name)?;
                Some(format!("{name}:{}", path.display()))
            })
        })
        .clone()
}

pub fn engine_binary() -> AppResult<std::path::PathBuf> {
    tools::require_engine()
}

/// Turn the engine's diagnostic text into the app's error taxonomy. The raw
/// text is preserved as the technical detail; only the classification is used
/// to decide what the user is shown.
pub fn classify_engine_error(stderr: &str) -> AppError {
    let lower = stderr.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));

    // Ordered most-specific first: "http error 404" has to be recognised before
    // the broader 4xx check, and a missing extractor before anything else.
    if has(&["unsupported url", "no suitable extractor", "is not a valid url"]) {
        return AppError::Unsupported(first_error_line(stderr));
    }

    // A request that never reached the source says nothing about the media.
    // It goes ahead of the checks below, which look for words such as "geo" or
    // "paid" that the id quoted in the same line could happen to contain.
    if net::is_lookup_failure(stderr) || has(&["network is unreachable"]) {
        return AppError::Network(first_error_line(stderr));
    }

    if has(&[
        "http error 404",
        "http error 410",
        "video unavailable",
        "has been removed",
        "no longer available",
        "does not exist",
    ]) {
        return AppError::NotFound {
            status: 404,
            detail: first_error_line(stderr),
        };
    }

    // A bot check and a membership wall are different walls, and one run can
    // mention both -- a warning about a missing proof-of-origin token above the
    // real refusal. Only the reported error decides, and a bot check decides
    // first: a request YouTube would not answer at all never got as far as
    // asking who the viewer is, and telling someone the browser they have just
    // connected is at fault is the worst answer this feature could give.
    let reported = first_error_line(stderr).to_ascii_lowercase();
    if ["not a bot", "po token", "po_token", "proof of origin"]
        .iter()
        .any(|needle| reported.contains(needle))
    {
        return AppError::Forbidden {
            status: 403,
            detail: first_error_line(stderr),
        };
    }

    // The membership wall, in the words YouTube itself uses and yt-dlp passes
    // through: "Join this channel to get access to members-only content like
    // this video", and for a tiered channel "This video is available to this
    // channel's members on level: <tier>". Recognised ahead of the broad
    // refusal below because it is the one case the app can do something about.
    if has(&[
        "members-only",
        "members only",
        "join this channel",
        "channel's members",
        "members on level",
    ]) {
        return AppError::MembershipRequired {
            detail: first_error_line(stderr),
        };
    }

    // Everything else that means "you are not allowed to see this", including
    // the engine's own suggestion to supply cookies or credentials. The caller
    // may repeat one of these with a linked browser's session behind it, but
    // only where one is stored and only once; a refusal that survives that is
    // reported as it stands.
    if has(&[
        "http error 401",
        "http error 402",
        "http error 403",
        "http error 429",
        "sign in",
        "sign-in",
        "signed in",
        "log in",
        "logged in",
        "logged-in",
        "login required",
        "credentials",
        "--cookies",
        "requires authentication",
        "private",
        "members",
        "premium",
        "subscriber",
        "paid",
        "purchase",
        "not available in your",
        "geo",
    ]) || (lower.contains("age") && lower.contains("confirm"))
    {
        return AppError::Forbidden {
            status: 403,
            detail: first_error_line(stderr),
        };
    }

    if has(&[
        "unable to download",
        "connection",
        "timed out",
        "temporary failure",
        "getaddrinfo",
        "network is unreachable",
    ]) {
        return AppError::Network(first_error_line(stderr));
    }

    AppError::Engine(first_error_line(stderr))
}

/// Warnings the engine prints about any post without a stream. They say that
/// there is nothing, not why.
const NO_STREAM_NOTES: &[&str] = &[
    "no video formats found",
    "requested format is not available",
    "falling back on generic information extractor",
];

/// A platform's own way of saying a post has no video, which on a post whose
/// photos could not be read either is simply "nothing to download here".
const NO_VIDEO_NOTES: &[&str] = &["there is no video in this post", "no video could be found"];

/// Put the engine's own reason on a link that turned out to have no stream.
///
/// Asked to report such links rather than fail on them, the engine gives its
/// reason as a warning instead of an error. The last specific one is it; with
/// none, the plain "nothing to download" stands.
fn explain_missing_streams(err: AppError, stderr: &str) -> AppError {
    if !matches!(err, AppError::Unsupported(_)) {
        return err;
    }
    let Some(reason) = stderr
        .lines()
        .rev()
        .filter_map(|line| line.trim().strip_prefix("WARNING:"))
        .map(str::trim)
        .find(|note| {
            let lower = note.to_ascii_lowercase();
            !note.is_empty() && !NO_STREAM_NOTES.iter().any(|generic| lower.contains(generic))
        })
    else {
        return err;
    };

    let lower = reason.to_ascii_lowercase();
    if NO_VIDEO_NOTES.iter().any(|note| lower.contains(note)) {
        AppError::Unsupported(logging::redact(reason).chars().take(600).collect())
    } else {
        classify_engine_error(&format!("ERROR: {reason}"))
    }
}

/// The one line of the engine's output that explains the failure, cut down to
/// something safe to hand out.
///
/// Every error the user sees carries this text, and the error card has a Copy
/// button that ends up in public issue trackers -- so a run that was given the
/// browser's session must not be able to put any of it there.
fn first_error_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("ERROR:") || line.starts_with("error:"))
        .or_else(|| stderr.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or("the engine reported no detail");

    logging::redact(line).chars().take(600).collect()
}

// -- JSON -> model ---------------------------------------------------------

/// What the engine printed for a link: one piece of media, or -- for a
/// carousel, gallery or album -- a playlist of them.
///
/// An item nothing can be downloaded from is left out of a gallery rather
/// than failing the whole post. Positions therefore count the items that can
/// be downloaded, which is also the number the interface shows.
pub fn parse_result(root: &Value, requested_url: &str) -> AppResult<MediaMetadata> {
    if root.get("_type").and_then(Value::as_str) != Some("playlist") {
        return parse_metadata(root, requested_url);
    }

    let mut items = Vec::new();
    let mut first_error = None;
    for entry in root.get("entries").and_then(Value::as_array).into_iter().flatten() {
        match parse_metadata(entry, requested_url) {
            Ok(item) => items.push(item),
            Err(err) => {
                log_debug!("engine", "skipping an item with nothing to download: {err}");
                first_error.get_or_insert(err);
            }
        }
    }

    let title = root.get("title").and_then(Value::as_str).map(str::to_string);
    let canonical_url = root.get("webpage_url").and_then(Value::as_str).map(str::to_string);
    providers::gallery(title, canonical_url, items).ok_or_else(|| {
        first_error.unwrap_or_else(|| AppError::Unsupported("the source returned no media".into()))
    })
}

pub fn parse_metadata(node: &Value, requested_url: &str) -> AppResult<MediaMetadata> {
    let platform = detect::detect_platform(requested_url);

    let title = node
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| caption_line(node))
        .unwrap_or_else(|| "Untitled".to_string());

    let creator = ["uploader", "channel", "creator", "artist", "uploader_id"]
        .iter()
        .find_map(|key| node.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);

    let mut formats: Vec<MediaFormat> = node
        .get("formats")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(parse_format).collect())
        .unwrap_or_default();

    // Some extractors return a single stream at the top level with no formats
    // array at all (many image posts, and a few simple video hosts).
    if formats.is_empty() {
        if let Some(single) = parse_format(node) {
            formats.push(single);
        }
    }

    // A photo post: pictures and no stream. What the engine lists as its
    // thumbnails are the photo itself, in several sizes.
    if formats.is_empty() && is_photo_post(node, platform) {
        if let Some(photo) = best_image(node) {
            formats.push(photo);
        }
    }

    if formats.is_empty() {
        // The engine says outright when a video sits behind a channel
        // membership, and reading that field beats recognising an English
        // warning line that changes between releases and is not written in the
        // user's language either.
        if node.get("availability").and_then(Value::as_str) == Some("subscriber_only") {
            return Err(AppError::MembershipRequired {
                detail: "this video is for members of the channel".into(),
            });
        }

        return Err(AppError::Unsupported(
            "the source offered no downloadable stream".into(),
        ));
    }

    let (watermark_support, _) = apply_watermark_flags(platform, &mut formats);
    sort_formats(&mut formats);

    let is_live = node
        .get("is_live")
        .and_then(Value::as_bool)
        .or_else(|| {
            node.get("live_status")
                .and_then(Value::as_str)
                .map(|status| status == "is_live")
        })
        .unwrap_or(false);

    let media_kind = infer_media_kind(&formats, node);
    let title = if media_kind == MediaKind::Image {
        photo_title(title, node, creator.as_deref())
    } else {
        title
    };

    // Only notes the UI cannot infer for itself belong here. A live stream, for
    // instance, is already obvious from `is_live` and gets its own badge.
    let warnings = Vec::new();

    Ok(MediaMetadata {
        url: requested_url.to_string(),
        canonical_url: node
            .get("webpage_url")
            .and_then(Value::as_str)
            .unwrap_or(requested_url)
            .to_string(),
        platform,
        platform_label: platform.label().to_string(),
        provider_id: PROVIDER_ID.to_string(),
        media_kind,
        title,
        creator,
        description: node
            .get("description")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.chars().take(600).collect()),
        thumbnail_url: pick_thumbnail(node),
        duration_sec: node.get("duration").and_then(Value::as_f64),
        view_count: node.get("view_count").and_then(Value::as_u64),
        like_count: node.get("like_count").and_then(Value::as_u64),
        upload_date: node
            .get("upload_date")
            .and_then(Value::as_str)
            .map(str::to_string),
        is_live,
        formats,
        entry_count: None,
        watermark_support,
        warnings,
        entries: Vec::new(),
    })
}

/// The first line of a post's caption, for when it has no title of its own.
fn caption_line(node: &Value) -> Option<String> {
    node.get("description")
        .and_then(Value::as_str)
        .and_then(|text| text.lines().map(str::trim).find(|line| !line.is_empty()))
        .map(|line| {
            if line.chars().count() > 100 {
                format!("{}...", line.chars().take(97).collect::<String>().trim_end())
            } else {
                line.to_string()
            }
        })
}

/// Whether a result with no stream is a photo post.
///
/// Only the platforms that publish photo posts qualify. Anywhere else, a
/// thumbnail without a stream is the poster of a video that is protected, not
/// live yet or still processing, and offering that frame as "the photo" would
/// be a false answer. Even on these platforms a result that has a running
/// time, or is live, is a video that could not be read.
fn is_photo_post(node: &Value, platform: PlatformId) -> bool {
    let publishes_photos = matches!(platform, PlatformId::Instagram | PlatformId::Pinterest);
    let protected = node.get("_has_drm").and_then(Value::as_bool).unwrap_or(false);
    let live = node
        .get("live_status")
        .and_then(Value::as_str)
        .is_some_and(|status| status != "not_live");
    let timed = node
        .get("duration")
        .and_then(Value::as_f64)
        .is_some_and(|seconds| seconds > 0.0);
    publishes_photos && !protected && !live && !timed
}

/// A photo's engine-given name, corrected.
///
/// Where a source gives no title the engine names what it read a video: "Video
/// by someone" on Instagram, "Pinterest video #123" in general. For a photo
/// that is simply wrong.
fn photo_title(title: String, node: &Value, creator: Option<&str>) -> String {
    if let Some(account) = title.strip_prefix("Video by ") {
        return format!("Photo by {account}");
    }

    let id = node.get("id").and_then(Value::as_str).unwrap_or_default();
    if !id.is_empty() && title.ends_with(&format!(" video #{id}")) {
        return caption_line(node)
            .or_else(|| creator.map(|name| format!("Photo by {name}")))
            .unwrap_or_else(|| format!("Photo {id}"));
    }
    title
}

/// A size in an image address: `s1080x1080` or `p640x640` between separators.
/// CDNs serving several renditions of one picture name them this way.
static SIZE_IN_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[_/=.,-][sp](\d{2,5})x(\d{2,5})(?:[_/&.,?-]|$)").expect("size pattern is valid")
});

/// A crop box in an image address, e.g. `c0.0.1439.1439a`: a square cut of
/// the picture made for a grid, not the picture.
static CROP_IN_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[_/=,-]c\d+\.\d+\.\d+\.\d+a?[_/&.,-]").expect("crop pattern is valid")
});

struct ImageCandidate {
    url: String,
    width: Option<u32>,
    height: Option<u32>,
    headers: Vec<(String, String)>,
}

impl ImageCandidate {
    fn from_json(node: &Value) -> Option<Self> {
        let url = node
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("http"))?
            .to_string();
        let dimension = |key: &str| {
            node.get(key)
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
        };
        Some(Self {
            width: dimension("width"),
            height: dimension("height"),
            headers: string_pairs(node.get("http_headers")),
            url,
        })
    }

    /// Larger first, uncropped ahead of cropped. A rendition whose size is
    /// neither stated nor written into its address is the undecorated
    /// original, which is larger than any bounded rendition of it.
    fn rank(&self) -> (u64, bool) {
        let pixels = match (self.width, self.height) {
            (Some(width), Some(height)) => u64::from(width) * u64::from(height),
            _ => SIZE_IN_URL
                .captures(&self.url)
                .and_then(|size| {
                    let width: u64 = size.get(1)?.as_str().parse().ok()?;
                    let height: u64 = size.get(2)?.as_str().parse().ok()?;
                    Some(width * height)
                })
                .unwrap_or(u64::MAX),
        };
        (pixels, !CROP_IN_URL.is_match(&self.url))
    }
}

/// The full-size photo out of the renditions the engine lists for a post.
fn best_image(node: &Value) -> Option<MediaFormat> {
    let mut candidates: Vec<ImageCandidate> = node
        .get("thumbnails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(ImageCandidate::from_json)
        .collect();

    if let Some(url) = node
        .get("thumbnail")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("http"))
    {
        if !candidates.iter().any(|candidate| candidate.url == url) {
            candidates.push(ImageCandidate {
                url: url.to_string(),
                width: None,
                height: None,
                headers: Vec::new(),
            });
        }
    }

    let best = candidates.into_iter().max_by_key(ImageCandidate::rank)?;

    // Headers the post was read with (a Referer, usually), then any the
    // rendition itself asks for.
    let mut headers = string_pairs(node.get("http_headers"));
    for (name, value) in best.headers {
        headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        headers.push((name, value));
    }

    Some(providers::image_format("image", best.url, best.width, best.height, headers))
}

fn string_pairs(value: Option<&Value>) -> Vec<(String, String)> {
    value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_format(node: &Value) -> Option<MediaFormat> {
    let url = node.get("url").and_then(Value::as_str)?.to_string();
    if url.is_empty() {
        return None;
    }

    let id = node
        .get("format_id")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();

    let protocol = node
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("https")
        .to_string();

    // yt-dlp uses the literal string "none" rather than null for an absent codec.
    let vcodec = node
        .get("vcodec")
        .and_then(Value::as_str)
        .filter(|value| *value != "none" && !value.is_empty())
        .map(str::to_string);
    let acodec = node
        .get("acodec")
        .and_then(Value::as_str)
        .filter(|value| *value != "none" && !value.is_empty())
        .map(str::to_string);

    let width = node.get("width").and_then(Value::as_u64).map(|v| v as u32);
    let height = node.get("height").and_then(Value::as_u64).map(|v| v as u32);
    let container = node
        .get("ext")
        .and_then(Value::as_str)
        .unwrap_or("bin")
        .to_string();

    let has_video = vcodec.is_some() || (height.is_some() && detect::is_video_extension(&container));
    let has_audio = acodec.is_some();
    let is_image = !has_video && !has_audio && detect::is_image_extension(&container);

    let kind = if is_image {
        FormatKind::Image
    } else if has_video && has_audio {
        FormatKind::Muxed
    } else if has_video {
        FormatKind::Video
    } else if has_audio {
        FormatKind::Audio
    } else {
        return None;
    };

    let abr = node.get("abr").and_then(Value::as_f64);
    let tbr = node.get("tbr").and_then(Value::as_f64);

    let http_headers = string_pairs(node.get("http_headers"));

    let note = node
        .get("format_note")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);

    let quality_label = quality_label(kind, height, width, abr.or(tbr), note.as_deref());

    Some(MediaFormat {
        id,
        kind,
        container,
        // Segmented protocols need a real HLS/DASH client; a ranged GET cannot
        // fetch a manifest's worth of segments.
        needs_engine_download: is_segmented(&protocol),
        protocol,
        has_video,
        has_audio,
        width,
        height,
        fps: node.get("fps").and_then(Value::as_f64),
        vcodec,
        acodec,
        tbr,
        vbr: node.get("vbr").and_then(Value::as_f64),
        abr,
        filesize: node.get("filesize").and_then(Value::as_u64),
        filesize_approx: node.get("filesize_approx").and_then(Value::as_u64),
        quality_label,
        watermarked: None,
        note,
        url: Some(url),
        http_headers,
    })
}

pub fn is_segmented(protocol: &str) -> bool {
    protocol.contains("m3u8")
        || protocol.contains("dash")
        || protocol.contains("ism")
        || protocol.contains("f4m")
        || protocol.starts_with("rtmp")
        || protocol.starts_with("rtsp")
        || protocol.contains("websocket")
}

fn quality_label(
    kind: FormatKind,
    height: Option<u32>,
    width: Option<u32>,
    bitrate: Option<f64>,
    note: Option<&str>,
) -> String {
    match kind {
        FormatKind::Audio => bitrate
            .filter(|value| *value > 0.0)
            .map(|value| format!("{} kbps", value.round() as u64))
            .or_else(|| note.map(str::to_string))
            .unwrap_or_else(|| "Audio".to_string()),
        FormatKind::Image => match (width, height) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => "Image".to_string(),
        },
        _ => match (height, width) {
            (Some(h), _) if h > 0 => format!("{h}p"),
            (_, Some(w)) if w > 0 => format!("{w}w"),
            _ => note.unwrap_or("Video").to_string(),
        },
    }
}

fn pick_thumbnail(node: &Value) -> Option<String> {
    // The webview cannot show HEIC, which is what some sources keep a photo's
    // original in; a smaller JPEG of it previews where the original would not.
    let viewable = |url: &&str| {
        url.starts_with("http")
            && !detect::classify(url)
                .and_then(|info| info.direct_extension)
                .is_some_and(|extension| extension == "heic" || extension == "heif")
    };

    if let Some(url) = node.get("thumbnail").and_then(Value::as_str).filter(viewable) {
        return Some(url.to_string());
    }

    // Otherwise take the widest thumbnail the source offers, which is the one
    // that will still look sharp in the preview card.
    node.get("thumbnails")
        .and_then(Value::as_array)?
        .iter()
        .filter(|entry| entry.get("url").and_then(Value::as_str).is_some_and(|url| viewable(&url)))
        .max_by_key(|entry| entry.get("width").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|entry| entry.get("url").and_then(Value::as_str))
        .map(str::to_string)
}

fn infer_media_kind(formats: &[MediaFormat], node: &Value) -> MediaKind {
    if formats.iter().any(|f| f.has_video) {
        MediaKind::Video
    } else if formats.iter().any(|f| f.kind == FormatKind::Image) {
        MediaKind::Image
    } else if formats.iter().any(|f| f.has_audio) {
        MediaKind::Audio
    } else if node.get("duration").and_then(Value::as_f64).is_some() {
        MediaKind::Video
    } else {
        MediaKind::Image
    }
}

/// Mark which formats carry a platform watermark.
///
/// This only reports what the source itself distinguishes -- TikTok publishes a
/// watermarked rendition alongside clean ones, and yt-dlp labels them. Nothing
/// here removes a watermark; when a platform only offers a stamped rendition,
/// that is reported as `WatermarkedOnly` and the UI says so.
fn apply_watermark_flags(
    platform: PlatformId,
    formats: &mut [MediaFormat],
) -> (WatermarkSupport, usize) {
    if platform != PlatformId::Tiktok {
        return (WatermarkSupport::NotApplicable, 0);
    }

    let mut clean = 0usize;
    let mut stamped = 0usize;

    for format in formats.iter_mut() {
        if !format.has_video {
            continue;
        }
        let id = format.id.to_ascii_lowercase();
        let note = format.note.as_deref().unwrap_or("").to_ascii_lowercase();

        let is_watermarked = note.contains("watermark")
            || id.starts_with("download")
            || id.contains("watermark");

        format.watermarked = Some(is_watermarked);
        if is_watermarked {
            stamped += 1;
        } else {
            clean += 1;
        }
    }

    let support = if clean > 0 {
        WatermarkSupport::CleanAvailable
    } else if stamped > 0 {
        WatermarkSupport::WatermarkedOnly
    } else {
        WatermarkSupport::NotApplicable
    };

    (support, clean)
}

/// Best first, so the default selection is a simple "take the head".
fn sort_formats(formats: &mut [MediaFormat]) {
    formats.sort_by(|a, b| {
        b.pixels()
            .cmp(&a.pixels())
            .then_with(|| {
                b.tbr
                    .unwrap_or(0.0)
                    .partial_cmp(&a.tbr.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                b.abr
                    .unwrap_or(0.0)
                    .partial_cmp(&a.abr.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            // Prefer a stream we can fetch ourselves over one that needs the
            // engine, all else being equal.
            .then_with(|| a.needs_engine_download.cmp(&b.needs_engine_download))
    });
}

/// Where the engine should write when it handles a download itself.
pub fn output_template(target: &Path) -> String {
    target.to_string_lossy().into_owned()
}

pub fn log_engine_invocation(args: &[String]) {
    log_debug!(
        "engine",
        "invoking with {} args: {:?}",
        args.len(),
        logging::redact_args(args)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format_json(id: &str, note: Option<&str>) -> Value {
        serde_json::json!({
            "format_id": id,
            "url": "https://cdn.test/v.mp4",
            "ext": "mp4",
            "vcodec": "h264",
            "acodec": "aac",
            "height": 1080,
            "width": 1920,
            "format_note": note,
        })
    }

    #[test]
    fn classifies_unsupported_urls() {
        let err = classify_engine_error("ERROR: Unsupported URL: https://example.test/");
        assert_eq!(err.code(), "unsupported");
    }

    #[test]
    fn classifies_private_media_as_forbidden() {
        let err = classify_engine_error("ERROR: This video is private");
        assert_eq!(err.code(), "forbidden");
    }

    #[test]
    fn classifies_missing_media_as_not_found() {
        assert_eq!(
            classify_engine_error("ERROR: HTTP Error 404: Not Found").code(),
            "notFound"
        );
        assert_eq!(
            classify_engine_error("ERROR: Video unavailable").code(),
            "notFound"
        );
    }

    #[test]
    fn a_login_wall_is_forbidden_however_the_engine_words_it() {
        let messages = [
            "ERROR: [vimeo] 76979871: The web client only works when logged-in.              Use --cookies, --cookies-from-browser, --username and --password to              provide account credentials",
            "ERROR: Sign in to confirm your age",
            "ERROR: This video is available to Music Premium members",
            "ERROR: Please log in to view this content",
            "ERROR: Login required",
            "ERROR: HTTP Error 401: Unauthorized",
            "ERROR: HTTP Error 429: Too Many Requests",
            "ERROR: This video is not available in your country",
        ];
        for message in messages {
            assert_eq!(
                classify_engine_error(message).code(),
                "forbidden",
                "misclassified: {message}"
            );
        }
    }

    #[test]
    fn a_membership_wall_is_told_apart_from_every_other_refusal() {
        // As YouTube words it, passed through by the engine: the plain case,
        // the tiered case, and a members-only live stream.
        let walls = [
            "ERROR: [youtube] AbCdEf12345: Join this channel to get access to members-only content like this video, and other exclusive perks.",
            "ERROR: [youtube] AbCdEf12345: This video is available to this channel's members on level: Supporters (or any higher level). Join this channel to get access to members-only content and other exclusive perks.",
            "ERROR: [youtube] AbCdEf12345: This live stream is members-only content",
        ];
        for wall in walls {
            let err = classify_engine_error(wall);
            assert_eq!(err.code(), "membershipRequired", "misclassified: {wall}");
        }
    }

    #[test]
    fn a_bot_check_is_never_blamed_on_the_membership() {
        // The worst message this feature could produce: the user connects a
        // browser, the download still fails, and the app says the membership
        // is the problem. A bot check is its own wall and stays one.
        let checks = [
            "ERROR: [youtube] AbCdEf12345: Sign in to confirm you're not a bot. Use --cookies-from-browser or --cookies for the authentication.",
            "ERROR: [youtube] AbCdEf12345: Sign in to confirm you\u{2019}re not a bot",
            "ERROR: [youtube] AbCdEf12345: Some formats are missing a GVS PO Token",
        ];
        for check in checks {
            assert_eq!(classify_engine_error(check).code(), "forbidden", "{check}");
        }

        // Even when a stale members-only warning is still on the buffer above
        // the refusal that actually stopped the run.
        let both = "WARNING: [youtube] AbCdEf12345: members-only content\n\
                    ERROR: [youtube] AbCdEf12345: Sign in to confirm you're not a bot";
        assert_eq!(classify_engine_error(both).code(), "forbidden");
    }

    #[test]
    fn a_paid_tier_that_is_not_a_channel_membership_stays_a_plain_refusal() {
        // Music Premium is not something a linked browser is asked to answer
        // differently, so it must not borrow the membership message.
        assert_eq!(
            classify_engine_error("ERROR: This video is available to Music Premium members").code(),
            "forbidden"
        );
    }

    #[test]
    fn a_session_is_only_offered_for_a_wall_on_a_host_the_link_serves() {
        let settings = Settings::default();

        let membership = AppError::MembershipRequired { detail: "x".into() };
        let missing = AppError::NotFound { status: 404, detail: "x".into() };

        // Nothing but a refusal, and nothing off the hosts the link serves,
        // ever reaches the point of asking for a lease at all.
        assert!(session_for(&missing, "https://www.youtube.com/watch?v=abc", &settings).is_none());
        assert!(session_for(&membership, "https://vimeo.com/76979871", &settings).is_none());
    }

    #[test]
    fn a_cookie_value_never_reaches_the_error_card() {
        let stderr = "ERROR: [youtube] abc: Unable to load cookies: SID=g.a000SESSIONVALUE9f";
        let detail = classify_engine_error(stderr).technical().unwrap();
        assert!(!detail.contains("g.a000SESSIONVALUE9f"), "{detail}");
    }

    #[test]
    fn a_login_wall_is_not_reported_as_retryable_nonsense() {
        let err = classify_engine_error("ERROR: The web client only works when logged-in.");
        // Retrying an auth wall is pointless but harmless; what matters is that
        // the user is told the real reason rather than "something went wrong".
        assert_eq!(err.code(), "forbidden");
        assert!(err.technical().is_some_and(|t| t.contains("logged-in")));
    }

    #[test]
    fn classifies_connection_problems_as_network() {
        let err = classify_engine_error("ERROR: Unable to download webpage: getaddrinfo failed");
        assert_eq!(err.code(), "network");
        assert!(err.retryable());
    }

    #[test]
    fn a_failed_lookup_is_a_network_error_whatever_the_id_spells() {
        // As reported from a phone whose DNS gave no answer, and the same
        // with the backup lookup's reason and an id that contains "geo".
        let reported = "ERROR: [vm.tiktok] ZSq4hQv5y: Unable to download webpage: [Errno 7] No address associated with hostname (caused by TransportError('[Errno 7] No address associated with hostname'))";
        let with_backup = "ERROR: [vm.tiktok] ZSgeo4Qv5: Unable to download webpage: [Errno 7] No address associated with hostname; backup lookup: 1.1.1.1 timed out, 8.8.8.8 timed out (caused by TransportError('...'))";

        for stderr in [reported, with_backup] {
            let err = classify_engine_error(stderr);
            assert_eq!(err.code(), "network", "misclassified: {stderr}");
            assert!(err.retryable());
            assert!(err.technical().is_some_and(|detail| detail.contains("No address associated with hostname")));
        }
    }

    /// The engine has to be told what a runtime is, not only where it is, and
    /// a pair it does not recognise is one it silently ignores.
    #[test]
    #[cfg(not(target_os = "android"))]
    fn a_runtime_is_named_after_the_program_it_points_at() {
        assert_eq!(
            runtime_spec(Path::new(r"C:\Users\x\AppData\Roaming\UniversalDownloader\tools\js\deno.exe")),
            Some(r"deno:C:\Users\x\AppData\Roaming\UniversalDownloader\tools\js\deno.exe".to_string())
        );
        assert_eq!(
            runtime_spec(Path::new(r"C:\Program Files\nodejs\node.exe")),
            Some(r"node:C:\Program Files\nodejs\node.exe".to_string())
        );
    }

    #[test]
    fn segmented_protocols_are_delegated_to_the_engine() {
        assert!(is_segmented("m3u8_native"));
        assert!(is_segmented("http_dash_segments"));
        assert!(!is_segmented("https"));
        assert!(!is_segmented("http"));
    }

    #[test]
    fn tiktok_watermarked_formats_are_flagged_and_clean_ones_offered() {
        let mut formats: Vec<MediaFormat> = [
            format_json("download_addr-0", None),
            format_json("play_addr-1", None),
        ]
        .iter()
        .filter_map(parse_format)
        .collect();

        let (support, clean) = apply_watermark_flags(PlatformId::Tiktok, &mut formats);
        assert_eq!(support, WatermarkSupport::CleanAvailable);
        assert_eq!(clean, 1);
        assert_eq!(formats[0].watermarked, Some(true));
        assert_eq!(formats[1].watermarked, Some(false));
    }

    #[test]
    fn tiktok_with_only_stamped_formats_reports_watermarked_only() {
        let mut formats: Vec<MediaFormat> = [format_json("download_addr-0", Some("watermarked"))]
            .iter()
            .filter_map(parse_format)
            .collect();
        let (support, clean) = apply_watermark_flags(PlatformId::Tiktok, &mut formats);
        assert_eq!(support, WatermarkSupport::WatermarkedOnly);
        assert_eq!(clean, 0);
    }

    #[test]
    fn other_platforms_report_watermarking_as_not_applicable() {
        let mut formats: Vec<MediaFormat> =
            [format_json("137", None)].iter().filter_map(parse_format).collect();
        let (support, _) = apply_watermark_flags(PlatformId::Youtube, &mut formats);
        assert_eq!(support, WatermarkSupport::NotApplicable);
        assert_eq!(formats[0].watermarked, None);
    }

    #[test]
    fn a_video_only_stream_is_not_marked_as_muxed() {
        let node = serde_json::json!({
            "format_id": "137",
            "url": "https://cdn.test/v.mp4",
            "ext": "mp4",
            "vcodec": "avc1.640028",
            "acodec": "none",
            "height": 1080,
        });
        let format = parse_format(&node).unwrap();
        assert_eq!(format.kind, FormatKind::Video);
        assert!(format.has_video && !format.has_audio);
        assert_eq!(format.quality_label, "1080p");
    }

    #[test]
    fn an_audio_stream_is_labelled_by_bitrate() {
        let node = serde_json::json!({
            "format_id": "140",
            "url": "https://cdn.test/a.m4a",
            "ext": "m4a",
            "vcodec": "none",
            "acodec": "mp4a.40.2",
            "abr": 192.0,
        });
        let format = parse_format(&node).unwrap();
        assert_eq!(format.kind, FormatKind::Audio);
        assert_eq!(format.quality_label, "192 kbps");
    }

    #[test]
    fn formats_without_a_url_are_dropped() {
        assert!(parse_format(&serde_json::json!({ "format_id": "x" })).is_none());
    }

    #[test]
    fn parses_a_minimal_payload_end_to_end() {
        let node = serde_json::json!({
            "title": "Example",
            "uploader": "someone",
            "duration": 42.0,
            "webpage_url": "https://www.youtube.com/watch?v=abc",
            "thumbnails": [
                { "url": "https://cdn.test/small.jpg", "width": 120 },
                { "url": "https://cdn.test/large.jpg", "width": 1280 }
            ],
            "formats": [format_json("18", None)],
        });
        let meta = parse_metadata(&node, "https://www.youtube.com/watch?v=abc").unwrap();
        assert_eq!(meta.title, "Example");
        assert_eq!(meta.creator.as_deref(), Some("someone"));
        assert_eq!(meta.platform, PlatformId::Youtube);
        assert_eq!(meta.media_kind, MediaKind::Video);
        assert_eq!(meta.thumbnail_url.as_deref(), Some("https://cdn.test/large.jpg"));
        assert_eq!(meta.formats.len(), 1);
    }

    #[test]
    fn a_payload_with_no_usable_stream_is_unsupported() {
        let node = serde_json::json!({ "title": "Example", "formats": [] });
        let err = parse_metadata(&node, "https://example.test/x").unwrap_err();
        assert_eq!(err.code(), "unsupported");
    }

    /// Renditions of one Instagram photo as the engine lists them: square
    /// crops and bounded sizes, and the undecorated original.
    fn instagram_renditions(file: &str) -> Value {
        let base = format!("https://scontent.cdninstagram.test/v/t51.82787-15/{file}_n.jpg");
        serde_json::json!([
            { "url": format!("{base}?stp=c0.0.1439.1439a_dst-jpg_e35_s1080x1080_tt6&oh=1") },
            { "url": format!("{base}?stp=c0.0.1439.1439a_dst-jpg_e35_s150x150_tt6&oh=1") },
            { "url": format!("{base}?stp=dst-jpg_e35_s1080x1080_tt6&oh=1") },
            { "url": format!("{base}?stp=dst-jpg_e35_s640x640_sh2.08_tt6&oh=1") },
            { "url": format!("{base}?stp=dst-jpg_e35_tt6&oh=1") },
            { "url": format!("{base}?stp=c0.0.1439.1439a_dst-jpg_e35_tt6&oh=1") },
        ])
    }

    fn instagram_photo(id: &str, file: &str) -> Value {
        serde_json::json!({
            "id": id,
            "title": "Video by someone",
            "description": "A caption\n#tags",
            "channel": "someone",
            "uploader": "Some One",
            "formats": [],
            "thumbnails": instagram_renditions(file),
            "thumbnail": format!("https://scontent.cdninstagram.test/v/t51.82787-15/{file}_n.jpg?stp=dst-jpg_e35_tt6&oh=1"),
            "http_headers": { "Referer": "https://www.instagram.com/" },
            "webpage_url": "https://www.instagram.com/p/POST/",
        })
    }

    #[test]
    fn an_instagram_photo_is_offered_at_full_size() {
        let meta = parse_result(
            &instagram_photo("BsOGulcndj-", "625727639"),
            "https://www.instagram.com/p/BsOGulcndj-/",
        )
        .unwrap();

        assert_eq!(meta.media_kind, MediaKind::Image);
        assert_eq!(meta.title, "Photo by someone");
        assert_eq!(meta.formats.len(), 1);

        let photo = &meta.formats[0];
        assert_eq!(photo.kind, FormatKind::Image);
        assert_eq!(photo.container, "jpg");
        assert!(
            photo.url.as_deref().unwrap().contains("stp=dst-jpg_e35_tt6"),
            "picked {:?} instead of the uncropped original",
            photo.url
        );
        assert!(photo
            .http_headers
            .iter()
            .any(|(name, value)| name == "Referer" && value == "https://www.instagram.com/"));
    }

    #[test]
    fn a_carousel_becomes_a_gallery_of_its_photos_and_videos() {
        let video = serde_json::json!({
            "id": "DQ3yk18DLbd",
            "title": "Video by someone",
            "formats": [
                { "format_id": "dash-1v", "url": "https://cdn.test/v.mp4", "ext": "mp4",
                  "vcodec": "vp09.00.21.08", "acodec": "none", "width": 480, "height": 480 },
                { "format_id": "dash-1a", "url": "https://cdn.test/a.m4a", "ext": "m4a",
                  "vcodec": "none", "acodec": "mp4a.40.5", "abr": 62.8 },
            ],
            "thumbnails": instagram_renditions("579688490"),
        });
        let root = serde_json::json!({
            "_type": "playlist",
            "title": "Post by someone",
            "webpage_url": "https://www.instagram.com/p/DQ3zR6-DPGm/",
            "entries": [
                instagram_photo("DQ3zRtyjGi6", "576509622"),
                video,
                instagram_photo("DQ3zRt3DClt", "576111569"),
            ],
        });

        let post = parse_result(&root, "https://www.instagram.com/p/DQ3zR6-DPGm/").unwrap();
        assert_eq!(post.media_kind, MediaKind::Gallery);
        assert_eq!(post.entry_count, Some(3));
        assert_eq!(post.title, "Post by someone");
        assert_eq!(post.canonical_url, "https://www.instagram.com/p/DQ3zR6-DPGm/");

        let kinds: Vec<_> = post.entries.iter().map(|item| item.media_kind).collect();
        assert_eq!(kinds, [MediaKind::Image, MediaKind::Video, MediaKind::Image]);
        assert_eq!(post.entries[1].title, "Post by someone (2)");
        assert!(post.entries[1].formats.iter().all(|format| format.kind != FormatKind::Image));
    }

    #[test]
    fn an_item_with_nothing_to_download_is_left_out_of_a_gallery() {
        let empty = serde_json::json!({ "id": "gone", "title": "Video by someone", "formats": [] });
        let root = serde_json::json!({
            "_type": "playlist",
            "title": "Post by someone",
            "entries": [instagram_photo("a", "1"), empty, instagram_photo("b", "2")],
        });
        let post = parse_result(&root, "https://www.instagram.com/p/x/").unwrap();
        assert_eq!(post.entry_count, Some(2));

        let nothing = serde_json::json!({ "_type": "playlist", "entries": [{ "id": "gone", "formats": [] }] });
        assert_eq!(
            parse_result(&nothing, "https://www.instagram.com/p/x/").unwrap_err().code(),
            "unsupported"
        );
    }

    #[test]
    fn a_thumbnail_is_not_a_photo_where_photo_posts_do_not_exist() {
        // A premiere that has not started has a poster and no stream.
        let node = serde_json::json!({
            "id": "abc",
            "title": "Premiere",
            "formats": [],
            "live_status": "is_upcoming",
            "thumbnails": [{ "url": "https://i.ytimg.test/vi/abc/maxresdefault.jpg", "width": 1280, "height": 720 }],
        });
        assert_eq!(
            parse_metadata(&node, "https://www.youtube.com/watch?v=abc").unwrap_err().code(),
            "unsupported"
        );
    }

    /// The engine states a membership wall in a field of its own, which is
    /// what the browser link has to key on: the warning line that says the
    /// same thing is English prose and changes between releases.
    #[test]
    fn a_members_only_video_says_so_rather_than_reading_as_unsupported() {
        let node = serde_json::json!({
            "id": "abc",
            "title": "Members only",
            "formats": [],
            "availability": "subscriber_only",
            "thumbnails": [{ "url": "https://i.ytimg.test/vi/abc/maxresdefault.jpg", "width": 1280, "height": 720 }],
        });
        assert_eq!(
            parse_metadata(&node, "https://www.youtube.com/watch?v=abc")
                .unwrap_err()
                .code(),
            "membershipRequired"
        );
    }

    /// Which refusals are worth spending the stored session on. The
    /// only-images case is here because that is the shape a signed-in request
    /// takes when the engine could not decipher the formats it was served.
    #[test]
    fn a_wall_is_worth_the_session_and_an_empty_post_is_not() {
        assert!(worth_a_session(&AppError::MembershipRequired {
            detail: "members".into()
        }));
        assert!(worth_a_session(&AppError::Forbidden {
            status: 403,
            detail: "sign in".into()
        }));
        assert!(worth_a_session(&AppError::Engine(
            "ERROR: Only images are available for download".into()
        )));

        assert!(!worth_a_session(&AppError::Unsupported(
            "nothing to download".into()
        )));
        assert!(!worth_a_session(&AppError::Network("timed out".into())));
        assert!(!worth_a_session(&AppError::Engine(
            "ERROR: unable to extract player version".into()
        )));
    }

    /// And a picture post on a platform that really publishes pictures must
    /// never reach for it, whatever the engine called the failure.
    #[test]
    fn only_youtube_addresses_are_worth_a_session() {
        assert!(bridge::wants_cookies("https://www.youtube.com/watch?v=a"));
        assert!(bridge::wants_cookies("https://youtu.be/a"));
        assert!(!bridge::wants_cookies("https://www.instagram.com/p/x/"));
        assert!(!bridge::wants_cookies("https://youtube.com.example.test/watch?v=a"));
    }

    #[test]
    fn a_video_that_could_not_be_read_is_not_offered_as_its_poster() {
        let mut node = instagram_photo("reel", "1");
        node["duration"] = serde_json::json!(12.5);
        assert!(parse_metadata(&node, "https://www.instagram.com/reel/x/").is_err());

        let mut node = instagram_photo("drm", "1");
        node["_has_drm"] = serde_json::json!(true);
        assert!(parse_metadata(&node, "https://www.instagram.com/p/x/").is_err());
    }

    #[test]
    fn a_pinterest_image_pin_takes_the_original_and_previews_a_viewable_copy() {
        let node = serde_json::json!({
            "id": "873276184017534617",
            "title": "Pinterest video #873276184017534617",
            "description": "Mont Blanc at dawn\nmore text",
            "formats": [],
            "thumbnails": [
                { "url": "https://i.pinimg.test/236x/c3/d6/b4/c3.jpg", "width": 236, "height": 177 },
                { "url": "https://i.pinimg.test/736x/c3/d6/b4/c3.jpg", "width": 736, "height": 552 },
                { "url": "https://i.pinimg.test/originals/c3/d6/b4/c3.heic", "width": 4032, "height": 3024 },
            ],
            "thumbnail": "https://i.pinimg.test/originals/c3/d6/b4/c3.heic",
        });
        let meta = parse_metadata(&node, "https://www.pinterest.com/pin/873276184017534617/").unwrap();

        assert_eq!(meta.title, "Mont Blanc at dawn");
        let photo = &meta.formats[0];
        assert_eq!(photo.container, "heic");
        assert_eq!((photo.width, photo.height), (Some(4032), Some(3024)));
        assert_eq!(photo.quality_label, "4032x3024");
        assert_eq!(
            meta.thumbnail_url.as_deref(),
            Some("https://i.pinimg.test/736x/c3/d6/b4/c3.jpg")
        );
    }

    #[test]
    fn a_stated_size_outranks_a_guess_and_a_crop_loses_a_tie() {
        let candidate = |url: &str, width: Option<u32>, height: Option<u32>| ImageCandidate {
            url: url.to_string(),
            width,
            height,
            headers: Vec::new(),
        };
        let bounded = candidate("https://cdn.test/a.jpg?stp=dst-jpg_s1080x1080_tt6", None, None);
        let original = candidate("https://cdn.test/a.jpg?stp=dst-jpg_tt6", None, None);
        let cropped = candidate("https://cdn.test/a.jpg?stp=c0.0.1439.1439a_dst-jpg_tt6", None, None);
        let stated = candidate("https://cdn.test/a.jpg", Some(640), Some(640));

        assert!(original.rank() > bounded.rank());
        assert!(original.rank() > cropped.rank());
        assert!(bounded.rank() > stated.rank());
        assert_eq!(bounded.rank().0, 1080 * 1080);
    }

    #[test]
    fn the_reason_a_link_has_no_stream_comes_from_the_engines_warning() {
        let unsupported = || AppError::Unsupported("the source offered no downloadable stream".into());

        let stderr = "WARNING: [youtube] abc: This live event will begin in 5 hours.\n\
                      WARNING: No video formats found!\n\
                      WARNING: Requested format is not available\n";
        let err = explain_missing_streams(unsupported(), stderr);
        assert!(err.technical().is_some_and(|detail| detail.contains("live event")));

        let stderr = "WARNING: [youtube] abc: Sign in to confirm your age\nWARNING: No video formats found!\n";
        assert_eq!(explain_missing_streams(unsupported(), stderr).code(), "forbidden");

        // Nothing specific: the plain answer stands.
        let stderr = "WARNING: No video formats found!\nWARNING: Requested format is not available\n";
        assert_eq!(explain_missing_streams(unsupported(), stderr).code(), "unsupported");

        // Other failures are already explained.
        let err = AppError::Network("offline".into());
        assert_eq!(explain_missing_streams(err, stderr).code(), "network");
    }

    #[test]
    fn a_height_only_format_still_ranks_by_resolution() {
        let tall = serde_json::json!({
            "format_id": "a", "url": "https://x/1", "ext": "mp4",
            "vcodec": "h264", "height": 1080
        });
        let short = serde_json::json!({
            "format_id": "b", "url": "https://x/2", "ext": "mp4",
            "vcodec": "h264", "height": 360
        });
        let tall = parse_format(&tall).unwrap();
        let short = parse_format(&short).unwrap();
        assert!(tall.pixels() > short.pixels());
    }

    #[test]
    fn formats_sort_best_first() {
        let node = serde_json::json!({
            "title": "t",
            "formats": [
                { "format_id": "a", "url": "https://x/1", "ext": "mp4", "vcodec": "h264", "height": 360 },
                { "format_id": "b", "url": "https://x/2", "ext": "mp4", "vcodec": "h264", "height": 1080 },
                { "format_id": "c", "url": "https://x/3", "ext": "mp4", "vcodec": "h264", "height": 720 },
            ]
        });
        let meta = parse_metadata(&node, "https://example.test/x").unwrap();
        let heights: Vec<_> = meta.formats.iter().map(|f| f.height).collect();
        assert_eq!(heights, vec![Some(1080), Some(720), Some(360)]);
    }
}
