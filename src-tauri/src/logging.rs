//! A deliberately small file logger.
//!
//! Production runs write warnings and errors only; debug logging is opt-in from
//! Settings. Logs rotate by size so a long-running install cannot fill a disk.
//!
//! It also owns the redactor. Since the browser link, the engine is sometimes
//! handed a real Google session, and everything the engine says or is told can
//! end up in a log file or -- worse -- behind the error card's Copy button,
//! which is what people paste into public issue trackers. The cut therefore
//! happens in one place that every producer of engine-shaped text runs through.

use std::fmt::Arguments;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const KEEP_ROTATIONS: usize = 2;

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static WRITE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO ",
            Self::Warn => "WARN ",
            Self::Error => "ERROR",
        }
    }

    /// Errors get their own file so a support request does not require sifting
    /// through routine chatter.
    fn file_name(self) -> &'static str {
        match self {
            Self::Error => "error.log",
            _ => "app.log",
        }
    }
}

pub fn set_debug_enabled(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

pub fn log(level: Level, target: &str, args: Arguments<'_>) {
    if level == Level::Debug && !debug_enabled() {
        return;
    }
    if level < Level::Warn && !debug_enabled() && level != Level::Info {
        return;
    }

    let line = format!(
        "{} [{}] {}: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        level.tag(),
        target,
        args
    );

    if cfg!(debug_assertions) {
        eprint!("{line}");
    }

    let Ok(dir) = crate::paths::logs_dir() else {
        return;
    };
    let path = dir.join(level.file_name());

    let _guard = WRITE_LOCK.lock();
    rotate_if_needed(&path);

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn rotate_if_needed(path: &PathBuf) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() < MAX_BYTES {
        return;
    }

    // app.log -> app.log.1 -> app.log.2, dropping whatever falls off the end.
    for index in (1..=KEEP_ROTATIONS).rev() {
        let from = if index == 1 {
            path.clone()
        } else {
            path.with_extension(format!(
                "{}.{}",
                path.extension().and_then(|e| e.to_str()).unwrap_or("log"),
                index - 1
            ))
        };
        let to = path.with_extension(format!(
            "{}.{}",
            path.extension().and_then(|e| e.to_str()).unwrap_or("log"),
            index
        ));
        let _ = std::fs::rename(from, to);
    }
}

// -- redaction -----------------------------------------------------------------

/// What replaces anything that could be part of a session.
const MARK: &str = "[redacted]";

/// Punctuation that surrounds a token without being part of it: quotes from a
/// debug-formatted vector, the comma between arguments, the semicolon between
/// cookie pairs.
const EDGES: &[char] = &['"', '\'', ',', '[', ']', '(', ')', ';', ':'];

/// Cut anything session-shaped out of engine output before it is written.
///
/// Three things go: the jar handed to `--cookies`, any `NAME=VALUE` or
/// `NAME<TAB>VALUE` naming a cookie that carries a Google sign-in, and every
/// line that still mentions a lease file after the first two have run. That
/// order matters -- masking the `--cookies` value is what stops the argument
/// vector, which is genuinely useful in a bug report, being thrown away whole
/// by the third rule.
pub fn redact(text: &str) -> String {
    let mut out = text
        .lines()
        .map(redact_line)
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The argument vector as it is safe to write down.
///
/// Redacting the vector rather than its debug form is what makes the jar's
/// path recognisable: once `{:?}` has escaped the backslashes, a Windows path
/// is no longer a path that `is_lease_path` would know.
pub fn redact_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut jar_next = false;
    for arg in args {
        if jar_next {
            out.push(MARK.to_string());
        } else {
            out.push(redact(arg));
        }
        jar_next = arg == "--cookies";
    }
    out
}

fn redact_line(line: &str) -> String {
    let masked = mask_tokens(line);

    // A line that still names a lease file is a line about the jar itself --
    // the engine quoting what it failed to parse, say. There is nothing in
    // such a line worth keeping.
    let names_a_lease = masked
        .split_whitespace()
        .any(|token| crate::bridge::is_lease_path(Path::new(token.trim_matches(EDGES))));
    if names_a_lease {
        return format!("{MARK} (browser session)");
    }

    masked
}

/// Walk the line token by token, preserving its spacing, masking a jar path
/// introduced by `--cookies` and any value that follows a sensitive name.
fn mask_tokens(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut previous = String::new();
    let mut jar_next = false;
    // Once a bare session-cookie name has been seen, the rest of the line is
    // its value. Cookie values may contain spaces, so masking only the token
    // that follows the name would leave the tail of one readable.
    let mut value_follows = false;
    let mut rest = line;

    while !rest.is_empty() {
        let text = rest.trim_start();
        out.push_str(&rest[..rest.len() - text.len()]);
        if text.is_empty() {
            break;
        }

        let end = text.find(char::is_whitespace).unwrap_or(text.len());
        let (token, tail) = text.split_at(end);
        rest = tail;

        let bare = token.trim_matches(EDGES);
        // `--cookies` also appears in the engine's own advice to the user
        // ("Use --cookies for the authentication"), where the next word is
        // prose, not a file. Only something path-shaped is treated as the jar.
        let is_jar = jar_next && (bare.contains('/') || bare.contains('\\'));
        let is_value = value_follows || is_sensitive_name(&previous);
        if is_jar || is_value {
            out.push_str(MARK);
            value_follows = is_value;
        } else {
            out.push_str(&mask_fields(token));
        }

        jar_next = bare == "--cookies";
        previous.clear();
        previous.push_str(bare);
    }

    out
}

/// A cookie row whose tabs arrived escaped.
///
/// A Netscape line that reached a log through `{:?}` carries `\t` as two
/// characters rather than as whitespace, so the walk above sees the whole row
/// as one token: no spaces to split on and no `=` to find, and the value comes
/// through untouched. Splitting on the escape as well, and masking every field
/// after a sensitive name, closes that -- in this format the value is what
/// follows the name.
fn mask_fields(token: &str) -> String {
    const ESCAPED_TAB: &str = "\\t";

    if !token.contains(ESCAPED_TAB) {
        return mask_pairs(token);
    }

    let mut after_name = false;
    token
        .split(ESCAPED_TAB)
        .map(|field| {
            if after_name {
                return MARK.to_string();
            }
            after_name = is_sensitive_name(field.trim_matches(EDGES));
            mask_pairs(field)
        })
        .collect::<Vec<_>>()
        .join(ESCAPED_TAB)
}

/// `SID=value`, including several of them separated by semicolons inside one
/// token, as a `Cookie:` header or a parse error quotes them.
fn mask_pairs(token: &str) -> String {
    if !token.contains('=') {
        return token.to_string();
    }

    token
        .split_inclusive(';')
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) if is_sensitive_name(name.trim_matches(EDGES)) => {
                let separator = if value.ends_with(';') { ";" } else { "" };
                format!("{name}={MARK}{separator}")
            }
            _ => pair.to_string(),
        })
        .collect()
}

