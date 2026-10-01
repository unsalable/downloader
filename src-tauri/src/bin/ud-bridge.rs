//! The browser link's native messaging host.
//!
//! Chrome starts this program, speaks to it over stdin and stdout, and kills it
//! when the extension's port closes. It exists so a browser can hand the app a
//! session the app is not allowed to read for itself: Chrome 127 and later
//! encrypt the cookie database with a key bound to the browser's own
//! executable, so reading it from outside is no longer possible -- and should
//! not be. An extension the user installed, pushing through a channel the
//! browser owns, is the honest version of the same thing. The same channel
//! carries the videos the user sends from a page to download, which this
//! program leaves in the app's inbox before starting the app -- and, before
//! that, the popup's question of what each would download, which this program
//! answers itself by running the app's own analysis, without starting the app.
//!
//! The app is usually closed while this runs, which shapes everything here: it
//! holds no state of its own, takes every decision from the files under
//! `bridge/`, and enforces the user's own toggle rather than trusting the app
//! to be running and to say no on its behalf.
//!
//! Only framed messages ever reach stdout. Anything worth recording goes to the
//! app's log; a stray `println!` here would desynchronise the stream and read,
//! from the extension's side, as the app having broken.

use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use universal_downloader_lib::bridge::handoff;
use universal_downloader_lib::bridge::protocol::{
    self, Download, ErrorCode, HostStatus, Peer, Push, Request, Response,
};
use universal_downloader_lib::bridge::LinkState;
use universal_downloader_lib::downloader::{self, DownloadPreview};
use universal_downloader_lib::error::{AppError, AppResult};
use universal_downloader_lib::model::DownloadRequest;
use universal_downloader_lib::settings::Settings;
use universal_downloader_lib::{db, log_debug, log_warn, logging, paths, tools};

fn main() {
    // Chrome passes the calling extension's origin as the first argument. This
    // is authentication of the *caller*, by the browser, and it is worth being
    // precise about what it proves: that the browser started us for that
    // extension. It says nothing about this host being the one the user meant
    // to run -- that is what the registry values and the host path in
    // Settings' diagnostics are for.
    let origin = std::env::args().nth(1).unwrap_or_default();
    if !protocol::origin_allowed(&origin) {
        log_warn!("bridge-host", "refused a caller claiming to be {origin}");
        return;
    }

    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();

    // Until stdin ends, because both shapes of the API arrive here: a single
    // `sendNativeMessage` is one framed message and then EOF, while a
    // `connectNative` port sends as many as it likes down the same pipe.
    loop {
        let response = match read_frame(&mut input) {
            Frame::Eof | Frame::Broken => return,
            Frame::TooLarge => {
                // The length was read but the body was not, so the stream is no
                // longer aligned to a frame boundary and cannot be recovered.
                // Say so once, then stop reading.
                let _ = send(
                    &mut output,
                    &Response::err(ErrorCode::Malformed, "that message was too large"),
                );
                return;
            }
            Frame::Body(mut body) => {
                let parsed = serde_json::from_slice::<Request>(&body);
                body.fill(0);
                match parsed {
                    Ok(request) => handle(request),
                    Err(err) => {
                        // Deliberately not the parser's own message: it quotes
                        // the input it choked on, and the input is a cookie jar.
                        log_warn!(
                            "bridge-host",
                            "unreadable message at line {} column {}",
                            err.line(),
                            err.column()
                        );
                        Response::err(ErrorCode::Malformed, "that message could not be read")
                    }
                }
            }
        };

        if send(&mut output, &response).is_err() {
            return;
        }
    }
}

enum Frame {
    Body(Vec<u8>),
    Eof,
    TooLarge,
    Broken,
}

/// Four bytes of length in the machine's own byte order, then that many bytes
/// of UTF-8 JSON. Native endianness is not a choice; it is what Chrome writes.
fn read_frame(input: &mut impl Read) -> Frame {
    let mut header = [0u8; 4];
    match input.read_exact(&mut header) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::UnexpectedEof => return Frame::Eof,
        Err(_) => return Frame::Broken,
    }

    // Checked before the allocation, not after: the length is four bytes from
    // another process and believing it would be a request to allocate four
    // gigabytes on demand.
    let length = u32::from_ne_bytes(header) as usize;
    if length > protocol::MAX_MESSAGE_BYTES {
        return Frame::TooLarge;
    }

    let mut body = vec![0u8; length];
    match input.read_exact(&mut body) {
        Ok(()) => Frame::Body(body),
        Err(_) => Frame::Eof,
    }
}

