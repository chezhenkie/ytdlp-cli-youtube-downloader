//! The interactive menu (specsheet section 4, phase-6 port notes):
//! pick_list, ask_bool, the batch list, run_batch, multi_url, the
//! folder dialog, act and the main loop. Everything here is driven
//! through the tty seams, so the whole menu runs in tests and in the
//! differential harness without a terminal.
//!
//! Port rules recorded in the phase-6 notes: items 1-9, 11, 13 and 14
//! are wired in full; the batch downloads (item 10), the item 12
//! updaters and the URL quick mode are the caller's jobs (phase 7/8)
//! and arrive here as typed refusals or injected closures. "keep
//! current" answers return None instead of python's raw-value
//! round-trip: a loaded settings object always carries every key, so
//! the two are indistinguishable in every reachable state.

use crate::args::{TEMPLATE_DEFAULT, clean_url, parse_lang_input};
use crate::config::{self, AUDIO_FORMATS, CONTAINERS, Obj, RESOLUTIONS, SRT_MODES, TYPES, Value};
use crate::resolve::{self, ShapeKind};
use crate::term::{A_AMBER, A_DARKGREY, A_GREY, CLEAR, c};
use crate::tty::{self, Out, ReadOutcome, Reader};
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
    /// the system-clipboard read (FR-17): a seam so tests script it
    pub clipboard: &'a dyn Fn() -> Option<String>,
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

