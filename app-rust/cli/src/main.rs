//! ytdlp cli - native binary.
//!
//! Phase 7 (RUST-PORT-PLAN.md) wires the download run: the URL quick mode
//! with the argparse-compatible parser (IF-2) and the menu's real
//! downloader. `--check` keeps its phase-4 path, the menu its phase-6
//! path; the updaters (`--update`, `--selftest`) print typed refusals
//! until phases 8 and 9.

use std::process::ExitCode;

use ytdlp_cli::cli_args::{self, Outcome};
use ytdlp_cli::config::{self, LoadOutcome, Obj, Value};
use ytdlp_cli::exec;
use ytdlp_cli::menu::{self, MenuErr};
use ytdlp_cli::resolve;
use ytdlp_cli::term::{self, A_AMBER};
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
    let mut argv = std::env::args();
    let arg0 = argv.next().unwrap_or_default();
    let args: Vec<String> = argv.collect();
    // python's prog: os.path.basename(sys.argv[0])
    let prog = std::path::Path::new(&arg0)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| arg0.clone());
    let width = cli_args::cli_width();
    let invocation = match cli_args::parse(&prog, &args, width) {
        Outcome::Help(text) => {
            print_stdout(&text)?;
            return Ok(ExitCode::SUCCESS);
        }
        Outcome::Error(text) => {
            eprint!("{text}");
            return Ok(ExitCode::from(2));
        }
        Outcome::Parsed(invocation) => invocation,
    };

    let root = resolve::real_root()?;
    let is_windows = cfg!(windows);
    let use_color = tty::stdout_is_tty();
    let config_path = resolve::py_path_join(&root, "config/settings.json");
    let shared = tty::SharedOut::new(tty::StdoutOut::new());
    let mut settings = load_and_announce(&config_path, use_color, &shared)?;
    apply_overrides(&mut settings, &invocation);

    // python's dispatch order: --check, --selftest, --update, url, menu
    if invocation.check {
        return run_check(&settings, &root, is_windows);
    }
    if invocation.selftest {
        eprintln!(
            "ytdlp: the port's checks run as cargo tests; a runtime --selftest lands in \
             native port phase 9"
        );
        return Ok(ExitCode::from(2));
    }
    if invocation.update {
        eprintln!("ytdlp: the updaters land in native port phase 8");
        return Ok(ExitCode::from(2));
    }
    if let Some(url) = &invocation.url
        && !invocation.menu
    {
        return Ok(run_quick(
            &settings, url, &root, is_windows, use_color, &shared,
        ));
    }
    run_menu(settings, &root, is_windows, use_color, &config_path, shared)
}

/// `load_settings` plus python's two announcement forms: the unreadable
/// line plain, the coercion notes amber when stdout is a tty.
fn load_and_announce(
    config_path: &str,
    use_color: bool,
    shared: &tty::SharedOut,
) -> Result<Obj, String> {
    let mut out = shared.clone();
    match config::load_settings(std::path::Path::new(config_path)).map_err(|e| e.to_string())? {
        LoadOutcome::Loaded { settings, notes } => {
            for note in &notes {
                out.write("  ");
                out.write(&term::c(use_color, A_AMBER, &format!("note: {note}")));
                out.write("\n");
            }
            out.flush();
            Ok(settings)
        }
        LoadOutcome::Unreadable { settings, message } => {
            out.write(&message);
            out.write("\n");
            out.flush();
            Ok(settings)
        }
    }
}

/// `apply_overrides`: one-shot overrides mutate the in-memory settings
/// only (FR-3); quick runs never persist them.
fn apply_overrides(s: &mut Obj, inv: &cli_args::Invocation) {
    if inv.audio {
        config::set(s, "type", Value::Str("audio only".to_string()));
    }
    if let Some(format) = &inv.format {
        config::set(s, "video_container", Value::Str(format.clone()));
    }
    if let Some(resolution) = &inv.resolution {
        config::set(s, "video_resolution", Value::Str(resolution.clone()));
    }
    if let Some(af) = &inv.af {
        config::set(s, "audio_format", Value::Str(af.clone()));
    }
    if let Some(outdir) = &inv.outdir {
        config::set(s, "output_dir", Value::Str(outdir.clone()));
    }
    if inv.subs {
        config::set(s, "subtitles", Value::Bool(true));
    }
    if inv.playlist {
        config::set(s, "playlist", Value::Bool(true));
    }
    if let Some(template) = &inv.template {
        config::set(s, "filename_template", Value::Str(template.clone()));
    }
}

