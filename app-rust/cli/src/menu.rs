//! The interactive menu (specsheet section 4, phase-6 port notes):
//! pick_list, ask_bool, the batch list, run_batch, multi_url, the
//! folder dialog, act and the main loop. Everything here is driven
//! through the tty seams, so the whole menu runs in tests and in the
//! differential harness without a terminal.
//!
//! Port rules recorded in the phase-6 notes: items 1-8, 10, 12 and 13
//! are wired in full; the batch downloads, the item 11 updaters and the
//! URL quick mode are the caller's jobs (phase 7/8) and arrive here as
//! typed refusals or injected closures. "keep current" answers return
//! None instead of python's raw-value round-trip: a loaded settings
//! object always carries every key, so the two are indistinguishable in
//! every reachable state.

use crate::args::{TEMPLATE_DEFAULT, clean_url, parse_lang_input};
use crate::config::{self, AUDIO_FORMATS, CONTAINERS, RESOLUTIONS, TYPES, Obj, Value};
use crate::resolve::{self, ShapeKind};
use crate::term::{A_AMBER, A_DARKGREY, A_GREY, CLEAR, c};
use crate::tty::{self, Out, Reader, ReadOutcome};
use crate::ui::{self, WIDTH};

/// Everything the menu reads from its process world (python reads these
/// from module globals, os.name, the filesystem and the console).
pub struct World<'a> {
    pub version: &'a str,
    pub root: &'a str,
    pub config_path: &'a str,
    pub is_windows: bool,
    pub use_color: bool,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub terminal: Option<(i64, i64)>,
    /// the auto_outdir() answer (memoized in the real glue, audit F6)
    pub auto: Option<&'a str>,
}

/// The two seams for one menu session.
pub struct Io<'a> {
    pub out: &'a mut dyn Out,
    pub reader: &'a mut dyn Reader,
}

#[derive(Debug)]
pub enum MenuErr {
    GoBack,
    Eof,
    Interrupted,
    Shape(ShapeKind),
    Io(String),
}

impl From<ShapeKind> for MenuErr {
    fn from(shape: ShapeKind) -> Self {
        MenuErr::Shape(shape)
    }
}

/// One menu pick's answer: repaint or leave the loop.
pub enum ActOutcome {
    Repaint(bool),
    Exit,
}

fn read(io: &mut Io, prompt: &str, below: Option<&[String]>) -> ReadOutcome {
    io.reader.read_line(prompt, below, io.out)
}

/// python `print(...)`: the line plus its newline, flushed (the extra
/// flush is unobservable in bytes; python's print flushes a tty line
/// buffer the same way).
fn print_line(io: &mut Io, text: &str) {
    io.out.write(text);
    io.out.write("\n");
    io.out.flush();
}

fn print_lines(io: &mut Io, lines: &[String]) {
    for line in lines {
        io.out.write(line);
        io.out.write("\n");
    }
    io.out.flush();
}

/// python `%s` of a possibly missing value ("None" for a missing key).
fn value_text(value: Option<&Value>) -> String {
    match value {
        Some(v) => config::py_str(v),
        None => "None".to_string(),
    }
}

fn outdir_of(s: &Obj, world: &World) -> Result<String, MenuErr> {
    Ok(resolve::resolve_outdir(s, world.auto, world.root)?)
}

fn save(s: &Obj, world: &World) -> Result<(), MenuErr> {
    config::save_settings(std::path::Path::new(world.config_path), s)
        .map_err(|e| MenuErr::Io(e.to_string()))
}

fn read_line_or(io: &mut Io, prompt: &str, below: Option<&[String]>) -> Result<String, MenuErr> {
    match read(io, prompt, below) {
        ReadOutcome::Line(line) => Ok(line),
        ReadOutcome::Escape => Err(MenuErr::GoBack),
        ReadOutcome::Eof => Err(MenuErr::Eof),
        ReadOutcome::Interrupted => Err(MenuErr::Interrupted),
    }
}

// ------------------------------------------------------------- submenus

