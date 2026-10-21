//! The selftest plumbing: python's check sink, the capture out, the
//! scripted reader and http fixtures, and the small helpers the
//! sections share.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::args;
use crate::config::{self, Obj, Value};
use crate::net;
use crate::pyfs;
use crate::pytext;
use crate::resolve;
use crate::term;
use crate::tty::{Out, ReadOutcome, Reader};

// ------------------------------------------------------------ plumbing

/// Python's default `got`: `check(name, cond)` passes none, and `%r`
/// of `""` renders `''`.
pub(super) const NO_GOT: &str = "''";

/// `''` renderer for the checks python ran without a `got`.
pub(super) fn bare_got() -> String {
    NO_GOT.to_string()
}

pub(super) fn now_secs() -> f64 {
    // mirrors main.rs's `now` semantics exactly
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The windows-only registry folders when on windows, empty elsewhere
/// (main.rs's `shell_folders`, the same shape).
pub(super) fn shell_folders(env: &resolve::EnvMap) -> (Option<String>, Option<String>) {
    #[cfg(windows)]
    {
        resolve::shell_folder_values(env)
    }
    #[cfg(not(windows))]
    {
        let _ = env;
        (None, None)
    }
}

/// The check sink: `PASS name` / `FAIL name  got: <repr>`, the running
/// count and the failing names. Python's `check()`, byte for byte.
pub(super) struct Checker<'a> {
    pub(super) count: i64,
    pub(super) fails: Vec<String>,
    pub(super) out: &'a mut dyn Out,
}

impl<'a> Checker<'a> {
    pub(super) fn new(out: &'a mut dyn Out) -> Self {
        Self {
            count: 0,
            fails: Vec::new(),
            out,
        }
    }

    /// The report's own sink, for the one menu subcall python printed
    /// straight into the report (item 13's hold; audit F8).
    pub(super) fn report_out(&mut self) -> &mut dyn Out {
        self.out
    }

    pub(super) fn check(&mut self, name: &str, cond: bool, got: &str) {
        self.count += 1;
        let verb = if cond { "PASS" } else { "FAIL" };
        let tail = if cond {
            String::new()
        } else {
            format!("  got: {got}")
        };
        self.out.write(&format!("{verb:<4} {name}{tail}"));
        self.out.write("\n");
        self.out.flush();
        if !cond {
            self.fails.push(name.to_string());
        }
    }
}

/// Captured stdout (python's `io.StringIO()` copies): raw bytes, so
/// every capture is exactly what the menu wrote. `Arc<Mutex<..>>`
/// keeps the capture `Send` for the `SharedOut`/`SharedSink` seams.
#[derive(Clone, Default)]
pub(super) struct CaptureOut {
    pub(super) lines: Arc<Mutex<Vec<u8>>>,
}

impl Out for CaptureOut {
    fn write(&mut self, text: &str) {
        self.lines
            .lock()
            .unwrap()
            .extend_from_slice(text.as_bytes());
    }
    fn flush(&mut self) {}
}

impl CaptureOut {
    pub(super) fn text(&self) -> String {
        String::from_utf8_lossy(&self.lines.lock().unwrap()).into_owned()
    }
}

/// A scripted reader: queues python's fake answers and records every
/// `(prompt, below)` python's `fake_esc` appended. Echoes the answer
/// the way python's fakes do - value, then Enter.
pub(super) struct ScriptedReader {
    pub(super) replies: VecDeque<String>,
    pub(super) seen: Vec<(String, Option<Vec<String>>)>,
}

impl ScriptedReader {
    pub(super) fn new(replies: &[&str]) -> Self {
        Self {
            replies: replies.iter().map(|r| (*r).to_string()).collect(),
            seen: Vec::new(),
        }
    }
}

impl Reader for ScriptedReader {
    fn read_line(
        &mut self,
        prompt: &str,
        below: Option<&[String]>,
        out: &mut dyn Out,
    ) -> ReadOutcome {
        let below_vec = below.map(|b| b.to_vec());
        self.seen.push((prompt.to_string(), below_vec.clone()));
        if !prompt.is_empty() {
            out.write(prompt);
        }
        if let Some(lines) = &below_vec {
            out.write("\n");
            for line in lines {
                out.write(line);
                out.write("\n");
            }
            out.write(&format!("\x1b[{}A", lines.len() + 1));
        }
        let reply = self.replies.pop_front().unwrap_or_default();
        out.write(&reply);
        out.write("\n");
        ReadOutcome::Line(reply)
    }
}

/// A reader that must never be read; a read is a programming fault the
/// report then catches through whatever the caller observes.
pub(super) struct NoReader;
impl Reader for NoReader {
    fn read_line(
        &mut self,
        _prompt: &str,
        _below: Option<&[String]>,
        _out: &mut dyn Out,
    ) -> ReadOutcome {
        ReadOutcome::Eof
    }
}

