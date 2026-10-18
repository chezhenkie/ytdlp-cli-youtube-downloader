//! User interface, pure half (specsheet section 4, layer 7; FR-2, FR-12,
//! audit F10): menu rows, path shortening, the subtitle language block
//! and batch rows. The stateful UI (menu_items, render, act) lands with
//! its phase; the row builders here are pure and column-width correct so
//! the differential harness can drive them with and without color.

use crate::args::{KNOWN_LANG_CODES, clean_url, playlist_max_items};
use crate::config::{self, Obj, Value};
use crate::resolve::ShapeKind;
use crate::term::{
    A_AMBER, A_AMBER_DARK, A_DARKGREY, A_GREY, A_LBLUE, A_RESET, c, cut_to_width, dwidth,
    tail_to_width,
};

/// The menu box width (WIDTH in the python source; a pinned constant).
pub const WIDTH: i64 = 64;

/// Selection box: amber `[x]` when chosen, plain `[ ]` otherwise.
pub fn sel_mark(sel: bool, use_color: bool) -> String {
    if sel {
        c(use_color, A_AMBER, "[x]")
    } else {
        "[ ]".to_string()
    }
}

/// One menu row: side markers, label, value, padded to the box width.
/// Values are cut or tail-cut by display columns (audit F10), so a CJK or
/// emoji value cannot break the box.
pub fn menu_line(idx: i64, label: &str, value: &str, grey: bool, use_color: bool) -> String {
    let plain = format!(" {idx:>2}  {label}");
    let left = if grey {
        c(use_color, A_GREY, &plain)
    } else {
        plain.clone()
    };
    let mut maxv = WIDTH - dwidth(&plain) - 3;
    if maxv < 1 {
        maxv = 1;
    }
    let value = if dwidth(value) > maxv {
        if maxv > 3 {
            format!("...{}", tail_to_width(value, maxv - 3))
        } else {
            cut_to_width(value, maxv)
        }
    } else {
        value.to_string()
    };
    let mut pad = WIDTH - dwidth(&plain) - dwidth(&value);
    if pad < 1 {
        pad = 1;
    }
    let m = c(use_color, A_DARKGREY, "|");
    format!("{m}{left}{}{value}{m}", " ".repeat(pad as usize))
}

/// The v2.0.2 quick-paste row (FR-17): it takes the place of the blank
/// top box row and is a little darker than the box amber. Fixed text,
/// no value column; the padding math is menu_line's.
pub const QUICK_ROW_TEXT: &str = "  0  press zero '0' to paste link and start direct download";

pub fn quick_row(use_color: bool) -> String {
    let m = c(use_color, A_DARKGREY, "|");
    let mut pad = WIDTH - dwidth(QUICK_ROW_TEXT);
    if pad < 1 {
        pad = 1;
    }
    format!(
        "{m}{}{}{m}",
        c(use_color, A_AMBER_DARK, QUICK_ROW_TEXT),
        " ".repeat(pad as usize)
    )
}

/// Path for menu item 5: always `...<sep><folder name>`.
pub fn short_path(p: &str, maxv: i64, is_windows: bool) -> String {
    let sep = if is_windows { "\\" } else { "/" };
    let normalized = p.replace('/', sep);
    let parts: Vec<&str> = normalized.split(sep).filter(|x| !x.is_empty()).collect();
    let name = parts.last().copied().unwrap_or("downloads");
    let out = format!("...{sep}{name}");
    if dwidth(&out) <= maxv {
        out
    } else {
        cut_to_width(&out, maxv)
    }
}

/// The subtitle code reference for menu item 7: every known code, space
/// separated, alphabetical, filled to the box width. Pure, so the
/// selftest can pin the layout without a tty.
pub fn sub_lang_lines(width: i64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut row = String::new();
    for code in KNOWN_LANG_CODES {
        if row.is_empty() {
            row = (*code).to_string();
        } else if (row.chars().count() + 1 + code.chars().count()) as i64 + 2 <= width {
            row.push(' ');
            row.push_str(code);
        } else {
            lines.push(format!("  {row}"));
            row = (*code).to_string();
        }
    }
    if !row.is_empty() {
        lines.push(format!("  {row}"));
    }
    lines
}

