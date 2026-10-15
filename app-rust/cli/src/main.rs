//! ytdlp cli - native binary.
//!
//! Phase 6 (RUST-PORT-PLAN.md) wires the interactive menu: no
//! arguments or `--menu` runs `main_menu`, `--check` keeps its phase-4
//! path. Downloads, the updaters and the URL quick mode land in later
//! phases; until then they print a clear message and exit 2.

use std::io::Write;
use std::process::ExitCode;

use ytdlp_cli::config::{self, LoadOutcome, Obj};
use ytdlp_cli::menu::{self, MenuErr};
use ytdlp_cli::resolve;
use ytdlp_cli::term;
use ytdlp_cli::tty::{self, Out};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("ytdlp: {msg}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--check") {
        return run_check();
    }
    if args.is_empty() || args.iter().any(|a| a == "--menu") {
        return run_menu();
    }
    eprintln!(
        "ytdlp: this build wires --check and the interactive menu (native port phase 6); \
         downloads and updaters land in later phases"
    );
    Ok(ExitCode::from(2))
}

/// `--check` (phase 4, G4): the seven diagnostic lines.
fn run_check() -> Result<ExitCode, String> {
    let root = resolve::real_root()?;
    let env = resolve::env_map_from_process();
    let home = resolve::home_from_process(&env)?;
    let platform = resolve::platform_name(os_name(), sys_platform());
    let is_windows = cfg!(windows);
    let bin_dir = resolve::py_path_join(&root, "bin");
    let config_path = resolve::py_path_join(&root, "config/settings.json");

    // python's load_settings prints first: the unreadable line early-
    // returns the defaults; otherwise the coercion notes.
    let settings = match config::load_settings(std::path::Path::new(&config_path))
        .map_err(|e| e.to_string())?
    {
        LoadOutcome::Loaded { settings, notes } => {
            let mut head: Vec<String> = Vec::with_capacity(notes.len());
            for note in &notes {
                head.push(format!("  note: {note}"));
            }
            print_lines(&head)?;
            settings
        }
        LoadOutcome::Unreadable { settings, message } => {
            print_lines(&[message])?;
            settings
        }
    };

    let (win_desktop, win_downloads) = shell_folders(&env);
    let config_exists = std::fs::metadata(&config_path).is_ok();
    let world = resolve::CheckWorld {
        platform: &platform,
        root: &root,
        bin_dir: &bin_dir,
        config_path: &config_path,
        config_exists,
        saved: &settings,
        is_windows,
        home: &home,
        env: &env,
        win_desktop: win_desktop.as_deref(),
        win_downloads: win_downloads.as_deref(),
    };
    let mut which = |cmd: &str| resolve::which_from_process(cmd);
    let mut version_cache = resolve::VersionCache::default();
    let mut probe = |binary: &str| {
        let mtime = resolve::mtime_seconds(binary);
        resolve::probe_version(binary, mtime, &mut version_cache, &mut |b, f| {
            resolve::spawn_version_probe(b, f)
        })
    };
    let lines = resolve::show_check(&world, &mut which, &mut probe).map_err(|e| e.to_string())?;
    print_lines(&lines)?;
    Ok(ExitCode::SUCCESS)
}