/// A network that records `(url, timeout)` per call and always fails:
/// python's F3 `fake_fetch` raising offline.
pub(super) struct RecordingHttp {
    pub(super) calls: Rc<RefCell<Vec<(String, u64)>>>,
}

impl net::HttpRt for RecordingHttp {
    fn text(&mut self, url: &str, timeout: u64) -> Result<String, String> {
        self.calls.borrow_mut().push((url.to_string(), timeout));
        Err("offline".to_string())
    }
    fn file(
        &mut self,
        url: &str,
        _dest: &str,
        timeout: u64,
        _live: Option<&mut dyn FnMut(&str)>,
    ) -> Result<net::FileOutcome, String> {
        self.calls.borrow_mut().push((url.to_string(), timeout));
        Err("offline".to_string())
    }
}

/// A network nobody reads (the installer dance reads nothing).
pub(super) struct DeadHttp;
impl net::HttpRt for DeadHttp {
    fn text(&mut self, _url: &str, _timeout: u64) -> Result<String, String> {
        Err("dead".to_string())
    }
    fn file(
        &mut self,
        _url: &str,
        _dest: &str,
        _timeout: u64,
        _live: Option<&mut dyn FnMut(&str)>,
    ) -> Result<net::FileOutcome, String> {
        Err("dead".to_string())
    }
}

pub(super) fn platform_name() -> &'static str {
    if cfg!(windows) { "windows" } else { "linux" }
}

pub(super) fn arch_name() -> &'static str {
    std::env::consts::ARCH
}

// ------------------------------------------------------- repr helpers

/// python `repr` of an option string.
pub(super) fn py_opt_str(v: Option<&str>) -> String {
    match v {
        None => "None".to_string(),
        Some(s) => format!("'{}'", pytext::py_repr_str(s)),
    }
}

/// python `repr` of a settings value (None spelled).
pub(super) fn py_value(v: Option<&Value>) -> String {
    match v {
        None => "None".to_string(),
        Some(v) => config::py_repr(v),
    }
}

/// python `repr` of a small tuple of strings: `('a', 'b')`.
pub(super) fn got_strs(values: &[&str]) -> String {
    let parts: Vec<String> = values
        .iter()
        .map(|v| format!("'{}'", pytext::py_repr_str(v)))
        .collect();
    format!("({})", parts.join(", "))
}

/// python `repr` of a list of strings: `['a', 'b']`.
pub(super) fn got_list<S: AsRef<str>>(values: &[S]) -> String {
    let parts: Vec<String> = values
        .iter()
        .map(|v| format!("'{}'", pytext::py_repr_str(v.as_ref())))
        .collect();
    format!("[{}]", parts.join(", "))
}

/// Strips python's three `A_*` tokens from captured bytes (python's
/// `plain7` comprehension does exactly these replaces).
pub(super) fn strip_tokens(text: &str) -> String {
    text.replace(term::A_AMBER, "")
        .replace(term::A_DARKGREY, "")
        .replace(term::A_RESET, "")
}

