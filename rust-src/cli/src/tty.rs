//! Terminal layer, tty-bound half (specsheet section 4, phase-6 port
//! notes): the two seams every interactive side effect goes through
//! (`Out`, `Reader`), their production implementations over stdout and
//! stdin, `read_esc` with the piped and the raw branches, `LineEdit` and
//! the session history, `KeyWatch`, `Stage`, `block_under_input`,
//! terminal size, VT enable and `hold`.
//!
//! The python oracle writes to `sys.stdout` and reads through
//! `msvcrt`/`termios` directly; the port routes both through seams so
//! the whole menu is drivable in tests and in the differential harness
//! with recording implementations, exactly like the python selftest
//! redirects stdout and fakes `read_esc`. Where python reads module
//! globals (USE_COLOR, HISTORY) the port takes parameters.
//!
//! Byte fidelity that the seams keep: the production `Out` reproduces
//! python's text-mode newline translation (windows non-console stdout
//! translates `\n` to `\r\n` - measured 2026-10-15; a windows console
//! takes UTF-16 through WriteConsoleW); recording sinks carry raw LF
//! and no translation, which is what io.StringIO does in the harness.

use std::collections::VecDeque;
use std::io::{IsTerminal, Read, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::term::{DOTS, FRAME_SECS, Spinner, flat_frames, flat_freeze, plain_cells, render_cells};

/// Locking that ignores poisoning: a panicking writer must not take the
/// whole menu down (python has no equivalent state).
fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ------------------------------------------------------------------ seams

/// One `read_esc` answer. Line is a submitted line; Escape is python's
/// None; Eof and Interrupted are python's EOFError and KeyboardInterrupt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    Line(String),
    Escape,
    Eof,
    Interrupted,
}

/// The input side. `below` is the reference block under a live prompt
/// (python's `read_esc(..., below=...)`); the reader writes prompts and
/// blocks itself, like python's read_esc does.
pub trait Reader {
    fn read_line(
        &mut self,
        prompt: &str,
        below: Option<&[String]>,
        out: &mut dyn Out,
    ) -> ReadOutcome;

    /// The main prompt's read (FR-17): on a real tty a bare "0" on the
    /// empty line fires the quick paste at once - no enter. Defaults to
    /// `read_line`, so scripted readers and piped stdin keep the plain
    /// line contract, where 0 still acts when the line is submitted.
    fn read_line_quick0(
        &mut self,
        prompt: &str,
        below: Option<&[String]>,
        out: &mut dyn Out,
    ) -> ReadOutcome {
        self.read_line(prompt, below, out)
    }
}

/// The output side: write plus flush, mirroring the python pairs
/// `sys.stdout.write(...)` + `sys.stdout.flush()`.
pub trait Out {
    fn write(&mut self, text: &str);
    fn flush(&mut self);
}

/// A recording sink for tests and the differential harness. Cloning
/// shares the recording, so a `Stage` and its test can look at the same
/// event log from two sides. Events keep write/flush order, so a test
/// can assert that a frame never sits in a buffer.
#[derive(Clone, Default)]
pub struct RecordingOut {
    inner: Arc<Mutex<RecordingInner>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecEvent {
    Write(String),
    Flush,
}

#[derive(Default)]
struct RecordingInner {
    events: Vec<RecEvent>,
}

impl RecordingOut {
    pub fn events(&self) -> Vec<RecEvent> {
        lock(&self.inner).events.clone()
    }
    pub fn writes(&self) -> Vec<String> {
        lock(&self.inner)
            .events
            .iter()
            .filter_map(|e| match e {
                RecEvent::Write(text) => Some(text.clone()),
                RecEvent::Flush => None,
            })
            .collect()
    }
    pub fn flush_count(&self) -> usize {
        lock(&self.inner)
            .events
            .iter()
            .filter(|e| matches!(e, RecEvent::Flush))
            .count()
    }
    pub fn text(&self) -> String {
        lock(&self.inner)
            .events
            .iter()
            .filter_map(|e| match e {
                RecEvent::Write(text) => Some(text.as_str()),
                RecEvent::Flush => None,
            })
            .collect()
    }
}

impl Out for RecordingOut {
    fn write(&mut self, text: &str) {
        lock(&self.inner)
            .events
            .push(RecEvent::Write(text.to_string()));
    }
    fn flush(&mut self) {
        lock(&self.inner).events.push(RecEvent::Flush);
    }
}

/// A scripted reader for tests and the harness: one queued outcome per
/// read, prompts recorded in call order (the python hooks record the
/// same list from their fake read_esc).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptItem {
    Line(String),
    Escape,
    Eof,
    Interrupted,
}

pub struct ScriptedReader {
    pub replies: VecDeque<ScriptItem>,
    pub prompts: Vec<String>,
    pub belows: Vec<Option<Vec<String>>>,
}

impl ScriptedReader {
    pub fn new(replies: Vec<ScriptItem>) -> Self {
        Self {
            replies: replies.into(),
            prompts: Vec::new(),
            belows: Vec::new(),
        }
    }
}

impl Reader for ScriptedReader {
    fn read_line(
        &mut self,
        prompt: &str,
        below: Option<&[String]>,
        _out: &mut dyn Out,
    ) -> ReadOutcome {
        self.prompts.push(prompt.to_string());
        self.belows.push(below.map(|b| b.to_vec()));
        match self.replies.pop_front() {
            Some(ScriptItem::Line(line)) => ReadOutcome::Line(line),
            Some(ScriptItem::Escape) => ReadOutcome::Escape,
            Some(ScriptItem::Eof) | None => ReadOutcome::Eof,
            Some(ScriptItem::Interrupted) => ReadOutcome::Interrupted,
        }
    }
}

// ------------------------------------------------------- production sides

