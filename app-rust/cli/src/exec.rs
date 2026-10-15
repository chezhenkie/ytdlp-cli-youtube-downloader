//! Process execution (specsheet section 4, layer 5): the frozen failure
//! sentence and the subtitle verdict lines (FR-10, IF-4), the download
//! run itself (FR-7: two pump threads, an escape-only KeyWatch, the kill
//! tree, the 6-line stderr tail), the partial/work-folder cleanup
//! (FR-11) and the subtitle pass (FR-9). The process surface is a seam
//! (`RunRt`), so the differential harness scripts whole runs offline;
//! the real one spawns yt-dlp and never touches the network.

use crate::args;
use crate::config::{self, Obj, Value};
use crate::pytext::{py_repr_str, py_rstrip, py_split_ws, py_strip};
use crate::resolve::{self, ShapeKind};
use crate::term::{self, A_AMBER, ascii_safe, c};
use crate::tty::{self, Out, ReadOutcome, Reader, SharedSink, Stage};
use std::io::BufRead;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// Frozen sentence for a failed run: the rc always, yt-dlp's error line
/// when there is one, full length - the old `err[:WIDTH]` cut kept the
/// error class but dropped the tail (FR-10).
pub fn fail_sentence(rc: i64, err: &str) -> String {
    if err.is_empty() {
        format!("downloading - failed (rc={rc})")
    } else {
        format!("downloading - failed (rc={rc}): {err}")
    }
}

/// Short reason for a failed subtitle pass, classified from yt-dlp's
/// last error line. None when nothing is recognizable - the caller then
/// shows no second line.
pub fn sub_fail_reason(err: Option<&str>) -> Option<String> {
    let low = err.unwrap_or("").to_lowercase();
    if low.contains("429") || low.contains("too many requests") {
        return Some("the subtitle api answered 429 too many requests".to_string());
    }
    if low.contains("timed out") || low.contains("timeout") {
        return Some("the subtitle request timed out".to_string());
    }
    if low.contains("connection") || low.contains("network") || low.contains("unreachable") {
        return Some("the network was unreachable".to_string());
    }
    None
}

/// Verdict lines for a subtitle pass that exited 0: (verdict, miss_note).
/// miss_note is empty when codes were read, else the IF-4 parse-miss line:
/// yt-dlp succeeded but the stdout scrape read nothing, which points at
/// reworded upstream output rather than missing subs.
pub fn sub_success_lines(codes: &[String]) -> (String, String) {
    if !codes.is_empty() {
        let joined = codes
            .iter()
            .map(|code| ascii_safe(code))
            .collect::<Vec<_>>()
            .join(", ");
        (format!("subtitles - {joined}"), String::new())
    } else {
        (
            "subtitles - none available".to_string(),
            "note: yt-dlp reported success but no subtitle files were read \
             - wording may have changed, please report"
                .to_string(),
        )
    }
}

/// Verdict lines for a failed subtitle pass: the not-fetched verdict plus
/// the classified reason, or the IF-4 note when the error line matched no
/// known wording.
pub fn sub_fail_lines(err: Option<&str>) -> Vec<String> {
    let mut lines =
        vec!["subtitles - not fetched (video kept). unable to download subtitles".to_string()];
    match sub_fail_reason(err) {
        Some(reason) => lines.push(format!("subtitle error - {reason}")),
        None => {
            lines.push("subtitle error - yt-dlp wording not recognized - please report".to_string())
        }
    }
    lines
}

// ------------------------------------------------- download support

/// `temp_dir(s)`: the work folder for yt-dlp intermediates, inside the
/// output folder (FR-8: per-chunk fragments, `.part` files, `.ytdl`
/// resume markers and pre-merge files all land here).
pub fn temp_dir(saved: &Obj, auto: Option<&str>, root: &str) -> Result<String, ShapeKind> {
    Ok(resolve::py_path_join(
        &resolve::resolve_outdir(saved, auto, root)?,
        resolve::TEMP_DIR_NAME,
    ))
}

/// `drop_temp_dir(s)`: remove the work folder when it ends empty; a
/// failed run keeps its partials there for resume (FR-11).
pub fn drop_temp_dir(saved: &Obj, auto: Option<&str>, root: &str) {
    if let Ok(dir) = temp_dir(saved, auto, root) {
        let _ = std::fs::remove_dir(dir);
    }
}

