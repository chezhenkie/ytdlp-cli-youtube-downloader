//! The 19 check sections, mirroring python's selftest() block by block.

use std::cell::RefCell;
use std::rc::Rc;

use super::plumbing::*;
use crate::archive;
use crate::args;
use crate::config::{self, Obj, Value};
use crate::exec;
use crate::menu::{self, ActOutcome, Io, World};
use crate::net;
use crate::pyfs;
use crate::resolve;
use crate::term;
use crate::tty::{self, Out, Reader};
use crate::ui;
use crate::ui::WIDTH;
use crate::update;

pub(super) fn section_history(c: &mut Checker<'_>, capture: &CaptureOut) {
    let mut history = tty::History::new();
    history.remember("https://a.example");
    history.remember("https://b.example");
    let mut echo = capture.clone();
    let mut ed = tty::LineEdit::new("> ", &history);
    ed.add('x', &mut echo);
    ed.add('y', &mut echo);
    ed.recall(true, &history, &mut echo);
    let v1 = ed.text();
    ed.recall(true, &history, &mut echo);
    let v2 = ed.text();
    ed.recall(false, &history, &mut echo);
    let v3 = ed.text();
    ed.recall(false, &history, &mut echo);
    let v4 = ed.text();

    let mut history2 = tty::History::new();
    let mut ed2 = tty::LineEdit::new("> ", &history2);
    ed2.add('q', &mut echo);
    ed2.remember(&mut history2);
    let v5 = history2.entries.last().cloned().unwrap_or_default();
    let mut ed3 = tty::LineEdit::new("> ", &history2);
    ed3.remember(&mut history2);
    let v6 = history2.entries.last().cloned().unwrap_or_default();

    c.check(
        "history newest first",
        v1 == "https://b.example" && v2 == "https://a.example",
        &got_strs(&[&v1, &v2]),
    );
    c.check(
        "history returns to draft",
        v3 == "https://b.example" && v4 == "xy",
        &got_strs(&[&v3, &v4]),
    );
    c.check(
        "history remembers submitted",
        v5 == "q",
        &py_opt_str(Some(&v5)),
    );
    c.check(
        "history skips empty lines",
        v6 == "q",
        &py_opt_str(Some(&v6)),
    );
}

pub(super) fn section_subs(c: &mut Checker<'_>) {
    let r429_src = "ERROR: Unable to download video subtitles \
                    for 'en': HTTP Error 429: Too Many Requests";
    let r429 = exec::sub_fail_reason(Some(r429_src));
    c.check(
        "sub reason 429",
        r429.as_deref() == Some("the subtitle api answered 429 too many requests"),
        &py_opt_str(r429.as_deref()),
    );
    let odd = exec::sub_fail_reason(Some("SOMETHING ODD"));
    c.check(
        "sub reason unknown -> none",
        odd.is_none(),
        &py_opt_str(odd.as_deref()),
    );

    let (sv, sm) = exec::sub_success_lines(&["en".to_string(), "en-GB".to_string()]);
    c.check(
        "sub success verdict joins codes",
        sv == "subtitles - en, en-GB",
        &py_opt_str(Some(&sv)),
    );
    c.check(
        "sub success with codes has no miss note",
        sm.is_empty(),
        &py_opt_str(Some(&sm)),
    );
    let (sv2, sm2) = exec::sub_success_lines(&[]);
    c.check(
        "sub success no codes verdict",
        sv2 == "subtitles - none available",
        &py_opt_str(Some(&sv2)),
    );
    c.check(
        "sub success no codes miss note",
        sm2 == "note: yt-dlp reported success but no subtitle files were read \
                - wording may have changed, please report",
        &py_opt_str(Some(&sm2)),
    );
    let fl = exec::sub_fail_lines(Some(r429_src));
    c.check(
        "sub fail lines verdict",
        fl.first().map(String::as_str)
            == Some("subtitles - not fetched (video kept). unable to download subtitles"),
        &py_opt_str(fl.first().map(String::as_str)),
    );
    c.check(
        "sub fail lines classified reason",
        fl.get(1).map(String::as_str)
            == Some("subtitle error - the subtitle api answered 429 too many requests"),
        &py_opt_str(fl.get(1).map(String::as_str)),
    );
    let fl2 = exec::sub_fail_lines(Some("SOMETHING ODD"));
    c.check(
        "sub fail lines parse-miss note",
        fl2.get(1).map(String::as_str)
            == Some("subtitle error - yt-dlp wording not recognized - please report"),
        &py_opt_str(fl2.get(1).map(String::as_str)),
    );
    let _ = &c;
}

pub(super) fn section_args(c: &mut Checker<'_>, root: &str, config_path: &str, auto: Option<&str>) {
    let settings = loaded_settings(config_path);
    let d = config::defaults();
    c.check(
        "load_settings fills missing keys",
        d.iter()
            .all(|(k, _)| settings.iter().any(|(k2, _)| k2 == k)),
        &py_value(config::get(&settings, "auto_update_last_check")),
    );

    let url = "https://x/y";
    let mut tmp = d.clone();
    config::set(&mut tmp, "video_container", Value::Str("mkv".to_string()));

    // the defaults run: 1080p, mp4, playlist off (python builds this
    // block from `d` BEFORE the container mutation)
    let a = argv_of(c, &d, url, ff_dir(), root, auto);
    c.check(
        "video1080 fmt",
        a_index(&a, "-f").map(String::as_str) == Some("bv*[height<=1080]+ba/b"),
        &got_list(&a),
    );
    c.check(
        "video1080 merge mp4",
        a_index(&a, "--merge-output-format").map(String::as_str) == Some("mp4"),
        &got_list(&a),
    );
    c.check(
        "no-playlist default",
        a_contains(&a, "--no-playlist"),
        &got_list(&a),
    );
    c.check(
        "ffmpeg location passed",
        a_contains(&a, "--ffmpeg-location"),
        &got_list(&a),
    );
    c.check(
        "url last",
        a.last().map(String::as_str) == Some(url),
        &got_list(&a),
    );

    // best resolution
    config::set(&mut tmp, "video_resolution", Value::Str("best".to_string()));
    let a = argv_of(c, &tmp, url, ff_dir(), root, auto);
    c.check(
        "best fmt",
        a_index(&a, "-f").map(String::as_str) == Some("bv*+ba/b"),
        &got_list(&a),
    );

    // container best drops the merge
    config::set(&mut tmp, "video_resolution", Value::Str("720".to_string()));
    config::set(&mut tmp, "video_container", Value::Str("best".to_string()));
    let a = argv_of(c, &tmp, url, ff_dir(), root, auto);
    c.check(
        "no merge when container best",
        !a_contains(&a, "--merge-output-format"),
        &got_list(&a),
    );

    // audio only
    config::set(&mut tmp, "type", Value::Str("audio only".to_string()));
    let a = argv_of(c, &tmp, url, ff_dir(), root, auto);
    let audio_extract =
        a_contains(&a, "-x") && a_index(&a, "--audio-format").map(String::as_str) == Some("mp3");
    c.check("audio extract", audio_extract, &got_list(&a));
    c.check(
        "audio quality",
        a_index(&a, "--audio-quality").map(String::as_str) == Some("0"),
        &got_list(&a),
    );

    // no ffmpeg: audio only falls back to bestaudio, no extract flag
    let a = argv_of(c, &tmp, url, None, root, auto);
    let no_ff = a_index(&a, "-f").map(String::as_str) == Some("bestaudio") && !a_contains(&a, "-x");
    c.check("no ffmpeg -> bestaudio", no_ff, &got_list(&a));
    c.check("no ffmpeg -> no -x", !a_contains(&a, "-x"), &got_list(&a));

    // no ffmpeg: video sticks to format b and carries no location
    config::set(&mut tmp, "type", Value::Str("video".to_string()));
    let a = argv_of(c, &tmp, url, None, root, auto);
    let video_no_ff =
        a_index(&a, "-f").map(String::as_str) == Some("b") && !a_contains(&a, "--ffmpeg-location");
    c.check("no ffmpeg video -> -f b", video_no_ff, &got_list(&a));
    c.check(
        "no ffmpeg -> no ffmpeg-location",
        !a_contains(&a, "--ffmpeg-location"),
        &got_list(&a),
    );

    // video no audio: one format flag, no merge, no extract
    config::set(&mut tmp, "type", Value::Str("video no audio".to_string()));
    let a = argv_of(c, &tmp, url, None, root, auto);
    let vna = a_index(&a, "-f").map(String::as_str) == Some("bv*[height<=720]")
        && !a_contains(&a, "--merge-output-format")
        && !a_contains(&a, "-x");
    c.check("video no audio 720 fmt", vna, &got_list(&a));
    c.check(
        "video no audio no merge",
        !a_contains(&a, "--merge-output-format"),
        &got_list(&a),
    );
    c.check("video no audio no -x", !a_contains(&a, "-x"), &got_list(&a));
    config::set(&mut tmp, "type", Value::Str("video".to_string()));

    // playlist mode
    config::set(&mut tmp, "playlist", Value::Bool(true));
    config::set(&mut tmp, "playlist_max_items", Value::Int(3));
    let a = argv_of(c, &tmp, url, ff_dir(), root, auto);
    let pl = a_index(&a, "--playlist-items").map(String::as_str) == Some("1-3")
        && !a_contains(&a, "--no-playlist");
    c.check("playlist items", pl, &got_list(&a));
    c.check(
        "playlist drops no-playlist",
        !a_contains(&a, "--no-playlist"),
        &got_list(&a),
    );

    // the video run never carries subtitle flags, the sub pass does
    config::set(&mut tmp, "subtitles", Value::Bool(false));
    let a = argv_of(c, &tmp, url, None, root, auto);
    let no_subs_flags = !a_contains(&a, "--sub-langs") && !a_contains(&a, "--write-subs");
    c.check(
        "video run carries no subs flags",
        no_subs_flags,
        &got_list(&a),
    );

    // the subtitle pass always lands its own commands - with ffmpeg the
    // conversion target is srt, without it vtt, and the output template
    // is shared with the video run
    let sa = sub_argv_of(c, &tmp, url, ff_dir(), root, auto);
    c.check(
        "sub args skip download",
        a_contains(&sa, "--skip-download"),
        &got_list(&sa),
    );
    c.check(
        "sub args lang pattern",
        a_index(&sa, "--sub-langs").map(String::as_str) == Some("en(-.*)?"),
        &got_list(&sa),
    );
    c.check(
        "sub args convert srt with ffmpeg",
        a_index(&sa, "--convert-subs").map(String::as_str) == Some("srt"),
        &got_list(&sa),
    );
    let sa2 = sub_argv_of(c, &tmp, url, None, root, auto);
    c.check(
        "sub args vtt without ffmpeg",
        a_index(&sa2, "--convert-subs").map(String::as_str) == Some("vtt"),
        &got_list(&sa2),
    );
    let shared_template = a_index(&sa, "-o") == a_index(&a, "-o");
    {
        let got_existing = a_index(&sa, "-o").cloned().unwrap_or_default();
        c.check(
            "sub args share output template",
            shared_template,
            &py_opt_str(Some(&got_existing)),
        );
    }
    c.check(
        "lang pattern multi",
        args::sub_lang_patterns(Some("en,nl")) == "en(-.*)?,nl(-.*)?",
        &py_opt_str(Some("en,nl")),
    );
    c.check(
        "lang pattern passthrough",
        args::sub_lang_patterns(Some("en.*")) == "en.*",
        &py_opt_str(Some("en.*")),
    );
    // python's `_ = a; _ = settings;` equivalents
    let _ = a;
}