pub fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// The production sink. Windows: a console handle takes UTF-16 through
/// WriteConsoleW (non-ASCII stays intact, the console does its own
/// newline handling), anything else translates `\n` to `\r\n` like
/// python's text stdout on a pipe. Posix: UTF-8 bytes.
pub struct StdoutOut {
    #[cfg(windows)]
    console: bool,
}

impl StdoutOut {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            Self {
                console: windows_stdout_is_console(),
            }
        }
        #[cfg(not(windows))]
        {
            Self {}
        }
    }
}

impl Default for StdoutOut {
    fn default() -> Self {
        Self::new()
    }
}

impl Out for StdoutOut {
    fn write(&mut self, text: &str) {
        #[cfg(windows)]
        {
            if self.console {
                write_console_w(text);
                return;
            }
            let _ = std::io::stdout().write_all(text.replace('\n', "\r\n").as_bytes());
        }
        #[cfg(not(windows))]
        {
            let _ = std::io::stdout().write_all(text.as_bytes());
        }
    }

    fn flush(&mut self) {
        let _ = std::io::stdout().flush();
    }
}

/// `sys.stderr` text-mode output: python's handle translates `\n` to
/// `os.linesep` on write, so a redirected stderr on windows captures
/// CRLF (the session differ pins the argparse error case byte for
/// byte). Every native stderr message goes through this helper.
pub fn write_stderr(text: &str) {
    let mut err = std::io::stderr().lock();
    let body = stderr_text(text, cfg!(windows));
    let _ = err.write_all(body.as_bytes());
    let _ = err.flush();
}

/// The newline translation alone, flag-injectable so the windows
/// spelling is pinned on every os.
pub fn stderr_text(text: &str, windows: bool) -> String {
    if windows {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}

#[cfg(windows)]
fn windows_stdout_is_console() -> bool {
    use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE};
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode: u32 = 0;
        GetConsoleMode(handle, &mut mode) != 0
    }
}

#[cfg(windows)]
fn write_console_w(text: &str) {
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE, WriteConsoleW};
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return;
    }
    let mut written: u32 = 0;
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        // WriteConsoleW takes at most 64k units per call; interactive
        // text is far below that.
        let _ = WriteConsoleW(
            handle,
            wide.as_ptr(),
            wide.len() as u32,
            &mut written,
            std::ptr::null(),
        );
    }
}

/// A sink that locks per call, so several writers (a menu and a stage
/// thread) can share one terminal with whole-write atomicity.
pub type SharedSink = Arc<Mutex<dyn Out + Send>>;

/// python's `print(text)` onto a shared sink: the text, its newline and
/// a flush, under one lock.
pub fn emit_line(sink: &SharedSink, text: &str) {
    let mut out = lock(sink);
    out.write(text);
    out.write("\n");
    out.flush();
}

#[derive(Clone)]
pub struct SharedOut {
    sink: SharedSink,
}

impl SharedOut {
    pub fn new(inner: impl Out + Send + 'static) -> Self {
        Self {
            sink: Arc::new(Mutex::new(inner)),
        }
    }

    pub fn from_sink(sink: SharedSink) -> Self {
        Self { sink }
    }

    pub fn sink(&self) -> SharedSink {
        Arc::clone(&self.sink)
    }
}

impl Out for SharedOut {
    fn write(&mut self, text: &str) {
        lock(&self.sink).write(text);
    }

    fn flush(&mut self) {
        lock(&self.sink).flush();
    }
}