/// `pick_list`: a numbered choice block, escape backs out. Some(value)
/// stores the picked option; None keeps the current value.
pub fn pick_list(
    title: &str,
    options: &[&str],
    current: Option<&Value>,
    io: &mut Io,
    world: &World,
) -> Result<Option<Value>, MenuErr> {
    print_line(io, &format!("  {title}"));
    let current_text = value_text(current);
    for (i, option) in options.iter().enumerate() {
        let row = ui::pick_row(option, &current_text, (i + 1) as i64, world.use_color);
        print_line(io, &row);
    }
    let prompt = format!("  number (enter = keep {current_text}): ");
    let raw = read_line_or(io, &prompt, None)?;
    let raw = raw.trim().to_string();
    if raw.is_empty() {
        return Ok(None);
    }
    match config::py_int(&raw) {
        None => {
            print_line(io, "  invalid number");
            tty::hold(io.out, io.reader);
            Ok(None)
        }
        Some(idx) if idx >= 1 && idx <= options.len() as i64 => {
            Ok(Some(Value::Str(options[(idx - 1) as usize].to_string())))
        }
        Some(_) => {
            print_line(io, "  out of range");
            tty::hold(io.out, io.reader);
            Ok(None)
        }
    }
}

/// `ask_bool`: y/yes/on/1 true, n/no/off/0 false, anything else keeps
/// the current value.
pub fn ask_bool(
    label: &str,
    current: Option<&Value>,
    io: &mut Io,
) -> Result<Option<Value>, MenuErr> {
    let prompt = format!("  {label} (now {}) y/n (enter = keep): ", ui::onoff(current));
    let raw = read_line_or(io, &prompt, None)?;
    match raw.trim().to_lowercase().as_str() {
        "y" | "yes" | "on" | "1" => Ok(Some(Value::Bool(true))),
        "n" | "no" | "off" | "0" => Ok(Some(Value::Bool(false))),
        _ => Ok(None),
    }
}

/// `paint_batch`: menu on top, the collected list under it, input last.
fn paint_batch(s: &Obj, entries: &[String], io: &mut Io, world: &World) -> Result<(), MenuErr> {
    if world.use_color {
        io.out.write(CLEAR);
        io.out.flush();
    }
    let outdir = outdir_of(s, world)?;
    print_lines(
        io,
        &ui::render_lines(world.version, s, &outdir, world.is_windows, world.use_color)?,
    );
    print_lines(io, &ui::hint_lines(&outdir, world.use_color));
    print_line(
        io,
        &c(
            world.use_color,
            A_AMBER,
            "  multiple urls - one per line, empty line starts the downloads",
        ),
    );
    print_line(
        io,
        &c(world.use_color, A_DARKGREY, "  escape returns to the menu"),
    );
    for (n, text) in entries.iter().enumerate() {
        print_line(io, &ui::batch_row((n + 1) as i64, text, world.use_color));
    }
    Ok(())
}

/// `run_batch`: the collected entries in order, one at a time; a bad
/// entry is marked and skipped, escape (rc 130) stops the batch.
/// Returns True when at least one download started.
pub fn run_batch(
    s: &mut Obj,
    entries: &[String],
    io: &mut Io,
    world: &World,
    download: &mut dyn FnMut(&mut Obj, &str) -> i32,
) -> Result<bool, MenuErr> {
    let total = entries.len();
    let mut started = false;
    for (i, text) in entries.iter().enumerate() {
        if world.use_color {
            io.out.write(CLEAR);
            io.out.flush();
        }
        let outdir = outdir_of(s, world)?;
        print_lines(
            io,
            &ui::render_lines(world.version, s, &outdir, world.is_windows, world.use_color)?,
        );
        print_lines(io, &ui::hint_lines(&outdir, world.use_color));
        let url = clean_url(Some(text)).filter(|u| !u.is_empty());
        let tail = match &url {
            Some(u) => u.clone(),
            None => c(world.use_color, A_AMBER, "error in url"),
        };
        print_line(
            io,
            &(c(
                world.use_color,
                A_GREY,
                &format!("  url {} of {}", i + 1, total),
            ) + "  "
                + &tail),
        );
        let Some(url) = url else {
            print_line(
                io,
                &format!("  {} not a url - skipped", c(world.use_color, A_AMBER, "...")),
            );
            continue;
        };
        if download(s, &url) == 130 {
            break;
        }
        started = true;
    }
    Ok(started)
}

