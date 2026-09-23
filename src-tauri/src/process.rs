//! Helpers for running the external tools.
//!
//! Three rules hold everywhere in this module:
//!   1. Arguments are always passed as a vector, never interpolated into a
//!      shell string. No user-supplied URL ever reaches a command line as text
//!      that could be re-parsed.
//!   2. Child processes are spawned without a console window, so a download
//!      never flashes a black box over the user's screen.
//!   3. A tool that starts other tools is killed as a family. `Child::kill`
//!      reaches one process and no further, which is fine for everything that
//!      spawns nothing -- and quietly wrong for yt-dlp, which hands a ranged
//!      fetch to an FFmpeg of its own.
//!
//! On Android a tool is not always the program that gets started: see
//! `android::command`.

use std::process::Stdio;

use tokio::process::{Child, Command};

use crate::error::{AppError, AppResult};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Build a `Command` that will not pop a console window on Windows.
pub fn command(program: &std::path::Path) -> Command {
    #[cfg(target_os = "android")]
    let mut cmd = crate::android::command(program);
    #[cfg(not(target_os = "android"))]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.stdin(Stdio::null());
    cmd
}

/// A child process together with everything it goes on to start.
///
/// Killing a process does not kill its children. For every tool this app ran
/// until the editor could fetch a range that was a distinction without a
/// difference, because none of them started anything. yt-dlp given
/// `--download-sections` does: the transfer goes through an FFmpeg it spawns,
/// and `--downloader native` does not change that. Killing yt-dlp alone there
/// leaves that FFmpeg running, still downloading and still writing to the file
/// the user has just cancelled.
pub struct Tree {
    /// The process itself, for the pipes and the exit status. Nothing here
    /// wraps those: reading output is the caller's business, and a family is
    /// only a different thing from a process at the moment it is killed.
    pub child: Child,
    #[cfg(windows)]
    job: Job,
}

impl Tree {
    /// Kill the child and everything it started, then reap it.
    ///
    /// Reaping matters as much as killing: an unwaited child stays a zombie
    /// holding its handles, and the pipe the caller is reading from never ends.
    pub async fn kill(&mut self) {
        #[cfg(windows)]
        self.job.terminate();
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

/// Spawn a tool as a family that can be killed as one.
///
/// On Windows the family is a job object: every process the child starts joins
/// it, and terminating the job terminates all of them. Elsewhere this is a
/// plain spawn, which is the truth about what it can do rather than a claim it
/// cannot keep -- the one caller that starts a grandchild is the editor's
/// ranged fetch, and the editor is a desktop screen.
pub fn spawn_tree(command: &mut Command) -> AppResult<Tree> {
    let child = command
        .spawn()
        .map_err(|err| AppError::Io(format!("a tool could not be started: {err}")))?;
    Ok(into_tree(child))
}

/// The job is created empty and the child put into it after the fact, because
/// the alternative -- starting the process suspended and resuming it once it is
/// in -- needs the main thread's handle, which nothing in the standard library
/// hands out. The window that leaves is the microseconds between
/// `CreateProcess` returning and the assignment below, and what runs in it is a
/// Python interpreter reading its own arguments, not a downloader.
#[cfg(windows)]
fn into_tree(child: Child) -> Tree {
    let job = Job::create();
    if let Some(handle) = child.raw_handle() {
        job.adopt(handle);
    }
    Tree { child, job }
}

#[cfg(not(windows))]
fn into_tree(child: Child) -> Tree {
    Tree { child }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::RawHandle;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A Windows job object holding one process and its descendants.
    ///
    /// Every call here is allowed to fail and none of them is reported. A job
    /// object is a way of killing a tool more thoroughly, not a way of running
    /// it: a machine that refuses one still downloads, and the cost of the
    /// refusal is the old behaviour rather than no behaviour. Saying so loudly
    /// would put a warning in front of a user about something that has not
    /// gone wrong for them.
    pub struct Job(HANDLE);

    // The handle is owned by this value alone and every use of it below is a
    // call that takes it by value. Nothing is shared and nothing is aliased.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn create() -> Self {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Self(handle);
            }

            // Without this a job outlives the app that made it: a crash between
            // the spawn and the kill would leave the whole family running with
            // nothing left to stop it. With it, the last handle closing is
            // itself the kill.
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    std::ptr::addr_of!(limits).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
            }
            Self(handle)
        }

        pub fn adopt(&self, process: RawHandle) {
            if self.0.is_null() {
                return;
            }
            unsafe {
                AssignProcessToJobObject(self.0, process as HANDLE);
            }
        }

        pub fn terminate(&self) {
            if self.0.is_null() {
                return;
            }
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            if self.0.is_null() {
                return;
            }
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
use job::Job;

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