/// Windows consoles need ENABLE_VIRTUAL_TERMINAL_PROCESSING for ANSI.
/// Errors are swallowed, like python's `enable_vt_output`.
pub fn enable_vt_output() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{
            ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE,
            SetConsoleMode,
        };
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode: u32 = 0;
        if GetConsoleMode(handle, &mut mode) != 0 {
            let _ = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

// ------------------------------------------------------- line editing

pub const HISTORY_MAX: usize = 100;

/// The session history of submitted lines (python's module-global
/// HISTORY): urls and menu numbers alike, capped at HISTORY_MAX.
#[derive(Default)]
pub struct History {
    pub entries: Vec<String>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a submitted line (non-empty after strip) for later recall.
    pub fn remember(&mut self, line: &str) {
        if !line.trim().is_empty() {
            self.entries.push(line.to_string());
            if self.entries.len() > HISTORY_MAX {
                let cut = self.entries.len() - HISTORY_MAX;
                self.entries.drain(..cut);
            }
        }
    }
}

/// Line buffer with echo and history recall (python `LineEdit`): up/down
/// walk the history, the half-typed line is the draft and comes back
/// when walking down past the newest entry; left/right arrows are
/// consumed without effect because the reader does no cursor editing.
pub struct LineEdit {
    prompt: String,
    buf: Vec<char>,
    idx: usize,
    draft: String,
}

impl LineEdit {
    pub fn new(prompt: &str, history: &History) -> Self {
        Self {
            prompt: prompt.to_string(),
            buf: Vec::new(),
            idx: history.entries.len(),
            draft: String::new(),
        }
    }

    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn add(&mut self, ch: char, out: &mut dyn Out) {
        self.buf.push(ch);
        let mut tmp = [0u8; 4];
        out.write(ch.encode_utf8(&mut tmp));
        out.flush();
    }

    pub fn back(&mut self, out: &mut dyn Out) {
        if self.buf.pop().is_some() {
            out.write("\x08 \x08");
            out.flush();
        }
    }

    fn show(&mut self, text: &str, out: &mut dyn Out) {
        self.buf = text.chars().collect();
        out.write(&format!("\r\x1b[K{}{}", self.prompt, text));
        out.flush();
    }

    pub fn remember(&mut self, history: &mut History) {
        let line = self.text();
        history.remember(&line);
    }

    pub fn recall(&mut self, older: bool, history: &History, out: &mut dyn Out) {
        if history.entries.is_empty() {
            return;
        }
        if older {
            if self.idx == history.entries.len() {
                self.draft = self.text();
            }
            if self.idx > 0 {
                self.idx -= 1;
                let text = history.entries[self.idx].clone();
                self.show(&text, out);
            }
        } else if self.idx < history.entries.len() - 1 {
            self.idx += 1;
            let text = history.entries[self.idx].clone();
            self.show(&text, out);
        } else if self.idx == history.entries.len() - 1 {
            self.idx = history.entries.len();
            let draft = self.draft.clone();
            self.show(&draft, out);
        }
    }
}

// ------------------------------------------------------------ read_esc

/// The piped byte source. Universal newlines exactly as python's text
/// stdin (newline=None) delivers them, measured 2026-10-15: `\r\n` and
/// lone `\n` end a line immediately; a lone `\r` needs the next byte
/// (or EOF) to decide, so `abc\r` on an open pipe blocks; a partial
/// last line is returned and the *next* read is EOF. Bytes decode as
/// UTF-8; invalid sequences replace (python raises - recorded).
pub struct LineSource {
    pending: VecDeque<u8>,
    eof: bool,
    scripted: bool,
}

impl Default for LineSource {
    fn default() -> Self {
        Self::stdin()
    }
}

impl LineSource {
    pub fn stdin() -> Self {
        Self {
            pending: VecDeque::new(),
            eof: false,
            scripted: false,
        }
    }

    /// A source over fixed bytes, used by the differential harness and
    /// standalone tests.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            pending: bytes.iter().copied().collect(),
            eof: false,
            scripted: true,
        }
    }

    fn fill(&mut self) -> bool {
        if self.eof {
            return false;
        }
        if self.scripted {
            self.eof = true;
            return false;
        }
        let mut chunk = [0u8; 4096];
        match std::io::stdin().lock().read(&mut chunk) {
            Ok(0) => {
                self.eof = true;
                false
            }
            Ok(n) => {
                self.pending.extend(&chunk[..n]);
                true
            }
            Err(_) => {
                self.eof = true;
                false
            }
        }
    }

    fn next_byte(&mut self) -> Option<u8> {
        loop {
            if let Some(b) = self.pending.pop_front() {
                return Some(b);
            }
            if !self.fill() {
                return None;
            }
        }
    }

    fn push_front(&mut self, b: u8) {
        self.pending.push_front(b);
    }

    /// One line without its terminator, or None at EOF with nothing read.
    pub fn read_line(&mut self) -> Option<String> {
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let b = match self.next_byte() {
                Some(b) => b,
                // EOF: a partial line still counts, python's input()
                // returns it and the next read raises
                None => {
                    return if bytes.is_empty() {
                        None
                    } else {
                        Some(decode(&bytes))
                    };
                }
            };
            match b {
                b'\n' => return Some(decode(&bytes)),
                b'\r' => match self.next_byte() {
                    None => return Some(decode(&bytes)),
                    Some(b'\n') => return Some(decode(&bytes)),
                    Some(other) => {
                        self.push_front(other);
                        return Some(decode(&bytes));
                    }
                },
                _ => bytes.push(b),
            }
        }
    }
}

fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// python's piped read_esc branch: the `below` block first (each line
/// plus its newline), then the prompt, then one line; EOF maps to
/// python's EOFError.
pub fn read_esc_piped(
    prompt: &str,
    below: Option<&[String]>,
    lines: &mut LineSource,
    out: &mut dyn Out,
) -> ReadOutcome {
    if let Some(block) = below {
        for line in block {
            out.write(line);
            out.write("\n");
        }
        out.flush();
    }
    if !prompt.is_empty() {
        out.write(prompt);
        out.flush();
    }
    match lines.read_line() {
        Some(line) => ReadOutcome::Line(line),
        None => ReadOutcome::Eof,
    }
}

/// One input line; returns Escape (python None) when Escape was pressed.
/// The reader the menu uses: piped stdin takes the fallback branch, a
/// tty takes the raw branch. The raw branch keeps python's stdin state
/// across calls: the chunk buffer (`RawInput`) and the session history
/// survive from one read to the next, exactly like python's sys.stdin
/// BufferedReader and the module-global HISTORY.
pub struct TtyReader {
    pub lines: LineSource,
    pub history: History,
    #[cfg(unix)]
    raw: RawInput,
}

impl Default for TtyReader {
    fn default() -> Self {
        Self::new()
    }
}

impl TtyReader {
    pub fn new() -> Self {
        Self {
            lines: LineSource::stdin(),
            history: History::new(),
            #[cfg(unix)]
            raw: RawInput::new(0),
        }
    }

    /// The tty branch: prompt first, then the `below` block, then the
    /// raw loop (python read_esc lines 244-314); `quick0` arms the
    /// FR-17 bare-0 fire for the main prompt.
    fn read_tty(
        &mut self,
        prompt: &str,
        below: Option<&[String]>,
        out: &mut dyn Out,
        quick0: bool,
    ) -> ReadOutcome {
        if !prompt.is_empty() {
            out.write(prompt);
            out.flush();
        }
        if let Some(block) = below {
            out.write("\n");
            out.write(&block.join("\n"));
            out.write("\n");
            out.write(&format!("\x1b[{}A", block.len() + 1));
            out.flush();
        }
        #[cfg(unix)]
        {
            read_esc_tty_unix(prompt, out, &mut self.history, &mut self.raw, quick0)
        }
        #[cfg(windows)]
        {
            read_esc_tty_windows(prompt, out, &mut self.history, quick0)
        }
    }
}

impl Reader for TtyReader {
    fn read_line(
        &mut self,
        prompt: &str,
        below: Option<&[String]>,
        out: &mut dyn Out,
    ) -> ReadOutcome {
        if stdin_is_tty() {
            self.read_tty(prompt, below, out, false)
        } else {
            read_esc_piped(prompt, below, &mut self.lines, out)
        }
    }

