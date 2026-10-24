//! The run seam (split out of exec.rs 2026-10-16, size pass): the
//! injectable `RunRt` surface the differential harness scripts whole
//! runs through, and the real implementation behind it - the spawn with
//! piped stdio, the raw KeyWatch escape reader, the kill tree, and the
//! Ctrl+C scope for the duration of one download call. Text moved
//! verbatim; behaviour unchanged.

use crate::term;
use crate::tty;
use std::io::BufRead;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The child process surface one run uses, split like python's
/// `proc.stdout` / `proc.stderr` / `proc.poll()` / `proc.wait()`.
pub struct Spawned {
    pub stdout: Box<dyn BufRead + Send>,
    pub stderr: Box<dyn BufRead + Send>,
    pub child: Box<dyn ChildHandle>,
}

pub trait ChildHandle: Send {
    fn poll(&mut self) -> Option<i64>;
    fn wait(&mut self) -> i64;
    fn pid(&self) -> u32;
}

pub trait EscapeWatch {
    fn escape_hit(&mut self) -> bool;
}

/// The injectable process surface: the real instance spawns yt-dlp with
/// the posix `start_new_session=True` kill target and watches the raw
/// KeyWatch; the differential arm scripts spawn/poll/escape/kill.
pub type SpawnFn = Box<dyn FnMut(&str, &[String]) -> std::io::Result<Spawned>>;
pub type WatchFn = Box<dyn FnMut() -> Box<dyn EscapeWatch>>;
pub type KillFn = Box<dyn FnMut(&mut dyn ChildHandle)>;

pub struct RunRt {
    pub spawn: SpawnFn,
    pub watch: WatchFn,
    pub kill: KillFn,
    pub interrupted: Box<dyn FnMut() -> bool>,
    pub sleep_frame: Box<dyn FnMut()>,
    pub interrupt_scope: Option<InterruptScope>,
}

struct RealChild {
    child: std::process::Child,
}

/// Python reads the windows exit code unsigned; a signal death is the
/// negative signal number, like `Popen.returncode`.
pub fn exit_code(status: &std::process::ExitStatus) -> i64 {
    match status.code() {
        Some(code) => {
            #[cfg(windows)]
            {
                i64::from(code as u32)
            }
            #[cfg(not(windows))]
            {
                i64::from(code)
            }
        }
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                -(i64::from(status.signal().unwrap_or(0)))
            }
            #[cfg(not(unix))]
            {
                0
            }
        }
    }
}

impl ChildHandle for RealChild {
    fn poll(&mut self) -> Option<i64> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(exit_code(&status)),
            _ => None,
        }
    }

    fn wait(&mut self) -> i64 {
        match self.child.wait() {
            Ok(status) => exit_code(&status),
            Err(_) => -1,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }
}

struct RealWatch(tty::KeyWatch);

impl EscapeWatch for RealWatch {
    fn escape_hit(&mut self) -> bool {
        self.0.escape_hit()
    }
}

/// `subprocess.Popen([binary] + opts, stdout=PIPE, stderr=PIPE)` plus
/// `start_new_session=True` on posix (python builds the same argv; rust
/// keeps stdin inherited, like python's default).
fn spawn_real(binary: &str, opts: &[String]) -> std::io::Result<Spawned> {
    use std::process::Stdio;
    let mut cmd = Command::new(binary);
    cmd.args(opts);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // the kill target of `os.killpg(os.getpgid(pid))`; setsid and
        // setpgid(0,0) give the same pgroup for signal purposes
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("no stdout pipe"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("no stderr pipe"))?;
    Ok(Spawned {
        stdout: Box::new(std::io::BufReader::new(stdout)),
        stderr: Box::new(std::io::BufReader::new(stderr)),
        child: Box::new(RealChild { child }),
    })
}