/// `newest_since(folder, since, slack)`: newest file with an mtime at or
/// just after `since`, as a path; `None` when the folder is unreadable
/// or holds no candidate. Ties keep the first file read (python's glob
/// order; fixtures keep mtimes distinct).
pub fn newest_since(folder: &str, since: f64, slack: f64) -> Option<String> {
    let entries = std::fs::read_dir(folder).ok()?;
    let mut best: Option<(f64, String)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let text = path.to_string_lossy().into_owned();
        let Some(mtime) = resolve::mtime_seconds(&text) else {
            continue;
        };
        if mtime >= since - slack && best.as_ref().is_none_or(|(bm, _)| mtime > *bm) {
            best = Some((mtime, text));
        }
    }
    best.map(|(_, path)| path)
}

/// One `*.part*` match (python `folder.glob("*.part*")`; fnmatch is
/// case-insensitive on windows).
fn name_is_part(name: &str) -> bool {
    if cfg!(windows) {
        name.to_lowercase().contains(".part")
    } else {
        name.contains(".part")
    }
}

/// One `*.ytdl` match (python `folder.glob("*.ytdl")`).
fn name_is_ytdl(name: &str) -> bool {
    if cfg!(windows) {
        name.to_lowercase().ends_with(".ytdl")
    } else {
        name.ends_with(".ytdl")
    }
}

/// `clean_partials(folder, since, slack)`: delete fresh `*.part*` and
/// `*.ytdl` leftovers after a stopped download; returns the removed
/// names (the caller prints the count; the harness sorts them).
pub fn clean_partials(folder: &str, since: f64, slack: f64) -> Vec<String> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return removed;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name_is_part(&name) || name_is_ytdl(&name)) {
            continue;
        }
        let Some(mtime) = resolve::mtime_seconds(&path.to_string_lossy()) else {
            continue;
        };
        if mtime < since - slack || !path.is_file() {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            removed.push(name);
        }
    }
    removed
}

/// The language code out of a subtitle file name: python's
/// `name.rsplit(".", 2)[-2] if name.count(".") >= 2 else name`.
fn sub_code(name: &str) -> String {
    if name.matches('.').count() >= 2 {
        let parts: Vec<&str> = name.rsplitn(3, '.').collect();
        parts[1].to_string()
    } else {
        name.to_string()
    }
}

