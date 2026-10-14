//! User interface, pure half (specsheet section 4, layer 7; FR-2, FR-12,
//! audit F10): menu rows, path shortening, the subtitle language block
//! and batch rows. The stateful UI (menu_items, render, act) lands with
//! its phase; the row builders here are pure and column-width correct so
//! the differential harness can drive them with and without color.

use crate::args::{KNOWN_LANG_CODES, clean_url};
use crate::term::{A_AMBER, A_DARKGREY, A_GREY, c, cut_to_width, dwidth, tail_to_width};

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

    /// Full strings recorded from the oracle with USE_COLOR forced
    /// (scratch probe, 2026-10-14).
    #[test]
    fn menu_line_matches_the_oracle_strings() {
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

    /// Strings recorded from the oracle (scratch probe, 2026-10-14).
    #[test]
    fn batch_row_matches_the_oracle_strings() {
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

    /// Rows recorded from the oracle's pick_list with a stubbed
    /// read_esc, colored and plain (scratch probe, 2026-10-14).
    #[test]
    fn pick_rows_match_the_oracle_strings() {
        assert_eq!(
            pick_row("mkv", "mkv", 2, true),
            "    \x1b[38;5;214m[x]\x1b[0m  2  mkv"
        );
        assert_eq!(pick_row("mkv", "mkv", 2, false), "    [x]  2  mkv");
        assert_eq!(pick_row("mp4", "mkv", 1, false), "    [ ]  1  mp4");
        assert_eq!(pick_row("best", "mkv", 10, false), "    [ ] 10  best");
    }
}