fn send(output: &mut impl Write, response: &Response) -> std::io::Result<()> {
    // A `Response` is plain data and cannot fail to serialise; the literal is
    // there so that a future field which somehow could still leaves the
    // extension with an answer instead of a silent pipe.
    let body = serde_json::to_vec(response).unwrap_or_else(|_| {
        br#"{"v":1,"ok":false,"error":{"code":"internal","message":"the app could not answer"}}"#
            .to_vec()
    });

    output.write_all(&(body.len() as u32).to_ne_bytes())?;
    output.write_all(&body)?;
    output.flush()
}

fn handle(request: Request) -> Response {
    let version = match &request {
        Request::Push(push) => push.peer.v,
        Request::Download(download) | Request::Probe(download) => download.peer.v,
        Request::Status(peer)
        | Request::Claim(peer)
        | Request::Forget(peer)
        | Request::OpenApp(peer) => peer.v,
    };

    // An extension updates itself and this app does not, so the pairing that
    // breaks in the field is a new extension against an old app. Saying which
    // half is behind turns a dead popup into one instruction.
    if version > protocol::WIRE_VERSION {
        return Response::err(
            ErrorCode::Version,
            "this extension is newer than the installed app; update Universal Downloader",
        );
    }

    let state = LinkState::read();

    match request {
        // Status changes nothing and is answered whatever the toggle says: the
        // popup's job at that moment is to explain why the link is not working,
        // and `enabled: false` in a successful status does that, where an error
        // would leave it guessing.
        Request::Status(peer) => Response::ok(status(&state, &peer)),

        // Not gated on the toggle either. Refusing to raise a window is not
        // enforcement of anything, and the window is where the toggle lives.
        Request::OpenApp(peer) => {
            open_app(&[]);
            Response::ok(status(&state, &peer))
        }

        // Nor is a download. The toggle is about lending the app a YouTube
        // session, and a link the user pressed Download on lends nothing: the
        // app fetches it as it would one pasted into its own window.
        Request::Download(download) => hand_over(&state, &download),

        // Nor is asking what a download would fetch, for the same reason. The
        // analysis it runs is the download's own, so it leans on a stored
        // session exactly when the download would, toggle and all.
        Request::Probe(download) => probe(&state, &download),

        // Nor is deleting. A user who turns the link off in the app and then
        // presses Forget in the popup is asking for the same thing twice, and
        // the second one must not be the one that fails.
        Request::Forget(peer) => match LinkState::forget() {
            Ok(()) => Response::ok(status(&LinkState::read(), &peer)),
            Err(err) => {
                log_warn!("bridge-host", "could not forget the session: {err}");
                Response::err(ErrorCode::Internal, "the app could not forget the session")
            }
        },

        Request::Claim(peer) => {
            if !state.enabled {
                return disabled();
            }
            match LinkState::claim(&peer) {
                Ok(()) => Response::ok(status(&LinkState::read(), &peer)),
                Err(err) => {
                    log_warn!("bridge-host", "could not bind the profile: {err}");
                    Response::err(
                        ErrorCode::Internal,
                        "the app could not connect this profile",
                    )
                }
            }
        }

        Request::Push(mut push) => {
            let answer = accept(&state, &push);
            // Whatever happened, this process is done with the values.
            scrub(&mut push);
            answer
        }
    }
}