// ------------------------------------------------------- the run seam

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
fn exit_code(status: &std::process::ExitStatus) -> i64 {
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

fn interrupt_pending() -> bool {
    INTERRUPT_SEEN.load(Ordering::SeqCst)
}

#[cfg(unix)]
extern "C" fn sigint_handler(_sig: libc::c_int) {
    INTERRUPT_SEEN.store(true, Ordering::SeqCst);
}

#[cfg(windows)]
unsafe extern "system" fn ctrl_handler(_ctrl_type: u32) -> i32 {
    INTERRUPT_SEEN.store(true, Ordering::SeqCst);
    1
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

/// `run_download(binary, opts, stage)`: run yt-dlp with no input; escape
/// stops it and the caller cleans partials. stdout is piped so
/// `[download]` progress lines feed the stage line, subtitle write lines
/// are collected for the subtitle verdict, stderr keeps a 6-line tail
/// for the failure sentence. Returns (rc, stopped, err, sub_codes); the
/// error line is untruncated and already ascii_safe (FR-10).
pub fn run_download(
    binary: &str,
    opts: &[String],
    stage: Option<&Stage>,
    use_color: bool,
    rt: &mut RunRt,
) -> (i64, bool, String, Vec<String>) {
    let spawned = match (rt.spawn)(binary, opts) {
        Ok(spawned) => spawned,
        // python's Popen raise would traceback; the port speaks instead
        // (recorded deviation, unreachable once the probe succeeded)
        Err(e) => return (2, false, format!("could not start yt-dlp: {e}"), Vec::new()),
    };
    let Spawned {
        stdout,
        stderr,
        mut child,
    } = spawned;
    let live = stage.map(Stage::live_handle);

    let (out_tx, out_rx) = mpsc::channel::<Vec<String>>();
    std::thread::spawn(move || {
        let mut codes: Vec<String> = Vec::new();
        let mut reader = stdout;
        let mut buf: Vec<u8> = Vec::with_capacity(512);
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let decoded = String::from_utf8_lossy(&buf);
            let line = py_rstrip(&decoded);
            if let Some(rest) = line.strip_prefix("[download]") {
                // no Destination flash: it would replace the spinner
                let body = py_split_ws(rest).join(" ");
                if !body.is_empty()
                    && !body.starts_with("Destination:")
                    && let Some(handle) = &live
                {
                    let text = if use_color {
                        term::color_progress(&body)
                    } else {
                        body
                    };
                    handle.set(Some(text));
                }
            } else if let Some((_, rest)) = line.split_once("Writing video subtitles to:") {
                codes.push(sub_code(py_strip(rest)));
            }
        }
        let _ = out_tx.send(codes);
    });

    let (err_tx, err_rx) = mpsc::channel::<Vec<String>>();
    std::thread::spawn(move || {
        let mut tail: Vec<String> = Vec::new();
        let mut reader = stderr;
        let mut buf: Vec<u8> = Vec::with_capacity(512);
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let decoded = String::from_utf8_lossy(&buf);
            let line = py_rstrip(&decoded);
            if !line.is_empty() {
                tail.push(line.to_string());
                if tail.len() > 6 {
                    tail.remove(0);
                }
            }
        }
        let _ = err_tx.send(tail);
    });

    enum Finish {
        Rc(i64),
        Stopped,
        Cancelled,
    }
    let mut watch = (rt.watch)();
    let finish = loop {
        if let Some(rc) = child.poll() {
            break Finish::Rc(rc);
        }
        if watch.escape_hit() {
            (rt.kill)(&mut *child);
            break Finish::Stopped;
        }
        if (rt.interrupted)() {
            (rt.kill)(&mut *child);
            break Finish::Cancelled;
        }
        (rt.sleep_frame)();
    };
    drop(watch);
    match finish {
        Finish::Stopped => (-1, true, String::new(), Vec::new()),
        Finish::Cancelled => (-1, false, "cancelled".to_string(), Vec::new()),
        Finish::Rc(rc) => {
            // python joins each pump with a 2.0 s timeout
            let timeout = Duration::from_millis(2000);
            let sub_codes = out_rx.recv_timeout(timeout).unwrap_or_default();
            let tail = err_rx.recv_timeout(timeout).unwrap_or_default();
            let mut err = String::new();
            for line in tail.iter().rev() {
                if line.contains("ERROR") || line.to_lowercase().contains("error") {
                    err = py_strip(line).to_string();
                    break;
                }
            }
            if err.is_empty()
                && let Some(last) = tail.last()
            {
                err = py_strip(last).to_string();
            }
            (rc, false, ascii_safe(&err), sub_codes)
        }
    }
}

// ----------------------------------------------------- ffmpeg offer

/// Where a lean install fetches ffmpeg from (python `FFMPEG_SOURCES`):
/// the windows gyan.dev essentials zip and the linux BtbN tarball, each
/// with the file that carries the published digest; macos has no source
/// here (no free static build is published), so it gets instructions
/// instead of a download (FR-14, IF-7).
pub struct FfmpegSource {
    pub url: &'static str,
    pub sha_url: &'static str,
    pub binname: &'static str,
    pub how: &'static str,
}

pub fn ffmpeg_source(platform: &str) -> Option<FfmpegSource> {
    match platform {
        "windows" => Some(FfmpegSource {
            url: "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
            sha_url: "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256",
            binname: "ffmpeg.exe",
            how: "zip",
        }),
        "linux" => Some(FfmpegSource {
            url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz",
            sha_url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256",
            binname: "ffmpeg",
            how: "tar.xz",
        }),
        _ => None,
    }
}

