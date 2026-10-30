//! Process execution (specsheet section 4, layer 5): the frozen failure
//! sentence and the subtitle verdict lines (FR-10, IF-4), the download
//! run itself (FR-7: two pump threads, an escape-only KeyWatch, the kill
//! tree, the 6-line stderr tail), the partial/work-folder cleanup
//! (FR-11) and the subtitle pass (FR-9). The process surface is a seam
//! (`RunRt`), so the differential harness scripts whole runs offline;
//! the real one spawns yt-dlp and never touches the network.

use crate::args;
use crate::config::{self, Obj, Value};
use crate::pyfs;
use crate::pytext::{py_repr_str, py_rstrip, py_split_ws, py_strip};
use crate::resolve::{self, ShapeKind};
use crate::term::{self, A_AMBER, ascii_safe, c};
use crate::tty::{self, Out, Reader, SharedSink, Stage};
use std::io::BufRead;
use std::sync::mpsc;
use std::time::Duration;

mod run;

pub use run::{
    ChildHandle, EscapeWatch, InterruptScope, KillFn, RunRt, SpawnFn, Spawned, WatchFn, exit_code,
    interrupt_pending, set_interrupt,
};

mod srtstripper;

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

/// Verdict lines for a subtitle pass that exited nonzero after writing
/// files: yt-dlp fails the whole pass when one requested track fails, but
/// the tracks it did write are on disk and are kept. Empty when nothing
/// was written, so the caller falls back to the hard failure lines.
pub fn sub_partial_lines(codes: &[String], err: Option<&str>) -> Vec<String> {
    if codes.is_empty() {
        return Vec::new();
    }
    let (verdict, _) = sub_success_lines(codes);
    let mut lines = vec![verdict];
    if let Some(reason) = sub_fail_reason(err) {
        lines.push(format!("subtitle error - {reason}"));
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
/// failed run keeps its partials there for resume (FR-11). The native
/// run ends no longer use this shape - see `drop_temp_dir_force` - but
/// the function keeps python's semantics for its selftest checks.
pub fn drop_temp_dir(saved: &Obj, auto: Option<&str>, root: &str) {
    if let Ok(dir) = temp_dir(saved, auto, root) {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Native-only (FR-11 amendment, owner-decided 2026-10-16c): the work
/// folder never survives a run - every ending removes it with whatever
/// it holds. Python's `drop_temp_dir` keeps a non-empty folder for
/// resume; the native rule is unconditional (nothing of value survives,
/// so the 429-subtitle keep-for-retry across runs does not apply).
pub fn drop_temp_dir_force(saved: &Obj, auto: Option<&str>, root: &str) {
    if let Ok(dir) = temp_dir(saved, auto, root) {
        pyfs::rmtree_quiet(&dir);
    }
}

/// `newest_since(folder, since, slack)`: newest file with an mtime at or
/// just after `since`, as a path rendered like python's `str(Path(...))`
/// (the value is only ever printed, and pathlib normalizes separators -
/// the raw read_dir spelling can be mixed on windows, caught by the
/// windows differential run). `None` when the folder is unreadable or
/// holds no candidate. Ties keep the first file read (python's glob
/// order; fixtures keep mtimes distinct).
pub fn newest_since(folder: &str, since: f64, slack: f64) -> Option<String> {
    let entries = std::fs::read_dir(folder).ok()?;
    let mut best: Option<(f64, String)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        // one stat per entry: is_file and the mtime answer from the
        // same metadata call (audit perf 8)
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(mtime) = resolve::mtime_from_meta(&meta) else {
            continue;
        };
        let text = path.to_string_lossy().into_owned();
        if mtime >= since - slack && best.as_ref().is_none_or(|(bm, _)| mtime > *bm) {
            best = Some((mtime, text));
        }
    }
    best.map(|(_, path)| resolve::path_render(&path))
}

/// The wall clock a run start is stamped with (seconds since epoch).
fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Escape-stop cleanup: fresh partials out of the work folder, then the
/// folder itself gone (FR-11). Returns the verdict tail, empty when no
/// partial file was removed.
fn stop_cleanup(s: &Obj, auto: Option<&str>, root: &str, since: f64) -> String {
    let removed = match temp_dir(s, auto, root) {
        Ok(dir) => clean_partials(&dir, since, 5.0),
        Err(_) => Vec::new(),
    };
    drop_temp_dir_force(s, auto, root);
    if removed.is_empty() {
        String::new()
    } else {
        format!(
            ", cleaned {} partial file{}",
            removed.len(),
            if removed.len() == 1 { "" } else { "s" }
        )
    }
}

/// FR-21: yt-dlp writes the info field as a fixed `...description` file;
/// the field reads as a text file, so the successful run renames every
/// `*.description` next to the media to `.txt` (best effort, silent).
/// Returns the rename count.
pub fn rename_descriptions(folder: &str) -> usize {
    let mut n = 0;
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) == Some("description") {
                let txt = path.with_extension("txt");
                n += std::fs::rename(&path, &txt).is_ok() as usize;
            }
        }
    }
    n
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
        // one stat per entry: is_file and the mtime answer from the
        // same metadata call (audit perf 8)
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(mtime) = resolve::mtime_from_meta(&meta) else {
            continue;
        };
        if mtime < since - slack {
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

// --------------------------------------------------- download context

/// Everything `do_download` reads from its world (python reads module
/// globals, the filesystem and the console).
pub struct DownloadCtx<'a> {
    pub root: &'a str,
    pub is_windows: bool,
    pub use_color: bool,
    pub stdin_tty: bool,
    pub sink: SharedSink,
    pub out: &'a mut dyn Out,
    pub reader: &'a mut dyn Reader,
    pub auto: Option<&'a str>,
    /// `probe_version` with its memo, shared by the preflight check and
    /// the updater half so both answer from one cache - python's is a
    /// single module-level memo for the whole process
    pub probe: &'a mut dyn FnMut(&str) -> (String, i64),
    /// `which(cmd)`, injected so a test never sees the host's PATH
    pub which: &'a mut dyn FnMut(&str) -> Option<String>,
}

/// python `print(...)`: the text plus its newline, flushed (one write
/// lock per line via the shared sink).
fn print_out(sink: &SharedSink, text: &str) {
    tty::emit_line(sink, text);
}

fn shape_abort(shape: ShapeKind) -> i32 {
    tty::write_stderr(&format!(
        "ytdlp: settings value refused with python shape {}\n",
        shape.token()
    ));
    1
}

/// The shared download prelude (FR-7): the yt-dlp gate, the grouped
/// preflight stage and the ffmpeg lookup. `kind` only picks the wording
/// of the missing-ffmpeg line. Returns the binary and the ffmpeg dir, or
/// the rc the caller answers with.
fn preflight(s: &Obj, ctx: &mut DownloadCtx, kind: &str) -> Result<(String, Option<String>), i32> {
    let bin_dir = resolve::py_path_join(ctx.root, "bin");
    let platform = resolve::host_platform();
    let binary = match resolve::find_yt_dlp(s, &bin_dir, &platform, ctx.which) {
        Ok(Some(binary)) => binary,
        Ok(None) => {
            print_out(
                &ctx.sink,
                &format!(
                    "{} yt-dlp not found - put it in bin/ or on PATH, or set yt_dlp_path in config/settings.json",
                    c(ctx.use_color, A_AMBER, "...")
                ),
            );
            return Err(2);
        }
        Err(shape) => return Err(shape_abort(shape)),
    };

    // sub-second preflight checks share one grouped stage line
    let mut st = tty::dot_stage(
        "checking yt-dlp and ffmpeg",
        ctx.use_color,
        ctx.sink.clone(),
    );
    st.start();
    let (ver, vrc) = (ctx.probe)(&binary);
    let ffdir = match resolve::find_ffmpeg_dir(s, &bin_dir, ctx.is_windows, ctx.which) {
        Ok(dir) => dir,
        Err(shape) => {
            st.done(None);
            return Err(shape_abort(shape));
        }
    };
    if vrc != 0 || ver.is_empty() {
        st.done(Some(&format!("yt-dlp check failed (rc={vrc})")));
        return Err(2);
    }
    let ffs = if ffdir.is_some() {
        "yt-dlp, ytdlp-cli, ffmpeg are all present".to_string()
    } else if kind == "audio only" {
        "yt-dlp, ytdlp-cli are present, ffmpeg is missing - saving original audio".to_string()
    } else {
        "yt-dlp, ytdlp-cli are present, ffmpeg is missing - single best file".to_string()
    };
    st.done(Some(&ffs));
    Ok((binary, ffdir))
}

/// The saved file line after a zero-exit media run: the newest fresh file
/// in the output folder, or the folder itself when nothing is fresh.
fn report_saved(ctx: &DownloadCtx, outdir: &str, started: f64) {
    match newest_since(outdir, started, 2.0) {
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
                    "{} done - {} ({:.0}MB)",
                    c(ctx.use_color, A_AMBER, "..."),
                    ascii_safe(&name),
                    size as f64 / 1048576.0
                ),
            );
        }
    }
}