/// python `_kill_tree(proc)`: taskkill /F /T on windows, killpg SIGKILL
/// on posix, then wait (FR-11).
fn kill_tree_real(child: &mut dyn ChildHandle) {
    let pid = child.pid();
    #[cfg(windows)]
    {
        use std::process::Stdio;
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    unsafe {
        let pgid = libc::getpgid(pid as i32);
        if pgid >= 0 {
            libc::killpg(pgid, libc::SIGKILL);
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
    }
    child.wait();
}

static INTERRUPT_SEEN: AtomicBool = AtomicBool::new(false);

/// Whether a Ctrl+C has been seen since the last download run armed the
/// handler. The updater's `-U` child inherits the terminal, so python's
/// `except KeyboardInterrupt` around `subprocess.call` is checked the
/// same way here.
pub fn interrupt_pending() -> bool {
    INTERRUPT_SEEN.load(Ordering::SeqCst)
}

/// Set or clear the Ctrl+C flag without a real signal: the differential
/// harness scripts a Ctrl+C during the updater's `-U` child, and there
/// is no console to press one on.
pub fn set_interrupt(seen: bool) {
    INTERRUPT_SEEN.store(seen, Ordering::SeqCst);
}

#[cfg(unix)]
extern "C" fn sigint_handler(_sig: libc::c_int) {
    INTERRUPT_SEEN.store(true, Ordering::SeqCst);
}

/// The console handler claims only a Ctrl+C or Ctrl+Break (flag + 1):
/// every other event answers 0 so the default termination proceeds
/// instead of being swallowed mid-download.
#[cfg(windows)]
unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    if ctrl_type == CTRL_C_EVENT || ctrl_type == CTRL_BREAK_EVENT {
        INTERRUPT_SEEN.store(true, Ordering::SeqCst);
        1
    } else {
        0
    }
}

/// Ctrl+C during a download run: python raises KeyboardInterrupt in its
/// poll loop; the port sets a flag from the SIGINT / console handler and
/// the loop lands in the same `cancelled` branch. Installed for the
/// duration of one download call and restored on drop (python's handler
/// is the interpreter default, so the terminal mode restore the
/// `finally` performs stays the only cleanup needed).
pub struct InterruptScope {
    #[cfg(unix)]
    previous: libc::sigaction,
}

impl InterruptScope {
    pub fn install() -> Self {
        INTERRUPT_SEEN.store(false, Ordering::SeqCst);
        #[cfg(unix)]
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = sigint_handler as *const () as usize;
            action.sa_flags = 0;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGINT, &action, &mut previous);
            Self { previous }
        }
        #[cfg(windows)]
        {
            unsafe {
                windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(ctrl_handler), 1);
            }
            Self {}
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self {}
        }
    }
}

impl Drop for InterruptScope {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut());
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(ctrl_handler), 0);
        }
    }
}

impl RunRt {
    /// The production runtime: a real spawn, a raw KeyWatch, the real
    /// kill tree, the Ctrl+C flag and the frame tick sleep.
    pub fn real() -> Self {
        Self {
            spawn: Box::new(spawn_real),
            watch: Box::new(|| Box::new(RealWatch(tty::KeyWatch::new())) as Box<dyn EscapeWatch>),
            kill: Box::new(kill_tree_real),
            interrupted: Box::new(interrupt_pending),
            sleep_frame: Box::new(|| std::thread::sleep(Duration::from_secs_f64(term::FRAME_SECS))),
            interrupt_scope: Some(InterruptScope::install()),
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT};

    #[test]
    fn ctrl_handler_claims_only_c_and_break() {
        unsafe {
            set_interrupt(false);
            assert_eq!(ctrl_handler(CTRL_C_EVENT), 1);
            assert!(interrupt_pending(), "a ctrl+c sets the flag");
            set_interrupt(false);
            assert_eq!(ctrl_handler(CTRL_BREAK_EVENT), 1);
            assert!(interrupt_pending(), "a ctrl+break sets the flag");
            set_interrupt(false);
            assert_eq!(ctrl_handler(CTRL_CLOSE_EVENT), 0);
            assert!(!interrupt_pending(), "other events stay default");
        }
    }
}
