//! A deliberately small file logger.
//!
//! Production runs write warnings and errors only; debug logging is opt-in from
//! Settings. Logs rotate by size so a long-running install cannot fill a disk.

use std::fmt::Arguments;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
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