/// `multi_url`: collect urls one per line, an empty line starts the
/// downloads, escape returns to the menu. Returns True when the main
/// menu must repaint.
pub fn multi_url(
    s: &mut Obj,
    io: &mut Io,
    world: &World,
    download: &mut dyn FnMut(&mut Obj, &str) -> i32,
) -> Result<bool, MenuErr> {
    let mut entries: Vec<String> = Vec::new();
    loop {
        paint_batch(s, &entries, io, world)?;
        let prompt = format!("  url {}: ", entries.len() + 1);
        match read(io, &prompt, None) {
            ReadOutcome::Eof | ReadOutcome::Interrupted | ReadOutcome::Escape => return Ok(true),
            ReadOutcome::Line(raw) => {
                let text = raw.trim().to_string();
                if text.is_empty() {
                    break;
                }
                entries.push(text);
            }
        }
    }
    if entries.is_empty() {
        return Ok(true);
    }
    Ok(!run_batch(s, &entries, io, world, download)?)
}

// ---------------------------------------------------------- folder dialog

/// Native folder picker: a path or None (cancel / not available).
/// Silently None when not attached to a console, so piped and test runs
/// never trigger a GUI. Windows: SHBrowseForFolderW; posix: the
/// zenity/kdialog shell-out (python tries tkinter first - not ported,
/// recorded deviation).
pub fn folder_dialog(initial: &str, world: &World) -> Option<String> {
    if !(world.stdin_tty && world.stdout_tty) {
        return None;
    }
    #[cfg(windows)]
    {
        windows_folder_dialog()
    }
    #[cfg(unix)]
    {
        posix_folder_dialog(initial)
    }
}

#[cfg(windows)]
fn windows_folder_dialog() -> Option<String> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        BIF_EDITBOX, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW,
        SHGetPathFromIDListW,
    };
    const MAX_PATH_W: usize = 260;
    let title: Vec<u16> = "choose download folder\0".encode_utf16().collect();
    let mut display = [0u16; MAX_PATH_W];
    let mut info: BROWSEINFOW = unsafe { std::mem::zeroed() };
    info.pszDisplayName = display.as_mut_ptr();
    info.lpszTitle = title.as_ptr();
    info.ulFlags = BIF_RETURNONLYFSDIRS | BIF_EDITBOX | BIF_NEWDIALOGSTYLE;
    let pidl = unsafe { SHBrowseForFolderW(&info) };
    if pidl.is_null() {
        return None;
    }
    let mut path = [0u16; MAX_PATH_W];
    let ok = unsafe { SHGetPathFromIDListW(pidl, path.as_mut_ptr()) };
    unsafe { CoTaskMemFree(Some(pidl.cast())) };
    if ok == 0 {
        return None;
    }
    let len = path.iter().position(|&c| c == 0).unwrap_or(path.len());
    let picked = String::from_utf16_lossy(&path[..len]);
    if picked.is_empty() { None } else { Some(picked) }
}

#[cfg(unix)]
fn posix_folder_dialog(initial: &str) -> Option<String> {
    let attempts: [(&str, &[&str]); 2] = [
        ("zenity", &["--file-selection", "--directory"]),
        ("kdialog", &["--getexistingdirectory"]),
    ];
    for (program, args) in attempts {
        if resolve::which_from_process(program).is_none() {
            continue;
        }
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        if !initial.is_empty() {
            cmd.arg(initial);
        }
        let Ok(out) = cmd.output() else {
            continue;
        };
        if out.status.success() {
            let picked = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !picked.is_empty() {
                return Some(picked);
            }
        }
    }
    None
}

// ------------------------------------------------------------------ act