/// Everything `do_download` and `offer_ffmpeg` read from their world
/// (python reads module globals, the filesystem and the console).
pub struct DownloadCtx<'a> {
    pub root: &'a str,
    pub is_windows: bool,
    pub use_color: bool,
    pub stdin_tty: bool,
    pub sink: SharedSink,
    pub out: &'a mut dyn Out,
    pub reader: &'a mut dyn Reader,
    pub version_cache: &'a mut resolve::VersionCache,
    pub auto: Option<&'a str>,
}

/// python `print(...)`: the text plus its newline, flushed (one write
/// lock per line via the shared sink).
fn print_out(sink: &SharedSink, text: &str) {
    let mut out = sink.lock().unwrap_or_else(|e| e.into_inner());
    out.write(text);
    out.write("\n");
    out.flush();
}

/// `offer_ffmpeg(s, kind)`: offer the published build when no ffmpeg is
/// found and a human is on the line. The phase-8 network install is not
/// wired yet: accepting the offer prints one typed line instead
/// (recorded deviation) and the run continues without ffmpeg.
pub fn offer_ffmpeg(s: &Obj, kind: &str, ctx: &mut DownloadCtx) -> Option<String> {
    let bin_dir = resolve::py_path_join(ctx.root, "bin");
    let mut which = resolve::which_from_process;
    if let Ok(Some(dir)) = resolve::find_ffmpeg_dir(s, &bin_dir, ctx.is_windows, &mut which) {
        return Some(dir);
    }
    if !ctx.stdin_tty {
        return None;
    }
    let platform = resolve::host_platform();
    let source = ffmpeg_source(&platform);
    let why = if kind == "audio only" {
        "without it the audio is saved as-is, no mp3/opus/wav conversion"
    } else {
        "without it you get one single best file - no separate audio track, no merge, \
         no mp3, no srt subtitles"
    };
    print_out(&ctx.sink, "");
    print_out(
        &ctx.sink,
        &c(ctx.use_color, A_AMBER, "  ffmpeg is not installed"),
    );
    print_out(&ctx.sink, &format!("  {why}"));
    let Some(source) = source else {
        print_out(
            &ctx.sink,
            &format!(
                "  no published download for {platform} - install it with your package manager instead"
            ),
        );
        return None;
    };
    let archive = if source.how == "zip" { "zip" } else { "tar.xz" };
    print_out(
        &ctx.sink,
        &format!("  download the official {platform} build? ({archive})"),
    );
    print_out(&ctx.sink, &format!("  {}", source.url));
    let raw = match ctx.reader.read_line("  y/n (enter = no): ", None, ctx.out) {
        ReadOutcome::Line(line) => line,
        // python read_esc: escape answers None (decline); EOF and Ctrl+C
        // raise out of the call (recorded deviation: the port continues
        // without ffmpeg)
        ReadOutcome::Escape | ReadOutcome::Eof | ReadOutcome::Interrupted => {
            print_out(&ctx.sink, "  continuing without ffmpeg");
            return None;
        }
    };
    let answer = py_strip(&raw).to_lowercase();
    if answer != "y" && answer != "yes" {
        print_out(&ctx.sink, "  continuing without ffmpeg");
        return None;
    }
    print_out(
        &ctx.sink,
        "  the ffmpeg download is not wired in this build yet (native port phase 8)",
    );
    None
}

// ------------------------------------------------------- do_download

/// A shape error out of the resolution or argument layer: python raises
/// a traceback and exits 1; the port prints one typed stderr line
/// (recorded deviation) and returns 1.
fn shape_abort(shape: ShapeKind) -> i32 {
    eprintln!(
        "ytdlp: settings value refused with python shape {}",
        shape.token()
    );
    1
}