/// `--check` (phase 4, G4): the seven diagnostic lines.
fn run_check(settings: &Obj, root: &str, is_windows: bool) -> Result<ExitCode, String> {
    let env = resolve::env_map_from_process();
    let home = resolve::home_from_process(&env)?;
    let platform = resolve::host_platform();
    let bin_dir = resolve::py_path_join(root, "bin");
    let config_path = resolve::py_path_join(root, "config/settings.json");
    let (win_desktop, win_downloads) = shell_folders(&env);
    let config_exists = std::fs::metadata(&config_path).is_ok();
    let world = resolve::CheckWorld {
        platform: &platform,
        root,
        bin_dir: &bin_dir,
        config_path: &config_path,
        config_exists,
        saved: settings,
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
    let shared = tty::SharedOut::new(tty::StdoutOut::new());
    let mut sink = shared.clone();
    for line in &lines {
        sink.write(line);
        sink.write("\n");
    }
    sink.flush();
    Ok(ExitCode::SUCCESS)
}

/// The URL quick mode: python's `main()` fast path - dispatch straight to
/// `do_download` and exit with its rc (`sys.exit(rc)`).
fn run_quick(
    settings: &Obj,
    url: &str,
    root: &str,
    is_windows: bool,
    use_color: bool,
    shared: &tty::SharedOut,
) -> ExitCode {
    let env = resolve::env_map_from_process();
    let home = resolve::home_from_env(&env).unwrap_or_default();
    let (win_desktop, win_downloads) = shell_folders(&env);
    let auto = resolve::auto_outdir_lookup(
        is_windows,
        win_desktop.as_deref(),
        win_downloads.as_deref(),
        &home,
        &env,
    )
    .ok()
    .flatten();
    let mut out = shared.clone();
    let mut reader = tty::TtyReader::new();
    let mut version_cache = resolve::VersionCache::default();
    let mut ctx = exec::DownloadCtx {
        root,
        is_windows,
        use_color,
        stdin_tty: tty::stdin_is_tty(),
        sink: shared.sink(),
        out: &mut out,
        reader: &mut reader,
        version_cache: &mut version_cache,
        auto: auto.as_deref(),
    };
    let mut rt = exec::RunRt::real();
    let rc = exec::do_download(settings, url, &mut ctx, &mut rt);
    // python's `sys.exit(rc)`: -1 lands as 255 on posix and 0xFFFFFFFF
    // on windows (measured 2026-10-15)
    std::process::exit(rc);
}

/// The interactive menu (phase 6, G6) with the phase-7 downloader wired.
fn run_menu(
    settings: Obj,
    root: &str,
    is_windows: bool,
    use_color: bool,
    config_path: &str,
    shared: tty::SharedOut,
) -> Result<ExitCode, String> {
    let mut settings = settings;
    let env = resolve::env_map_from_process();
    let home = resolve::home_from_env(&env).unwrap_or_default();
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

    let mut out = shared.clone();
    let mut reader = tty::TtyReader::new();
    let mut version_cache = resolve::VersionCache::default();
    let world = menu::World {
        version: ytdlp_cli::VERSION,
        root,
        config_path,
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
    let mut download = |s: &mut Obj, url: &str, io: &mut menu::Io| -> i32 {
        let mut ctx = exec::DownloadCtx {
            root: world.root,
            is_windows: world.is_windows,
            use_color: world.use_color,
            stdin_tty: world.stdin_tty,
            sink: shared.sink(),
            out: io.out,
            reader: io.reader,
            version_cache: &mut version_cache,
            auto: world.auto,
        };
        let mut rt = exec::RunRt::real();
        exec::do_download(s, url, &mut ctx, &mut rt)
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

/// The `auto_outdir` memo answer for the quick path (the menu path keeps
/// its own OutdirCache).
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

/// `print()` of one text block to stdout: python's text-mode newline
/// translation and console handling live in the production sink.
fn print_stdout(text: &str) -> Result<(), String> {
    let mut out = tty::StdoutOut::new();
    out.write(text);
    out.flush();
    Ok(())
}