fn is_sensitive_name(name: &str) -> bool {
    crate::bridge::SENSITIVE_COOKIE_NAMES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(name))
}

#[macro_export]
macro_rules! log_debug {
    ($target:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Debug, $target, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Info, $target, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Warn, $target, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::Level::Error, $target, format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session value in this string would be a working Google login for
    /// anyone reading the issue it was pasted into.
    const SESSION: &str = "g.a000zQh7nSxVb1kR9fJmWq4tLpY8dCvN3eH5aZ2oXbKtP";

    #[test]
    fn a_session_value_never_survives_a_stderr_line() {
        let stderr = format!(
            "ERROR: [youtube] dQw4w9WgXcQ: Unable to parse cookie __Secure-3PSID={SESSION}; Domain=.youtube.com; Path=/"
        );
        let safe = redact(&stderr);

        assert!(!safe.contains(SESSION), "the value survived: {safe}");
        // The name stays: which cookies the jar held is the diagnostic half of
        // the line, and it is not the half that compromises anyone.
        assert!(safe.contains("__Secure-3PSID=[redacted]"), "{safe}");
        assert!(safe.contains("dQw4w9WgXcQ"), "{safe}");
    }

    #[test]
    fn a_netscape_row_loses_its_value_and_keeps_its_shape() {
        let row = format!(".youtube.com\tTRUE\t/\tTRUE\t1800000000\tSID\t{SESSION}");
        let safe = redact(&row);

        assert!(!safe.contains(SESSION), "{safe}");
        assert!(safe.contains("\tSID\t[redacted]"), "{safe}");
    }

    /// The same row, but reached the log through `{:?}`: its tabs are two
    /// characters each, so it arrives as one word with no `=` in it. That shape
    /// once came through the redactor byte for byte.
    #[test]
    fn a_row_whose_tabs_arrived_escaped_still_loses_its_value() {
        let row = format!(".youtube.com\\tTRUE\\t/\\tTRUE\\t1800000000\\tSID\\t{SESSION}");
        let safe = redact(&row);

        assert!(!safe.contains(SESSION), "{safe}");
        assert!(safe.contains("SID"), "{safe}");
    }

    /// A cookie value is allowed to contain spaces, and masking only the one
    /// word after the name would leave the rest of it readable.
    #[test]
    fn a_value_with_spaces_is_masked_to_the_end_of_the_line() {
        let row = format!("LOGIN_INFO {SESSION} tail-of-the-same-value");
        let safe = redact(&row);

        assert!(!safe.contains(SESSION), "{safe}");
        assert!(!safe.contains("tail-of-the-same-value"), "{safe}");
    }

    #[test]
    fn the_jar_handed_to_the_engine_is_not_written_beside_the_url() {
        let args = [
            "--ignore-config".to_string(),
            "--cookies".to_string(),
            "C:/Users/someone/AppData/Roaming/UniversalDownloader/bridge/leases/7f3a.txt".to_string(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
        ];
        let safe = redact_args(&args);

        assert_eq!(safe[1], "--cookies");
        assert_eq!(safe[2], MARK);
        // Everything else is what makes the log worth having.
        assert_eq!(safe[0], "--ignore-config");
        assert_eq!(safe[3], "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    }

    #[test]
    fn a_line_that_quotes_the_jar_goes_entirely() {
        let line = "WARNING: could not read /home/someone/.config/UniversalDownloader/bridge/leases/7f3a.txt: SID\tvalue";
        assert_eq!(redact(line), "[redacted] (browser session)");
    }

    #[test]
    fn the_engines_own_advice_about_cookies_is_left_readable() {
        let stderr = "ERROR: [vimeo] 76979871: The web client only works when logged-in. Use --cookies, --cookies-from-browser, --username and --password to provide account credentials";
        assert_eq!(redact(stderr), stderr);
    }

    #[test]
    fn ordinary_engine_output_is_returned_untouched() {
        let stderr = "ERROR: [youtube] abc: Video unavailable\nWARNING: No video formats found!\n";
        assert_eq!(redact(stderr), stderr);
    }
}