/// `act(choice, s)`: one menu pick. Returns Repaint(needs_repaint),
/// python's bool(act(...)) with the None branches as Repaint(false), or
/// Exit for item 13.
pub fn act(
    choice: &str,
    s: &mut Obj,
    io: &mut Io,
    world: &World,
    download: &mut dyn FnMut(&mut Obj, &str) -> i32,
) -> Result<ActOutcome, MenuErr> {
    let is_audio_only = matches!(config::get(s, "type"), Some(Value::Str(t)) if t == "audio only");
    let repaint = match choice {
        "1" => {
            let picked = pick_list(
                "change type: video / video no audio / audio only",
                &TYPES,
                config::get(s, "type"),
                io,
                world,
            )?;
            if let Some(value) = picked {
                config::set(s, "type", value);
            }
            true
        }
        "2" => {
            if is_audio_only {
                print_line(io, "  audio only mode has no container");
                return Ok(ActOutcome::Repaint(false));
            }
            let picked = pick_list(
                "container",
                &CONTAINERS,
                config::get(s, "video_container"),
                io,
                world,
            )?;
            if let Some(value) = picked {
                config::set(s, "video_container", value);
            }
            true
        }
        "3" => {
            if is_audio_only {
                print_line(io, "  audio only mode has no resolution");
                return Ok(ActOutcome::Repaint(false));
            }
            let picked = pick_list(
                "max height",
                &RESOLUTIONS,
                config::get(s, "video_resolution"),
                io,
                world,
            )?;
            if let Some(value) = picked {
                config::set(s, "video_resolution", value);
            }
            true
        }
        "4" => {
            let picked = pick_list(
                "audio format (needs ffmpeg to convert)",
                &AUDIO_FORMATS,
                config::get(s, "audio_format"),
                io,
                world,
            )?;
            if let Some(value) = picked {
                config::set(s, "audio_format", value);
            }
            true
        }
        "5" => {
            let current = outdir_of(s, world)?;
            if let Some(picked) = folder_dialog(&current, world) {
                config::set(s, "output_dir", Value::Str(picked));
                return Ok(ActOutcome::Repaint(false));
            }
            let prompt = format!("  folder (enter = keep {current}): ");
            let raw = read_line_or(io, &prompt, None)?;
            let raw = raw.trim().trim_matches('"').to_string();
            if !raw.is_empty() {
                config::set(s, "output_dir", Value::Str(raw));
            }
            true
        }
        "6" => {
            print_line(io, "  filename template - standard templates:");
            let current = config::get(s, "filename_template");
            for (i, preset) in TEMPLATE_PRESETS.iter().enumerate() {
                let selected = matches!(current, Some(Value::Str(t)) if t == preset);
                let row = format!(
                    "    {} {:>2}  {}",
                    ui::sel_mark(selected, world.use_color),
                    i + 1,
                    preset
                );
                print_line(io, &row);
            }
            let prompt = format!(
                "  number 1-{}, or type a template (enter = keep): ",
                TEMPLATE_PRESETS.len()
            );
            let raw = read_line_or(io, &prompt, None)?;
            let raw = raw.trim().to_string();
            if !raw.is_empty() {
                // python: raw.isdigit() and 1 <= int(raw) <= len
                let isdigits = raw.chars().all(|ch| ch.is_ascii_digit());
                let slot = if isdigits { raw.parse::<i64>().ok() } else { None };
                match slot {
                    Some(n) if (1..=TEMPLATE_PRESETS.len() as i64).contains(&n) => {
                        config::set(
                            s,
                            "filename_template",
                            Value::Str(TEMPLATE_PRESETS[(n - 1) as usize].to_string()),
                        );
                    }
                    _ => config::set(s, "filename_template", Value::Str(raw)),
                }
            }
            true
        }
        "7" => {
            let answer = ask_bool("subtitles", config::get(s, "subtitles"), io)?;
            if let Some(value) = answer {
                config::set(s, "subtitles", value);
            }
            let subtitles = config::get(s, "subtitles").is_some_and(config::py_truthy);
            if subtitles {
                let plain = ui::sub_lang_lines(WIDTH);
                let block: Vec<String> = plain
                    .iter()
                    .map(|line| c(world.use_color, A_DARKGREY, line))
                    .collect();
                let under =
                    tty::block_under_input(&plain, world.use_color, world.stdin_tty, world.terminal);
                if !under {
                    // no room under a live cursor: the block goes above the
                    // input, where it is on screen while the value is typed
                    print_lines(io, &block);
                }
                print_line(io, "  enter languages separated by commas (enter = keep):");
                let below = if under { Some(plain.as_slice()) } else { None };
                let raw = read_line_or(io, "", below)?;
                let raw = raw.trim().to_string();
                if !raw.is_empty() {
                    let (kept, bad, unknown) = parse_lang_input(Some(&raw));
                    if !kept.is_empty() {
                        config::set(s, "subtitle_langs", Value::Str(kept.join(",")));
                    }
                    let mut said: Vec<String> = Vec::new();
                    if !bad.is_empty() {
                        said.push(format!("not a language code, dropped: {}", bad.join(", ")));
                    }
                    if !unknown.is_empty() {
                        said.push(format!(
                            "not in the language list, kept: {}",
                            unknown.join(", ")
                        ));
                    }
                    if !bad.is_empty() && kept.is_empty() {
                        said.push(format!(
                            "nothing valid left, kept {}",
                            value_text(config::get(s, "subtitle_langs"))
                        ));
                    }
                    if under && !said.is_empty() {
                        // the input row becomes the verdict and the block is
                        // repainted under it: step back onto the input row,
                        // rewrite it, redraw the block, step back up
                        io.out.write("\x1b[1A\r\x1b[2K");
                        for text in &said {
                            print_line(io, &c(world.use_color, A_AMBER, &format!("  {text}")));
                        }
                        print_lines(io, &block);
                        io.out.write(&format!("\x1b[{}A", block.len() + 1));
                        io.out.flush();
                        tty::hold(io.out, io.reader);
                    } else {
                        for text in &said {
                            print_line(io, &c(world.use_color, A_AMBER, &format!("  {text}")));
                        }
                        if !under {
                            tty::hold(io.out, io.reader);
                        }
                    }
                }
            }
            true
        }
        "8" => {
            let answer = ask_bool("playlist mode", config::get(s, "playlist"), io)?;
            if let Some(value) = answer {
                config::set(s, "playlist", value);
            }
            let playlist = config::get(s, "playlist").is_some_and(config::py_truthy);
            if playlist {
                let prompt = format!(
                    "  max items (0 = all, enter = keep {}): ",
                    value_text(config::get(s, "playlist_max_items"))
                );
                let raw = read_line_or(io, &prompt, None)?;
                let raw = raw.trim().to_string();
                if !raw.is_empty() {
                    match config::py_int(&raw) {
                        Some(n) => config::set(s, "playlist_max_items", Value::Int(n)),
                        None => {
                            print_line(
                                io,
                                &format!(
                                    "  not a number - kept {}",
                                    value_text(config::get(s, "playlist_max_items"))
                                ),
                            );
                            tty::hold(io.out, io.reader);
                        }
                    }
                }
            }
            true
        }
        "9" => return Ok(ActOutcome::Repaint(multi_url(s, io, world, download)?)),
        "10" => {
            for (key, value) in config::defaults() {
                config::set(s, &key, value);
            }
            print_line(io, "  defaults restored");
            tty::hold(io.out, io.reader);
            true
        }
        "11" => {
            // the updaters arrive with phase 8; until then a typed
            // refusal (the phase-4 wires-only discipline)
            print_line(io, "  updaters are not wired in this build yet (native port phase 8)");
            tty::hold(io.out, io.reader);
            true
        }
        "12" => {
            let new_value = !config::get(s, "auto_update").is_some_and(config::py_truthy);
            config::set(s, "auto_update", Value::Bool(new_value));
            // no save here (audit F8): main_menu saves after act returns,
            // so every action has exactly one save site
            print_line(
                io,
                &format!(
                    "  auto update {} - checked once a day at start",
                    ui::onoff(Some(&Value::Bool(new_value)))
                ),
            );
            tty::hold(io.out, io.reader);
            true
        }
        "13" | "q" | "quit" | "exit" => {
            save(s, world)?;
            print_line(io, &format!("bye. defaults saved to {}", world.config_path));
            return Ok(ActOutcome::Exit);
        }
        _ => {
            if !choice.is_empty() {
                print_line(io, "  pick 1-13 or paste a url");
            }
            true
        }
    };
    Ok(ActOutcome::Repaint(repaint))
}