/// The main prompt's read (FR-17): on a real tty a bare "0" acts on the
/// keypress; every other call site keeps the plain line contract.
fn read_quick0(io: &mut Io, prompt: &str, below: Option<&[String]>) -> ReadOutcome {
    io.reader.read_line_quick0(prompt, below, io.out)
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
    let prompt = format!(
        "  {label} (now {}) y/n (enter = keep): ",
        ui::onoff(current)
    );
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
    download: &mut dyn FnMut(&mut Obj, &str, &mut Io) -> i32,
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
                &format!(
                    "  {} not a url - skipped",
                    c(world.use_color, A_AMBER, "...")
                ),
            );
            continue;
        };
        if download(s, &url, io) == 130 {
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
    download: &mut dyn FnMut(&mut Obj, &str, &mut Io) -> i32,
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
        // python's windows dialog ignores the initial path (BROWSEINFO
        // has no initial-directory field)
        let _ = initial;
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
    unsafe { CoTaskMemFree(pidl.cast()) };
    if ok == 0 {
        return None;
    }
    let len = path.iter().position(|&c| c == 0).unwrap_or(path.len());
    let picked = String::from_utf16_lossy(&path[..len]);
    if picked.is_empty() {
        None
    } else {
        Some(picked)
    }
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
    download: &mut dyn FnMut(&mut Obj, &str, &mut Io) -> i32,
    update: &mut dyn FnMut(&mut Obj, &mut Io) -> Result<(), MenuErr>,
) -> Result<ActOutcome, MenuErr> {
    let is_audio_only = matches!(config::get(s, "type"), Some(Value::Str(t)) if t == "audio only");
    let repaint = match choice {
        // FR-17: the clipboard quick-paste - the same screen and result
        // state as a pasted url plus enter
        "0" => {
            let link = (world.clipboard)()
                .and_then(|text| clean_url(Some(&text)))
                .filter(|u| !u.is_empty());
            match link {
                Some(url) => {
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
                    download(s, &url, io);
                    return Ok(ActOutcome::Repaint(false));
                }
                None => {
                    print_line(
                        io,
                        &c(world.use_color, A_AMBER, "  no link on the clipboard"),
                    );
                    tty::hold(io.out, io.reader);
                    return Ok(ActOutcome::Repaint(true));
                }
            }
        }
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
                let slot = if isdigits {
                    raw.parse::<i64>().ok()
                } else {
                    None
                };
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
                let under = tty::block_under_input(
                    &plain,
                    world.use_color,
                    world.stdin_tty,
                    world.terminal,
                );
                if !under {
                    // no room under a live cursor: the block goes above the
                    // input, where it is on screen while the value is typed
                    print_lines(io, &block);
                }
                print_line(io, "  enter languages separated by commas (enter = keep):");
                // python passes the COLORED block as `below` and the plain
                // lines to the room check
                let below = if under { Some(block.as_slice()) } else { None };
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
            let answer = ask_bool("srt timecode stripper", config::get(s, "srt_stripper"), io)?;
            if let Some(value) = answer {
                config::set(s, "srt_stripper", value);
            }
            if config::get(s, "srt_stripper").is_some_and(config::py_truthy) {
                let picked = pick_list(
                    "stripper mode",
                    &SRT_MODES,
                    config::get(s, "srt_stripper_mode"),
                    io,
                    world,
                )?;
                if let Some(value) = picked {
                    config::set(s, "srt_stripper_mode", value);
                }
            }
            true
        }
        "9" => {
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
        "10" => return Ok(ActOutcome::Repaint(multi_url(s, io, world, download)?)),
        "11" => {
            for (key, value) in config::defaults() {
                config::set(s, &key, value);
            }
            print_line(io, "  defaults restored");
            tty::hold(io.out, io.reader);
            true
        }
        "12" => {
            // the updaters run here (phase 8): both halves behind
            // update_all, then python's "press enter to go back" hold
            update(s, io)?;
            match read(io, "  press enter to go back: ", None) {
                ReadOutcome::Line(_) | ReadOutcome::Escape => {}
                ReadOutcome::Eof => return Err(MenuErr::Eof),
                ReadOutcome::Interrupted => return Err(MenuErr::Interrupted),
            }
            true
        }
        "13" => {
            let new_value = !config::get(s, "auto_update").is_some_and(config::py_truthy);
            config::set(s, "auto_update", Value::Bool(new_value));
            // no save here (audit F8): main_menu saves after act returns,
            // so every action has exactly one save site
            // owner call 2026-10-17i: the off notice says what off means
            let tail = if new_value {
                "checked once a day at start"
            } else {
                "no startup check"
            };
            print_line(
                io,
                &format!(
                    "  auto update {} - {tail}",
                    ui::onoff(Some(&Value::Bool(new_value)))
                ),
            );
            tty::hold(io.out, io.reader);
            true
        }
        "14" | "q" | "quit" | "exit" => {
            save(s, world)?;
            print_line(io, &format!("bye. defaults saved to {}", world.config_path));
            return Ok(ActOutcome::Exit);
        }
        _ => {
            if !choice.is_empty() {
                print_line(io, "  pick 1-14 or paste a url");
            }
            true
        }
    };
    Ok(ActOutcome::Repaint(repaint))
}

// ------------------------------------------------------------- main loop