pub(super) fn sub_argv_of(
    c: &mut Checker<'_>,
    s: &Obj,
    url: &str,
    ffdir: Option<&str>,
    root: &str,
    auto: Option<&str>,
) -> Vec<String> {
    let mut has_node = || false;
    match args::build_sub_args(s, url, ffdir, root, auto, &mut has_node) {
        Ok(argv) => argv,
        Err(kind) => {
            c.check("build_sub_args shape error", false, &format!("{kind:?}"));
            Vec::new()
        }
    }
}

pub(super) fn section_args2(c: &mut Checker<'_>, root: &str, auto: Option<&str>) {
    let url = "https://x/y";
    let mut tmp = config::defaults();
    config::set(&mut tmp, "subtitles", Value::Bool(false));
    let a = argv_of(c, &tmp, url, None, root, auto);
    c.check(
        "video run never has convert-subs",
        !a_contains(&a, "--convert-subs"),
        &got_list(&a),
    );

    // an absolute template suppresses -P and the work folder entirely
    config::set(
        &mut tmp,
        "filename_template",
        Value::Str("C:/out/%(title)s.%(ext)s".to_string()),
    );
    let a = argv_of(c, &tmp, url, ff_dir(), root, auto);
    c.check(
        "absolute template -> no -P",
        !a_contains(&a, "-P"),
        &got_list(&a),
    );
    c.check(
        "absolute template -> no temp work folder",
        !a.join(" ").contains("temp:"),
        &got_list(&a),
    );

    // a relative template keeps -P home and carries the work folder
    config::set(
        &mut tmp,
        "filename_template",
        Value::Str("play/%(title)s.%(ext)s".to_string()),
    );
    let a = argv_of(c, &tmp, url, ff_dir(), root, auto);
    c.check(
        "relative template keeps -P",
        a_index(&a, "-P")
            .map(|v| v.starts_with("home:"))
            .unwrap_or(false),
        &got_list(&a),
    );
    let mut second_p: Option<String> = None;
    let mut hits = 0;
    for (i, token) in a.iter().enumerate() {
        if token.as_str() == "-P" {
            hits += 1;
            if hits == 2 {
                second_p = a.get(i + 1).cloned();
            }
        }
    }
    let temp_flag = exec::temp_dir(&tmp, auto, root)
        .map(|temp| hits == 2 && second_p.as_deref() == Some(format!("temp:{temp}").as_str()))
        .unwrap_or(false);
    c.check(
        "relative template carries the temp work folder",
        temp_flag,
        &got_list(&a),
    );

    // saved folder survives; auto stays a real folder or None
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut fixed = config::defaults();
    config::set(&mut fixed, "output_dir", Value::Str(cwd.clone()));
    let saved = resolve::resolve_outdir(&fixed, auto, root).unwrap_or_default();
    c.check(
        "resolve_outdir keeps saved folder",
        saved == cwd,
        &py_opt_str(Some(&saved)),
    );
    let auto_folder = real_auto_outdir();
    let auto_ok = match &auto_folder {
        None => true,
        Some(p) => std::path::Path::new(p).is_dir(),
    };
    c.check(
        "auto_outdir is an existing folder or None",
        auto_ok,
        &py_opt_str(auto_folder.as_deref()),
    );
    let mut empty = config::defaults();
    config::set(&mut empty, "output_dir", Value::Str(String::new()));
    let a = argv_of(c, &empty, url, ff_dir(), root, auto);
    c.check(
        "empty output_dir still passes -P home",
        a_index(&a, "-P")
            .map(|v| v.starts_with("home:"))
            .unwrap_or(false),
        &got_list(&a),
    );

    let fixed_temp = exec::temp_dir(&fixed, auto, root).unwrap_or_default();
    let stacked = resolve::py_path_join(&saved, resolve::TEMP_DIR_NAME);
    c.check(
        "temp_dir sits inside the output folder",
        fixed_temp == stacked && resolve::TEMP_DIR_NAME == ".ytdlp-tmp",
        &got_strs(&[&fixed_temp, &stacked]),
    );
}

pub(super) fn section_partials(c: &mut Checker<'_>) {
    let wt = mkdtemp("ytdlp-wf-");
    let j = |name: &str| resolve::py_path_join(&wt, name);
    let fresh_paths: Vec<String> = vec![
        j("v.mp4.part"),
        j("v.mp4.part-Frag7.part"),
        j("v.mp4.ytdl"),
        j("s.en.vtt.part"),
    ];
    let fresh_names: Vec<String> = vec![
        "v.mp4.part".to_string(),
        "v.mp4.part-Frag7.part".to_string(),
        "v.mp4.ytdl".to_string(),
        "s.en.vtt.part".to_string(),
    ];
    let old = j("stale.mp4.part");
    for path in fresh_paths.iter().chain(std::iter::once(&old)) {
        write_bytes(path, b"x");
    }
    set_mtime_back(&old, 3600.0);
    let now = now_secs();
    let mut removed = exec::clean_partials(&wt, now, 5.0);
    removed.sort();
    let mut want = fresh_names.clone();
    want.sort();
    c.check(
        "clean_partials removes fresh part, frag and ytdl files",
        removed == want,
        &got_list(&removed),
    );
    c.check(
        "clean_partials keeps a stale leftover",
        std::path::Path::new(&old).exists(),
        &got_list(&removed),
    );

    let mut ws = config::defaults();
    config::set(&mut ws, "output_dir", Value::Str(wt.clone()));
    let work = resolve::py_path_join(&wt, resolve::TEMP_DIR_NAME);
    let _ = std::fs::create_dir(&work);
    exec::drop_temp_dir(&ws, None, wt.clone().as_str());
    c.check(
        "drop_temp_dir removes an empty work folder",
        !std::path::Path::new(&work).exists(),
        &got_strs(&[&work]),
    );

    let _ = std::fs::create_dir(&work);
    write_bytes(&resolve::py_path_join(&work, "v.mp4.part"), b"x");
    exec::drop_temp_dir(&ws, None, &wt);
    c.check(
        "drop_temp_dir keeps a work folder with resume state",
        std::path::Path::new(&work).is_dir(),
        &got_strs(&[&work]),
    );

    let missing = resolve::py_path_join(&wt, "missing");
    let mut gone = config::defaults();
    config::set(&mut gone, "output_dir", Value::Str(missing.clone()));
    exec::drop_temp_dir(&gone, None, &wt);
    c.check(
        "drop_temp_dir tolerates a missing folder",
        true,
        &got_strs(&[&missing]),
    );

    pyfs::rmtree_quiet(&wt);
}