/// The one request that stores anything, and so the one with rules.
fn accept(state: &LinkState, push: &Push) -> Response {
    // The toggle is enforced here rather than in the app, because the app is
    // usually not running when a push arrives. A setting that only takes effect
    // while the window is open is not a setting.
    if !state.enabled {
        return disabled();
    }

    // A push never takes the binding -- not from a second profile, and not
    // from no profile at all.
    //
    // The first half stops a work Chrome and a personal one overwriting each
    // other's jar in turn, which reaches the user as members-only downloads
    // that work some days and not others rather than as an error. The second
    // half is what makes turning the link off stick: Forget clears the
    // binding, and if an unbound profile could bind by pushing, the cookie
    // change a few seconds later would quietly undo it. Only Claim binds, and
    // Claim is a button someone pressed.
    if !state.is_bound_to(&push.peer.profile_id) {
        return Response::err(
            ErrorCode::Unpaired,
            if state.bound.is_some() {
                "another browser profile is connected to Universal Downloader"
            } else {
                "no browser is connected to Universal Downloader yet"
            },
        );
    }

    match LinkState::accept_push(push) {
        Ok(()) => Response::ok(status(&LinkState::read(), &push.peer)),
        Err(err) => {
            log_warn!("bridge-host", "could not store the session: {err}");
            Response::err(ErrorCode::Internal, "the app could not store the session")
        }
    }
}

/// Leave the link in the app's inbox and start the app to take it.
///
/// The inbox rather than the command line: a command line is split on `|` by
/// the single-instance plugin, capped in length, and readable by every other
/// program the user runs, and a signed media address is none of their
/// business. The argument only says that something is waiting.
fn hand_over(state: &LinkState, download: &Download) -> Response {
    let handoff = match handoff::validate(download, chrono::Utc::now().timestamp()) {
        Ok(handoff) => handoff,
        Err(reason) => return Response::err(ErrorCode::Malformed, reason),
    };

    if let Err(err) = handoff::deposit(&handoff) {
        log_warn!("bridge-host", "could not leave a link for the app: {err}");
        return Response::err(ErrorCode::Internal, "the app could not take the link");
    }

    open_app(&[handoff::LAUNCH_ARG]);
    Response::ok(status(state, &download.peer))
}

/// How long a probe may take, start to answer.
///
/// A YouTube link read with a session after the first look was refused is two
/// engine runs, which is the slowest honest case and takes well under this. A
/// site that has not answered by then is not going to give the popup anything
/// worth having waited for.
const PROBE_TIMEOUT: Duration = Duration::from_secs(25);

/// Say what pressing Download on this link would fetch, without fetching it.
///
/// The answer has to be the app's, not a guess made in the browser, so it is
/// worked out from the very request the app would queue for this link
/// (`Handoff::to_request`, with the user's own defaults) by the code the
/// download runs (`downloader::preview`). Nothing is left in the inbox and
/// the app is not started: the settings are read without opening the
/// database for writing, and the only programs run are the ones an analysis
/// runs.
fn probe(state: &LinkState, download: &Download) -> Response {
    let handoff = match handoff::validate(download, chrono::Utc::now().timestamp()) {
        Ok(handoff) => handoff,
        Err(reason) => return Response::err(ErrorCode::Malformed, reason),
    };

    let mut settings = paths::database_path()
        .map(|path| db::load_settings_read_only(&path))
        .unwrap_or_default();
    // A preview never borrows the browser's YouTube session. A download does,
    // for the one run that needs it, by writing the cookies out in plain text
    // and deleting them when the engine is done -- and this process is the
    // browser's to kill at any moment, which would leave that file behind
    // until the app next starts. A members-only video shows no details in the
    // popup rather than risk that; pressing İndir still downloads it with the
    // session.
    settings.browser_link_enabled = false;
    // The pipeline logs what it does where the app would, when the user has
    // asked the app to.
    logging::set_debug_enabled(settings.debug_logging);
    let request = handoff.to_request(&settings);

    let Some(runtime) = runtime() else {
        log_warn!("bridge-host", "could not start the runtime a preview runs on");
        return Response::err(ErrorCode::Internal, "the app could not look at the link");
    };
    let outcome = runtime.block_on(async {
        tokio::time::timeout(PROBE_TIMEOUT, look(&request, &settings)).await
    });

    match outcome {
        Ok(Ok(preview)) => Response::previewed(status(state, &download.peer), preview.into()),
        Ok(Err(err)) => refused(&err),
        Err(_) => {
            log_debug!("bridge-host", "a preview ran out of time");
            Response::err(ErrorCode::Unavailable, "the link took too long to read")
        }
    }
}