/// `do_download(s, url)`: the whole download flow (FR-7, FR-9, FR-10).
/// Returns the rc the callers use; 130 stops a batch (FR-12).
pub fn do_download(s: &Obj, url: &str, ctx: &mut DownloadCtx, rt: &mut RunRt) -> i32 {
    let low = url.to_lowercase();
    if !(low.starts_with("http://") || low.starts_with("https://")) {
        print_out(&ctx.sink, &format!("not a URL: {}", py_repr_str(url)));
        return 2;
    }

    let kind = match config::get(s, "type") {
        Some(Value::Str(t)) => t.clone(),
        _ => "video".to_string(),
    };
    let (binary, ffdir) = match preflight(s, ctx, &kind) {
        Ok(pair) => pair,
        Err(rc) => return rc,
    };

    // node goes through the injected which seam like every other
    // binary lookup (audit perf 6): one memoized PATH scan per session,
    // and the lazy call keeps the invariant that node is queried only
    // after output_args succeeded
    let which = &mut *ctx.which;
    let mut has_node = || which("node").is_some();
    let opts = match args::build_args(s, url, ffdir.as_deref(), ctx.root, ctx.auto, &mut has_node) {
        Ok(opts) => opts,
        Err(shape) => return shape_abort(shape),
    };
    let outdir = match resolve::resolve_outdir(s, ctx.auto, ctx.root) {
        Ok(dir) => dir,
        Err(shape) => return shape_abort(shape),
    };

    // FR-9: the subtitle pass runs before the media pass, so the subtitle
    // files are on disk before the video starts. Every outcome stays a
    // report line; the media pass runs whatever the subtitle pass
    // answered, so a rate-limited subtitle api never costs the video.
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
        let which = &mut *ctx.which;
        let mut has_node = || which("node").is_some();
        let sub_opts =
            match args::build_sub_args(s, url, ffdir.as_deref(), ctx.root, ctx.auto, &mut has_node)
            {
                Ok(opts) => opts,
                Err(shape) => {
                    st2.done(None);
                    return shape_abort(shape);
                }
            };
        let sub_started = now_secs();
        let (rc2, stopped2, err2, codes) =
            run_download(&binary, &sub_opts, Some(&st2), ctx.use_color, rt);
        if stopped2 {
            let tail = stop_cleanup(s, ctx.auto, ctx.root, sub_started);
            st2.done(Some(&format!("subtitles - stopped by escape{tail}")));
            return 130;
        }
        if rc2 == 0 {
            let (verdict, miss) = sub_success_lines(&codes);
            st2.done(Some(&verdict));
            if !miss.is_empty() {
                st2.done(Some(&miss));
            }
            strip_subtitles(s, ctx, &outdir, sub_started, rt);
        } else {
            let kept = sub_partial_lines(&codes, Some(&err2));
            if kept.is_empty() {
                let lines = sub_fail_lines(Some(&err2));
                st2.done(Some(&lines[0]));
                st2.done(Some(&lines[1]));
            } else {
                for line in &kept {
                    st2.done(Some(line.as_str()));
                }
                strip_subtitles(s, ctx, &outdir, sub_started, rt);
            }
        }
    }

    let started = now_secs();
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
        let tail = stop_cleanup(s, ctx.auto, ctx.root, started);
        st.done(Some(&format!("downloading - stopped by escape{tail}")));
        return 130;
    }
    if rc != 0 {
        let sentence = fail_sentence(rc, &err);
        st.done(Some(&sentence));
        drop_temp_dir_force(s, ctx.auto, ctx.root);
        return rc as i32;
    }

    st.done(Some("downloading - finished"));
    report_saved(ctx, &outdir, started);

    // FR-21: the description file lands as `.description`; the menu
    // field reads as text, so the successful run renames it to `.txt`
    if config::get(s, "info_field").is_some_and(config::py_truthy) {
        rename_descriptions(&outdir);
    }
    drop_temp_dir_force(s, ctx.auto, ctx.root);
    0
}