pub(super) fn section_misc(c: &mut Checker<'_>) {
    // clean_url
    let basic = args::clean_url(Some("watch https://youtu.be/abc-123_x now"));
    c.check(
        "clean_url basic",
        basic.as_deref() == Some("https://youtu.be/abc-123_x"),
        &py_opt_str(basic.as_deref()),
    );
    let punct = args::clean_url(Some("https://youtu.be/z9?si=1, ok"));
    c.check(
        "clean_url trims punctuation",
        punct.as_deref() == Some("https://youtu.be/z9?si=1"),
        &py_opt_str(punct.as_deref()),
    );
    let none = args::clean_url(Some("no link here"));
    c.check("clean_url none", none.is_none(), &py_opt_str(None::<&str>));

    // fail_sentence
    let long_err = "ERROR: unable to rename file: [WinError 32] The process \
                    cannot access the file because it is being used by another process";
    c.check(
        "fail sentence has rc",
        exec::fail_sentence(7, "") == "downloading - failed (rc=7)",
        &bare_got(),
    );
    let keep = exec::fail_sentence(1, long_err);
    c.check(
        "fail sentence keeps whole error line",
        keep == format!("downloading - failed (rc=1): {long_err}"),
        &py_opt_str(Some(&keep)),
    );
    c.check(
        "fail sentence no width cut",
        keep.chars().count() > WIDTH as usize,
        &py_opt_str(Some(&keep)),
    );
}

/// ---------------- progress pins ----------------
pub(super) fn section_progress(c: &mut Checker<'_>) {
    let cp = term::color_progress("15.2% of 198.15MiB at 2.32MiB/s ETA 01:12");
    c.check(
        "progress percent amber",
        cp.contains(&format!("{}15.2%{}", term::A_AMBER, term::A_RESET)),
        &got_strs(&["15.2%"]),
    );
    c.check(
        "progress size one decimal",
        cp.contains(&format!("{}198.1MiB{}", term::A_MINT, term::A_RESET)),
        &got_strs(&["198.15MiB"]),
    );
    c.check(
        "progress speed padded",
        cp.contains(&format!("{}002.3MiB/s{}", term::A_LBLUE, term::A_RESET)),
        &got_strs(&["2.32MiB/s"]),
    );
    c.check(
        "progress eta purple",
        cp.contains(&format!("{}01:12{}", term::A_PURPLE, term::A_RESET)),
        &got_strs(&["01:12"]),
    );
    c.check(
        "progress words grey",
        cp.contains(&format!("{}of{}", term::A_GREY, term::A_RESET))
            && cp.contains(&format!("{}at{}", term::A_GREY, term::A_RESET))
            && cp.contains(&format!("{}ETA{}", term::A_GREY, term::A_RESET)),
        &got_strs(&["of", "at", "ETA"]),
    );
    let final_line = term::color_progress("100% of 198.15MiB in 00:45");
    c.check(
        "progress final line",
        final_line.contains(&format!("{}100%{}", term::A_AMBER, term::A_RESET)),
        &got_strs(&["100%"]),
    );
    let cp2 = term::color_progress("5.0% of ~4.63MiB at 9.94MiB/s ETA 00:57");
    c.check(
        "speed pads the 9 to 10 crossing",
        cp2.contains(&format!("{}009.9MiB/s{}", term::A_LBLUE, term::A_RESET))
            && cp2.contains(&format!("{}~4.6MiB{}", term::A_MINT, term::A_RESET)),
        &got_strs(&["9.94MiB/s", "~4.63MiB"]),
    );
    c.check(
        "speed keeps three digits unpadded",
        term::pad_speed("999.9MiB/s") == "999.9MiB/s",
        &got_strs(&["999.9MiB/s"]),
    );
    c.check(
        "speed truncates one decimal",
        term::pad_speed("99.99MiB/s") == "099.9MiB/s",
        &got_strs(&["99.99MiB/s"]),
    );
    c.check(
        "percent never gains a digit",
        term::one_dec_num("15.2%") == "15.2%",
        &got_strs(&["15.2%"]),
    );
}

pub(super) fn section_spinners(c: &mut Checker<'_>, capture: &CaptureOut) {
    let all = term::spinners();
    let names: Vec<&str> = all.iter().map(|s| s.name).collect();
    c.check("eight spinners", all.len() == 8, &got_strs_vec(&names));
    let mut unique: Vec<&str> = names.clone();
    unique.sort_unstable();
    unique.dedup();
    c.check(
        "spinner names unique",
        unique.len() == 8,
        &got_strs_vec(&names),
    );

    for spinner in all {
        let shaped = !spinner.frames.is_empty()
            && spinner.tick > 0.0
            && spinner.tick <= 0.2
            && spinner.frames.iter().all(|f| f.len() == 6)
            && spinner.freeze.len() == 6;
        c.check(
            &format!("spinner {} shape", spinner.name),
            shaped,
            &bare_got(),
        );
    }

    let pick = term::pick_spinner();
    let picked_in = all.iter().any(|s| s.name == pick.name);
    c.check(
        "spinner pick is one of them",
        picked_in,
        &got_strs(&[pick.name]),
    );
    c.check(
        "render_cells one color run",
        term::render_cells(&[("@".to_string(), 231), (".".to_string(), 231)])
            == "\x1b[38;5;231m@.\x1b[0m",
        &bare_got(),
    );
    c.check(
        "render_cells color change",
        term::render_cells(&[("@".to_string(), 231), (".".to_string(), 202)])
            == "\x1b[38;5;231m@\x1b[38;5;202m.\x1b[0m",
        &bare_got(),
    );
    let first = &all[0];
    c.check(
        "plain_cells strips padding",
        term::plain_cells(&first.freeze) == "@",
        &got_strs(&[""]),
    );
    let laid = term::flat_frames(&[".  ".to_string()], 214);
    let expected_layout = vec![vec![
        (".".to_string(), 214),
        (" ".to_string(), 214),
        (" ".to_string(), 214),
    ]];
    c.check(
        "flat_frames cell layout",
        laid == expected_layout,
        &got_strs(&[".  "]),
    );
    c.check(
        "flat freeze stays plain",
        term::plain_cells(&term::flat_freeze("...", 214)) == "...",
        &got_strs(&["..."]),
    );
    let dots: Vec<String> = term::DOTS.iter().map(|d| (*d).to_string()).collect();
    let ds = tty::dot_stage(
        "probe stage",
        true,
        tty::SharedOut::new(capture.clone()).sink(),
    );
    c.check(
        "dot stage carries amber dots",
        ds.frames() == term::flat_frames(&dots, 214) && ds.tick_secs() == term::FRAME_SECS,
        &got_strs(&["probe stage"]),
    );
    c.check(
        "dot stage renders in color",
        term::render_cells(ds.freeze_cells()) == "\x1b[38;5;214m...\x1b[0m",
        &bare_got(),
    );
    let freeze_colored = all[0]
        .frames
        .first()
        .map(|f| term::render_cells(f).ends_with(term::A_RESET))
        .unwrap_or(false);
    c.check(
        "spinner freeze renders in color",
        freeze_colored,
        &got_strs(&[""]),
    );
}