    fn read_line_quick0(
        &mut self,
        prompt: &str,
        below: Option<&[String]>,
        out: &mut dyn Out,
    ) -> ReadOutcome {
        if stdin_is_tty() {
            self.read_tty(prompt, below, out, true)
        } else {
            read_esc_piped(prompt, below, &mut self.lines, out)
        }
    }
}

/// Wait for enter without printing a prompt (python `hold`): the menu
/// repaint wipes anything that does not hold, so a caller with
/// something to say prints it and then holds.
pub fn hold(out: &mut dyn Out, reader: &mut dyn Reader) {
    let _ = reader.read_line("", None, out);
}

// ------------------------------------------------------- posix raw mode

#[cfg(unix)]
struct RawMode {
    fd: i32,
    saved: libc::termios,
}

#[cfg(unix)]
impl RawMode {
    /// python `tty.setraw`: the flag clears of tty.py, applied with
    /// TCSAFLUSH (which also drains pending input).
    fn enter(fd: i32) -> Option<Self> {
        let mut saved = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(fd, saved.as_mut_ptr()) } != 0 {
            return None;
        }
        let saved = unsafe { saved.assume_init() };
        let mut raw = saved;
        raw.c_iflag &= !(libc::IGNBRK
            | libc::BRKINT
            | libc::IGNPAR
            | libc::PARMRK
            | libc::INPCK
            | libc::ISTRIP
            | libc::INLCR
            | libc::IGNCR
            | libc::ICRNL
            | libc::IXON
            | libc::IXANY
            | libc::IXOFF);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag &= !(libc::PARENB | libc::CSIZE);
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO
            | libc::ECHOE
            | libc::ECHOK
            | libc::ECHONL
            | libc::ICANON
            | libc::IEXTEN
            | libc::ISIG
            | libc::NOFLSH
            | libc::TOSTOP);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) } != 0 {
            return None;
        }
        Some(Self { fd, saved })
    }

    fn restore(&self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.saved);
        }
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        self.restore();
    }
}

/// The main read of the raw loop. python reads characters through
/// `sys.stdin.read(1)` - a buffered text reader - and the buffering is
/// observable: measured 2026-10-15 under a pty, a burst like ESC [ A is
/// drained into that reader by the first read, so `_esc_more` (which
/// reads the fd directly) sees nothing and the sequence degrades to
/// escape plus literal "[A"; a UTF-8 character split across writes
/// still comes back whole because the decoder is incremental and
/// blocking; CR passes through untranslated. This reader reproduces
/// all three behaviours, so pty transcripts match byte for byte.
#[cfg(unix)]
struct RawInput {
    fd: i32,
    pending: VecDeque<u8>,
}

#[cfg(unix)]
impl RawInput {
    fn new(fd: i32) -> Self {
        Self {
            fd,
            pending: VecDeque::new(),
        }
    }

    /// One text character, blocking. An invalid byte maps to U+FFFD
    /// where python raises UnicodeDecodeError (recorded deviation;
    /// terminals deliver valid UTF-8).
    fn next_char(&mut self) -> Option<char> {
        loop {
            if self.pending.is_empty() {
                if !self.fill() {
                    return None;
                }
                continue;
            }
            let bytes: Vec<u8> = self.pending.iter().copied().collect();
            match std::str::from_utf8(&bytes) {
                Ok(text) => {
                    let ch = text.chars().next()?;
                    self.pending.drain(..ch.len_utf8());
                    return Some(ch);
                }
                Err(e) => match e.error_len() {
                    // an incomplete sequence: read on, exactly like the
                    // incremental decoder (a split char still returns whole)
                    None => {
                        if self.fill() {
                            continue;
                        }
                        self.pending.pop_front();
                        return Some('\u{fffd}');
                    }
                    Some(_) => {
                        let valid = e.valid_up_to();
                        if valid == 0 {
                            self.pending.pop_front();
                            return Some('\u{fffd}');
                        }
                        let text = std::str::from_utf8(&bytes[..valid]).ok()?;
                        let ch = text.chars().next()?;
                        self.pending.drain(..ch.len_utf8());
                        return Some(ch);
                    }
                },
            }
        }
    }

    /// A chunked raw read (python's BufferedReader size), so a burst
    /// lands ahead of later reads.
    fn fill(&mut self) -> bool {
        let mut chunk = [0u8; 8192];
        loop {
            let n = unsafe { libc::read(self.fd, chunk.as_mut_ptr().cast(), chunk.len()) };
            if n > 0 {
                self.pending.extend(&chunk[..n as usize]);
                return true;
            }
            if n == 0 {
                return false;
            }
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return false;
        }
    }
}

#[cfg(unix)]
fn byte_ready(fd: i32, timeout_ms: i32) -> bool {
    let mut p = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let r = unsafe { libc::poll(&mut p, 1, timeout_ms) };
    r > 0
}

/// One byte, or None on EOF or a non-retryable error. Retries EINTR
/// like python's os.read (PEP 475).
#[cfg(unix)]
fn read_byte(fd: i32) -> Option<u8> {
    let mut b: u8 = 0;
    loop {
        let n = unsafe { libc::read(fd, (&mut b as *mut u8).cast(), 1) };
        if n == 1 {
            return Some(b);
        }
        if n == 0 {
            return None;
        }
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return None;
    }
}

/// python `_esc_more`: the next byte of an in-flight ESC sequence, or
/// None when nothing follows within the timeout. One byte decodes as
/// UTF-8 with replacement, python's `os.read(fd,1).decode(...)`.
#[cfg(unix)]
fn esc_more(fd: i32, timeout_ms: i32) -> Option<char> {
    if !byte_ready(fd, timeout_ms) {
        return None;
    }
    match read_byte(fd) {
        None => None,
        Some(b) if b < 0x80 => Some(b as char),
        Some(_) => Some('\u{fffd}'),
    }
}

