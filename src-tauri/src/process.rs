//! Helpers for running the external tools.
//!
//! Two rules hold everywhere in this module:
//!   1. Arguments are always passed as a vector, never interpolated into a
//!      shell string. No user-supplied URL ever reaches a command line as text
//!      that could be re-parsed.
//!   2. Child processes are spawned without a console window, so a download
//!      never flashes a black box over the user's screen.

use std::process::Stdio;

use tokio::process::Command;

use crate::error::{AppError, AppResult};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Build a `Command` that will not pop a console window on Windows.
pub fn command(program: &std::path::Path) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.stdin(Stdio::null());
    cmd
}

pub struct CapturedOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CapturedOutput {
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }
}

/// Run a tool to completion and capture its output.
pub async fn run(program: &std::path::Path, args: &[String]) -> AppResult<CapturedOutput> {
    let output = command(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| AppError::Other(format!("could not start {}: {err}", program.display())))?;

    Ok(CapturedOutput {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// `run`, bounded: a tool that has not finished within `limit` is killed and
/// reported as an error rather than stalling its caller indefinitely.
pub async fn run_with_timeout(
    program: &std::path::Path,
    args: &[String],
    limit: std::time::Duration,
) -> AppResult<CapturedOutput> {
    let output = command(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();

    let output = tokio::time::timeout(limit, output)
        .await
        .map_err(|_| {
            AppError::Other(format!(
                "{} did not respond within {}s",
                program.display(),
                limit.as_secs()
            ))
        })?
        .map_err(|err| AppError::Other(format!("could not start {}: {err}", program.display())))?;

    Ok(CapturedOutput {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Locate an executable on `PATH`. Used to prefer a copy the user already has
/// installed over downloading a second one.
pub fn which(name: &str) -> Option<std::path::PathBuf> {
    let exe = if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    };

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(&exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