/// The interactive menu (phase 6, G6).
fn run_menu() -> Result<ExitCode, String> {
    let root = resolve::real_root()?;
    let env = resolve::env_map_from_process();
    let home = resolve::home_from_env(&env).unwrap_or_default();
    let config_path = resolve::py_path_join(&root, "config/settings.json");
    let use_color = tty::stdout_is_tty();

    // load_settings first, like python: the unreadable line or the
    // coercion notes print before the menu paints (colored notes when
    // stdout is a tty; enable_vt_output comes later, in main_menu, as in
    // python - 2026-10-15 measurement, windows terminal already has VT)
    let mut sink = tty::StdoutOut::new();
    let mut settings: Obj = match config::load_settings(std::path::Path::new(&config_path))
        .map_err(|e| e.to_string())?
    {
        LoadOutcome::Loaded { settings, notes } => {
            for note in &notes {
                sink.write(&format!(
                    "  {}\n",
                    term::c(use_color, term::A_AMBER, &format!("note: {note}"))
                ));
            }
            sink.flush();
            settings
        }
        LoadOutcome::Unreadable { settings, message } => {
            sink.write(&message);
            sink.write("\n");
            sink.flush();
            settings
        }
    };

    let is_windows = cfg!(windows);
    let (win_desktop, win_downloads) = shell_folders(&env);
    let mut outdir_cache = resolve::OutdirCache::default();
    let auto = outdir_cache
        .get(|| {
            resolve::auto_outdir_lookup(
                is_windows,
                win_desktop.as_deref(),
                win_downloads.as_deref(),
                &home,
                &env,
            )
        })
        .map_err(|e| e.to_string())?;

    let shared = tty::SharedOut::new(tty::StdoutOut::new());
    let mut out = shared.clone();
    let mut reader = tty::TtyReader::new();
    let world = menu::World {
        version: ytdlp_cli::VERSION,
        root: &root,
        config_path: &config_path,
        is_windows,
        use_color,
        stdin_tty: tty::stdin_is_tty(),
        stdout_tty: use_color,
        terminal: tty::terminal_size_stdout(),
        auto: auto.as_deref(),
    };
    let mut io = menu::Io {
        out: &mut out,
        reader: &mut reader,
    };
    let download_sink = shared.clone();
    let mut download = move |_s: &mut Obj, url: &str| -> i32 {
        // phase 7 wires the real downloader
        let mut sink = download_sink.clone();
        sink.write(&format!(
            "  downloading is not wired in this build yet (native port phase 7): {url}\n"
        ));
        sink.flush();
        0
    };
    let mut quiet_check = |s: &mut Obj| menu::auto_update_check_startup(s);

    match menu::main_menu(
        &mut settings,
        &mut io,
        &world,
        &mut download,
        &mut quiet_check,
    ) {
        Ok(code) => Ok(ExitCode::from(code as u8)),
        // python's uncaught path for a read that returns EOF or Ctrl+C
        // inside a submenu: exit 1 (the traceback text is not ported,
        // recorded deviation)
        Err(MenuErr::Eof) => Ok(exit_one("input ended inside a submenu")),
        Err(MenuErr::Interrupted) => Ok(exit_one("cancelled")),
        Err(MenuErr::GoBack) => Ok(exit_one("escape escaped")),
        Err(MenuErr::Shape(shape)) => Ok(exit_one(&format!(
            "settings value refused with python shape {}",
            shape.token()
        ))),
        Err(MenuErr::Io(msg)) => Ok(exit_one(&format!("could not save settings: {msg}"))),
    }
}

fn exit_one(message: &str) -> ExitCode {
    eprintln!("ytdlp: {message}");
    ExitCode::from(1)
}

/// The two `_win_shell_folder` answers; empty everywhere else.
fn shell_folders(env: &resolve::EnvMap) -> (Option<String>, Option<String>) {
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

/// python's os.name for the running interpreter.
fn os_name() -> &'static str {
    if cfg!(windows) { "nt" } else { "posix" }
}

/// python's sys.platform for the running interpreter, mapped from the
/// targets this product builds for.
fn sys_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "linux" => "linux",
        "macos" => "darwin",
        other => other,
    }
}

/// python text-mode stdout line endings: `\n` translates to os.linesep
/// on windows (measured 2026-10-15: CRLF on redirected stdout). The
/// text is written UTF-8; python uses the locale encoding on windows, so
/// non-ASCII path content can differ in bytes (recorded deviation).
fn print_lines(lines: &[String]) -> Result<(), String> {
    let newline: &[u8] = if cfg!(windows) { b"\r\n" } else { b"\n" };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in lines {
        out.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        out.write_all(newline).map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())
}