/// python `_csi_final`: the final letter of a CSI key sequence; the
/// parameter bytes (< 0x40) are swallowed.
#[cfg(unix)]
fn csi_final(fd: i32) -> Option<char> {
    loop {
        match esc_more(fd, 50) {
            None => return None,
            Some(ch) if (ch as u32) >= 0x40 => return Some(ch),
            Some(_) => {}
        }
    }
}

/// FR-17: the quick-paste trigger - only the main prompt's read asks for
/// it (`quick0`), only on a bare 0 keypress, only on an empty line. A 0
/// typed later inside a line stays an ordinary character.
fn quick0_fires(quick0: bool, ch: char, ed: &LineEdit) -> bool {
    quick0 && ch == '0' && ed.is_empty()
}

#[cfg(unix)]
fn read_esc_tty_unix(
    prompt: &str,
    out: &mut dyn Out,
    history: &mut History,
    input: &mut RawInput,
    quick0: bool,
) -> ReadOutcome {
    let fd = 0;
    let Some(_raw) = RawMode::enter(fd) else {
        // python would raise from tcgetattr; a stdin that is a tty but
        // has no termios is outside this product's world
        return ReadOutcome::Eof;
    };
    let mut ed = LineEdit::new(prompt, history);
    loop {
        // a 0-byte read (closed pty master) is a python spin; the port
        // answers escape (recorded deviation)
        let Some(ch) = input.next_char() else {
            return ReadOutcome::Escape;
        };
        match ch {
            '\r' | '\n' => {
                out.write("\n");
                out.flush();
                ed.remember(history);
                return ReadOutcome::Line(ed.text());
            }
            '\x1b' => match esc_more(fd, 50) {
                None => {
                    out.write("\r\x1b[K");
                    out.flush();
                    return ReadOutcome::Escape;
                }
                Some('[') => {
                    if let Some(fin) = csi_final(fd) {
                        match fin {
                            'A' => ed.recall(true, history, out),
                            'B' => ed.recall(false, history, out),
                            _ => {}
                        }
                    }
                }
                Some(_) => {}
            },
            '\x08' | '\x7f' => ed.back(out),
            '\x03' => return ReadOutcome::Interrupted,
            ch if quick0_fires(quick0, ch, &ed) => {
                out.write("\n");
                out.flush();
                return ReadOutcome::Line("0".to_string());
            }
            ch if ch >= ' ' => {
                ed.add(ch, out);
            }
            _ => {}
        }
    }
}

// ----------------------------------------------------- windows raw mode

#[cfg(windows)]
unsafe extern "C" {
    // the CRT console functions CPython's msvcrt module calls
    fn _getwch() -> u16;
    fn _kbhit() -> i32;
}

/// The windows raw loop: msvcrt.getwch semantics - extended keys arrive
/// as `\x00`/`\xe0` plus a scan code, H recalls older and P newer; a
/// lone surrogate half (a non-BMP key) maps to U+FFFD because a rust
/// String cannot carry it (recorded deviation).
#[cfg(windows)]
fn read_esc_tty_windows(
    prompt: &str,
    out: &mut dyn Out,
    history: &mut History,
    quick0: bool,
) -> ReadOutcome {
    let mut ed = LineEdit::new(prompt, history);
    loop {
        let w = unsafe { _getwch() };
        match w {
            0x000d | 0x000a => {
                out.write("\n");
                out.flush();
                ed.remember(history);
                return ReadOutcome::Line(ed.text());
            }
            0x001b => {
                out.write("\r\x1b[K");
                out.flush();
                return ReadOutcome::Escape;
            }
            0x0008 | 0x007f => ed.back(out),
            0x0000 | 0x00e0 => {
                let code = unsafe { _getwch() };
                match code {
                    0x0048 => ed.recall(true, history, out),
                    0x0050 => ed.recall(false, history, out),
                    _ => {}
                }
            }
            0x0003 => return ReadOutcome::Interrupted,
            c if quick0_fires(quick0, char::from_u32(c as u32).unwrap_or('\u{fffd}'), &ed) => {
                out.write("\n");
                out.flush();
                return ReadOutcome::Line("0".to_string());
            }
            c if c >= 0x20 => {
                let ch = char::from_u32(c as u32).unwrap_or('\u{fffd}');
                ed.add(ch, out);
            }
            _ => {}
        }
    }
}

// -------------------------------------------------------------- KeyWatch

/// Raw key watcher for the download run: only escape is acted on. Inert
/// off a tty; restores the mode on drop (python restores in `restore()`
/// and the runner calls it from `finally`).
pub struct KeyWatch {
    #[cfg(unix)]
    fd: i32,
    #[cfg(unix)]
    saved: Option<libc::termios>,
}

impl Default for KeyWatch {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyWatch {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            Self {}
        }
        #[cfg(unix)]
        {
            let fd = 0;
            if !stdin_is_tty() {
                return Self { fd, saved: None };
            }
            Self {
                fd,
                saved: cbreak(fd),
            }
        }
    }

    pub fn escape_hit(&mut self) -> bool {
        #[cfg(windows)]
        {
            unsafe {
                if _kbhit() == 0 {
                    return false;
                }
                let ch = _getwch();
                if ch == 0 || ch == 0xe0 {
                    let _ = _getwch();
                    return false;
                }
                ch == 0x1b
            }
        }
        #[cfg(unix)]
        {
            if self.saved.is_none() {
                return false;
            }
            if !byte_ready(self.fd, 0) {
                return false;
            }
            read_byte(self.fd) == Some(0x1b)
        }
    }
}

#[cfg(unix)]
impl Drop for KeyWatch {
    fn drop(&mut self) {
        if let Some(saved) = &self.saved {
            unsafe {
                // python's KeyWatch.restore uses TCSADRAIN (phase-7
                // parity fix: the phase-6 draft had TCSAFLUSH, which
                // would discard type-ahead the user already typed).
                libc::tcsetattr(self.fd, libc::TCSADRAIN, saved);
            }
        }
    }
}

