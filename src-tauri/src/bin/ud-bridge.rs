//! The browser link's native messaging host.
//!
//! Chrome starts this program, speaks to it over stdin and stdout, and kills it
//! when the extension's port closes. It exists so a browser can hand the app a
//! session the app is not allowed to read for itself: Chrome 127 and later
//! encrypt the cookie database with a key bound to the browser's own
//! executable, so reading it from outside is no longer possible -- and should
//! not be. An extension the user installed, pushing through a channel the
//! browser owns, is the honest version of the same thing.
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

use universal_downloader_lib::bridge::protocol::{
    self, ErrorCode, HostStatus, Peer, Push, Request, Response,
};
use universal_downloader_lib::bridge::LinkState;
use universal_downloader_lib::log_warn;

fn main() {
    // Chrome passes the calling extension's origin as the first argument. This
    // is authentication of the *caller*, by the browser, and it is worth being
    // precise about what it proves: that the browser started us for that
    // extension. It says nothing about this host being the one the user meant
    // to run -- that is what the registry values and the path shown in the
    // popup are for.
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
            open_app();
            Response::ok(status(&state, &peer))
        }

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
    }
}

/// Raise the app's window by starting the app.
///
/// There is no need for anything cleverer: the desktop build registers
/// `tauri-plugin-single-instance`, so a second launch never becomes a second
/// app -- it hands its arguments to the copy already running, which shows its
/// window and exits. If no copy is running, the user gets the app they asked
/// for. One spawn covers both.
fn open_app() {
    let Some(exe) = app_exe() else {
        log_warn!("bridge-host", "could not find the app beside the helper");
        return;
    };

    let mut command = std::process::Command::new(exe);
    command
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