/// Find the tools an analysis runs, then analyse and plan.
async fn look(request: &DownloadRequest, settings: &Settings) -> AppResult<DownloadPreview> {
    tools::discover_for_analysis(settings).await;
    downloader::preview(request, settings).await
}

/// Why there is no preview, said without the link.
///
/// A protected service is the one refusal told apart, because it is the one
/// that changes what the popup offers. Everything else reaches the popup as
/// "no details", with the app's own short title for what went wrong. That
/// title is fixed text: the error's own detail can quote the address, and the
/// reply goes back into the browser.
fn refused(err: &AppError) -> Response {
    log_debug!("bridge-host", "no preview: {}", err.code());
    let code = match err {
        AppError::Protected(_) => ErrorCode::Protected,
        _ => ErrorCode::Unavailable,
    };
    Response::err(code, err.to_info().title)
}

/// The runtime previews run on: one for the life of the process, made by the
/// first probe.
///
/// One rather than one per probe, because the HTTP client the analysis uses is
/// kept between calls, and the connections it pools belong to the runtime
/// they were opened on -- a second probe on a fresh runtime would find them
/// dead. Never dropped, either: dropping a runtime waits for its blocking
/// threads, and this process must not outlive its last answer waiting on a
/// name lookup nobody needs. Returning from `main` ends them all.
fn runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
}

fn disabled() -> Response {
    Response::err(
        ErrorCode::Disabled,
        "the browser link is turned off in Universal Downloader",
    )
}

/// What the popup is told. Read the header of `protocol.rs` before adding
/// anything: no reply from this process may describe cookie contents.
fn status(state: &LinkState, peer: &Peer) -> HostStatus {
    let ours = state.is_bound_to(&peer.profile_id);

    HostStatus {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        host_path: std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        enabled: state.enabled,
        bound: ours,
        // Which browser and profile hold the binding is what lets an unpaired
        // popup say "your other Chrome has it" instead of just refusing. The
        // account hint is not that: it is about the person signed in, and a
        // profile that does not hold the binding has no business learning it.
        bound_browser: state.bound.as_ref().map(|bound| bound.browser),
        bound_profile_label: state
            .bound
            .as_ref()
            .and_then(|bound| bound.profile_label.clone()),
        session: state.session_state(),
        account_hint: if ours {
            state.account_hint.clone()
        } else {
            None
        },
        last_push_at: state.last_push_at,
        can_download: true,
    }
}

/// Raise the app's window by starting the app with `args`.
///
/// There is no need for anything cleverer: the desktop build registers
/// `tauri-plugin-single-instance`, so a second launch never becomes a second
/// app -- it hands its arguments to the copy already running, which shows its
/// window and exits. If no copy is running, the user gets the app they asked
/// for. One spawn covers both.
fn open_app(args: &[&str]) {
    let Some(exe) = app_exe() else {
        log_warn!("bridge-host", "could not find the app beside the helper");
        return;
    };

    let mut command = std::process::Command::new(exe);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

        // Chrome puts its native messaging hosts in a job object and closes the
        // job when the port does, which would take a freshly started app down
        // with this process moments after the user asked for it. Breaking away
        // is refused outright by some jobs rather than ignored, so the plain
        // spawn stays as a fallback: a window that might close is better than
        // no window at all.
        command.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
        if command.spawn().is_ok() {
            return;
        }
        command.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }

    if let Err(err) = command.spawn() {
        log_warn!("bridge-host", "could not start the app: {err}");
    }
}

/// The app sits beside this helper, under one of two names: the bundler renames
/// the executable to the product name, while a development build keeps the one
/// cargo gave it. An installed copy may also keep resources one level down.
fn app_exe() -> Option<PathBuf> {
    const NAMES: [&str; 3] = [
        "Universal Downloader.exe",
        "universal-downloader.exe",
        "universal-downloader",
    ];

    let here = std::env::current_exe().ok()?;
    let dir = here.parent()?;

    let mut candidates: Vec<PathBuf> = NAMES.iter().map(|name| dir.join(name)).collect();
    if let Some(parent) = dir.parent() {
        candidates.extend(NAMES.iter().map(|name| parent.join(name)));
    }

    candidates.into_iter().find(|path| path.is_file())
}