/// The srt timecode stripper pass (FR-20): when the setting is on, the
/// standalone srtstripper binary runs over the subtitle files the
/// subtitle pass wrote and the stripped copies land next to them. A
/// missing binary or a failing run is a report line; the download stays
/// successful. Called only after a subtitle pass exited 0, so subs off
/// or a failed pass never reach it.
fn strip_subtitles(s: &Obj, ctx: &mut DownloadCtx, outdir: &str, started: f64, rt: &mut RunRt) {
    if !config::get(s, "srt_stripper").is_some_and(config::py_truthy) {
        return;
    }
    let mode = config::srt_mode(config::get(s, "srt_stripper_mode")).to_string();
    let mut st = tty::dot_stage("srt timecode stripper", ctx.use_color, ctx.sink.clone());
    st.start();
    let bin_dir = resolve::py_path_join(ctx.root, "bin");
    let binary = resolve::find_srtstripper(&bin_dir, ctx.is_windows, &mut *ctx.which);
    let Some(binary) = binary else {
        st.done(Some("srt timecode stripper - not found (bin/ or PATH)"));
        return;
    };
    let paths = srtstripper::subs_since(outdir, started, 2.0);
    if paths.is_empty() {
        st.done(None);
        return;
    }
    strip_files(&binary, &paths, &mode, &mut st, rt);
}