pub(super) fn section_menu(
    c: &mut Checker<'_>,
    root: &str,
    config_path: &str,
    auto: Option<&str>,
    is_windows: bool,
) {
    let settings = loaded_settings(config_path);
    let outdir = outdir_of(&settings, root, auto);
    let rows = match ui::menu_items(&settings, &outdir, is_windows) {
        Ok(rows) => rows,
        Err(kind) => {
            c.check("menu_items shape error", false, &format!("{kind:?}"));
            Vec::new()
        }
    };
    let mut nums: Vec<i64> = rows.iter().map(|r| r.idx).collect();
    let sequential = rows.len() == 13 && {
        let want: Vec<i64> = (1..=13).collect();
        nums == want
    };
    nums.sort_unstable();
    c.check("menu has 13 rows", sequential, &format!("{nums:?}"));
    let row_nine = rows.get(8).map(|r| r.label.clone()).unwrap_or_default();
    c.check(
        "menu row 9 is multi url",
        row_nine == "add multiple youtube urls",
        &got_strs(&[&row_nine]),
    );
    let row_thirteen = rows.get(12).map(|r| r.label.clone()).unwrap_or_default();
    c.check(
        "menu row 13 is quit",
        row_thirteen == "quit",
        &got_strs(&[&row_thirteen]),
    );
    let nine_value = rows.get(8).map(|r| r.value.clone()).unwrap_or_default();
    let nine_grey = rows.get(8).map(|r| r.grey).unwrap_or(false);
    let nine_ok = nine_value.is_empty() && nine_grey;
    let _ = &nums;
    c.check(
        "menu row 9 has no value",
        nine_ok,
        &got_strs(&[&nine_value]),
    );
    let labels: Vec<String> = (9..12)
        .map(|i| {
            rows.get(i as usize)
                .map(|r| r.label.clone())
                .unwrap_or_default()
        })
        .collect();
    let shifted = labels.first().map(String::as_str) == Some("reset defaults")
        && labels.get(1).map(String::as_str) == Some("update yt-dlp + cli")
        && labels.get(2).map(String::as_str) == Some("auto update");
    c.check(
        "menu rows 10 to 12 shifted",
        shifted,
        &got_strs(&[
            labels.first().map(String::as_str).unwrap_or(""),
            labels.get(1).map(String::as_str).unwrap_or(""),
            labels.get(2).map(String::as_str).unwrap_or(""),
        ]),
    );
    let auto_state = rows.get(11).map(|r| r.value.clone()).unwrap_or_default();
    c.check(
        "auto update row shows its state",
        auto_state == "on" || auto_state == "off",
        &got_strs(&[&auto_state]),
    );

    let d = config::defaults();
    c.check(
        "auto update default is on",
        config::get(&d, "auto_update") == Some(&Value::Bool(true)),
        &py_value(Some(&Value::Bool(true))),
    );
    c.check(
        "auto update stamps its last check",
        config::get(&d, "auto_update_last_check").is_some(),
        &py_value(Some(&Value::Null)),
    );

    // ffmpeg sources: windows BtbN zip (FR-14 amendment 2026-10-16c),
    // linux BtbN tar.xz, macos shaka-project raw binary (FR-14
    // amendment 2026-10-17g)
    let windows_source = exec::ffmpeg_source("windows");
    let linux_source = exec::ffmpeg_source("linux");
    let macos_source = exec::ffmpeg_source("macos");
    let covered = windows_source.is_some() && linux_source.is_some() && macos_source.is_some();
    c.check(
        "ffmpeg sources cover windows, linux and macos",
        covered,
        &got_strs(&["windows", "linux", "macos"]),
    );
    // the native macos source is the shaka-project static build: a
    // public repo with contributors, CI-built, sha256 per asset in the
    // API, and the binary published raw - owner call 2026-10-17g
    let macos_well = macos_source
        .as_ref()
        .map(|src| {
            src.url.contains("shaka-project/static-ffmpeg-binaries")
                && src.sha_url.contains("api.github.com")
                && src.binname == "ffmpeg"
                && src.how == "raw"
                && src.sha_kind == "gh-api"
        })
        .unwrap_or(false);
    c.check(
        "macos ffmpeg source is shaka-project",
        macos_well,
        &macos_source
            .as_ref()
            .map(|src| got_strs(&[src.url]))
            .unwrap_or_default(),
    );
    for (platform, source) in [("linux", &linux_source), ("windows", &windows_source)] {
        let well = match source {
            Some(src) => {
                src.url.starts_with("https://")
                    && src.sha_url.starts_with("https://")
                    && (src.binname == "ffmpeg" || src.binname == "ffmpeg.exe")
                    && (src.how == "zip" || src.how == "tar.xz")
            }
            None => false,
        };
        c.check(
            &format!("ffmpeg source {platform} is well formed"),
            well,
            &bare_got(),
        );
    }
    // owner-decided 2026-10-16c (FR-14 amendment): python asserted
    // gyan.dev; the native offer fetches the BtbN win64 build, and the
    // compare tools map this name back to python's (conformance map)
    let windows_well = windows_source
        .as_ref()
        .map(|src| src.url.contains("BtbN/FFmpeg-Builds"))
        .unwrap_or(false);
    c.check(
        "windows ffmpeg source is BtbN",
        windows_well,
        &windows_source
            .as_ref()
            .map(|src| got_strs(&[src.url]))
            .unwrap_or_default(),
    );
    let linux_well = linux_source
        .as_ref()
        .map(|src| src.url.contains("BtbN/FFmpeg-Builds"))
        .unwrap_or(false);
    c.check(
        "linux ffmpeg source is BtbN",
        linux_well,
        &linux_source
            .as_ref()
            .map(|src| got_strs(&[src.url]))
            .unwrap_or_default(),
    );

    // python pinned the raw-main script url; the native contract (FR-13)
    // is the releases api, so this check names the api pin instead
    let api_ok = update::RELEASES_API.ends_with("/releases/latest")
        && update::RELEASES_API.contains("api.github.com")
        && update::RELEASES_API.contains("ytdlp-cli-youtube-downloader");
    c.check(
        "cli update source is the releases api",
        api_ok,
        &got_strs(&[update::RELEASES_API]),
    );
}

pub(super) fn got_strs_vec(values: &[&str]) -> String {
    let parts: Vec<String> = values.iter().map(|v| format!("'{v}'")).collect();
    format!("[{}]", parts.join(", "))
}

pub(super) fn section_parsers(c: &mut Checker<'_>, root: &str, image_path: &str, is_windows: bool) {
    // version_tuple
    let v110 = update::version_tuple(Some("1.10.0"));
    let v190 = update::version_tuple(Some("1.9.0"));
    let v12 = update::version_tuple(Some("1.2.0"));
    let v13 = update::version_tuple(Some("1.3"));
    let vempty = update::version_tuple(Some(""));
    let v13b = update::version_tuple(Some("1.3-beta"));
    let vdev = update::version_tuple(Some("dev"));
    let compares = v110 > v190 && v12 < v13 && vempty == vec![0] && v13 == vec![1, 3];
    let junk_pieces = v13b >= v13 && vdev == vec![0];
    c.check(
        "version_tuple compares numerically",
        compares,
        &format!("({v110:?}, {v190:?})"),
    );
    c.check(
        "version_tuple counts junk pieces as 0",
        junk_pieces,
        &format!("{v13b:?}"),
    );

    // parse_sha256
    let digest = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
    let bare = update::parse_sha256(digest);
    c.check(
        "parse_sha256 reads a bare 64 char digest",
        bare == Some(digest.to_string()),
        &bare_got(),
    );
    let with_name = update::parse_sha256(&format!("{digest}  ffmpeg-9.0.2.zip"));
    c.check(
        "parse_sha256 reads digest then filename",
        with_name == Some(digest.to_string()),
        &bare_got(),
    );
    let refusals = update::parse_sha256("no digest here").is_none()
        && update::parse_sha256("").is_none()
        && update::parse_sha256("tooshort").is_none()
        && update::parse_sha256(&"g".repeat(64)).is_none();
    c.check(
        "parse_sha256 refuses anything without a digest",
        refusals,
        &bare_got(),
    );

    // pick_checksum
    let asset = "ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz";
    let digest_again = "afa2b69c9965919e0b519a633520321f838d74690f8fb28c76393c54a9448d60";
    let checksum = format!("aaa  x.zip\n{digest_again}  {asset}\nbbb  y.zip\n");
    let picked = update::pick_checksum(&checksum, asset);
    c.check(
        "pick_checksum picks the line naming the asset",
        picked == Some(digest_again.to_string()),
        &bare_got(),
    );
    let missed = update::pick_checksum("aaa  x.zip\nbbb  y.zip\n", "z.zip").is_none()
        && update::pick_checksum("", "z.zip").is_none();
    c.check(
        "pick_checksum misses an asset it was not given",
        missed,
        &bare_got(),
    );

    // the installer dance on a scratch side-name pair: python's checks
    // never renamed the running image either (its dance wrote into
    // `<self>.new` and replaced a side name); the gate is scripted here
    // because a live-spawned gate would recurse (specsheet section 4)
    let scratch = mkdtemp("ytdlp-index-");
    let target = resolve::py_path_join(&scratch, "copy");
    let staged = resolve::py_path_join(&scratch, "copy.new");
    let old_image = format!("{target}.old");
    let image = file_bytes(image_path);
    write_bytes(&target, &image);
    let mut staged_bytes = image.clone();
    staged_bytes.extend_from_slice(b"  selftest staged\n");
    write_bytes(&staged, &staged_bytes);

    let gate_rc = Rc::<RefCell<i64>>::new(RefCell::new(2));
    let mut capture = CaptureOut::default();
    {
        let mut http = DeadHttp;
        let mut probe = |_b: &str| (String::new(), -1i64);
        let mut selftest_gate = {
            let gate_rc = Rc::clone(&gate_rc);
            move |_staged: &str| *gate_rc.borrow_mut()
        };
        let mut call = |_b: &str, _a: &[String]| 0i64;
        let mut now = now_secs;
        let mut which = |_cmd: &str| None::<String>;
        let mut reader = NoReader;
        let shared = tty::SharedOut::new(capture.clone());
        let sink = shared.sink();
        let mut ctx = update::UpdateCtx {
            platform: platform_name(),
            arch: arch_name(),
            self_path: &target,
            root,
            is_windows,
            use_color: false,
            stdin_tty: false,
            sink,
            http: &mut http,
            probe: &mut probe,
            selftest: &mut selftest_gate,
            call: &mut call,
            now: &mut now,
            which: &mut which,
            out: &mut capture,
            reader: &mut reader,
        };
        let (ok, message) = update::install_cli(&mut ctx, &staged);
        c.check(
            "install_cli refuses an image that fails its own checks",
            !ok,
            &got_strs(&[&message]),
        );
        let untouched = file_bytes(&target) == image
            && !std::path::Path::new(&old_image).exists()
            // python's own refusal leaves no `.new` behind either
            // (it never created one; the native gate removes the staged
            // image it refused)
            && !std::path::Path::new(&staged).exists();
        c.check(
            "install_cli wrote nothing when it refused",
            untouched,
            &got_list(&dir_names(&scratch)),
        );

        *gate_rc.borrow_mut() = 0;
        let mut staged_bytes = image.clone();
        staged_bytes.extend_from_slice(b"  selftest staged\n");
        write_bytes(&staged, &staged_bytes);
        let (installed, placed) = update::install_cli(&mut ctx, &staged);
        let placed_ok = installed
            && file_bytes(&target) == staged_bytes
            && std::path::Path::new(&old_image).exists();
        c.check(
            "install_cli writes exactly the bytes it was given",
            placed_ok,
            &got_strs(&[&placed]),
        );
        let _ = &installed;
    }

    pyfs::rmtree_quiet(&scratch);
}