/// python `tty.setcbreak`: echo and canonical mode off, VMIN=1/VTIME=0.
#[cfg(unix)]
fn cbreak(fd: i32) -> Option<libc::termios> {
    let mut saved = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, saved.as_mut_ptr()) } != 0 {
        return None;
    }
    let saved = unsafe { saved.assume_init() };
    let mut mode = saved;
    mode.c_lflag &= !(libc::ECHO | libc::ICANON);
    mode.c_cc[libc::VMIN] = 1;
    mode.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &mode) } != 0 {
        return None;
    }
    Some(saved)
}

// ----------------------------------------------------------------- Stage

struct StageState {
    live: Option<String>,
    stop: bool,
}

/// One animated stage line: cells spin, then the line freezes. Every
/// rewrite clears the line first. The animation runs on its own thread
/// over the shared sink; off color the thread is skipped and only the
/// final line prints. `done` writes the freeze frame plus final text
/// plus newline, exactly python's three write forms.
pub struct Stage {
    text: String,
    frames: Vec<Vec<(String, u8)>>,
    tick: f64,
    freeze: Vec<(String, u8)>,
    use_color: bool,
    state: Arc<Mutex<StageState>>,
    sink: SharedSink,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Stage {
    pub fn new(
        text: &str,
        frames: Vec<Vec<(String, u8)>>,
        tick: f64,
        freeze: Vec<(String, u8)>,
        use_color: bool,
        sink: SharedSink,
    ) -> Self {
        Self {
            text: text.to_string(),
            frames,
            tick,
            freeze,
            use_color,
            state: Arc::new(Mutex::new(StageState {
                live: None,
                stop: false,
            })),
            sink,
            thread: None,
        }
    }

    pub fn start(&mut self) {
        if !self.use_color {
            return;
        }
        let state = Arc::clone(&self.state);
        let sink = Arc::clone(&self.sink);
        let frames = self.frames.clone();
        let text = self.text.clone();
        let tick = self.tick;
        self.thread = Some(std::thread::spawn(move || {
            let count = frames.len().max(1);
            let mut i: usize = 0;
            loop {
                {
                    let mut out = lock(&sink);
                    let st = lock(&state);
                    if st.stop {
                        break;
                    }
                    let label = match &st.live {
                        Some(live) => live.clone(),
                        None => text.clone(),
                    };
                    let frame = &frames[i % count];
                    i += 1;
                    drop(st);
                    out.write(&format!("\r\x1b[K{} {}", render_cells(frame), label));
                    out.flush();
                }
                std::thread::sleep(std::time::Duration::from_secs_f64(tick));
            }
        }));
    }

    /// The live progress text the pump threads publish (python reads and
    /// writes `stage.live` from two threads; a lock replaces the plain
    /// attribute).
    pub fn set_live(&self, text: Option<String>) {
        lock(&self.state).live = text;
    }

    /// A shared handle for the download pump threads (python's
    /// `stage.live = ...` from another thread).
    pub fn live_handle(&self) -> StageLive {
        StageLive {
            state: Arc::clone(&self.state),
        }
    }

    /// The current live text (tests and the differential arm read it
    /// after the pump threads joined).
    pub fn live(&self) -> Option<String> {
        lock(&self.state).live.clone()
    }

    pub fn done(&mut self, text: Option<&str>) {
        lock(&self.state).stop = true;
        // python joins with a 1.0 s timeout; the loop checks the flag
        // every tick (<= 0.12 s) and never blocks on the sink for long
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        let final_text = match text {
            Some(t) => t.to_string(),
            None => self.text.clone(),
        };
        let mut out = lock(&self.sink);
        if self.use_color {
            out.write(&format!(
                "\r\x1b[K{} {}\n",
                render_cells(&self.freeze),
                final_text
            ));
        } else {
            out.write(&format!("{} {}\n", plain_cells(&self.freeze), final_text));
        }
        out.flush();
    }

    /// The built-in frame set (the selftest and the differential arm
    /// read them back; python reads `stage.frames` directly).
    pub fn frames(&self) -> &[Vec<(String, u8)>] {
        &self.frames
    }

    /// The built-in tick (python `stage.tick`).
    pub fn tick_secs(&self) -> f64 {
        self.tick
    }

    /// The built-in freeze frame (python `stage.freeze`).
    pub fn freeze_cells(&self) -> &[(String, u8)] {
        &self.freeze
    }
}

/// A shared live-text handle for writers other than the Stage owner
/// (the download pump threads).
pub struct StageLive {
    state: Arc<Mutex<StageState>>,
}

impl StageLive {
    pub fn set(&self, text: Option<String>) {
        lock(&self.state).live = text;
    }
}

/// A dropped Stage stops its spin thread at the next tick. Python's
/// daemon thread would keep running until process exit, which only ever
/// mattered on traceback paths that end the process; the port may
/// continue (menu flows), so the flag is set here.
impl Drop for Stage {
    fn drop(&mut self) {
        lock(&self.state).stop = true;
    }
}

/// The quiet amber dot stage for preflight, subtitle and ffmpeg waits.
pub fn dot_stage(text: &str, use_color: bool, sink: SharedSink) -> Stage {
    let frames = flat_frames(
        &DOTS.iter().map(|d| (*d).to_string()).collect::<Vec<_>>(),
        214,
    );
    Stage::new(
        text,
        frames,
        FRAME_SECS,
        flat_freeze("...", 214),
        use_color,
        sink,
    )
}

/// The download stage: one of the eight animations, chosen at random by
/// the caller (python `pick_spinner`).
pub fn spinner_stage(text: &str, spinner: &Spinner, use_color: bool, sink: SharedSink) -> Stage {
    Stage::new(
        text,
        spinner.frames.clone(),
        spinner.tick,
        spinner.freeze.clone(),
        use_color,
        sink,
    )
}

// ------------------------------------------------------ block and size

/// True when a reference block can be painted under a live input line.
/// Needs color, a real stdin console, and room: a block wider than the
/// window would wrap and a block taller would scroll the prompt off the
/// top. python measures with len() (code points), not display width.
pub fn block_under_input(
    block: &[String],
    use_color: bool,
    stdin_tty: bool,
    terminal: Option<(i64, i64)>,
) -> bool {
    if !use_color || !stdin_tty {
        return false;
    }
    let Some((columns, lines)) = terminal else {
        return false;
    };
    let maxw = block
        .iter()
        .map(|line| line.chars().count() as i64)
        .max()
        .unwrap_or(0);
    maxw + 1 < columns && (block.len() as i64) + 2 < lines
}

/// os.get_terminal_size() on stdout: windows takes the window rect from
/// the console screen buffer (the same math as CPython's
/// TERMSIZE_USE_CONIO), posix takes TIOCGWINSZ; any error is None and
/// reads as no room in block_under_input.
pub fn terminal_size_stdout() -> Option<(i64, i64)> {
    #[cfg(unix)]
    {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } != 0 {
            return None;
        }
        Some((i64::from(ws.ws_col), i64::from(ws.ws_row)))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{
            CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo, GetStdHandle, STD_OUTPUT_HANDLE,
        };
        let mut info = std::mem::MaybeUninit::<CONSOLE_SCREEN_BUFFER_INFO>::uninit();
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE);
            if GetConsoleScreenBufferInfo(handle, info.as_mut_ptr()) == 0 {
                return None;
            }
            let info = info.assume_init();
            Some((
                i64::from(info.srWindow.Right) - i64::from(info.srWindow.Left) + 1,
                i64::from(info.srWindow.Bottom) - i64::from(info.srWindow.Top) + 1,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    /// Python's TextIOWrapper (newline=None) writes os.linesep: CRLF on
    /// windows, LF on posix. Every native stderr text goes through this
    /// translation (the windows differential run of the argparse error
    /// case caught the raw-LF spelling).
    #[test]
    fn stderr_text_translates_newlines_like_python() {
        assert_eq!(stderr_text("a\nb\n", true), "a\r\nb\r\n");
        assert_eq!(stderr_text("a\nb\n", false), "a\nb\n");
        assert_eq!(stderr_text("plain", true), "plain");
    }

    /// The python selftest's History matrix, twin by twin.
    #[test]
    fn history_matrix_matches_the_python_selftest() {
        let mut h = History::new();
        h.entries.push("https://a.example".to_string());
        h.entries.push("https://b.example".to_string());
        let mut out = RecordingOut::default();
        let mut ed = LineEdit::new("> ", &h);
        ed.add('x', &mut out);
        ed.add('y', &mut out);
        ed.recall(true, &h, &mut out);
        assert_eq!(ed.text(), "https://b.example");
        ed.recall(true, &h, &mut out);
        assert_eq!(ed.text(), "https://a.example");
        ed.recall(false, &h, &mut out);
        assert_eq!(ed.text(), "https://b.example");
        ed.recall(false, &h, &mut out);
        assert_eq!(
            ed.text(),
            "xy",
            "walking down past the newest entry brings the draft back"
        );
        ed.back(&mut out);
        assert_eq!(ed.text(), "x");
        let mut ed2 = LineEdit::new("> ", &h);
        ed2.add('q', &mut out);
        ed2.remember(&mut h);
        assert_eq!(h.entries.last().unwrap(), "q");
        let mut ed3 = LineEdit::new("> ", &h);
        ed3.remember(&mut h);
        assert_eq!(
            h.entries.last().unwrap(),
            "q",
            "empty lines are not remembered"
        );
        // the echo and repaint writes
        let text = out.text();
        assert!(text.contains("\r\x1b[K> https://b.example"));
        assert!(text.contains("\x08 \x08"));
    }

    #[test]
    fn history_caps_at_100() {
        let mut h = History::new();
        for i in 0..100 {
            h.remember(&format!("line{i}"));
        }
        assert_eq!(h.entries.len(), 100);
        h.remember("newest");
        assert_eq!(h.entries.len(), 100);
        assert_eq!(h.entries[0], "line1");
        assert_eq!(h.entries.last().unwrap(), "newest");
    }

    /// FR-17: the bare 0 fires the quick paste only for the main prompt's
    /// read, only on an empty line; a 0 inside a typed line (or any other
    /// read) stays an ordinary character.
    #[test]
    fn quick0_fires_only_for_the_bare_zero_on_the_main_prompt() {
        let h = History::new();
        let mut ed = LineEdit::new("> ", &h);
        assert!(quick0_fires(true, '0', &ed));
        assert!(!quick0_fires(false, '0', &ed));
        assert!(!quick0_fires(true, '1', &ed));
        let mut out = RecordingOut::default();
        ed.add('1', &mut out);
        assert!(!quick0_fires(true, '0', &ed));
    }

    /// Scripted and piped reads keep the line contract: 0 acts when the
    /// line is submitted, exactly as before FR-17 armed the tty key.
    #[test]
    fn quick0_read_defaults_to_the_plain_line_read() {
        let mut reader = ScriptedReader::new(vec![ScriptItem::Line("0".to_string())]);
        let mut out = RecordingOut::default();
        assert_eq!(
            reader.read_line_quick0("> ", None, &mut out),
            ReadOutcome::Line("0".to_string())
        );
        assert_eq!(reader.prompts, vec!["> ".to_string()]);
    }

    /// The piped contract measured against python 3.14 (universal
    /// newlines, the lone-CR lookahead, partial-line-then-EOF).
    #[test]
    fn read_esc_piped_contract() {
        let mut out = RecordingOut::default();
        let mut src = LineSource::from_bytes(b"abc\n");
        assert_eq!(
            read_esc_piped("p> ", None, &mut src, &mut out),
            ReadOutcome::Line("abc".to_string())
        );
        assert_eq!(out.text(), "p> ");

        let mut src = LineSource::from_bytes(b"abc");
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Line("abc".to_string())
        );
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Eof
        );

        let mut src = LineSource::from_bytes(b"");
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Eof
        );