/// One strip pass over the given subtitle files: one spawn per file
/// through `RunRt.spawn`, the tool's PASS path reported on the stage.
/// Returns how many runs answered Ok (the tool wrote a stripped copy).
fn strip_files(
    binary: &str,
    paths: &[String],
    mode: &str,
    st: &mut Stage,
    rt: &mut RunRt,
) -> usize {
    let mut written = 0;
    for path in paths {
        match srtstripper::run_one(binary, path, mode, &mut rt.spawn) {
            Ok(out_path) => {
                let shown = if out_path.is_empty() {
                    path
                } else {
                    out_path.as_str()
                };
                st.done(Some(&format!("srt timecode stripper - {shown}")));
                written += 1;
            }
            Err(text) => st.done(Some(&text)),
        }
    }
    written
}

/// `do_summary` (FR-22): the summary grab behind the `s` key. The
/// subtitle pass runs first and the fresh files go through the srt
/// stripper in its text mode, whatever the strip settings say. Only when
/// that path leaves no text does the sound-only pass run: a single
/// video, the configured audio format, built from a copy of the settings
/// so the stored type and playlist stay untouched. Returns the same rc
/// shape as do_download; 130 stops the run.
pub fn do_summary(s: &Obj, url: &str, ctx: &mut DownloadCtx, rt: &mut RunRt) -> i32 {
    let low = url.to_lowercase();
    if !(low.starts_with("http://") || low.starts_with("https://")) {
        print_out(&ctx.sink, &format!("not a URL: {}", py_repr_str(url)));
        return 2;
    }
    let (binary, ffdir) = match preflight(s, ctx, "audio only") {
        Ok(pair) => pair,
        Err(rc) => return rc,
    };
    let outdir = match resolve::resolve_outdir(s, ctx.auto, ctx.root) {
        Ok(dir) => dir,
        Err(shape) => return shape_abort(shape),
    };

    let langs_text = match config::get(s, "subtitle_langs") {
        Some(v) => config::py_str(v),
        None => "None".to_string(),
    };
    let mut st = tty::dot_stage(
        &format!("summary - subtitles ({langs_text})"),
        ctx.use_color,
        ctx.sink.clone(),
    );
    st.start();
    let which = &mut *ctx.which;
    let mut has_node = || which("node").is_some();
    let sub_opts =
        match args::build_sub_args(s, url, ffdir.as_deref(), ctx.root, ctx.auto, &mut has_node) {
            Ok(opts) => opts,
            Err(shape) => {
                st.done(None);
                return shape_abort(shape);
            }
        };
    let sub_started = now_secs();
    let (rc, stopped, err, _codes) = run_download(&binary, &sub_opts, Some(&st), ctx.use_color, rt);
    if stopped {
        let tail = stop_cleanup(s, ctx.auto, ctx.root, sub_started);
        st.done(Some(&format!("summary - stopped by escape{tail}")));
        return 130;
    }
    let mut text_saved = false;
    if rc == 0 {
        let paths = srtstripper::subs_since(&outdir, sub_started, 2.0);
        if paths.is_empty() {
            st.done(Some("summary - no subtitles available"));
        } else {
            st.done(Some(&format!("summary - {} subtitle file(s)", paths.len())));
            let bin_dir = resolve::py_path_join(ctx.root, "bin");
            let stripper = resolve::find_srtstripper(&bin_dir, ctx.is_windows, &mut *ctx.which);
            match stripper {
                Some(stripper) => {
                    let mut st2 =
                        tty::dot_stage("summary - text stripper", ctx.use_color, ctx.sink.clone());
                    st2.start();
                    let written = strip_files(&stripper, &paths, "text", &mut st2, rt);
                    if written > 0 {
                        st2.done(Some("summary - text saved"));
                        text_saved = true;
                    }
                }
                None => st.done(Some("summary - text stripper not found (bin/ or PATH)")),
            }
        }
    } else {
        let lines = sub_fail_lines(Some(&err));
        st.done(Some(&lines[0]));
        st.done(Some(&lines[1]));
    }

    if text_saved {
        print_out(
            &ctx.sink,
            &format!("{} summary - text ready", c(ctx.use_color, A_AMBER, "...")),
        );
        drop_temp_dir_force(s, ctx.auto, ctx.root);
        return 0;
    }

    // fallback: sound only, one video, the configured audio format
    print_out(
        &ctx.sink,
        &c(
            ctx.use_color,
            A_AMBER,
            "  summary - no text, downloading sound only",
        ),
    );
    let mut audio = s.clone();
    config::set(&mut audio, "type", Value::Str("audio only".to_string()));
    config::set(&mut audio, "playlist", Value::Bool(false));
    let which = &mut *ctx.which;
    let mut has_node = || which("node").is_some();
    let opts = match args::build_args(
        &audio,
        url,
        ffdir.as_deref(),
        ctx.root,
        ctx.auto,
        &mut has_node,
    ) {
        Ok(opts) => opts,
        Err(shape) => return shape_abort(shape),
    };
    let started = now_secs();
    let spinner = term::pick_spinner();
    let mut st = tty::spinner_stage(
        "summary - sound only (escape to stop)",
        spinner,
        ctx.use_color,
        ctx.sink.clone(),
    );
    st.start();
    let (rc, stopped, err, _codes) = run_download(&binary, &opts, Some(&st), ctx.use_color, rt);
    if stopped {
        let tail = stop_cleanup(s, ctx.auto, ctx.root, started);
        st.done(Some(&format!("summary - stopped by escape{tail}")));
        return 130;
    }
    if rc != 0 {
        let sentence = fail_sentence(rc, &err);
        st.done(Some(&sentence));
        drop_temp_dir_force(s, ctx.auto, ctx.root);
        return rc as i32;
    }
    st.done(Some("summary - sound saved"));
    report_saved(ctx, &outdir, started);
    drop_temp_dir_force(s, ctx.auto, ctx.root);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testfake::LinesOut;
    use crate::tty::{RecordingOut, ScriptedReader};
    use crate::ui::WIDTH;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// One download test's world: a scratch root with bin/, a scripted
    /// console, a scripted probe and scripted which answers.
    struct InstallWorld {
        dir: String,
        bin_dir: String,
        platform: String,
        lines: Arc<Mutex<Vec<String>>>,
        rec: RecordingOut,
        reader: ScriptedReader,
        probe_answers: VecDeque<(String, i64)>,
        which_answers: Vec<(&'static str, Option<String>)>,
    }

    /// Run a body against a `DownloadCtx` built from an
    /// `InstallWorld`: every seam is a local fake, and the ctx, the
    /// closures and the world's borrows all die with the block. The
    /// continuation form is what keeps them in one lifetime.
    macro_rules! with_ctx {
        ($world:expr, $sink:expr, |$ctx:ident| $body:block) => {{
            let world = $world;
            let InstallWorld {
                rec,
                reader,
                probe_answers,
                which_answers,
                ..
            } = world;
            let is_windows = world.platform == "windows";
            let mut probe = |_binary: &str| {
                probe_answers
                    .pop_front()
                    .unwrap_or_else(|| ("2026.08.19".to_string(), 0))
            };
            let mut which = |cmd: &str| {
                which_answers
                    .iter()
                    .find(|(name, _)| *name == cmd)
                    .and_then(|(_, answer)| answer.clone())
            };
            let mut $ctx = DownloadCtx {
                root: &world.dir,
                is_windows,
                use_color: false,
                stdin_tty: true,
                sink: $sink,
                out: rec,
                reader,
                auto: None,
                probe: &mut probe,
                which: &mut which,
            };
            $body
        }};
    }

    impl InstallWorld {
        /// `platform` only decides the windows spelling of a path here,
        /// so the fixture does not depend on the host.
        fn new(tag: &str, platform: &str) -> Self {
            let dir = scratch(tag);
            let bin_dir = resolve::py_path_join(&dir.to_string_lossy(), "bin");
            std::fs::create_dir_all(&bin_dir).unwrap();
            Self {
                dir: dir.to_string_lossy().into_owned(),
                bin_dir,
                platform: platform.to_string(),
                lines: Arc::new(Mutex::new(Vec::new())),
                rec: RecordingOut::default(),
                reader: ScriptedReader::new(vec![]),
                probe_answers: VecDeque::new(),
                which_answers: Vec::new(),
            }
        }

        /// The sink the tests read back: one line per `emit_line`.
        fn sink(&self) -> SharedSink {
            tty::SharedOut::new(LinesOut {
                lines: Arc::clone(&self.lines),
            })
            .sink()
        }
    }

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
    fn drop_temp_dir_force_removes_a_work_folder_with_contents() {
        // FR-11 amendment 2026-10-16c: every native run end removes the
        // work folder with whatever it holds
        let dir = scratch("forcedrop");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let work = out.join(resolve::TEMP_DIR_NAME);
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join("v.mp4.part"), b"fresh partial").unwrap();
        let mut s = config::defaults();
        config::set(
            &mut s,
            "output_dir",
            Value::Str(out.to_string_lossy().into_owned()),
        );
        let root = dir.to_string_lossy().into_owned();
        drop_temp_dir_force(&s, None, &root);
        assert!(!work.exists(), "contents and all");
        // a missing folder is quiet
        drop_temp_dir_force(&s, None, &root);
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

    #[test]
    fn partial_subtitle_pass_keeps_the_files_it_wrote() {
        let codes = vec!["en".to_string()];
        let lines = sub_partial_lines(&codes, Some("HTTP Error 429: Too Many Requests"));
        assert_eq!(lines[0], "subtitles - en");
        assert_eq!(
            lines[1],
            "subtitle error - the subtitle api answered 429 too many requests"
        );
        // no file written: the hard failure lines own the verdict
        assert!(sub_partial_lines(&[], Some("HTTP Error 429")).is_empty());
        // an unclassified error keeps the file verdict and drops the reason
        assert_eq!(
            sub_partial_lines(&codes, Some("SOMETHING ODD")),
            vec!["subtitles - en".to_string()]
        );
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

    /// Recorded spawn calls: (binary, argv).
    type SpawnCalls = Arc<Mutex<Vec<(String, Vec<String>)>>>;

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

    /// FR-20: the stripper pass over a scripted srtstripper spawn - the
    /// PASS line's path is reported, the download text stays plain.
    /// The binary comes from `bin/`, which is what a bundle ships.
    #[test]
    fn strip_subtitles_reports_the_stripped_files() {
        let mut world = InstallWorld::new("strip-hit", "linux");
        std::fs::create_dir_all(resolve::py_path_join(&world.dir, "out")).unwrap();
        let outdir = resolve::py_path_join(&world.dir, "out");
        let sub = resolve::py_path_join(&outdir, "video.en.srt");
        std::fs::write(&sub, b"1\n00:00:00,000 --> 00:00:01,000\nhi\n").unwrap();
        let bin_path = resolve::py_path_join(&world.bin_dir, "srtstripper");
        std::fs::write(&bin_path, b"fake").unwrap();
        let expect_args = vec![sub.clone()];
        let lines = Arc::clone(&world.lines);
        let mut s = config::defaults();
        config::set(&mut s, "srt_stripper", Value::Bool(true));
        let sink = world.sink();
        let mut rt = RunRt {
            spawn: Box::new(move |binary, args| {
                assert_eq!(binary, bin_path);
                assert_eq!(args, expect_args, "text takes no flag");
                Ok(Spawned {
                    stdout: Box::new(std::io::Cursor::new(
                        b"PASS: mode=default, wrote 1 lines from 3 input lines -> /out/video.en - alpha.txt\n"
                            .to_vec(),
                    )),
                    stderr: Box::new(std::io::Cursor::new(Vec::new())),
                    child: Box::new(ScriptedHandle { polls: Vec::new().into() }),
                })
            }),
            watch: Box::new(|| {
                Box::new(ScriptedWatch {
                    hits: Vec::new().into(),
                })
            }),
            kill: Box::new(|_| {}),
            interrupted: Box::new(|| false),
            sleep_frame: Box::new(|| {}),
            interrupt_scope: None,
        };
        with_ctx!(&mut world, sink, |ctx| {
            strip_subtitles(&s, &mut ctx, &outdir, 0.0, &mut rt);
        });
        let printed = lines.lock().unwrap().join("\n");
        assert!(
            printed.contains("srt timecode stripper - /out/video.en - alpha.txt"),
            "the verdict names the stage and the file: {printed}"
        );
    }

    /// FR-20: no binary found - one report line, still not an error.
    #[test]
    fn strip_subtitles_reports_a_missing_binary() {
        let mut world = InstallWorld::new("strip-miss", "linux");
        std::fs::create_dir_all(resolve::py_path_join(&world.dir, "out")).unwrap();
        let outdir = resolve::py_path_join(&world.dir, "out");
        let sub = resolve::py_path_join(&outdir, "video.en.srt");
        std::fs::write(&sub, b"1\n").unwrap();
        let lines = Arc::clone(&world.lines);
        let mut s = config::defaults();
        config::set(&mut s, "srt_stripper", Value::Bool(true));
        let sink = world.sink();
        let mut rt = RunRt {
            spawn: Box::new(|_binary, _args| {
                panic!("no spawn when the binary is missing");
            }),
            watch: Box::new(|| {
                Box::new(ScriptedWatch {
                    hits: Vec::new().into(),
                })
            }),
            kill: Box::new(|_| {}),
            interrupted: Box::new(|| false),
            sleep_frame: Box::new(|| {}),
            interrupt_scope: None,
        };
        with_ctx!(&mut world, sink, |ctx| {
            strip_subtitles(&s, &mut ctx, &outdir, 0.0, &mut rt);
        });
        let printed = lines.lock().unwrap().join("\n");
        assert!(
            printed.contains("srt timecode stripper - not found (bin/ or PATH)"),
            "{printed}"
        );
    }

    /// FR-20: the setting off does nothing, and subs off never reach the
    /// stripper (that gate sits in do_download, asserted here for the fn).
    #[test]
    fn strip_subtitles_is_silent_when_off() {
        let mut world = InstallWorld::new("strip-off", "linux");
        std::fs::create_dir_all(resolve::py_path_join(&world.dir, "out")).unwrap();
        let outdir = resolve::py_path_join(&world.dir, "out");
        let sub = resolve::py_path_join(&outdir, "video.en.srt");
        std::fs::write(&sub, b"1\n").unwrap();
        let lines = Arc::clone(&world.lines);
        let s = config::defaults();
        let sink = world.sink();
        let mut rt = RunRt {
            spawn: Box::new(|_binary, _args| {
                panic!("no spawn when the stripper is off");
            }),
            watch: Box::new(|| {
                Box::new(ScriptedWatch {
                    hits: Vec::new().into(),
                })
            }),
            kill: Box::new(|_| {}),
            interrupted: Box::new(|| false),
            sleep_frame: Box::new(|| {}),
            interrupt_scope: None,
        };
        with_ctx!(&mut world, sink, |ctx| {
            strip_subtitles(&s, &mut ctx, &outdir, 0.0, &mut rt);
        });
        let printed = lines.lock().unwrap().join("\n");
        assert!(
            !printed.contains("srt timecode stripper"),
            "no stage line when off: {printed}"
        );
    }

    /// FR-22: the summary grab runs the subtitle pass, strips the fresh
    /// files in text mode whatever the strip settings say, and never
    /// starts a sound download.
    #[test]
    fn do_summary_strips_text_before_any_sound() {
        let mut world = InstallWorld::new("summary-text", "linux");
        let outdir = resolve::py_path_join(&world.dir, resolve::DOWNLOADS_DIR_NAME);
        std::fs::create_dir_all(&outdir).unwrap();
        let host = resolve::host_platform();
        let yt = resolve::platform_bin_name(&host);
        std::fs::write(resolve::py_path_join(&world.bin_dir, yt), b"x").unwrap();
        std::fs::write(resolve::py_path_join(&world.bin_dir, "ffmpeg"), b"x").unwrap();
        std::fs::write(resolve::py_path_join(&world.bin_dir, "srtstripper"), b"x").unwrap();
        world.probe_answers.push_back(("2026.08.19".to_string(), 0));
        let lines = Arc::clone(&world.lines);
        let sub = resolve::py_path_join(&outdir, "video.en.srt");
        let arg_sub = sub.clone();
        let calls: SpawnCalls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let mut rt = RunRt {
            spawn: Box::new(move |binary: &str, args: &[String]| {
                seen.lock()
                    .unwrap()
                    .push((binary.to_string(), args.to_vec()));
                let (stdout, rc) = if binary.ends_with("srtstripper") {
                    (
                        format!(
                            "PASS: mode=default, wrote 2 lines from 4 input lines -> {arg_sub} - text.txt\n"
                        ),
                        0,
                    )
                } else {
                    std::fs::write(&arg_sub, b"1\n").unwrap();
                    (String::new(), 0)
                };
                Ok(Spawned {
                    stdout: Box::new(std::io::Cursor::new(stdout.into_bytes())),
                    stderr: Box::new(std::io::Cursor::new(Vec::new())),
                    child: Box::new(ScriptedHandle {
                        polls: vec![Some(rc)].into(),
                    }),
                })
            }),
            watch: Box::new(|| {
                Box::new(ScriptedWatch {
                    hits: Vec::new().into(),
                })
            }),
            kill: Box::new(|_| {}),
            interrupted: Box::new(|| false),
            sleep_frame: Box::new(|| {}),
            interrupt_scope: None,
        };
        let s = config::defaults();
        let sink = world.sink();
        let rc = with_ctx!(&mut world, sink, |ctx| {
            do_summary(&s, "https://x/y", &mut ctx, &mut rt)
        });
        assert_eq!(rc, 0);
        let calls = calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2, "subtitle pass + text stripper: {calls:?}");
        assert!(calls[0].0.ends_with(yt));
        assert!(calls[0].1.contains(&"--skip-download".to_string()));
        assert!(calls[1].0.ends_with("srtstripper"));
        assert_eq!(calls[1].1, vec![sub.clone()], "text mode takes no flag");
        let printed = lines.lock().unwrap().join("\n");
        assert!(printed.contains("summary - text ready"), "{printed}");
        assert!(!printed.contains("sound only"), "{printed}");
    }

    /// FR-22: a failed subtitle pass falls back to one sound-only run:
    /// the configured audio format from a copy of the settings, so the
    /// stored type and playlist stay as they were.
    #[test]
    fn do_summary_falls_back_to_sound_only() {
        let mut world = InstallWorld::new("summary-sound", "linux");
        let outdir = resolve::py_path_join(&world.dir, resolve::DOWNLOADS_DIR_NAME);
        std::fs::create_dir_all(&outdir).unwrap();
        let host = resolve::host_platform();
        let yt = resolve::platform_bin_name(&host);
        std::fs::write(resolve::py_path_join(&world.bin_dir, yt), b"x").unwrap();
        std::fs::write(resolve::py_path_join(&world.bin_dir, "ffmpeg"), b"x").unwrap();
        world.probe_answers.push_back(("2026.08.19".to_string(), 0));
        let lines = Arc::clone(&world.lines);
        let media = resolve::py_path_join(&outdir, "video.mp3");
        let calls: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let written = media.clone();
        let mut rt = RunRt {
            spawn: Box::new(move |_binary: &str, args: &[String]| {
                let mut seen = seen.lock().unwrap();
                seen.push(args.to_vec());
                let (stderr, rc) = if seen.len() == 1 {
                    (b"ERROR: no subtitles for that video\n".to_vec(), 1)
                } else {
                    std::fs::write(&written, b"audio").unwrap();
                    (Vec::new(), 0)
                };
                Ok(Spawned {
                    stdout: Box::new(std::io::Cursor::new(Vec::new())),
                    stderr: Box::new(std::io::Cursor::new(stderr)),
                    child: Box::new(ScriptedHandle {
                        polls: vec![Some(rc)].into(),
                    }),
                })
            }),
            watch: Box::new(|| {
                Box::new(ScriptedWatch {
                    hits: Vec::new().into(),
                })
            }),
            kill: Box::new(|_| {}),
            interrupted: Box::new(|| false),
            sleep_frame: Box::new(|| {}),
            interrupt_scope: None,
        };
        let s = config::defaults();
        let sink = world.sink();
        let rc = with_ctx!(&mut world, sink, |ctx| {
            do_summary(&s, "https://x/y", &mut ctx, &mut rt)
        });
        assert_eq!(rc, 0);
        let calls = calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2, "subtitle pass then sound only: {calls:?}");
        let sound = &calls[1];
        assert!(sound.contains(&"-x".to_string()), "{sound:?}");
        assert!(sound.contains(&"--audio-format".to_string()), "{sound:?}");
        assert!(sound.contains(&"--no-playlist".to_string()), "{sound:?}");
        assert_eq!(
            config::get(&s, "type"),
            Some(&Value::Str("video".to_string())),
            "the stored type is untouched"
        );
        assert_eq!(config::get(&s, "playlist"), Some(&Value::Bool(false)));
        let printed = lines.lock().unwrap().join("\n");
        assert!(
            printed.contains("summary - no text, downloading sound only"),
            "{printed}"
        );
        assert!(printed.contains("summary - sound saved"), "{printed}");
        assert!(printed.contains("video.mp3"), "{printed}");
    }
}