/// One numbered row of the multi-url list (FR-12): the url, or an error
/// row when the entry holds no http(s) link. A wrong paste is kept in the
/// list and marked, never dropped.
pub fn batch_row(n: i64, text: &str, use_color: bool) -> String {
    let url = clean_url(Some(text));
    let head = c(use_color, A_GREY, &format!("  {n}  "));
    match url {
        Some(url) if !url.is_empty() => head + &url,
        _ => head + &c(use_color, A_AMBER, "error in url"),
    }
}

/// One row of a `pick_list` choice block: `    [x]  2  option`. The
/// python builds this inline in `pick_list`; the port separates the row
/// shape so twins can pin it and phase 6 reuses it.
pub fn pick_row(option: &str, current: &str, index: i64, use_color: bool) -> String {
    format!(
        "    {} {index:>2}  {option}",
        sel_mark(option == current, use_color)
    )
}

/// python `onoff(flag)`: "on" for a truthy raw value, "off" otherwise.
pub fn onoff(flag: Option<&Value>) -> String {
    if flag.is_some_and(config::py_truthy) {
        "on".to_string()
    } else {
        "off".to_string()
    }
}

/// `menu_summary(s)`: the one-line download summary (phase 7 shows it).
pub fn menu_summary(s: &Obj) -> Result<String, ShapeKind> {
    let kind = config::get(s, "type");
    let audio_only = matches!(kind, Some(Value::Str(t)) if t == "audio only");
    if audio_only {
        let fmt = required(s, "audio_format")?;
        return Ok(format!("audio-only {}", config::py_str(fmt)));
    }
    let container = required(s, "video_container")?;
    let res = required(s, "video_resolution")?;
    Ok(format!(
        "{} {}p",
        config::py_str(container),
        config::py_str(res)
    ))
}

/// A key the loaded settings always carries; missing is python's
/// KeyError and stays out of contract (recorded, phase-5 style).
fn required<'a>(s: &'a Obj, key: &str) -> Result<&'a Value, ShapeKind> {
    config::get(s, key).ok_or(ShapeKind::TypeError)
}

/// One menu row: (number, label, value, grey).
pub struct MenuRow {
    pub idx: i64,
    pub label: String,
    pub value: String,
    pub grey: bool,
}