/// The `\x1b[Nm` stripper behind the F10 menu-row check (python's
/// compiled `ansi` regex; no regex dependency - the scan follows the
/// same rule: only a completed `[0-9;]*m` sequence drops).
pub(super) fn strip_ansi(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\x1b' && i + 1 < chars.len() && chars[i + 1] == '[' {
            let mut j = i + 2;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == ';') {
                j += 1;
            }
            if j < chars.len() && chars[j] == 'm' {
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

// -------------------------------------------------------- scratch dirs

/// python `tempfile.mkdtemp(prefix=...)` into the system temp dir.
pub(super) fn mkdtemp(prefix: &str) -> String {
    let base = std::env::temp_dir().to_string_lossy().into_owned();
    pyfs::mkdtemp(prefix, &base).expect("selftest: mkdtemp failed")
}

pub(super) fn write_bytes(path: &str, bytes: &[u8]) {
    File::create(path)
        .and_then(|mut f| f.write_all(bytes))
        .expect("selftest: fixture write failed");
}

pub(super) fn file_bytes(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

/// Backdates a file's mtime so `clean_partials` reads it as stale.
pub(super) fn set_mtime_back(path: &str, secs: f64) {
    let file = File::options()
        .write(true)
        .open(path)
        .expect("selftest: reopen");
    let mtime = std::time::SystemTime::now() - std::time::Duration::from_secs_f64(secs);
    file.set_modified(mtime).expect("selftest: set_modified");
}

/// The names in a folder (the untouched-listing assertion).
pub(super) fn dir_names(path: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// `os.chmod(path, 0o755)` on posix, nothing on windows (only the
/// shell shims need it).
pub(super) fn make_executable(path: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

// ------------------------------------------------------ argv fixtures

/// python `a_index(argv, flag)`.
pub(super) fn a_index<'a>(argv: &'a [String], flag: &str) -> Option<&'a String> {
    argv.iter()
        .position(|t| t == flag)
        .and_then(|i| argv.get(i + 1))
}

pub(super) fn a_contains(argv: &[String], flag: &str) -> bool {
    argv.iter().any(|t| t == flag)
}

/// The ffmpeg sentinel python's selftest passes: truthy, non-existent,
/// joined into the argv with no disk probe.
pub(super) fn ff_dir() -> Option<&'static str> {
    Some("FFMPEG_DIR_SENTINEL")
}

/// `build_args` with the selftest's node answer; a shape error is
/// reported through the failing check so the count never jumps when a
/// branch is skipped.
pub(super) fn argv_of(
    c: &mut Checker<'_>,
    s: &Obj,
    url: &str,
    ffdir: Option<&str>,
    root: &str,
    auto: Option<&str>,
) -> Vec<String> {
    let mut has_node = || false;
    match args::build_args(s, url, ffdir, root, auto, &mut has_node) {
        Ok(argv) => argv,
        Err(kind) => {
            c.check("build_args shape error", false, &format!("{kind:?}"));
            Vec::new()
        }
    }
}

/// The real auto-outdir answer (python's fresh memo after
/// `_outdir_cache.clear()`): the real environment, home and - on
/// windows - the registry folders.
pub(super) fn real_auto_outdir() -> Option<String> {
    let env = resolve::env_map_from_process();
    let home = resolve::home_from_env(&env).unwrap_or_default();
    let (win_desktop, win_downloads) = shell_folders(&env);
    resolve::auto_outdir_lookup(
        cfg!(windows),
        win_desktop.as_deref(),
        win_downloads.as_deref(),
        &home,
        &env,
    )
    .ok()
    .flatten()
}

/// The real settings or the defaults fallback (python's
/// `load_settings()` is silent; the boot notes print elsewhere).
pub(super) fn loaded_settings(config_path: &str) -> Obj {
    match config::load_settings(Path::new(config_path)) {
        Ok(config::LoadOutcome::Loaded { settings, .. }) => settings,
        Ok(config::LoadOutcome::Unreadable { settings, .. }) => settings,
        Err(_) => config::defaults(),
    }
}

/// The outdir a python call would resolve at this point (python's
/// helpers resolve it from the settings themselves).
pub(super) fn outdir_of(s: &Obj, root: &str, auto: Option<&str>) -> String {
    resolve::resolve_outdir(s, auto, root).unwrap_or_default()
}

// ------------------------------------------------------ shim builders

/// The ffmpeg-probe shim of python's check: `--version` prints its
/// banner to stderr and exits 2880417800, `-version` answers rc 0 -
/// the exact ffmpeg-9 bug this pin survives (the 2026 code shipped).
pub(super) fn probe_shim(dir: &str) -> String {
    let name = if cfg!(windows) { "ff9.bat" } else { "ff9.sh" };
    let path = resolve::py_path_join(dir, name);
    if cfg!(windows) {
        write_bytes(
            &path,
            concat!(
                "@echo off\r\n",
                "if \"%~1\"==\"--version\" echo ffmpeg version 9.0.2 fake 1>&2",
                " & exit /b 2880417800\r\n",
                "echo ffmpeg version 9.0.2 fake\r\nexit /b 0\r\n"
            )
            .as_bytes(),
        );
    } else {
        write_bytes(
            &path,
            concat!(
                "#!/bin/sh\n",
                "if [ \"$1\" = \"--version\" ]; then\n",
                "  echo 'ffmpeg version 9.0.2 fake' >&2\n",
                "  exit 2880417800\n",
                "fi\n",
                "echo 'ffmpeg version 9.0.2 fake'\n",
                "exit 0\n"
            )
            .as_bytes(),
        );
        make_executable(&path);
    }
    path
}

/// A cheap spawnable that answers `--version` with one line (python's
/// `sys.executable --version` in the F4 memo check has no native twin;
/// the shim is the guaranteed rc-0 answer).
pub(super) fn memo_shim(dir: &str) -> String {
    let name = if cfg!(windows) {
        "probe.bat"
    } else {
        "probe.sh"
    };
    let path = resolve::py_path_join(dir, name);
    if cfg!(windows) {
        write_bytes(&path, b"@echo off\r\necho shim probe 2026.10.16\r\n");
    } else {
        write_bytes(&path, b"#!/bin/sh\necho shim probe 2026.10.16\n");
        make_executable(&path);
    }
    path
}

// ============================================================== sections