pub(super) fn section_langs(c: &mut Checker<'_>) {
    let langs = ui::sub_lang_lines(WIDTH);
    let boxed = !langs.is_empty() && langs.iter().all(|l| l.chars().count() <= WIDTH as usize);
    c.check("lang lines fit the box", boxed, &got_list(&langs));
    let shaped = langs
        .iter()
        .all(|l| l.starts_with("  ") && !l.trim().contains("  ") && l.trim_end() == *l);
    c.check(
        "lang lines are indented and spaced",
        shaped,
        &got_list(&langs),
    );
    let words: Vec<&str> = langs.iter().flat_map(|l| l.split_whitespace()).collect();
    let carries = ["en", "lt", "et", "ru", "zh", "vi", "ja", "ko"]
        .iter()
        .all(|code| words.contains(code));
    c.check(
        "lang list carries the expected codes",
        carries,
        &got_list(&langs.iter().map(String::as_str).collect::<Vec<_>>()),
    );
    let two_letters = words.iter().all(|w| {
        w.chars().all(char::is_alphabetic)
            && (2..=3).contains(&w.chars().count())
            && w.chars().all(char::is_lowercase)
    }) && words.contains(&"fil");
    c.check(
        "lang codes are two letters, fil aside",
        two_letters,
        &got_list(
            &words
                .iter()
                .filter(|w| !w.chars().all(char::is_alphabetic))
                .map(|w| w.to_string())
                .collect::<Vec<_>>(),
        ),
    );
    let first_full = langs
        .first()
        .map(|l| l.chars().count() == WIDTH as usize)
        .unwrap_or(false);
    c.check(
        "lang first line fills the box",
        first_full,
        &format!("{}", langs.first().map(|l| l.chars().count()).unwrap_or(0)),
    );
    let reference: Vec<&str> = args::KNOWN_LANG_CODES.to_vec();
    let whole = {
        let mut flat: Vec<&str> = reference.clone();
        flat.sort_unstable();
        flat == words && words.len() == reference.len()
    };
    c.check(
        "lang list is the whole reference, alphabetical",
        whole,
        &got_strs(&["words"]),
    );
    c.check(
        "lang list fits in ten lines",
        langs.len() == 10,
        &format!("{}", langs.len()),
    );
    let narrow_well = ui::sub_lang_lines(24)
        .iter()
        .all(|l| l.chars().count() <= 24);
    {
        let narrow = ui::sub_lang_lines(24);
        c.check("lang lines wrap narrow", narrow_well, &got_list(&narrow));
    }
    let extra: Vec<&str> = reference
        .iter()
        .filter(|w| !words.contains(*w))
        .copied()
        .collect();
    let missing: Vec<&str> = words
        .iter()
        .filter(|w| !reference.contains(*w))
        .copied()
        .collect();
    let same = extra.is_empty()
        && missing.is_empty()
        && reference.iter().all(|w| w.len() == 2 || *w == "fil");
    {
        let mut mixed: Vec<&str> = Vec::new();
        mixed.extend(extra.iter().copied());
        mixed.extend(missing.iter().copied());
        let mixed_owned: Vec<String> = mixed.into_iter().map(|s| s.to_string()).collect();
        c.check(
            "lang ref is what the list shows",
            same,
            &got_list_sorted(&mixed_owned),
        );
    }
    let leftovers = reference.iter().filter(|w| w.len() > 3).count() == 0;
    c.check(
        "lang ref has no yt-dlp-only leftovers",
        leftovers,
        &got_list_sorted(&reference.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
    );
}

/// python's `sorted()` before a repr so the `got` reads sorted.
pub(super) fn got_list_sorted(values: &[String]) -> String {
    let mut sorted: Vec<String> = values.to_vec();
    sorted.sort();
    got_list(&sorted)
}

pub(super) fn section_lang_input(c: &mut Checker<'_>) {
    let (k1, b1, u1) = args::parse_lang_input(Some("en,8"));
    let first = k1 == ["en".to_string()] && b1 == ["8".to_string()] && u1.is_empty();
    let _ = (&k1, &b1, &u1);
    c.check("digit dropped, en kept", first, &got_strs(&["en", "8"]));

    let (k2, b2, u2) = args::parse_lang_input(Some("yue,nl-BE"));
    let second = k2 == ["yue".to_string(), "nl-be".to_string()]
        && b2.is_empty()
        && u2 == ["yue".to_string()];
    c.check(
        "unknown kept, variant kept",
        second,
        &got_strs(&["yue", "nl-be"]),
    );

    let (k3, b3, u3) = args::parse_lang_input(Some("en-GB, sr-Latn, zh-Hans, es-419"));
    let third =
        k3 == [
            "en-gb".to_string(),
            "sr-latn".to_string(),
            "zh-hans".to_string(),
            "es-419".to_string(),
        ] && b3.is_empty()
            && u3.is_empty();
    c.check(
        "variants all kept",
        third,
        &got_strs(&["en-gb", "sr-latn", "zh-hans", "es-419"]),
    );

    let (k4, b4, u4) = args::parse_lang_input(Some("  EN , , de  "));
    let fourth = k4 == ["en".to_string(), "de".to_string()] && b4.is_empty() && u4.is_empty();
    c.check("spaces, case and empties", fourth, &got_strs(&["en", "de"]));

    let (k5, b5, u5) = args::parse_lang_input(Some("en.*,nl,xx!!"));
    let fifth = k5.first().map(String::as_str) == Some("en.*")
        && b5 == ["xx!!".to_string()]
        && u5.is_empty();
    c.check(
        "wildcard passes through",
        fifth,
        &got_strs(&["en.*", "xx!!"]),
    );

    let (k5b, b5b, u5b) = args::parse_lang_input(Some("fil,yue"));
    let fifthb = k5b == ["fil".to_string(), "yue".to_string()]
        && b5b.is_empty()
        && u5b == ["yue".to_string()];
    c.check(
        "fil known, yue kept with note",
        fifthb,
        &got_strs(&["fil", "yue"]),
    );

    let (k6, b6, u6) = args::parse_lang_input(Some("8"));
    let sixth = k6.is_empty() && b6 == ["8".to_string()] && u6.is_empty();
    c.check("only digits leaves nothing", sixth, &got_strs(&["8"]));
    let _ = (&u1, &u2, &u3, &u4, &u5, &u5b, &u6);
}

fn no_clipboard() -> Option<String> {
    None
}

pub(super) fn menu_world<'w>(
    version: &'w str,
    root: &'w str,
    use_color: bool,
    stdin_tty: bool,
    terminal: Option<(i64, i64)>,
    auto: Option<&'w str>,
) -> World<'w> {
    World {
        version,
        root,
        config_path: "",
        is_windows: cfg!(windows),
        use_color,
        stdin_tty,
        stdout_tty: false,
        terminal,
        auto,
        clipboard: &no_clipboard,
    }
}

