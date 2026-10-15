//! ytdlp cli - native binary.
//!
//! Phase 4 (RUST-PORT-PLAN.md) wires the first main() path: `--check`
//! (specsheet FR-15). Every other mode lands in later phases; this build
//! answers them with a clear message and exit code 2.

use std::io::Write;
use std::process::ExitCode;

use ytdlp_cli::config::{self, LoadOutcome};
use ytdlp_cli::resolve;

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
    if !args.iter().any(|a| a == "--check") {
        eprintln!("ytdlp: this build wires only --check (native port phase 4)");
        return Ok(ExitCode::from(2));
    }

    let root = resolve::real_root()?;
    let env = resolve::env_map_from_process();
    let home = resolve::home_from_process(&env)?;
    let platform = resolve::platform_name(os_name(), sys_platform());
    let is_windows = cfg!(windows);
    let bin_dir = resolve::py_path_join(&root, "bin");
    let config_path = resolve::py_path_join(&root, "config/settings.json");

    // python's load_settings prints first: the unreadable line early-
    // returns the defaults; otherwise the coercion notes (colors on the
    // notes line arrive with the terminal layer, phase 6).
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