/// `do_download(s, url)`: the whole download flow (FR-7, FR-9, FR-10).
/// Returns the rc the callers use; 130 stops a batch (FR-12).
pub fn do_download(s: &Obj, url: &str, ctx: &mut DownloadCtx, rt: &mut RunRt) -> i32 {
    let low = url.to_lowercase();
    if !(low.starts_with("http://") || low.starts_with("https://")) {
        print_out(&ctx.sink, &format!("not a URL: {}", py_repr_str(url)));
        return 2;
    }

    let bin_dir = resolve::py_path_join(ctx.root, "bin");
    let platform = resolve::host_platform();
    let mut which = resolve::which_from_process;
    let binary = match resolve::find_yt_dlp(s, &bin_dir, &platform, &mut which) {
        Ok(Some(binary)) => binary,
        Ok(None) => {
            print_out(
                &ctx.sink,
                &format!(
                    "{} yt-dlp not found - put it in bin/ or on PATH, or set yt_dlp_path in config/settings.json",
                    c(ctx.use_color, A_AMBER, "...")
                ),
            );
            return 2;
        }
        Err(shape) => return shape_abort(shape),
    };

    // sub-second preflight checks share one grouped stage line
    let mut st = tty::dot_stage(
        "checking yt-dlp and ffmpeg",
        ctx.use_color,
        ctx.sink.clone(),
    );
    st.start();
    let mtime = resolve::mtime_seconds(&binary);
    let (ver, vrc) = resolve::probe_version(&binary, mtime, ctx.version_cache, &mut |b, f| {
        resolve::spawn_version_probe(b, f)
    });
    let mut ffdir = match resolve::find_ffmpeg_dir(s, &bin_dir, ctx.is_windows, &mut which) {
        Ok(dir) => dir,
        Err(shape) => {
            st.done(None);
            return shape_abort(shape);
        }
    };
    if vrc != 0 || ver.is_empty() {
        st.done(Some(&format!("yt-dlp check failed (rc={vrc}): {ver}")));
        return 2;
    }
    let kind = match config::get(s, "type") {
        Some(Value::Str(t)) => t.clone(),
        _ => "video".to_string(),
    };
    let ffs = if ffdir.is_some() {
        "ffmpeg found"
    } else if kind == "audio only" {
        "no ffmpeg - saving original audio"
    } else {
        "no ffmpeg - single best file"
    };
    st.done(Some(&format!("ready - yt-dlp v{ver}, {ffs}")));

    if ffdir.is_none() {
        ffdir = offer_ffmpeg(s, &kind, ctx);
    }

    let mut has_node = || resolve::which_from_process("node").is_some();
    let opts = match args::build_args(s, url, ffdir.as_deref(), ctx.root, ctx.auto, &mut has_node) {
        Ok(opts) => opts,
        Err(shape) => return shape_abort(shape),
    };

    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let spinner = term::pick_spinner();
    let mut st = tty::spinner_stage(
        "downloading (escape to stop)",
        spinner,
        ctx.use_color,
        ctx.sink.clone(),
    );
    st.start();
    let (rc, stopped, err, _codes) = run_download(&binary, &opts, Some(&st), ctx.use_color, rt);
    if stopped {
        let removed = match temp_dir(s, ctx.auto, ctx.root) {
            Ok(dir) => clean_partials(&dir, started, 5.0),
            Err(_) => Vec::new(),
        };
        drop_temp_dir(s, ctx.auto, ctx.root);
        let tail = if removed.is_empty() {
            String::new()
        } else {
            format!(
                ", cleaned {} partial file{}",
                removed.len(),
                if removed.len() == 1 { "" } else { "s" }
            )
        };
        st.done(Some(&format!("downloading - stopped by escape{tail}")));
        return 130;
    }
    if rc != 0 {
        let sentence = fail_sentence(rc, &err);
        st.done(Some(&sentence));
        drop_temp_dir(s, ctx.auto, ctx.root);
        return rc as i32;
    }

    st.done(Some("downloading - finished"));
    let outdir = match resolve::resolve_outdir(s, ctx.auto, ctx.root) {
        Ok(dir) => dir,
        Err(shape) => return shape_abort(shape),
    };
    match newest_since(&outdir, started, 2.0) {
        None => print_out(&ctx.sink, &format!("saved to {outdir}")),
        Some(path) => {
            let name = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            print_out(
                &ctx.sink,
                &format!(
                    "{} done - {} ({:.1}MB)",
                    c(ctx.use_color, A_AMBER, "..."),
                    ascii_safe(&name),
                    size as f64 / 1048576.0
                ),
            );
        }
    }

    if config::get(s, "subtitles").is_some_and(config::py_truthy) {
        let langs_text = match config::get(s, "subtitle_langs") {
            Some(v) => config::py_str(v),
            None => "None".to_string(),
        };
        let mut st2 = tty::dot_stage(
            &format!("subtitles ({langs_text})"),
            ctx.use_color,
            ctx.sink.clone(),
        );
        st2.start();
        let mut has_node = || resolve::which_from_process("node").is_some();
        let sub_opts =
            match args::build_sub_args(s, url, ffdir.as_deref(), ctx.root, ctx.auto, &mut has_node)
            {
                Ok(opts) => opts,
                Err(shape) => {
                    st2.done(None);
                    return shape_abort(shape);
                }
            };
        let (rc2, _stopped2, err2, codes) =
            run_download(&binary, &sub_opts, Some(&st2), ctx.use_color, rt);
        if rc2 == 0 {
            let (verdict, miss) = sub_success_lines(&codes);
            st2.done(Some(&verdict));
            if !miss.is_empty() {
                st2.done(Some(&miss));
            }
        } else {
            let lines = sub_fail_lines(Some(&err2));
            st2.done(Some(&lines[0]));
            st2.done(Some(&lines[1]));
        }
    }
    drop_temp_dir(s, ctx.auto, ctx.root);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::WIDTH;

    #[test]
    fn fail_sentence_matches_selftest_pins() {
        assert_eq!(fail_sentence(7, ""), "downloading - failed (rc=7)");
        let long_err = "ERROR: unable to rename file: [WinError 32] The process cannot \
                        access the file because it is being used by another process";
        assert_eq!(
            fail_sentence(1, long_err),
            "downloading - failed (rc=1): ERROR: unable to rename file: [WinError 32] \
             The process cannot access the file because it is being used by another process"
        );
        assert!(fail_sentence(1, long_err).len() as i64 > WIDTH);
    }

    #[test]
    fn sub_reasons_match_selftest_pins() {
        let r429_src =
            "ERROR: Unable to download video subtitles for 'en': HTTP Error 429: Too Many Requests";
        assert_eq!(
            sub_fail_reason(Some(r429_src)).as_deref(),
            Some("the subtitle api answered 429 too many requests")
        );
        assert_eq!(sub_fail_reason(Some("SOMETHING ODD")), None);
        assert_eq!(sub_fail_reason(None), None);
        assert_eq!(
            sub_fail_reason(Some("HTTP Error 429")).as_deref(),
            Some("the subtitle api answered 429 too many requests")
        );
        assert_eq!(
            sub_fail_reason(Some("The request timed out")).as_deref(),
            Some("the subtitle request timed out")
        );
        assert_eq!(
            sub_fail_reason(Some("connection reset by peer")).as_deref(),
            Some("the network was unreachable")
        );
    }

    #[test]
    fn sub_success_lines_match_selftest_pins() {
        let (verdict, miss) = sub_success_lines(&["en".to_string(), "en-GB".to_string()]);
        assert_eq!(verdict, "subtitles - en, en-GB");
        assert_eq!(miss, "");
        let (verdict, miss) = sub_success_lines(&[]);
        assert_eq!(verdict, "subtitles - none available");
        assert_eq!(
            miss,
            "note: yt-dlp reported success but no subtitle files were read \
             - wording may have changed, please report"
        );
    }

    #[test]
    fn sub_fail_lines_match_selftest_pins() {
        let r429_src =
            "ERROR: Unable to download video subtitles for 'en': HTTP Error 429: Too Many Requests";
        let lines = sub_fail_lines(Some(r429_src));
        assert_eq!(
            lines[0],
            "subtitles - not fetched (video kept). unable to download subtitles"
        );
        assert_eq!(
            lines[1],
            "subtitle error - the subtitle api answered 429 too many requests"
        );
        let lines = sub_fail_lines(Some("SOMETHING ODD"));
        assert_eq!(
            lines[1],
            "subtitle error - yt-dlp wording not recognized - please report"
        );
    }

    // ---- phase 7: download support ----

    /// A throwaway scratch dir per test.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ytdlp-exec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn now_secs() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }

    #[test]
    fn newest_since_picks_the_newest_fresh_file() {
        let dir = scratch("newest");
        let dir_text = dir.to_string_lossy().into_owned();
        std::fs::write(dir.join("old.txt"), b"x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("new.txt"), b"yy").unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir(dir.join("sub.part")).unwrap();

        let since = now_secs() - 60.0;
        let newest = newest_since(&dir_text, since, 2.0).expect("a candidate");
        assert!(newest.ends_with("new.txt"), "got {newest}");
        // the mtime gate: everything is older than a future `since`
        assert!(newest_since(&dir_text, now_secs() + 60.0, 2.0).is_none());
        // directories are not candidates (python's is_file)
        assert!(newest_since(&dir.join("sub").to_string_lossy(), since, 2.0).is_none());
        // an unreadable folder answers None like python's OSError branch
        assert!(newest_since(&format!("{dir_text}/gone"), since, 2.0).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_partials_removes_fresh_part_and_ytdl_only() {
        let dir = scratch("clean");
        let dir_text = dir.to_string_lossy().into_owned();
        std::fs::write(dir.join("a.part"), b"x").unwrap();
        std::fs::write(dir.join("b.f137.mp4.part-Frag3"), b"x").unwrap();
        std::fs::write(dir.join("c.ytdl"), b"x").unwrap();
        std::fs::write(dir.join("keep.txt"), b"x").unwrap();
        std::fs::create_dir(dir.join("d.part")).unwrap();

        let mut removed = clean_partials(&dir_text, now_secs() - 60.0, 5.0);
        removed.sort();
        assert_eq!(
            removed,
            vec![
                "a.part".to_string(),
                "b.f137.mp4.part-Frag3".to_string(),
                "c.ytdl".to_string()
            ]
        );
        assert!(dir.join("keep.txt").exists());
        assert!(dir.join("d.part").is_dir(), "a directory is never unlinked");
        // the slack gate: nothing is fresh for a future `since`
        std::fs::write(dir.join("late.part"), b"x").unwrap();
        assert!(clean_partials(&dir_text, now_secs() + 60.0, 5.0).is_empty());
        assert!(clean_partials(&format!("{dir_text}/gone"), now_secs(), 5.0).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sub_code_matches_the_python_rules() {
        assert_eq!(sub_code("video.en.vtt"), "en");
        assert_eq!(sub_code("video.en-GB.vtt"), "en-GB");
        assert_eq!(sub_code("video.srt"), "video.srt");
        assert_eq!(sub_code("/out/video.en.vtt"), "en");
        assert_eq!(sub_code("a.b.c"), "b");
        assert_eq!(sub_code(".vtt"), ".vtt");
    }

    struct ScriptedHandle {
        polls: std::collections::VecDeque<Option<i64>>,
    }

    impl ChildHandle for ScriptedHandle {
        fn poll(&mut self) -> Option<i64> {
            self.polls.pop_front().unwrap_or(Some(0))
        }
        fn wait(&mut self) -> i64 {
            0
        }
        fn pid(&self) -> u32 {
            4242
        }
    }

    struct ScriptedWatch {
        hits: std::collections::VecDeque<bool>,
    }

    impl EscapeWatch for ScriptedWatch {
        fn escape_hit(&mut self) -> bool {
            self.hits.pop_front().unwrap_or(false)
        }
    }

    type KillFlag = std::rc::Rc<std::cell::Cell<i64>>;

    #[allow(clippy::too_many_arguments)]
    fn scripted_run(
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        polls: Vec<Option<i64>>,
        hits: Vec<bool>,
        interrupted: bool,
        stage: Option<&Stage>,
        use_color: bool,
        kill_count: KillFlag,
    ) -> (i64, bool, String, Vec<String>) {
        let mut rt = RunRt {
            spawn: Box::new(move |_binary, _opts| {
                Ok(Spawned {
                    stdout: Box::new(std::io::Cursor::new(stdout.clone())),
                    stderr: Box::new(std::io::Cursor::new(stderr.clone())),
                    child: Box::new(ScriptedHandle {
                        polls: polls.clone().into(),
                    }),
                })
            }),
            watch: Box::new(move || {
                Box::new(ScriptedWatch {
                    hits: hits.clone().into(),
                }) as Box<dyn EscapeWatch>
            }),
            kill: Box::new(move |_child| kill_count.set(kill_count.get() + 1)),
            interrupted: Box::new(move || interrupted),
            sleep_frame: Box::new(|| {}),
            interrupt_scope: None,
        };
        run_download("yt-dlp", &[], stage, use_color, &mut rt)
    }

    #[test]
    fn run_download_scrapes_output_and_picks_the_error_line() {
        let stdout = b"[download] Destination: out/video.mp4\n\
                       [download]   5.0% of 1.50MiB at 2.34MiB/s ETA 00:12\n\
                       Writing video subtitles to: out/video.en-GB.vtt\n";
        let stderr = b"one\ntwo\nthree\nfour\nfive\nsix\nERROR: the real failure\nseven\n";
        let kill_count = std::rc::Rc::new(std::cell::Cell::new(0));
        let stage = Stage::new(
            "downloading (escape to stop)",
            Vec::new(),
            0.0,
            vec![(".".to_string(), 214)],
            false,
            std::sync::Arc::new(std::sync::Mutex::new(crate::tty::StdoutOut::new())),
        );
        let (rc, stopped, err, codes) = scripted_run(
            stdout.to_vec(),
            stderr.to_vec(),
            vec![None, Some(3)],
            vec![],
            false,
            Some(&stage),
            false,
            kill_count.clone(),
        );
        assert_eq!((rc, stopped), (3, false));
        // the tail keeps six lines: "three" onward; the ERROR line wins
        assert_eq!(err, "ERROR: the real failure");
        assert_eq!(codes, vec!["en-GB".to_string()]);
        // the live text is the last non-Destination progress body
        assert_eq!(
            stage.live().as_deref(),
            Some("5.0% of 1.50MiB at 2.34MiB/s ETA 00:12")
        );
        assert_eq!(kill_count.get(), 0);
    }

    #[test]
    fn run_download_falls_back_to_the_last_stderr_line() {
        let kill_count = std::rc::Rc::new(std::cell::Cell::new(0));
        let (rc, stopped, err, codes) = scripted_run(
            b"[download] Destination: out/video.mp4\n".to_vec(),
            b"alpha\nbeta\n".to_vec(),
            vec![Some(1)],
            vec![],
            false,
            None,
            false,
            kill_count.clone(),
        );
        assert_eq!((rc, stopped), (1, false));
        assert_eq!(err, "beta");
        assert!(codes.is_empty());
        // without a stage nothing is published (python's stage is not None
        // guard) and Destination lines are suppressed
    }

    #[test]
    fn run_download_escape_and_cancel_kill_the_tree() {
        let kill_count = std::rc::Rc::new(std::cell::Cell::new(0));
        let (rc, stopped, err, codes) = scripted_run(
            b"".to_vec(),
            b"".to_vec(),
            vec![None, None],
            vec![true],
            false,
            None,
            false,
            kill_count.clone(),
        );
        assert_eq!((rc, stopped, err.as_str(), codes.len()), (-1, true, "", 0));
        assert_eq!(kill_count.get(), 1);

        let kill_count2 = std::rc::Rc::new(std::cell::Cell::new(0));
        let (rc2, stopped2, err2, _) = scripted_run(
            b"".to_vec(),
            b"".to_vec(),
            vec![None],
            vec![],
            true,
            None,
            false,
            kill_count2.clone(),
        );
        assert_eq!((rc2, stopped2, err2.as_str()), (-1, false, "cancelled"));
        assert_eq!(kill_count2.get(), 1);
    }

    #[test]
    fn run_download_colors_live_text_only_with_use_color() {
        let kill_count = std::rc::Rc::new(std::cell::Cell::new(0));
        let stdout = b"[download]  9.94MiB/s at ETA 00:12\n";
        let stage = Stage::new(
            "downloading",
            Vec::new(),
            0.0,
            vec![(".".to_string(), 214)],
            false,
            std::sync::Arc::new(std::sync::Mutex::new(crate::tty::StdoutOut::new())),
        );
        let _ = scripted_run(
            stdout.to_vec(),
            b"".to_vec(),
            vec![Some(0)],
            vec![],
            false,
            Some(&stage),
            true,
            kill_count.clone(),
        );
        let expected = crate::term::color_progress("9.94MiB/s at ETA 00:12");
        assert_eq!(stage.live().as_deref(), Some(expected.as_str()));
    }
}