/// `main_menu`: the loop the whole interactive product runs in.
/// `download` is the phase-7 downloader (a typed refusal until then);
/// `update` is the phase-8 updater pair item 11 runs; `quiet_check` is
/// the startup auto-update check - python's `auto_update_check(s)` at
/// the one call site FR-13 pins, which now really fetches.
pub fn main_menu(
    s: &mut Obj,
    io: &mut Io,
    world: &World,
    download: &mut dyn FnMut(&mut Obj, &str, &mut Io) -> i32,
    update: &mut dyn FnMut(&mut Obj, &mut Io) -> Result<(), MenuErr>,
    quiet_check: &mut dyn FnMut(&mut Obj, &mut Io) -> Result<(), MenuErr>,
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
            quiet_check(s, io)?;
            save(s, world)?;
        }
        match read_quick0(io, "> ", None) {
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
                    download(s, &url, io);
                    results = true;
                    save(s, world)?;
                    continue;
                }
                let repaint = match act(&choice, s, io, world, download, update) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tty::{RecEvent, RecordingOut, ScriptItem, ScriptedReader};

    struct TempConfig {
        dir: std::path::PathBuf,
    }

    impl TempConfig {
        fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::SeqCst);
            let dir =
                std::env::temp_dir().join(format!("ytdlp-menu-test-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self { dir }
        }

        fn config_path(&self) -> String {
            self.dir
                .join("settings.json")
                .to_string_lossy()
                .into_owned()
        }

        fn read(&self) -> String {
            std::fs::read_to_string(self.config_path()).unwrap_or_default()
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn no_link() -> Option<String> {
        None
    }

    fn world<'a>(
        config_path: &'a str,
        version: &'a str,
        use_color: bool,
        stdin_tty: bool,
        terminal: Option<(i64, i64)>,
    ) -> World<'a> {
        world_clip(
            config_path,
            version,
            use_color,
            stdin_tty,
            terminal,
            &no_link,
        )
    }

    fn world_clip<'a>(
        config_path: &'a str,
        version: &'a str,
        use_color: bool,
        stdin_tty: bool,
        terminal: Option<(i64, i64)>,
        clipboard: &'a dyn Fn() -> Option<String>,
    ) -> World<'a> {
        World {
            version,
            root: "",
            config_path,
            is_windows: false,
            use_color,
            stdin_tty,
            stdout_tty: use_color,
            terminal,
            auto: None,
            clipboard,
        }
    }

    fn mega(choice: &str, replies: Vec<ScriptItem>, settings: &mut Obj) -> (String, Vec<String>) {
        let tmp = TempConfig::new();
        let path = tmp.config_path();
        std::fs::write(&path, config::render_settings_file(settings)).expect("seed");
        let mut rec = RecordingOut::default();
        let mut reader = ScriptedReader::new(replies);
        let mut download = |_s: &mut Obj, _url: &str, _io: &mut Io| -> i32 { 0 };
        let mut update = |_s: &mut Obj, _io: &mut Io| -> Result<(), MenuErr> { Ok(()) };
        let w = world(&path, "1.5.0", false, false, None);
        let outcome = {
            let mut io = Io {
                out: &mut rec,
                reader: &mut reader,
            };
            act(choice, settings, &mut io, &w, &mut download, &mut update)
        };
        assert!(
            outcome.is_ok(),
            "act({choice}) failed: {outcome:?}",
            outcome = outcome.err()
        );
        (rec.text(), reader.prompts)
    }

    /// Every interactive write lands flushed before the next logical
    /// write (G6's "no frame sits in a buffer" pin, menu side): for a
    /// pick_list flow the writes group as title + two rows, each group
    /// closed by a flush, and nothing dangles after the last flush.
    #[test]
    fn interactive_writes_flush() {
        let mut rec = RecordingOut::default();
        let mut reader = ScriptedReader::new(vec![ScriptItem::Line("2".to_string())]);
        let current = Value::Str("a".to_string());
        let w = world("", "1.5.0", false, false, None);
        {
            let mut io = Io {
                out: &mut rec,
                reader: &mut reader,
            };
            pick_list("t", &["a", "b"], Some(&current), &mut io, &w).unwrap();
        }
        let events = rec.events();
        assert!(matches!(events.last(), Some(RecEvent::Flush)), "{events:?}");
        assert_eq!(rec.flush_count(), 3, "title + two rows: one flush each");
        for (i, event) in events.iter().enumerate() {
            if let RecEvent::Flush = event {
                assert!(
                    matches!(events.get(i + 1), None | Some(RecEvent::Write(_))),
                    "a flush must close a write group: {events:?}"
                );
            }
        }
    }

    #[test]
    fn pick_list_flows_and_holds() {
        let current = Value::Str("a".to_string());
        let run = |replies: Vec<ScriptItem>| {
            let mut rec = RecordingOut::default();
            let mut reader = ScriptedReader::new(replies);
            let w = world("", "1.5.0", false, false, None);
            let result = {
                let mut io = Io {
                    out: &mut rec,
                    reader: &mut reader,
                };
                pick_list("t", &["a", "b"], Some(&current), &mut io, &w)
            };
            (rec.text(), reader.prompts.clone(), result)
        };

        let (text, prompts, result) = run(vec![ScriptItem::Line("2".to_string())]);
        assert_eq!(result.unwrap(), Some(Value::Str("b".to_string())));
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0], "  number (enter = keep a): ");
        assert!(text.contains("    [x]  1  a"), "{text}");
        assert!(text.contains("    [ ]  2  b"), "{text}");

        let (text, prompts, result) = run(vec![
            ScriptItem::Line("abc".to_string()),
            ScriptItem::Line(String::new()),
        ]);
        assert_eq!(result.unwrap(), None);
        assert_eq!(prompts, vec!["  number (enter = keep a): ", ""]);
        assert!(text.contains("  invalid number"));

        let (text, prompts, result) = run(vec![
            ScriptItem::Line("9".to_string()),
            ScriptItem::Line(String::new()),
        ]);
        assert_eq!(result.unwrap(), None);
        assert_eq!(prompts.len(), 2);
        assert!(text.contains("  out of range"));

        let (_, prompts, result) = run(vec![ScriptItem::Line(String::new())]);
        assert_eq!(result.unwrap(), None, "empty keeps the current value");
        assert_eq!(prompts.len(), 1, "keep does not hold");

        let (_, _, result) = run(vec![ScriptItem::Escape]);
        assert!(matches!(result, Err(MenuErr::GoBack)));
        let (_, _, result) = run(vec![ScriptItem::Eof]);
        assert!(matches!(result, Err(MenuErr::Eof)));
        let (_, _, result) = run(vec![ScriptItem::Interrupted]);
        assert!(matches!(result, Err(MenuErr::Interrupted)));
    }

    #[test]
    fn ask_bool_answers() {
        let run = |reply: &str| {
            let mut rec = RecordingOut::default();
            let mut reader = ScriptedReader::new(vec![ScriptItem::Line(reply.to_string())]);
            let current = Value::Bool(true);
            let mut io = Io {
                out: &mut rec,
                reader: &mut reader,
            };
            ask_bool("subtitles", Some(&current), &mut io).unwrap()
        };
        assert_eq!(run("y"), Some(Value::Bool(true)));
        assert_eq!(run("yes"), Some(Value::Bool(true)));
        assert_eq!(run("off"), Some(Value::Bool(false)));
        assert_eq!(run("0"), Some(Value::Bool(false)));
        assert_eq!(run("maybe"), None, "anything else keeps the current value");
        assert_eq!(run(""), None);
    }

    /// Audit F8-style save discipline: item 13 changes the value but
    /// saves nothing; the quit path saves; main_menu saves after act.
    #[test]
    fn save_points() {
        let tmp = TempConfig::new();
        let path = tmp.config_path();
        let mut s = config::defaults();
        std::fs::write(&path, config::render_settings_file(&s)).expect("seed");
        let seeded = tmp.read();

        let mut rec = RecordingOut::default();
        let mut reader = ScriptedReader::new(vec![ScriptItem::Line(String::new())]);
        let mut download = |_s: &mut Obj, _url: &str, _io: &mut Io| -> i32 { 0 };
        let mut update = |_s: &mut Obj, _io: &mut Io| -> Result<(), MenuErr> { Ok(()) };
        let w = world(&path, "1.5.0", false, false, None);
        {
            let mut io = Io {
                out: &mut rec,
                reader: &mut reader,
            };
            let outcome = act("13", &mut s, &mut io, &w, &mut download, &mut update).unwrap();
            assert!(matches!(outcome, ActOutcome::Repaint(true)));
        }
        assert_eq!(config::get(&s, "auto_update"), Some(&Value::Bool(false)));
        assert_eq!(tmp.read(), seeded, "item 13 must not save");

        let mut rec = RecordingOut::default();
        let mut reader = ScriptedReader::new(vec![
            ScriptItem::Line("13".to_string()),
            ScriptItem::Line(String::new()),
            ScriptItem::Line("14".to_string()),
        ]);
        let mut s = config::defaults();
        std::fs::write(&path, config::render_settings_file(&s)).expect("seed");
        let mut quiet = |_s: &mut Obj, _io: &mut Io| Ok(());
        {
            let mut io = Io {
                out: &mut rec,
                reader: &mut reader,
            };
            let code =
                main_menu(&mut s, &mut io, &w, &mut download, &mut update, &mut quiet).unwrap();
            assert_eq!(code, 0);
        }
        assert!(
            tmp.read().contains("\"auto_update\": false"),
            "quit saves the toggled value: {}",
            tmp.read()
        );
    }

    /// The hold rule per site: a refusal or notice followed by an
    /// unlabelled read, so the menu repaint cannot eat the message.
    #[test]
    fn hold_sites() {
        // item 9 with a non-numeric max
        let mut s = config::defaults();
        config::set(&mut s, "playlist", Value::Bool(true));
        let (text, prompts) = mega(
            "9",
            vec![
                ScriptItem::Line("y".to_string()),
                ScriptItem::Line("abc".to_string()),
                ScriptItem::Line(String::new()),
            ],
            &mut s,
        );
        assert!(text.contains("  not a number - kept 0"), "{text}");
        assert_eq!(prompts.last().unwrap(), "", "item 9 holds");

        // item 11's notice
        let mut s = config::defaults();
        config::set(&mut s, "auto_update", Value::Bool(false));
        let (text, prompts) = mega("11", vec![ScriptItem::Line(String::new())], &mut s);
        assert!(text.contains("  defaults restored"));
        assert_eq!(prompts.last().unwrap(), "", "item 11 holds");
        assert_eq!(config::get(&s, "auto_update"), Some(&Value::Bool(true)));

        // item 13's notice
        let mut s = config::defaults();
        let (text, prompts) = mega("13", vec![ScriptItem::Line(String::new())], &mut s);
        assert!(text.contains("  auto update off - no startup check"));
        assert_eq!(prompts.last().unwrap(), "", "item 13 holds");
        let mut s = config::defaults();
        config::set(&mut s, "auto_update", Value::Bool(false));
        let (text, _) = mega("13", vec![ScriptItem::Line(String::new())], &mut s);
        assert!(text.contains("  auto update on - checked once a day at start"));
    }

    /// FR-20: item 8 toggles the stripper and, when on, asks the mode
    /// with the pick list; off answers stop before the mode prompt.
    #[test]
    fn item8_toggles_and_picks_the_mode() {
        let mut s = config::defaults();
        let (text, prompts) = mega(
            "8",
            vec![
                ScriptItem::Line("y".to_string()),
                ScriptItem::Line("3".to_string()),
            ],
            &mut s,
        );
        assert!(text.contains("  stripper mode"), "{text}");
        assert!(text.contains("  3  numbered"), "{text}");
        assert_eq!(
            prompts[0], "  srt timecode stripper (now off) y/n (enter = keep): ",
            "the bool prompt names the item"
        );
        assert_eq!(prompts.len(), 2, "bool prompt then the pick prompt");
        assert_eq!(
            config::get(&s, "srt_stripper"),
            Some(&Value::Bool(true)),
            "y turns the stripper on"
        );
        assert_eq!(
            config::get(&s, "srt_stripper_mode")
                .map(config::py_str)
                .as_deref(),
            Some("numbered"),
            "pick 3 stores the mode"
        );

        let mut s = config::defaults();
        let (text, prompts) = mega("8", vec![ScriptItem::Line("n".to_string())], &mut s);
        assert_eq!(config::get(&s, "srt_stripper"), Some(&Value::Bool(false)));
        assert!(
            config::get(&s, "srt_stripper_mode")
                .map(config::py_str)
                .as_deref()
                == Some("alpha")
        );
        assert!(!text.contains("  stripper mode"), "off asks no mode");
        assert_eq!(prompts.len(), 1);

        let mut s = config::defaults();
        config::set(&mut s, "srt_stripper", Value::Bool(true));
        let (_, prompts) = mega(
            "8",
            vec![
                ScriptItem::Line(String::new()),
                ScriptItem::Line(String::new()),
            ],
            &mut s,
        );
        assert_eq!(prompts.len(), 2, "keep-current still asks the mode when on");
        assert_eq!(
            config::get(&s, "srt_stripper_mode")
                .map(config::py_str)
                .as_deref(),
            Some("alpha")
        );
    }

    /// FR-17: zero reads the clipboard seam; a link runs the url path's
    /// screen (Repaint(false) keeps the results on screen), and the paste
    /// is cleaned by clean_url before the download sees it.
    #[test]
    fn item0_pastes_and_starts() {
        let tmp = TempConfig::new();
        let path = tmp.config_path();
        let mut s = config::defaults();
        std::fs::write(&path, config::render_settings_file(&s)).expect("seed");
        let clip = || Some("watch https://youtu.be/abc, nice".to_string());
        let mut got: Option<String> = None;
        let mut rec = RecordingOut::default();
        let mut reader = ScriptedReader::new(vec![]);
        let mut download = |_s: &mut Obj, url: &str, _io: &mut Io| -> i32 {
            got = Some(url.to_string());
            0
        };
        let mut update = |_s: &mut Obj, _io: &mut Io| -> Result<(), MenuErr> { Ok(()) };
        let w = world_clip(&path, "2.0.2", false, false, None, &clip);
        let outcome = {
            let mut io = Io {
                out: &mut rec,
                reader: &mut reader,
            };
            act("0", &mut s, &mut io, &w, &mut download, &mut update).unwrap()
        };
        assert!(matches!(outcome, ActOutcome::Repaint(false)));
        assert_eq!(got.as_deref(), Some("https://youtu.be/abc"));
        assert!(rec.text().contains("yt-dlp cli  v2.0.2"), "menu repainted");
        assert!(reader.prompts.is_empty(), "no extra read");
    }

    /// FR-17: no clipboard (the default world) and a paste without a
    /// link both print one amber line and hold before the repaint.
    #[test]
    fn item0_reports_a_missing_link() {
        let mut s = config::defaults();
        let (text, prompts) = mega("0", vec![ScriptItem::Line(String::new())], &mut s);
        assert!(text.contains("  no link on the clipboard"), "{text}");
        assert_eq!(prompts.last().unwrap(), "", "item 0 holds");

        let tmp = TempConfig::new();
        let path = tmp.config_path();
        let mut s = config::defaults();
        std::fs::write(&path, config::render_settings_file(&s)).expect("seed");
        let clip = || Some("nothing interesting".to_string());
        let mut rec = RecordingOut::default();
        let mut reader = ScriptedReader::new(vec![ScriptItem::Line(String::new())]);
        let mut download = |_s: &mut Obj, _url: &str, _io: &mut Io| -> i32 { 0 };
        let mut update = |_s: &mut Obj, _io: &mut Io| -> Result<(), MenuErr> { Ok(()) };
        let w = world_clip(&path, "2.0.2", false, false, None, &clip);
        let outcome = {
            let mut io = Io {
                out: &mut rec,
                reader: &mut reader,
            };
            act("0", &mut s, &mut io, &w, &mut download, &mut update).unwrap()
        };
        assert!(matches!(outcome, ActOutcome::Repaint(true)));
        assert!(rec.text().contains("  no link on the clipboard"));
    }

    /// The [11,1,11] cursor dance: the ten-line language block is handed
    /// to the reader as the colored `below` block (len(block)+1 = 11 up),
    /// and the verdict rewinds one row, rewrites it, repaints the block
    /// and steps back up.
    #[test]
    fn item7_under_input_cursor_layout() {
        let tmp = TempConfig::new();
        let path = tmp.config_path();
        let mut s = config::defaults();
        std::fs::write(&path, config::render_settings_file(&s)).expect("seed");
        let mut rec = RecordingOut::default();
        let mut reader = ScriptedReader::new(vec![
            ScriptItem::Line("y".to_string()),
            ScriptItem::Line("en,8".to_string()),
            ScriptItem::Line(String::new()),
        ]);
        let mut download = |_s: &mut Obj, _url: &str, _io: &mut Io| -> i32 { 0 };
        let mut update = |_s: &mut Obj, _io: &mut Io| -> Result<(), MenuErr> { Ok(()) };
        let w = world(&path, "1.5.0", true, true, Some((120, 50)));
        {
            let mut io = Io {
                out: &mut rec,
                reader: &mut reader,
            };
            let outcome = act("7", &mut s, &mut io, &w, &mut download, &mut update).unwrap();
            assert!(matches!(outcome, ActOutcome::Repaint(true)));
        }
        let plain = ui::sub_lang_lines(WIDTH);
        assert_eq!(plain.len(), 10);
        let block: Vec<String> = plain.iter().map(|line| c(true, A_DARKGREY, line)).collect();
        let below = reader.belows[1]
            .clone()
            .expect("the lang read gets a below block");
        assert_eq!(below, block, "the colored block goes below the input line");
        let text = rec.text();
        assert!(
            text.contains(&format!("\x1b[{}A", block.len() + 1)),
            "the rewind is len(block)+1"
        );
        assert!(
            text.contains("\x1b[1A\r\x1b[2K"),
            "the verdict rewrites the input row"
        );
        assert!(text.contains("  not a language code, dropped: 8"));
        assert!(text.contains(&format!("\x1b[{}A", block.len() + 1)));
        // the block itself is painted once in the verdict repaint
        assert!(text.contains(&block[0]));
    }

    /// The results flag: only an action that paints its own screen sets
    /// results, and then escape wipes the screen with a repaint (item 9
    /// answered "downloads ran").
    #[test]
    fn escape_wipes_results_only_when_set() {
        let tmp = TempConfig::new();
        let path = tmp.config_path();
        let run = |replies: Vec<ScriptItem>| {
            let mut s = config::defaults();
            std::fs::write(&path, config::render_settings_file(&s)).expect("seed");
            let mut rec = RecordingOut::default();
            let mut reader = ScriptedReader::new(replies);
            let mut download = |_s: &mut Obj, _url: &str, _io: &mut Io| -> i32 { 0 };
            let mut update = |_s: &mut Obj, _io: &mut Io| -> Result<(), MenuErr> { Ok(()) };
            let mut quiet = |_s: &mut Obj, _io: &mut Io| Ok(());
            let w = world(&path, "1.5.0", false, false, None);
            {
                let mut io = Io {
                    out: &mut rec,
                    reader: &mut reader,
                };
                let code =
                    main_menu(&mut s, &mut io, &w, &mut download, &mut update, &mut quiet).unwrap();
                assert_eq!(code, 0);
            }
            rec.text()
        };

        // item 10 with one url and an empty line runs the batch (no
        // repaint requested); the escape then wipes the screen
        let with_escape = run(vec![
            ScriptItem::Line("10".to_string()),
            ScriptItem::Line("https://a/1".to_string()),
            ScriptItem::Line(String::new()),
            ScriptItem::Escape,
            ScriptItem::Line("14".to_string()),
        ]);
        let headers_with = with_escape.matches("yt-dlp cli  v1.5.0").count();
        assert_eq!(headers_with, 5, "boot + 3 batch paints + escape wipe");

        // the same session without the escape never repaints again
        let no_escape = run(vec![
            ScriptItem::Line("10".to_string()),
            ScriptItem::Line("https://a/1".to_string()),
            ScriptItem::Line(String::new()),
            ScriptItem::Line("14".to_string()),
        ]);
        let headers_without = no_escape.matches("yt-dlp cli  v1.5.0").count();
        assert_eq!(headers_without, 4, "boot + 3 batch paints");
    }

    #[test]
    fn main_menu_outcomes() {
        let tmp = TempConfig::new();
        let path = tmp.config_path();
        let run = |replies: Vec<ScriptItem>| {
            let mut s = config::defaults();
            std::fs::write(&path, config::render_settings_file(&s)).expect("seed");
            let mut rec = RecordingOut::default();
            let mut reader = ScriptedReader::new(replies);
            let mut download = |_s: &mut Obj, _url: &str, _io: &mut Io| -> i32 { 0 };
            let mut update = |_s: &mut Obj, _io: &mut Io| -> Result<(), MenuErr> { Ok(()) };
            let mut quiet = |_s: &mut Obj, _io: &mut Io| Ok(());
            let w = world(&path, "1.5.0", false, false, None);
            let mut io = Io {
                out: &mut rec,
                reader: &mut reader,
            };
            (
                main_menu(&mut s, &mut io, &w, &mut download, &mut update, &mut quiet),
                rec.text(),
            )
        };

        // quit exits zero and says bye
        let (result, text) = run(vec![ScriptItem::Line("14".to_string())]);
        assert_eq!(result.unwrap(), 0);
        assert!(text.contains("bye. defaults saved to "));

        // EOF at the main prompt is caught: a newline and exit 0
        let (result, text) = run(vec![ScriptItem::Eof]);
        assert_eq!(result.unwrap(), 0);
        assert!(text.ends_with('\n'));

        // Ctrl+C at the main prompt is caught the same way
        let (result, _) = run(vec![ScriptItem::Interrupted]);
        assert_eq!(result.unwrap(), 0);

        // EOF inside a submenu propagates (python's uncaught traceback
        // path; the port exits 1 with one stderr line in main.rs)
        let (result, _) = run(vec![ScriptItem::Line("7".to_string()), ScriptItem::Eof]);
        assert!(matches!(result, Err(MenuErr::Eof)));

        let (result, _) = run(vec![
            ScriptItem::Line("7".to_string()),
            ScriptItem::Interrupted,
        ]);
        assert!(matches!(result, Err(MenuErr::Interrupted)));

        // escape inside a submenu is GoBack: the menu repaints
        let tmp2 = TempConfig::new();
        let path2 = tmp2.config_path();
        let mut s = config::defaults();
        std::fs::write(&path2, config::render_settings_file(&s)).expect("seed");
        let mut rec = RecordingOut::default();
        let mut reader = ScriptedReader::new(vec![
            ScriptItem::Line("7".to_string()),
            ScriptItem::Escape,
            ScriptItem::Line("14".to_string()),
        ]);
        let mut download = |_s: &mut Obj, _url: &str, _io: &mut Io| -> i32 { 0 };
        let mut update = |_s: &mut Obj, _io: &mut Io| -> Result<(), MenuErr> { Ok(()) };
        let mut quiet = |_s: &mut Obj, _io: &mut Io| Ok(());
        let w = world(&path2, "1.5.0", false, false, None);
        let mut io = Io {
            out: &mut rec,
            reader: &mut reader,
        };
        assert_eq!(
            main_menu(&mut s, &mut io, &w, &mut download, &mut update, &mut quiet).unwrap(),
            0
        );
        assert_eq!(
            rec.text().matches("yt-dlp cli  v1.5.0").count(),
            2,
            "GoBack repaints the menu"
        );
    }
}