        let mut src = LineSource::from_bytes(b"a\rb\n");
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Line("a".to_string())
        );
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Line("b".to_string())
        );

        let mut src = LineSource::from_bytes(b"a\r\nb\rc");
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Line("a".to_string())
        );
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Line("b".to_string())
        );
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Line("c".to_string())
        );
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Eof
        );

        let mut src = LineSource::from_bytes(b"1\r");
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Line("1".to_string())
        );
        assert_eq!(
            read_esc_piped("", None, &mut src, &mut out),
            ReadOutcome::Eof
        );
    }

    #[test]
    fn read_esc_piped_writes_below_before_the_prompt() {
        let mut out = RecordingOut::default();
        let mut src = LineSource::from_bytes(b"x\n");
        let below = lines(&["one", "two"]);
        assert_eq!(
            read_esc_piped("p> ", Some(&below), &mut src, &mut out),
            ReadOutcome::Line("x".to_string())
        );
        assert_eq!(
            out.text(),
            "one\ntwo\np> ",
            "the block prints before the prompt when piped"
        );
    }

    /// The room gates and the margins, from the python selftest pins.
    #[test]
    fn block_room_gates() {
        let wide = "x".repeat(64);
        let block = lines(&[wide.as_str(); 10]);
        assert!(block_under_input(&block, true, true, Some((120, 50))));
        assert!(block_under_input(&block, true, true, Some((66, 14))));
        assert!(
            !block_under_input(&block, true, true, Some((65, 14))),
            "width + 1 < columns"
        );
        assert!(
            !block_under_input(&block, true, true, Some((64, 50))),
            "width + 1 < columns"
        );
        assert!(
            !block_under_input(&block, true, true, Some((120, 12))),
            "len + 2 < lines"
        );
        assert!(!block_under_input(&block, false, true, Some((120, 50))));
        assert!(!block_under_input(&block, true, false, Some((120, 50))));
        assert!(!block_under_input(&block, true, true, None));
        assert!(block_under_input(
            &lines(&["  x"]),
            true,
            true,
            Some((120, 50))
        ));
    }

    /// python measures the block in code points, not display columns.
    #[test]
    fn block_room_counts_code_points() {
        let wide = lines(&["\u{4e00}".repeat(60).as_str()]);
        // 60 code points, 120 columns; the width gate passes on len()
        assert!(block_under_input(&wide, true, true, Some((62, 50))));
        assert!(!block_under_input(&wide, true, true, Some((61, 50))));
    }

    /// Every stage frame is one whole write and is flushed before the
    /// next write: the G6 "spinner versus freeze contention" measurement.
    /// A noise writer hammers the same sink while the stage thread spins.
    #[test]
    fn stage_write_lock_contention_is_frame_atomic() {
        let rec = RecordingOut::default();
        let sink: SharedSink = Arc::new(Mutex::new(rec.clone()));
        let text = "downloading (escape to stop)".to_string();
        let frames = flat_frames(&lines(&["...", ">>"]), 214);
        let freeze = flat_freeze("...", 214);
        let mut stage = Stage::new(&text, frames, 0.001, freeze, true, Arc::clone(&sink));

        let mut noise = SharedOut::from_sink(Arc::clone(&sink));
        stage.start();
        for _ in 0..40 {
            noise.write("noise\n");
            noise.flush();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        stage.done(Some("finished"));
        let events = rec.events();

        let mut saw_frame = false;
        for (i, event) in events.iter().enumerate() {
            let RecEvent::Write(write) = event else {
                continue;
            };
            if !write.starts_with("\r\x1b[K") {
                continue;
            }
            if !write.contains("downloading") && !write.contains("finished") {
                panic!("frame write carried foreign text: {write:?}");
            }
            assert!(
                matches!(events.get(i + 1), Some(RecEvent::Flush)),
                "a frame was left unflushed: {write:?}"
            );
            saw_frame = true;
        }
        assert!(saw_frame, "the spinner thread never wrote a frame");
        let last = events.last().expect("events");
        assert!(
            matches!(last, RecEvent::Flush),
            "the final line must end flushed: {last:?}"
        );
        let text_out = rec.text();
        assert!(
            text_out.ends_with("\r\x1b[K\x1b[38;5;214m...\x1b[0m finished\n"),
            "done writes the freeze frame, the text and a newline"
        );
    }

    /// Off color the thread never starts and done prints the plain line.
    #[test]
    fn stage_without_color_writes_only_the_plain_line() {
        let rec = RecordingOut::default();
        let sink: SharedSink = Arc::new(Mutex::new(rec.clone()));
        let frames = flat_frames(&lines(&["..."]), 214);
        let freeze = flat_freeze("...", 214);
        let mut stage = Stage::new("preflight", frames, 0.001, freeze, false, Arc::clone(&sink));
        stage.start();
        std::thread::sleep(std::time::Duration::from_millis(5));
        stage.done(None);
        assert_eq!(rec.text(), "... preflight\n");
        assert_eq!(rec.flush_count(), 1);
    }

    #[test]
    fn keywatch_is_inert_without_a_tty() {
        // the test process has no tty stdin, so the watcher must be a no-op;
        // it drops at scope end, restoring the mode on posix
        let mut watch = KeyWatch::new();
        assert!(!watch.escape_hit());
    }

    #[test]
    fn vt_enable_is_harmless_off_a_console() {
        enable_vt_output();
    }
}