/// Overwrite cookie values before they drop.
///
/// Hygiene against this process leaving a jar in a crash dump or a page file,
/// and nothing more: serde already built and freed its own copies of these
/// strings on the way in, and anyone able to attach a debugger has long since
/// won. It costs a loop over a few dozen short strings, which is why it is here
/// at all.
fn scrub(push: &mut Push) {
    for cookie in &mut push.cookies {
        // Zero bytes are valid UTF-8, so the string stays well formed.
        unsafe { cookie.value.as_bytes_mut() }.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(body: serde_json::Value) -> Request {
        serde_json::from_value(body).unwrap()
    }

    fn code(response: &Response) -> Option<ErrorCode> {
        response.error.as_ref().map(|error| error.code)
    }

    /// The version is checked before anything is read or written, for a
    /// download as for everything else.
    #[test]
    fn a_download_from_a_newer_extension_asks_for_an_update() {
        let newer = request(serde_json::json!({
            "type": "download", "v": 2, "profileId": "p", "extensionVersion": "9.0.0",
            "url": "https://cdn.example/a.mp4",
        }));
        assert_eq!(code(&handle(newer)), Some(ErrorCode::Version));
    }

    /// Refused before the inbox or the app are touched.
    #[test]
    fn a_link_that_is_not_a_web_address_is_malformed() {
        let Request::Download(download) = request(serde_json::json!({
            "type": "download", "v": 1, "profileId": "p", "extensionVersion": "1.0.4",
            "url": "javascript:alert(1)",
        })) else {
            panic!("not read as a download");
        };
        let response = hand_over(&LinkState::default(), &download);
        assert!(!response.ok);
        assert_eq!(code(&response), Some(ErrorCode::Malformed));
    }

    /// A probe is held to the same version rule as everything else, so a
    /// newer extension's question is answered with "update the app" rather
    /// than with an analysis of something it may mean differently.
    #[test]
    fn a_probe_from_a_newer_extension_asks_for_an_update() {
        let newer = request(serde_json::json!({
            "type": "probe", "v": 2, "profileId": "p", "extensionVersion": "9.0.0",
            "url": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        }));
        let response = handle(newer);
        assert_eq!(code(&response), Some(ErrorCode::Version));
        assert!(response.preview.is_none());
    }

    /// Refused before settings are read, a tool is looked for or a process is
    /// started.
    #[test]
    fn a_probe_of_a_link_that_is_not_a_web_address_is_malformed() {
        let Request::Probe(probed) = request(serde_json::json!({
            "type": "probe", "v": 1, "profileId": "p", "extensionVersion": "1.0.4",
            "url": "file:///C:/Windows/win.ini", "kind": "video",
        })) else {
            panic!("not read as a probe");
        };
        let response = probe(&LinkState::default(), &probed);
        assert!(!response.ok);
        assert_eq!(code(&response), Some(ErrorCode::Malformed));
        assert!(response.preview.is_none());
    }

    #[test]
    fn only_a_protected_service_is_told_apart_and_no_refusal_quotes_the_link() {
        let link = "https://site.example/watch/secret-id";
        let protected = refused(&AppError::Protected("Netflix".into()));
        assert_eq!(code(&protected), Some(ErrorCode::Protected));

        for err in [
            AppError::InvalidUrl(format!("not an http(s) address: {link}")),
            AppError::Unsupported(format!("no provider could read {link}")),
            AppError::NotFound { status: 404, detail: link.into() },
            AppError::Network(format!("dns error for {link}")),
            AppError::Engine(format!("ERROR: [generic] {link}: unable to download")),
            AppError::EngineMissing,
        ] {
            let response = refused(&err);
            assert_eq!(code(&response), Some(ErrorCode::Unavailable), "{err:?}");
            let text = serde_json::to_string(&response).unwrap();
            assert!(!text.contains("site.example"), "{text}");
        }
    }
}