/// `menu_items(s)`: the menu rows, numbered 1 to 13. `outdir` is the
/// resolved output folder (python calls resolve_outdir here; the port
/// takes the phase-4 answer as a parameter). Values render through
/// py_str so a hand-edited raw value cannot diverge.
pub fn menu_items(s: &Obj, outdir: &str, is_windows: bool) -> Result<Vec<MenuRow>, ShapeKind> {
    let kind = config::get(s, "type");
    let res = required(s, "video_resolution")?;
    let audio_only = matches!(kind, Some(Value::Str(t)) if t == "audio only");
    let video_no_audio = matches!(kind, Some(Value::Str(t)) if t == "video no audio");
    let res_text = config::py_str(res);
    let (shown, container, media) = if audio_only {
        (
            format!(
                "audio only ({})",
                config::py_str(required(s, "audio_format")?)
            ),
            "-".to_string(),
            "-".to_string(),
        )
    } else if video_no_audio {
        (
            "video no audio".to_string(),
            "-".to_string(),
            if res_text == "best" {
                "best".to_string()
            } else {
                format!("{res_text}p")
            },
        )
    } else {
        (
            "video".to_string(),
            config::py_str(required(s, "video_container")?),
            if res_text == "best" {
                "best".to_string()
            } else {
                format!("{res_text}p")
            },
        )
    };
    let playlist = config::get(s, "playlist").is_some_and(config::py_truthy);
    let pl = if playlist {
        let n = playlist_max_items(s)?;
        if n <= 0 {
            "on (all)".to_string()
        } else {
            format!("on (max {n})")
        }
    } else {
        "off".to_string()
    };
    let maxv = WIDTH - "  5  output folder".chars().count() as i64 - 3;
    let rows = vec![
        MenuRow {
            idx: 1,
            label: "video/audio".to_string(),
            value: shown,
            grey: true,
        },
        MenuRow {
            idx: 2,
            label: "format container".to_string(),
            value: container,
            grey: true,
        },
        MenuRow {
            idx: 3,
            label: "video resolution".to_string(),
            value: media,
            grey: true,
        },
        MenuRow {
            idx: 4,
            label: "audio format".to_string(),
            value: config::py_str(required(s, "audio_format")?),
            grey: true,
        },
        MenuRow {
            idx: 5,
            label: "output folder".to_string(),
            value: short_path(outdir, maxv, is_windows),
            grey: true,
        },
        MenuRow {
            idx: 6,
            label: "filename template".to_string(),
            value: config::py_str(required(s, "filename_template")?),
            grey: true,
        },
        MenuRow {
            idx: 7,
            label: "subtitles".to_string(),
            value: format!(
                "{} ({})",
                onoff(config::get(s, "subtitles")),
                config::py_str(required(s, "subtitle_langs")?)
            ),
            grey: true,
        },
        MenuRow {
            idx: 8,
            label: "srt timecode stripper".to_string(),
            value: format!(
                "{} ({})",
                onoff(config::get(s, "srt_stripper")),
                config::py_str(required(s, "srt_stripper_mode")?)
            ),
            grey: true,
        },
        MenuRow {
            idx: 9,
            label: "playlist on/off".to_string(),
            value: pl,
            grey: true,
        },
        MenuRow {
            idx: 10,
            label: "add multiple youtube urls".to_string(),
            value: String::new(),
            grey: true,
        },
        MenuRow {
            idx: 11,
            label: "reset defaults".to_string(),
            value: "config/settings.json".to_string(),
            grey: true,
        },
        MenuRow {
            idx: 12,
            label: "update yt-dlp + cli".to_string(),
            value: String::new(),
            grey: true,
        },
        MenuRow {
            idx: 13,
            label: "auto update".to_string(),
            value: onoff(config::get(s, "auto_update")),
            grey: true,
        },
        MenuRow {
            idx: 14,
            label: "quit".to_string(),
            value: String::new(),
            grey: false,
        },
    ];
    Ok(rows)
}

/// `header()`: the two dotted lines and the centered title. `version` is
/// the product version the binary carries (python reads module VERSION;
/// the port takes it as a parameter so the arm can pin the layout).
pub fn header_lines(version: &str, use_color: bool) -> Vec<String> {
    let title = format!("yt-dlp cli  v{version}");
    let text = format!(" {title} ");
    let pad = WIDTH - text.chars().count() as i64;
    let left = pad / 2;
    let right = pad - left;
    let div = "-".repeat((WIDTH + 2) as usize);
    let line = if use_color {
        format!(
            "{A_LBLUE}|{A_RESET}{}{A_AMBER}{text}{A_RESET}{}{A_LBLUE}|{A_RESET}",
            " ".repeat(left as usize),
            " ".repeat(right as usize)
        )
    } else {
        format!(
            "|{}{text}{}|",
            " ".repeat(left as usize),
            " ".repeat(right as usize)
        )
    };
    vec![
        c(use_color, A_LBLUE, &div),
        line,
        c(use_color, A_LBLUE, &div),
    ]
}

/// `hint(s)`: amber instructions plus grey detail lines, above the
/// input field. `outdir` is the resolved output folder (python's hint
/// resolves it here; the port takes the phase-4 answer as a parameter).
pub fn hint_lines(outdir: &str, use_color: bool) -> Vec<String> {
    let div = "-".repeat((WIDTH + 2) as usize);
    let l1 = "paste url, press enter and download will start";
    let l2 = "enter number to change default settings";
    let l3 = "change download location by choosing menu item: 5";
    let l3b = "download several urls at once by choosing menu item: 10";
    let l4 = format!("DL location: {outdir}");
    vec![
        c(use_color, A_AMBER, l1),
        c(use_color, A_AMBER, l2),
        c(use_color, A_DARKGREY, l3),
        c(use_color, A_DARKGREY, l3b),
        c(use_color, A_DARKGREY, &l4),
        c(use_color, A_LBLUE, &div),
    ]
}

