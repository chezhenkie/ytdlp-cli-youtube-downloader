//! ytdlp cli - native binary.
//!
//! Phase 8 (RUST-PORT-PLAN.md) wires the updaters: `--update` runs
//! python's `update_all`, menu item 11 runs it inside the session, the
//! menu start runs the once-a-day auto-update check, and the self-update
//! gates the new image on its own `--selftest` (which refuses until
//! phase 9, so an install attempt is inert by construction). The
//! network is ureq behind the `net::HttpRt` seam; `src/main.rs` never
//! dials anything itself.

use std::process::ExitCode;

use ytdlp_cli::VERSION;
use ytdlp_cli::cli_args::{self, Outcome};
use ytdlp_cli::clipboard;
use ytdlp_cli::config::{self, LoadOutcome, Obj, Value};
use ytdlp_cli::exec;
use ytdlp_cli::menu::{self, MenuErr};
use ytdlp_cli::net::UreqHttp;
use ytdlp_cli::resolve;
use ytdlp_cli::selftest;
use ytdlp_cli::term::{self, A_AMBER};
use ytdlp_cli::tty::{self, Out};
use ytdlp_cli::update::{self, UpdErr, UpdateCtx};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(msg) => {
            tty::write_stderr(&format!("ytdlp: {msg}\n"));
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
            tty::write_stderr(&text);
            return Ok(ExitCode::from(2));
        }
        Outcome::Parsed(invocation) => invocation,
    };

    let root = resolve::real_root()?;
    let is_windows = cfg!(windows);
    let use_color = tty::stdout_is_tty();
    let config_path = resolve::py_path_join(&root, "config/settings.json");
    let shared = tty::SharedOut::new(tty::StdoutOut::new());
    // FR-13: the image a previous install renamed aside goes now, on the
    // first start that is not the one that replaced it
    update::remove_previous_image(&self_path());
    let mut settings = load_and_announce(&config_path, use_color, &shared)?;
    apply_overrides(&mut settings, &invocation);

    // python's dispatch order: --check, --selftest, --update, url, menu
    if invocation.check {
        return run_check(&settings, &root, is_windows);
    }
    if invocation.selftest {
        let mut shared = tty::SharedOut::new(tty::StdoutOut::new());
        let rc = selftest::run_selftest(VERSION, &root, &config_path, &self_path(), &mut shared);
        return Ok(ExitCode::from(rc as u8));
    }
    if invocation.update {
        return Ok(run_update(
            &mut settings,
            &root,
            is_windows,
            use_color,
            &shared,
        ));
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
    let cache = std::rc::Rc::new(std::cell::RefCell::new(resolve::VersionCache::default()));
    let mut probe = probe_closure(&cache);
    let mut http = UreqHttp::new();
    let mut sleep = sleep_closure();
    let mut ctx = exec::DownloadCtx {
        root,
        is_windows,
        use_color,
        stdin_tty: tty::stdin_is_tty(),
        sink: shared.sink(),
        out: &mut out,
        reader: &mut reader,
        auto: auto.as_deref(),
        http: &mut http,
        probe: &mut probe,
        which: &mut |cmd: &str| resolve::which_from_process(cmd),
        sleep: &mut sleep,
    };
    let mut rt = exec::RunRt::real();
    let rc = exec::do_download(settings, url, &mut ctx, &mut rt);
    // python's `sys.exit(rc)`: -1 lands as 255 on posix and 0xFFFFFFFF
    // on windows (measured 2026-10-15)
    std::process::exit(rc);
}

/// `probe_version` behind python's one module memo: an Rc of the cache
/// lets the menu session's downloads and updater runs answer from the
/// same map (a batch of two downloads must not spawn `--version` twice,
/// which the piped session transcripts pin), while `--url` holds one
/// call's worth. A hit answers from the map; a miss spawns and stores.
fn probe_closure(
    cache: &std::rc::Rc<std::cell::RefCell<resolve::VersionCache>>,
) -> impl FnMut(&str) -> (String, i64) {
    let cache = std::rc::Rc::clone(cache);
    move |binary: &str| {
        let mut memo = cache.borrow_mut();
        let mtime = resolve::mtime_seconds(binary);
        resolve::probe_version(binary, mtime, &mut memo, &mut |b, f| {
            resolve::spawn_version_probe(b, f)
        })
    }
}

/// python's `time.sleep`, real here: the only waits in the port are the
/// two one-second gaps between the three ffmpeg probe attempts.
fn sleep_closure() -> impl FnMut(f64) {
    |secs: f64| std::thread::sleep(std::time::Duration::from_secs_f64(secs))
}

/// `self_path()`: python's `CLI_SELF_PATH`, the file this image runs
/// from. `canonicalize` answers windows' verbatim `\\?\` spelling, so
/// the prefix is dropped - python's `Path.resolve()` never shows it, and
/// the install's messages print this path.
fn self_path() -> String {
    let path = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(&p).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    match path.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => path,
    }
}

/// One updater run with the production world. `mode` is python's own
/// split: `Check` is the menu start's quiet `auto_update_check(s)`,
/// `All` is `update_all(s)` (`--update` and menu item 11). The fresh
/// agent and memo per call match `probe_closure`'s
/// note; an agent is a config clone plus a connection pool, and both
/// updater paths are one-shot.
#[derive(Clone, Copy, PartialEq)]
enum UpdMode {
    Check,
    All,
}