pub(super) fn make_io<'a>(capture: &'a mut CaptureOut, reader: &'a mut dyn Reader) -> Io<'a> {
    Io {
        out: capture,
        reader,
    }
}

pub(super) fn section_holds(
    c: &mut Checker<'_>,
    version: &str,
    root: &str,
    config_path: &str,
    auto: Option<&str>,
) {
    // pick_list: every message printed after a last read must hold
    let cases: Vec<(&'static str, &'static str, &'static str)> = vec![
        ("pick_list holds on invalid number", "abc", "invalid number"),
        ("pick_list holds on out of range", "9", "out of range"),
        ("pick_list does not hold on a good pick", "2", ""),
    ];
    for (name, reply, marker) in cases {
        let mut capture = CaptureOut::default();
        let mut reader = ScriptedReader::new(&[reply, ""]);
        let mut io = make_io(&mut capture, &mut reader);
        let world = menu_world(version, root, false, false, None, auto);
        let current = Value::Str("a".to_string());
        let pick = menu::pick_list("t", &["a", "b"], Some(&current), &mut io, &world);
        let picked = match reply {
            "2" => matches!(pick, Ok(Some(Value::Str(v))) if v == "b"),
            _ => matches!(pick, Ok(None)),
        };
        let held = if marker.is_empty() {
            reader.seen.len() == 1
                && !capture.text().contains("range")
                && !capture.text().contains("invalid")
        } else {
            reader.seen.len() == 2
                && reader.seen.last().map(|(p, _)| p.as_str()) == Some("")
                && capture.text().contains(marker)
        };
        c.check(name, picked && held, &got_strs(&[reply]));
    }

    // item 8 and item 10 print after their last read and must hold too
    let actions: Vec<(&str, Vec<&str>, &str)> = vec![
        ("8", vec!["y", "abc"], "not a number - kept"),
        ("10", vec![], "defaults restored"),
    ];
    for (choice, replies, marker) in actions {
        let mut capture = CaptureOut::default();
        let mut reader = ScriptedReader::new(&replies);
        let mut io = make_io(&mut capture, &mut reader);
        let world = menu_world(version, root, false, false, None, auto);
        let mut s = loaded_settings(config_path);
        config::set(&mut s, "playlist", Value::Bool(true));
        let _outcome = menu::act(
            choice,
            &mut s,
            &mut io,
            &world,
            &mut no_download,
            &mut no_update,
        );
        let last_prompt = reader
            .seen
            .last()
            .map(|(p, _)| p.clone())
            .unwrap_or_default();
        c.check(
            &format!("item {choice} holds its message"),
            capture.text().contains(marker) && last_prompt.is_empty(),
            &got_strs(&[choice]),
        );
    }
}

pub(super) fn section_item7(
    c: &mut Checker<'_>,
    version: &str,
    root: &str,
    config_path: &str,
    auto: Option<&str>,
) {
    let mut capture = CaptureOut::default();
    let mut reader = ScriptedReader::new(&["y", "en,8", ""]);
    let mut io = make_io(&mut capture, &mut reader);
    let world = menu_world(version, root, true, false, None, auto);
    let mut s = loaded_settings(config_path);
    let _rep7 = menu::act(
        "7",
        &mut s,
        &mut io,
        &world,
        &mut no_download,
        &mut no_update,
    );

    let plain_line: Vec<String> = strip_tokens(&capture.text())
        .split('\n')
        .map(|l| l.to_string())
        .collect();
    let block7: Vec<String> = ui::sub_lang_lines(WIDTH)
        .iter()
        .map(|l| l.trim().to_string())
        .collect();
    let nb = block7.len();
    let order_len = reader.seen.len() as i64;
    let second_below = reader
        .seen
        .get(1)
        .map(|(_, b)| b.is_none())
        .unwrap_or(false);
    let fallback = tty::block_under_input(&block7, true, false, None);
    let block_above = {
        let plain_block: Vec<String> = plain7_slice(&plain_line, 1, nb)
            .iter()
            .map(|l| l.trim().to_string())
            .collect();
        plain_block == block7
    };
    if !block_above {
        for (i, line) in plain_line.iter().enumerate().take(6) {
            eprintln!("item7-debug [{i}] = {line:?}");
        }
        eprintln!("item7-debug nb={nb}");
    }
    c.check(
        "item 7 fallback puts the block above the input",
        order_len == 3 && second_below && !fallback && block_above,
        &got_strs(&["item 7"]),
    );
    let label_line = plain7_at(&plain_line, 1 + nb);
    let echo_line = plain7_at(&plain_line, 2 + nb);
    c.check(
        "item 7 label gets its own typing line",
        label_line.ends_with("(enter = keep):") && echo_line == "en,8",
        &got_strs(&[label_line.as_str(), echo_line.as_str()]),
    );
    let verdict_line = plain7_at(&plain_line, 3 + nb);
    let hold_line = plain7_at(&plain_line, 4 + nb);
    c.check(
        "item 7 verdict follows the typed value",
        verdict_line == "  not a language code, dropped: 8" && hold_line.is_empty(),
        &got_strs(&[verdict_line.as_str(), hold_line.as_str()]),
    );
}

pub(super) fn plain7_slice(text: &[String], start: usize, count: usize) -> Vec<String> {
    text.iter().skip(start).take(count).cloned().collect()
}

pub(super) fn plain7_at(text: &[String], index: usize) -> String {
    text.get(index).cloned().unwrap_or_default()
}

pub(super) fn no_download(_s: &mut Obj, _url: &str, _io: &mut Io<'_>) -> i32 {
    0
}

pub(super) fn no_update(_s: &mut Obj, _io: &mut Io<'_>) -> Result<(), menu::MenuErr> {
    Ok(())
}

pub(super) fn section_block(c: &mut Checker<'_>) {
    let block = ui::sub_lang_lines(WIDTH);
    let roomy = tty::block_under_input(&block, true, true, Some((120, 50)));
    c.check(
        "block fits under a live cursor when the window is roomy",
        roomy,
        &bare_got(),
    );
    let capture = CaptureOut::default();
    {
        let mut out = capture.clone();
        out.write("  enter languages: ");
        out.write("\n  x\n  x\n  x\n");
        out.write("\x1b[4A");
        let painted = capture.text();
        c.check(
            "block under input ends with the cursor back on the prompt",
            painted.ends_with("\x1b[4A") && painted.matches('\n').count() == 4,
            &bare_got(),
        );
    }
    let tight = tty::block_under_input(&block, true, true, Some((120, 12)));
    c.check(
        "a block taller than the window falls back",
        !tight,
        &bare_got(),
    );
    let narrow = tty::block_under_input(&block, true, true, Some((60, 50)));
    c.check(
        "a block wider than the window falls back",
        !narrow,
        &bare_got(),
    );
}