/// `render(s)`: header, the FR-17 quick-paste row, the 13 rows, the
/// escape hint, box bottom. Returns the lines; the caller prints them.
pub fn render_lines(
    version: &str,
    s: &Obj,
    outdir: &str,
    is_windows: bool,
    use_color: bool,
) -> Result<Vec<String>, ShapeKind> {
    let mut lines = header_lines(version, use_color);
    let m = c(use_color, A_DARKGREY, "|");
    // FR-17: the quick-paste row took the blank top row's place
    lines.push(quick_row(use_color));
    for row in menu_items(s, outdir, is_windows)? {
        lines.push(menu_line(
            row.idx, &row.label, &row.value, row.grey, use_color,
        ));
    }
    let desc = "press escape to exit submenu";
    let pad = WIDTH - desc.chars().count() as i64;
    let left = pad / 2;
    let right = pad - left;
    lines.push(format!(
        "{m}{}{}{}{m}",
        " ".repeat(left as usize),
        c(use_color, A_LBLUE, desc),
        " ".repeat(right as usize)
    ));
    lines.push(c(use_color, A_DARKGREY, &"-".repeat((WIDTH + 2) as usize)));
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    /// Full strings recorded from the reference with USE_COLOR forced
    /// (scratch probe, 2026-10-14).
    #[test]
    fn menu_line_matches_the_reference_strings() {
        assert_eq!(
            menu_line(1, "x", "plain", true, true),
            format!(
                "\x1b[38;5;244m|\x1b[0m\x1b[38;5;250m  1  x\x1b[0m{}plain\x1b[38;5;244m|\x1b[0m",
                " ".repeat(53)
            )
        );
        assert_eq!(
            menu_line(1, "x", "plain", true, false),
            format!("|  1  x{}plain|", " ".repeat(53))
        );
        // plain = "  5  output folder" is 18 columns, the value 13; the
        // pad is 64 - 18 - 13 = 33.
        assert_eq!(
            menu_line(5, "output folder", "...\\Downloads", true, true),
            format!(
                "\x1b[38;5;244m|\x1b[0m\x1b[38;5;250m  5  output folder\x1b[0m{}...\\Downloads\x1b[38;5;244m|\x1b[0m",
                " ".repeat(33)
            )
        );
    }

    /// F10: a wide value stays inside the box; the stripped row is
    /// exactly WIDTH + 2 columns in every case.
    #[test]
    fn menu_line_keeps_the_box_width_with_wide_values() {
        let wide = "\u{4e00}";
        for row in [
            menu_line(1, "x", "plain", true, true),
            menu_line(1, "x", &wide.repeat(100), true, true),
            menu_line(1, "x", "plain", false, false),
            menu_line(13, "quit", "", true, false),
        ] {
            assert_eq!(dwidth(&strip_ansi(&row)), WIDTH + 2, "row: {row:?}");
        }
        let row = menu_line(1, "x", &wide.repeat(100), true, false);
        assert!(row.contains("..."), "tail-cut keeps the ellipsis: {row}");
    }

    #[test]
    fn short_path_shows_only_the_folder_name() {
        assert_eq!(short_path("C:\\Users\\x\\Videos", 40, true), "...\\Videos");
        assert_eq!(short_path("/home/u/Videos", 40, false), ".../Videos");
        assert_eq!(short_path("C:/mixed\\path/Videos", 40, true), "...\\Videos");
        assert_eq!(short_path("", 40, true), "...\\downloads");
        let long = short_path(&format!("C:\\x\\{}", "\u{4e00}".repeat(40)), 10, true);
        assert_eq!(dwidth(&long), 10);
    }

    #[test]
    fn sub_lang_lines_match_selftest_pins() {
        let lines = sub_lang_lines(WIDTH);
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|l| l.chars().count() as i64 <= WIDTH));
        assert!(
            lines
                .iter()
                .all(|l| l.starts_with("  ") && !l.trim().contains("  ") && l.trim_end() == l)
        );
        let flat: Vec<&str> = lines.iter().flat_map(|l| l.split_whitespace()).collect();
        assert_eq!(flat, KNOWN_LANG_CODES, "whole reference, alphabetical");
        assert_eq!(flat.len(), 191);
        assert_eq!(lines[0].chars().count() as i64, WIDTH);
        assert_eq!(lines.len(), 10);
        assert!(sub_lang_lines(24).iter().all(|l| l.chars().count() <= 24));
    }

    /// Strings recorded from the reference (scratch probe, 2026-10-14).
    #[test]
    fn batch_row_matches_the_reference_strings() {
        assert_eq!(
            batch_row(2, "nope", true),
            "\x1b[38;5;250m  2  \x1b[0m\x1b[38;5;214merror in url\x1b[0m"
        );
        assert_eq!(batch_row(2, "nope", false), "  2  error in url");
        assert_eq!(
            batch_row(1, "https://x/y", true),
            "\x1b[38;5;250m  1  \x1b[0mhttps://x/y"
        );
        assert!(batch_row(1, "https://x/y", true).ends_with("https://x/y"));
        // "https://,,," rstrips to "https://" - the scheme's slash stops
        // the trim - so it is a url row, not an error row
        assert_eq!(batch_row(3, "https://,,,", false), "  3  https://");
    }

    /// Rows recorded from the reference's pick_list with a stubbed
    /// read_esc, colored and plain (scratch probe, 2026-10-14).
    #[test]
    fn pick_rows_match_the_reference_strings() {
        assert_eq!(
            pick_row("mkv", "mkv", 2, true),
            "    \x1b[38;5;214m[x]\x1b[0m  2  mkv"
        );
        assert_eq!(pick_row("mkv", "mkv", 2, false), "    [x]  2  mkv");
        assert_eq!(pick_row("mp4", "mkv", 1, false), "    [ ]  1  mp4");
        assert_eq!(pick_row("best", "mkv", 10, false), "    [ ] 10  best");
    }

    /// FR-17 pins (v2.0.2): the quick-paste row's exact bytes in both
    /// color modes and its box width (WIDTH + 2 columns stripped).
    #[test]
    fn quick_row_matches_the_pins() {
        assert_eq!(
            quick_row(false),
            format!("|{QUICK_ROW_TEXT}{}|", " ".repeat(5))
        );
        assert_eq!(
            quick_row(true),
            format!(
                "\x1b[38;5;244m|\x1b[0m\x1b[38;5;208m{QUICK_ROW_TEXT}\x1b[0m{}\x1b[38;5;244m|\x1b[0m",
                " ".repeat(5)
            )
        );
        for row in [quick_row(false), quick_row(true)] {
            assert_eq!(dwidth(&strip_ansi(&row)), WIDTH + 2, "row: {row:?}");
        }
    }

    /// FR-17: the quick row sits where the blank top row stood - directly
    /// after the 3 header lines, before row 1 - and no blank row remains.
    #[test]
    fn render_paints_the_quick_row_in_the_blank_rows_place() {
        let s = config::defaults();
        for use_color in [false, true] {
            let lines = render_lines("2.0.2", &s, "/x/Downloads", false, use_color).unwrap();
            assert_eq!(lines[3], quick_row(use_color));
            assert!(lines[4].contains("  1  video/audio"), "{:?}", lines[4]);
            assert_eq!(lines.len(), 20, "header 3 + quick + 14 rows + hint + rule");
            let blank = format!("|{}|", " ".repeat(WIDTH as usize));
            assert!(
                !lines.iter().any(|l| strip_ansi(l) == blank),
                "the blank top row is gone"
            );
        }
    }
}