fn updater_run(
    s: &mut Obj,
    mode: UpdMode,
    world: UpdWorld,
    io: &mut menu::Io,
) -> Result<bool, UpdErr> {
    let UpdWorld {
        root,
        platform,
        arch,
        self_path,
        is_windows,
        use_color,
        sink,
        cache,
    } = world;
    let mut http = UreqHttp::new();
    let mut probe = probe_closure(&cache);
    let mut selftest = |binary: &str| match std::process::Command::new(binary)
        .arg("--selftest")
        .status()
    {
        Ok(status) => exec::exit_code(&status),
        Err(_) => -1,
    };
    let mut call = |binary: &str, args: &[String]| match std::process::Command::new(binary)
        .args(args)
        .status()
    {
        Ok(status) => exec::exit_code(&status),
        Err(_) => -1,
    };
    let mut now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    };
    let mut which = |cmd: &str| resolve::which_from_process(cmd);
    let mut ctx = UpdateCtx {
        platform: &platform,
        arch,
        self_path: &self_path,
        root: &root,
        is_windows,
        use_color,
        stdin_tty: tty::stdin_is_tty(),
        sink,
        http: &mut http,
        probe: &mut probe,
        selftest: &mut selftest,
        call: &mut call,
        now: &mut now,
        which: &mut which,
        out: io.out,
        reader: io.reader,
    };
    match mode {
        // Check is only the menu start, which is python's quiet
        // `auto_update_check(s)`; All is `update_all(s)`
        UpdMode::Check => update::auto_update_check(s, false, &mut ctx),
        UpdMode::All => update::update_all(s, &mut ctx),
    }
}

/// The updater's world, owned so both menu closures can build one. The
/// memo is shared behind an Rc so the session's downloads and updater
/// runs answer from python's one module memo: a post-update probe is a
/// fresh spawn only when the mtime moved, whichever half asks.
#[derive(Clone)]
struct UpdWorld {
    root: String,
    platform: String,
    arch: &'static str,
    self_path: String,
    is_windows: bool,
    use_color: bool,
    sink: tty::SharedSink,
    cache: std::rc::Rc<std::cell::RefCell<resolve::VersionCache>>,
}

impl UpdWorld {
    fn new(root: &str, is_windows: bool, use_color: bool, shared: &tty::SharedOut) -> Self {
        Self {
            root: root.to_string(),
            platform: resolve::host_platform(),
            arch: std::env::consts::ARCH,
            self_path: self_path(),
            is_windows,
            use_color,
            sink: shared.sink(),
            cache: std::rc::Rc::new(std::cell::RefCell::new(resolve::VersionCache::default())),
        }
    }
}

/// `--update`: python's non-interactive form of item 11 (FR-15),
/// `return 0 if update_all(s) else 1`.
fn run_update(
    settings: &mut Obj,
    root: &str,
    is_windows: bool,
    use_color: bool,
    shared: &tty::SharedOut,
) -> ExitCode {
    let mut out = shared.clone();
    let mut reader = tty::TtyReader::new();
    let mut io = menu::Io {
        out: &mut out,
        reader: &mut reader,
    };
    let world = UpdWorld::new(root, is_windows, use_color, shared);
    match updater_run(settings, UpdMode::All, world, &mut io) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(UpdErr::Eof) => exit_one("input ended inside the update prompt"),
        Err(UpdErr::Interrupted) => exit_one("cancelled"),
    }
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
    // python's probe version memo is one for the process: the download
    // half and the updater half of one session answer from the same map
    let memo = std::rc::Rc::new(std::cell::RefCell::new(resolve::VersionCache::default()));
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
        clipboard: &clipboard::read_clipboard,
    };
    let mut io = menu::Io {
        out: &mut out,
        reader: &mut reader,
    };
    let mut download = |s: &mut Obj, url: &str, io: &mut menu::Io| -> i32 {
        let mut probe = probe_closure(&memo);
        let mut http = UreqHttp::new();
        let mut sleep = sleep_closure();
        let mut ctx = exec::DownloadCtx {
            root: world.root,
            is_windows: world.is_windows,
            use_color: world.use_color,
            stdin_tty: world.stdin_tty,
            sink: shared.sink(),
            out: io.out,
            reader: io.reader,
            auto: world.auto,
            http: &mut http,
            probe: &mut probe,
            which: &mut |cmd: &str| resolve::which_from_process(cmd),
            sleep: &mut sleep,
        };
        let mut rt = exec::RunRt::real();
        exec::do_download(s, url, &mut ctx, &mut rt)
    };
    // FR-13's trigger rule: the check lives at exactly one call site, the
    // top of the menu, tty only, once a day. `--update` and item 11 are
    // the forced forms and run no check of their own.
    let mut updater = UpdWorld::new(root, is_windows, use_color, &shared);
    updater.cache = std::rc::Rc::clone(&memo);
    let mut update =
        |s: &mut Obj, io: &mut menu::Io| match updater_run(s, UpdMode::All, updater.clone(), io) {
            Ok(_) => Ok(()),
            Err(UpdErr::Eof) => Err(MenuErr::Eof),
            Err(UpdErr::Interrupted) => Err(MenuErr::Interrupted),
        };
    let mut quiet_check = |s: &mut Obj, io: &mut menu::Io| match updater_run(
        s,
        UpdMode::Check,
        updater.clone(),
        io,
    ) {
        Ok(_) => Ok(()),
        Err(UpdErr::Eof) => Err(MenuErr::Eof),
        Err(UpdErr::Interrupted) => Err(MenuErr::Interrupted),
    };

    match menu::main_menu(
        &mut settings,
        &mut io,
        &world,
        &mut download,
        &mut update,
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
    tty::write_stderr(&format!("ytdlp: {message}\n"));
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
