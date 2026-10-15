//! Scripted yt-dlp stand-in for the offline differential and session
//! tests (RUST-PORT-PLAN.md phase 7 evidence: fake yt-dlp binaries in
//! the harness and session trees, never the network). Development tool
//! only: it is never part of a release bundle and nothing in `cli/`
//! depends on it.
//!
//! Scripting is entirely environment-driven, so one scenario needs no
//! respawn logic:
//!
//!   FAKE_YTDLP_MODE   ok (default) | fail | probe-fail | sleep | slow
//!                     | sub-ok | sub-miss | sub-429
//!   FAKE_YTDLP_SUB_MODE  the mode for a `--skip-download` invocation,
//!                     default sub-ok
//!   FAKE_YTDLP_ARGV   file to append the invocation record to: one
//!                     `mode=...` line, one line per argv element, one
//!                     blank line per invocation
//!   FAKE_YTDLP_PID_FILE  file to write this process's pid to (the
//!                     100-run test checks no process lingers)
//!   FAKE_YTDLP_NAME   the produced file name, default video.mp4
//!   FAKE_YTDLP_SIZE   bytes written for the final file, default 32768
//!   FAKE_YTDLP_SLEEP  seconds `sleep` mode stays alive, default 60
//!
//! It answers `--version`/`-version` like yt-dlp (rc 0, the pinned
//! string), reads `-P home:` out of its own argv exactly like the real
//! yt-dlp would, keeps intermediates in `<outdir>/.ytdlp-tmp`, and its
//! stdout shapes are the ones the wrapper scrapes ([download] lines,
//! `Writing video subtitles to:`). `fail` and `sub-429` write stderr
//! lines in yt-dlp's wording; everything is deterministic, ASCII and
//! flushed per line.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const VERSION_LINE: &str = "2026.08.19";
const DEFAULT_NAME: &str = "video.mp4";
const DEFAULT_SIZE: u64 = 32768;
const DEFAULT_SLEEP_SECS: u64 = 60;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let mode = env::var("FAKE_YTDLP_MODE").unwrap_or_else(|_| "ok".to_string());
    if let Ok(path) = env::var("FAKE_YTDLP_PID_FILE") {
        let _ = fs::write(path, std::process::id().to_string());
    }
    record_argv(&mode, &args);

    // the preflight probe answers before any mode logic
    if args.len() == 1 && (args[0] == "--version" || args[0] == "-version") {
        if mode == "probe-fail" {
            eprintln!("fake probe banner: this build will not answer");
            return ExitCode::from(1);
        }
        println!("{VERSION_LINE}");
        return ExitCode::SUCCESS;
    }

    let outdir = outdir_from_args(&args).unwrap_or_else(|| PathBuf::from("."));
    let name = env::var("FAKE_YTDLP_NAME").unwrap_or_else(|_| DEFAULT_NAME.to_string());
    let size = env::var("FAKE_YTDLP_SIZE")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SIZE);
    let tmp = outdir.join(".ytdlp-tmp");
    let base = Path::new(&name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.clone());

    // the wrapper runs yt-dlp twice (video pass, then a subtitle pass
    // with --skip-download); the sub mode script is separate
    let sub_mode = env::var("FAKE_YTDLP_SUB_MODE").unwrap_or_else(|_| "sub-ok".to_string());
    let effective = if args.iter().any(|a| a == "--skip-download") {
        sub_mode
    } else {
        mode
    };

    match effective.as_str() {
        "sleep" => {
            write_file(&tmp.join(format!("{name}.part")), 4096);
            write_file(&tmp.join(format!("{name}.ytdl")), 64);
            println!("[download]  12.3% of 1.50MiB at 2.34MiB/s ETA 00:12");
            flush_stdout();
            let secs = env::var("FAKE_YTDLP_SLEEP")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(DEFAULT_SLEEP_SECS);
            thread::sleep(Duration::from_secs(secs));
            ExitCode::SUCCESS
        }
        "fail" => {
            println!("[download] Destination: {}", outdir.join(&name).display());
            println!("[download]   5.0% of 1.50MiB at 2.34MiB/s ETA 00:12");
            flush_stdout();
            eprintln!("WARNING: fake warning line");
            eprintln!("ERROR: unable to rename file: [WinError 32] fake holder");
            write_file(&tmp.join(format!("{name}.part")), 2048);
            ExitCode::from(3)
        }
        "slow" => {
            println!("[download] Destination: {}", outdir.join(&name).display());
            flush_stdout();
            for pct in ["  10.0%", "  40.0%", "  70.0%", " 100.0%"] {
                thread::sleep(Duration::from_millis(200));
                println!("[download] {pct} of 1.50MiB at 2.34MiB/s ETA 00:12");
                flush_stdout();
            }
            write_file(&outdir.join(&name), size);
            ExitCode::SUCCESS
        }
        "sub-ok" => {
            for code in ["en", "en-GB"] {
                let sub = outdir.join(format!("{base}.{code}.vtt"));
                println!("Writing video subtitles to: {}", sub.display());
                flush_stdout();
                write_file(&outdir.join(format!("{base}.{code}.srt")), 128);
            }
            ExitCode::SUCCESS
        }
        "sub-miss" => ExitCode::SUCCESS,
        "sub-429" => {
            eprintln!(
                "ERROR: Unable to download video subtitles for 'en': \
                 HTTP Error 429: Too Many Requests"
            );
            write_file(&tmp.join(format!("{base}.en.vtt")), 128);
            ExitCode::from(1)
        }
        _ => {
            // "ok": the polite path that leaves a finished file behind
            println!("[download] Destination: {}", outdir.join(&name).display());
            println!("[download]   5.0% of 1.50MiB at 2.34MiB/s ETA 00:12");
            println!("[download] 100% of 1.50MiB in 00:01");
            flush_stdout();
            write_file(&outdir.join(&name), size);
            ExitCode::SUCCESS
        }
    }
}

/// `-P home:<dir>` out of the argv, exactly how the wrapper hands the
/// output folder over to yt-dlp.
fn outdir_from_args(args: &[String]) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "-P"
            && let Some(value) = it.next()
            && let Some(rest) = value.strip_prefix("home:")
        {
            return Some(PathBuf::from(rest));
        }
    }
    None
}

fn flush_stdout() {
    let _ = std::io::stdout().flush();
}

/// Append the invocation record when FAKE_YTDLP_ARGV is set.
fn record_argv(mode: &str, args: &[String]) {
    let Ok(path) = env::var("FAKE_YTDLP_ARGV") else {
        return;
    };
    let Ok(file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let mut w = BufWriter::new(file);
    let _ = writeln!(w, "mode={mode}");
    for arg in args {
        let _ = writeln!(w, "{arg}");
    }
    let _ = writeln!(w);
    let _ = w.flush();
}

/// Write `size` zero bytes to `path`, creating parent directories.
fn write_file(path: &Path, size: u64) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(file) = fs::File::create(path) {
        let mut w = BufWriter::new(file);
        let mut left = size;
        let chunk = vec![0u8; 65536];
        while left > 0 {
            let take = left.min(chunk.len() as u64) as usize;
            if w.write_all(&chunk[..take]).is_err() {
                return;
            }
            left -= take as u64;
        }
        let _ = w.flush();
    }
}