// ------------------------------------------------------------- main loop

/// The phase-6 startup auto-check: python's gates and throttle with the
/// fetch unwired until phase 8 - a due check is skipped silently, with
/// no network contact and no stamp (recorded in the phase-6 port notes;
/// FR-13's trigger rule - menu start only, tty only, once a day - is
/// already enforced here).
pub fn auto_update_check_startup(s: &mut Obj) {
    if !config::get(s, "auto_update").is_some_and(config::py_truthy) {
        return;
    }
    if !tty::stdin_is_tty() {
        return;
    }
    let last = match config::get(s, "auto_update_last_check") {
        Some(Value::Int(n)) => *n as f64,
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    if now - last < 86400.0 {
        return;
    }
    // phase 8: one GET, the version compare and the install prompt
}

/// `main_menu`: the loop the whole interactive product runs in.
/// `download` is the phase-7 downloader (a typed refusal until then);
/// `quiet_check` is the startup auto-update check (phase 8; the phase-6
/// implementation keeps its gates and skips the fetch).
pub fn main_menu(
    s: &mut Obj,
    io: &mut Io,
    world: &World,
    download: &mut dyn FnMut(&mut Obj, &str) -> i32,
    quiet_check: &mut dyn FnMut(&mut Obj),
) -> Result<i32, MenuErr> {
    tty::enable_vt_output();
    let mut dirty = true;
    let mut results = false;
    let mut started = false;
    loop {
        if dirty {
            if world.use_color {
                io.out.write(CLEAR);
                io.out.flush();
            }
            let outdir = outdir_of(s, world)?;
            print_lines(
                io,
                &ui::render_lines(world.version, s, &outdir, world.is_windows, world.use_color)?,
            );
            print_lines(io, &ui::hint_lines(&outdir, world.use_color));
            dirty = false;
            results = false;
        }
        if !started {
            // after the first paint, so the notice is not wiped by the clear
            started = true;
            quiet_check(s);
            save(s, world)?;
        }
        match read(io, "> ", None) {
            ReadOutcome::Eof | ReadOutcome::Interrupted => {
                io.out.write("\n");
                io.out.flush();
                return Ok(0);
            }
            ReadOutcome::Escape => {
                // escape: wipe the results if any are on screen, else no-op
                if results {
                    dirty = true;
                }
            }
            ReadOutcome::Line(line) => {
                let choice = line.trim().to_string();
                if let Some(url) = clean_url(Some(&choice)).filter(|u| !u.is_empty()) {
                    // a new url starts from a fresh menu on top, results below
                    if world.use_color {
                        io.out.write(CLEAR);
                        io.out.flush();
                    }
                    let outdir = outdir_of(s, world)?;
                    print_lines(
                        io,
                        &ui::render_lines(
                            world.version,
                            s,
                            &outdir,
                            world.is_windows,
                            world.use_color,
                        )?,
                    );
                    print_lines(io, &ui::hint_lines(&outdir, world.use_color));
                    download(s, &url);
                    results = true;
                    save(s, world)?;
                    continue;
                }
                let repaint = match act(&choice, s, io, world, download) {
                    Ok(ActOutcome::Repaint(repaint)) => repaint,
                    Ok(ActOutcome::Exit) => return Ok(0),
                    Err(MenuErr::GoBack) => true,
                    Err(other) => return Err(other),
                };
                // item 9 answers false when downloads ran: the screen then
                // already holds the menu with the last result lines under it
                dirty = repaint;
                results = !dirty;
                save(s, world)?;
            }
        }
    }
}

/// The four shipped filename templates (python TEMPLATE_PRESETS).
pub const TEMPLATE_PRESETS: [&str; 4] = [
    TEMPLATE_DEFAULT,
    "%(title)s %(upload_date)s",
    "%(title)s %(timestamp>%Y-%m-%d %H-%M)s",
    "%(title)s %(id)s",
];