pub(super) fn section_batch(
    c: &mut Checker<'_>,
    version: &str,
    root: &str,
    config_path: &str,
    auto: Option<&str>,
) {
    // batch rows
    let keep = ui::batch_row(1, "https://x/y", false);
    c.check(
        "batch row keeps url",
        keep.ends_with("https://x/y"),
        &got_strs(&["keep url"]),
    );
    let marked = format!(
        "{}{}",
        term::c(false, term::A_GREY, "  2  "),
        term::c(false, term::A_AMBER, "error in url")
    );
    c.check(
        "batch row marks bad entry",
        ui::batch_row(2, "nope", false) == marked,
        &got_strs(&["nope"]),
    );

    // run: the two good urls in order, the bad one skipped
    let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let mut capture = CaptureOut::default();
    let mut reader = ScriptedReader::new(&[]);
    let mut io = Io {
        out: &mut capture,
        reader: &mut reader,
    };
    let world = menu_world(version, root, false, false, None, auto);
    let mut s = loaded_settings(config_path);
    let entries: Vec<String> = vec![
        "https://a/1".to_string(),
        "not a url".to_string(),
        "https://b/2".to_string(),
    ];
    let mut downloader = {
        let seen = Rc::clone(&seen);
        move |_s: &mut Obj, url: &str, _io: &mut Io<'_>| {
            seen.borrow_mut().push(url.to_string());
            0
        }
    };
    let ran = menu::run_batch(&mut s, &entries, &mut io, &world, &mut downloader);
    let seen_in_order =
        seen.borrow().clone() == vec!["https://a/1".to_string(), "https://b/2".to_string()];
    c.check(
        "batch skips bad entry, keeps order",
        seen_in_order,
        &got_list(&seen.borrow().clone()),
    );
    c.check(
        "batch marks bad entry in run",
        capture.text().contains("error in url"),
        &got_strs(&["error in url"]),
    );
    let positions = capture.text().contains("url 2 of 3") && capture.text().contains("url 3 of 3");
    {
        let body = capture.text();
        c.check("batch reports position", positions, &got_strs(&[&body]));
    }
    c.check(
        "batch ran -> no repaint",
        matches!(ran, Ok(true)),
        &bare_got(),
    );

    // escape stops the batch at the first rc 130
    let attempts: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let mut capture2 = CaptureOut::default();
    let mut reader2 = ScriptedReader::new(&[]);
    let mut io2 = Io {
        out: &mut capture2,
        reader: &mut reader2,
    };
    let mut s2 = loaded_settings(config_path);
    let entries2: Vec<String> = vec!["https://a/1".to_string(), "https://b/2".to_string()];
    let mut stopper = {
        let attempts = Rc::clone(&attempts);
        move |_s: &mut Obj, url: &str, _io: &mut Io<'_>| {
            attempts.borrow_mut().push(url.to_string());
            130
        }
    };
    let stopped = menu::run_batch(&mut s2, &entries2, &mut io2, &world, &mut stopper);
    c.check(
        "batch stops on escape",
        matches!(stopped, Ok(false))
            && attempts.borrow().clone() == vec!["https://a/1".to_string()],
        &got_list(&attempts.borrow().clone()),
    );

    // hint + act 14 + act 9
    let mut capture3 = CaptureOut::default();
    {
        let outdir = outdir_of(&s, root, auto);
        for line in ui::hint_lines(&outdir, false) {
            capture3.write(&line);
            capture3.write("\n");
        }
        capture3.flush();
    }
    let mut reader3 = ScriptedReader::new(&[""]);
    let mut io3 = Io {
        out: &mut capture3,
        reader: &mut reader3,
    };
    let mut defaults = config::defaults();
    let result14 = menu::act(
        "14",
        &mut defaults,
        &mut io3,
        &world,
        &mut no_download,
        &mut no_update,
    );
    c.check(
        "hint points at item 9",
        capture3.text().contains("menu item: 9"),
        &got_strs(&["hint"]),
    );
    c.check(
        "act rejects 14 with new range",
        matches!(result14, Ok(ActOutcome::Repaint(true))) && capture3.text().contains("1-13"),
        &got_strs(&["1-13"]),
    );
    let mut capture4 = CaptureOut::default();
    let mut reader4 = ScriptedReader::new(&[""]);
    let mut io4 = Io {
        out: &mut capture4,
        reader: &mut reader4,
    };
    let mut defaults9 = config::defaults();
    let result9 = menu::act(
        "9",
        &mut defaults9,
        &mut io4,
        &world,
        &mut no_download,
        &mut no_update,
    );
    let text9 = capture4.text();
    let multi_screen = text9.contains("multiple urls");
    // python drove a fake `multi_url`; the real spread with zero entries
    // answers multi_url's own repaint(value True), so the screen content
    // and the non-repaint falseness carry the same pin natively
    let reached = matches!(result9, Ok(ActOutcome::Repaint(true)));
    if !(reached && multi_screen) {
        eprintln!("act9-debug reached={reached} multi={multi_screen} text={text9:?}");
    }
    c.check(
        "act 9 is multi url",
        reached && multi_screen,
        &got_strs(&["act 9"]),
    );
}

pub(super) fn section_audit(
    c: &mut Checker<'_>,
    version: &str,
    root: &str,
    config_path: &str,
    auto: Option<&str>,
) {
    // F1/F2: junk hand-edited settings coerce at the boundary, never raise,
    // never reach argv, and never rewrite the file
    let mut junk = config::defaults();
    config::set(&mut junk, "playlist", Value::Bool(true));
    config::set(
        &mut junk,
        "playlist_max_items",
        Value::Str("abc".to_string()),
    );
    config::set(
        &mut junk,
        "video_resolution",
        Value::Str("not-a-number".to_string()),
    );
    config::set(&mut junk, "audio_quality", Value::Str("banana".to_string()));
    config::set(&mut junk, "video_container", Value::Str("avi".to_string()));
    config::set(&mut junk, "type", Value::Str("tunnel".to_string()));
    let before = file_bytes(config_path);
    config::coerce_settings(&mut junk);
    let junk_outdir = outdir_of(&junk, root, auto);
    let menu_rows = match ui::menu_items(&junk, &junk_outdir, cfg!(windows)) {
        Ok(rows) => rows,
        Err(kind) => {
            c.check("menu_items shape error", false, &format!("{kind:?}"));
            Vec::new()
        }
    };
    let argv = argv_of(c, &junk, "https://x/y", ff_dir(), root, auto);
    let jargv = argv.join(" ");
    c.check(
        "coerce fixes junk max_items",
        config::get(&junk, "playlist_max_items") == Some(&Value::Int(0)),
        &py_value(config::get(&junk, "playlist_max_items")),
    );
    c.check(
        "coerce keeps junk out of argv",
        !jargv.contains("height<=not-a-number") && !jargv.contains("banana"),
        &got_list(&argv),
    );
    let vocab = config::get(&junk, "video_resolution") == Some(&Value::Str("1080".to_string()))
        && config::get(&junk, "video_container") == Some(&Value::Str("mp4".to_string()))
        && config::get(&junk, "audio_format") == Some(&Value::Str("mp3".to_string()))
        && config::get(&junk, "type") == Some(&Value::Str("video".to_string()));
    c.check(
        "coerce fixes junk vocabularies",
        vocab,
        &py_value(config::get(&junk, "type")),
    );
    c.check(
        "coerce renders the menu without raising",
        menu_rows.len() == 13,
        &format!("{}", menu_rows.len()),
    );
    let after = file_bytes(config_path);
    c.check(
        "coerce never rewrites settings.json",
        before == after,
        &got_strs(&["settings.json"]),
    );

    let mut q9 = config::defaults();
    config::set(&mut q9, "audio_quality", Value::Str("9".to_string()));
    config::coerce_settings(&mut q9);
    c.check(
        "coerce keeps number quality",
        config::get(&q9, "audio_quality") == Some(&Value::Str("9".to_string())),
        &py_value(config::get(&q9, "audio_quality")),
    );
    let mut q320 = config::defaults();
    config::set(&mut q320, "audio_quality", Value::Str("320K".to_string()));
    config::coerce_settings(&mut q320);
    c.check(
        "coerce keeps bitrate quality",
        config::get(&q320, "audio_quality") == Some(&Value::Str("320K".to_string())),
        &py_value(config::get(&q320, "audio_quality")),
    );
    let mut n5 = config::defaults();
    config::set(&mut n5, "playlist_max_items", Value::Str("5".to_string()));
    config::coerce_settings(&mut n5);
    c.check(
        "coerce parses numeric string max_items",
        config::get(&n5, "playlist_max_items") == Some(&Value::Int(5)),
        &py_value(config::get(&n5, "playlist_max_items")),
    );

    // F3: the non-forced daily check asks for the short timeout, forced keeps 60
    let calls: Rc<RefCell<Vec<(String, u64)>>> = Rc::new(RefCell::new(Vec::new()));
    let mut s_auto = config::defaults();
    {
        let mut capture = CaptureOut::default();
        let mut http = RecordingHttp {
            calls: Rc::clone(&calls),
        };
        let mut probe = |_b: &str| (String::new(), -1i64);
        let mut gate = |_b: &str| 0i64;
        let mut call = |_b: &str, _a: &[String]| 0i64;
        let mut now = now_secs;
        let mut which = |_cmd: &str| None::<String>;
        let shared = tty::SharedOut::new(capture.clone());
        let sink = shared.sink();
        let mut ctx = update::UpdateCtx {
            platform: platform_name(),
            arch: arch_name(),
            self_path: "",
            root,
            is_windows: cfg!(windows),
            use_color: false,
            stdin_tty: true,
            sink,
            http: &mut http,
            probe: &mut probe,
            selftest: &mut gate,
            call: &mut call,
            now: &mut now,
            which: &mut which,
            out: &mut capture,
            reader: &mut NoReader,
        };
        let quiet = update::auto_update_check(&mut s_auto, false, &mut ctx);
        let forced = update::auto_update_check(&mut s_auto, true, &mut ctx);
        let _ = (quiet, forced);
    }
    let timeouts: Vec<u64> = calls.borrow().iter().map(|(_, t)| *t).collect();
    c.check(
        "auto check uses the pinned 6s timeout",
        timeouts.first() == Some(&net::CHECK_TIMEOUT),
        &got_timeouts(&timeouts),
    );
    c.check(
        "forced check keeps the 60s timeout",
        timeouts.get(1) == Some(&net::FORCED_TIMEOUT),
        &got_timeouts(&timeouts),
    );

    // F4: the second probe of the same binary must not spawn it again
    {
        let scratch = mkdtemp("ytdlp-memo-");
        let shim = memo_shim(&scratch);
        let mut cache = resolve::VersionCache::default();
        let mtime = resolve::mtime_seconds(&shim);
        let spawns: Rc<RefCell<i64>> = Rc::new(RefCell::new(0));
        let mut spawn_counter = {
            let spawns = Rc::clone(&spawns);
            move |b: &str, f: &str| {
                *spawns.borrow_mut() += 1;
                resolve::spawn_version_probe(b, f)
            }
        };
        resolve::probe_version(&shim, mtime, &mut cache, &mut spawn_counter);
        let first = *spawns.borrow();
        resolve::probe_version(&shim, mtime, &mut cache, &mut spawn_counter);
        let second = *spawns.borrow();
        c.check(
            "probe memo skips the second spawn",
            (first == 1 || first == 2) && second == first,
            &format!("({first}, {second})"),
        );
        pyfs::rmtree_quiet(&scratch);
    }

    // F6: the OS lookup runs once; repaints and downloads reuse the memo
    {
        let mut cache = resolve::OutdirCache::default();
        let lookups: Rc<RefCell<i64>> = Rc::new(RefCell::new(0));
        let env = resolve::env_map_from_process();
        let home = resolve::home_from_env(&env).unwrap_or_default();
        let (win_desktop, win_downloads) = shell_folders(&env);
        let real_answer = resolve::auto_outdir_lookup(
            cfg!(windows),
            win_desktop.as_deref(),
            win_downloads.as_deref(),
            &home,
            &env,
        );
        let a1: Option<String> = cache
            .get(|| {
                *lookups.borrow_mut() += 1;
                real_answer.clone()
            })
            .unwrap_or_default();
        let a2: Option<String> = cache
            .get(|| {
                *lookups.borrow_mut() += 1;
                real_answer.clone()
            })
            .unwrap_or_default();
        c.check(
            "auto_outdir memo skips the second lookup",
            *lookups.borrow() == 1 && a1 == a2,
            &py_opt_str(a1.clone().as_deref()),
        );
    }

    // F8: item 12 leaves the save to the one save point in main_menu;
    // python does not capture this act call, its message lands in the
    // report between the checks, so the port prints it the same way
    {
        let before12 = file_bytes(config_path);
        let mut s12 = config::defaults();
        // python's patched `read_esc` for this act call answers "" and
        // echoes nothing; the hold is a silent end-of-input here
        let mut noreader = NoReader;
        {
            let out = c.report_out();
            let mut io = Io {
                out,
                reader: &mut noreader,
            };
            let world = menu_world(version, root, false, false, None, auto);
            let _outcome = menu::act(
                "12",
                &mut s12,
                &mut io,
                &world,
                &mut no_download,
                &mut no_update,
            );
        }
        c.check(
            "item 12 leaves the save to main_menu",
            file_bytes(config_path) == before12,
            &got_strs(&["item 12"]),
        );
    }

    // F10: wide code points count columns; the box row keeps its width
    let wide = '\u{4E00}';
    let wide_str = wide.to_string();
    c.check(
        "dwidth counts wide chars as two columns",
        term::dwidth("ab") == 2
            && term::dwidth(&wide_str) == 2
            && term::dwidth(&format!("a{wide_str}")) == 3,
        &got_strs(&["dwidth"]),
    );
    let cut_text = format!("ab{wide_str}{wide_str}{wide_str}");
    let cut = term::cut_to_width(&cut_text, 6) == format!("ab{wide_str}{wide_str}");
    c.check(
        "cut_to_width cuts by columns",
        cut && term::cut_to_width("ab", 1) == "a",
        &got_strs(&["cut"]),
    );
    c.check(
        "tail_to_width keeps the tail by columns",
        term::tail_to_width(&cut_text, 6) == format!("{wide_str}{wide_str}{wide_str}"),
        &got_strs(&["tail"]),
    );
    {
        let base_row = strip_ansi(&ui::menu_line(1, "x", "plain", true, false));
        let wide_value = wide_str.repeat(100);
        let wide_row = strip_ansi(&ui::menu_line(1, "x", &wide_value, true, false));
        c.check(
            "menu line keeps box width with a wide value",
            term::dwidth(&base_row) == term::dwidth(&wide_row)
                && term::dwidth(&base_row) == WIDTH + 2,
            &got_strs(&["menu line"]),
        );
    }
}

/// `(url, timeout)`-pair count for the F3 timeout records.
pub(super) fn got_timeouts(timeouts: &[u64]) -> String {
    let parts: Vec<String> = timeouts.iter().map(|t| t.to_string()).collect();
    format!("[{}]", parts.join(", "))
}

pub(super) fn section_archive(c: &mut Checker<'_>) {
    let scratch = mkdtemp("ytdlp-extract-");
    let zip_dir = resolve::py_path_join(&scratch, "z");
    let tar_dir = resolve::py_path_join(&scratch, "t");
    let err_dir = resolve::py_path_join(&scratch, "e");
    for dir in [&zip_dir, &tar_dir, &err_dir] {
        let _ = std::fs::create_dir(dir);
    }
    let blob_path = resolve::py_path_join(&scratch, "a.zip");
    let zip_blob = archive::zip_with(&[
        (
            "ffmpeg-9.0.2-essentials_build/bin/ffmpeg.exe",
            b"BINARY" as &[u8],
        ),
        ("ffmpeg-9.0.2-essentials_build/bin/ffplay.exe", b"OTHER"),
    ]);
    write_bytes(&blob_path, &zip_blob);
    let shared = tty::SharedOut::new(CaptureOut::default());
    let sink = shared.sink();
    let lifted = archive::extract_member(&blob_path, "zip", "ffmpeg.exe", &zip_dir, &sink);
    let lifted_ok =
        lifted.is_some() && file_bytes(&resolve::py_path_join(&zip_dir, "ffmpeg.exe")) == b"BINARY";
    c.check(
        "extract_member lifts the nested binary from a zip",
        lifted_ok,
        &py_opt_str(lifted.as_deref()),
    );
    c.check(
        "extract_member takes only the named binary",
        !std::path::Path::new(&resolve::py_path_join(&zip_dir, "ffplay.exe")).exists(),
        &got_list(&dir_names(&zip_dir)),
    );

    let tar_path = resolve::py_path_join(&scratch, "a.tar.xz");
    let tar_blob = archive::tar_xz_with(&[(
        "ffmpeg-n9.0-linux64-gpl-9.0/bin/ffmpeg",
        b"TARDATA" as &[u8],
    )]);
    write_bytes(&tar_path, &tar_blob);
    let lifted_tar = archive::extract_member(&tar_path, "tar.xz", "ffmpeg", &tar_dir, &sink);
    let tar_ok = lifted_tar.is_some()
        && file_bytes(&resolve::py_path_join(&tar_dir, "ffmpeg")) == b"TARDATA";
    c.check(
        "extract_member lifts the nested binary from a tar.xz",
        tar_ok,
        &py_opt_str(lifted_tar.as_deref()),
    );

    let bad_path = resolve::py_path_join(&scratch, "bad.zip");
    write_bytes(&bad_path, b"this is not a zip");
    {
        // python prints the unreadable message straight into the report;
        // the port replays the sink capture into the report, which is
        // byte-equal because the wording is pinned in phase 8
        let replay = CaptureOut::default();
        let bad_sink = tty::SharedOut::new(replay.clone()).sink();
        let none = archive::extract_member(&bad_path, "zip", "ffmpeg.exe", &err_dir, &bad_sink);
        let out = c.report_out();
        out.write(&replay.text());
        out.flush();
        c.check(
            "extract_member reports an unreadable archive",
            none.is_none(),
            &bare_got(),
        );
    }

    let miss_path = resolve::py_path_join(&scratch, "m.zip");
    let miss_blob = archive::zip_with(&[("ffmpeg-9.0.2/bin/ffplay.exe", b"x")]);
    write_bytes(&miss_path, &miss_blob);
    c.check(
        "extract_member returns None when the binary is absent",
        archive::extract_member(&miss_path, "zip", "ffmpeg.exe", &err_dir, &sink).is_none(),
        &bare_got(),
    );
    c.check(
        "extract_member refuses an unknown format",
        archive::extract_member(&blob_path, "rar", "ffmpeg.exe", &err_dir, &sink).is_none(),
        &bare_got(),
    );

    // the ffmpeg-9 probe: --version rejected loudly, -version answers
    let shim = probe_shim(&scratch);
    let mut cache = resolve::VersionCache::default();
    let mtime = resolve::mtime_seconds(&shim);
    let (version, rc) = resolve::probe_version(&shim, mtime, &mut cache, &mut |b, f| {
        resolve::spawn_version_probe(b, f)
    });
    c.check(
        "probe_version survives a binary that rejects --version",
        rc == 0 && version.contains("9.0.2"),
        &got_strs(&[&version]),
    );

    pyfs::rmtree_quiet(&scratch);
}
