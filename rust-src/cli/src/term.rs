//! Terminal plumbing, pure half (specsheet section 4, layer 1): palette,
//! color wrapper, width-stable progress words, display width over the
//! pinned east-asian table, cell rendering and the eight waiting
//! animations (FR-10).
//!
//! Nothing here reads or writes the terminal; the tty-bound half (Stage,
//! read_esc, KeyWatch) lands with its phase. Where the python original
//! reads the module-global `USE_COLOR`, the port takes it as a parameter
//! so the pure layer stays pure.

use crate::rng::Pcg32;
use crate::width_table::WIDE_RANGES;
use std::cell::RefCell;
use std::sync::OnceLock;

pub const A_AMBER: &str = "\x1b[38;5;214m";
/// FR-17: the quick-paste row's amber, a little darker than the key color
pub const A_AMBER_DARK: &str = "\x1b[38;5;208m";
pub const A_LBLUE: &str = "\x1b[38;5;117m";
pub const A_GREY: &str = "\x1b[38;5;250m";
pub const A_DARKGREY: &str = "\x1b[38;5;244m";
pub const A_MINT: &str = "\x1b[38;5;121m";
pub const A_PURPLE: &str = "\x1b[38;5;176m";
pub const A_RESET: &str = "\x1b[0m";
pub const CLEAR: &str = "\x1b[2J\x1b[H";

/// Wrap `text` in a color code; plain when color is off (python `c()`).
pub fn c(use_color: bool, code: &str, text: &str) -> String {
    if use_color {
        format!("{code}{text}{A_RESET}")
    } else {
        text.to_string()
    }
}

/// Display width in terminal columns (audit F10): east-asian wide and
/// fullwidth code points take 2 columns, everything else 1.
pub fn dwidth(text: &str) -> i64 {
    text.chars()
        .map(|ch| if is_wide(ch as u32) { 2 } else { 1 })
        .sum()
}

fn is_wide(cp: u32) -> bool {
    WIDE_RANGES
        .binary_search_by(|&(start, end)| {
            if cp < start {
                std::cmp::Ordering::Greater
            } else if cp >= end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Longest prefix of `text` whose display width is <= maxw.
pub fn cut_to_width(text: &str, maxw: i64) -> String {
    let mut out = String::new();
    let mut w: i64 = 0;
    for ch in text.chars() {
        let cw: i64 = if is_wide(ch as u32) { 2 } else { 1 };
        if w + cw > maxw {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

/// Longest suffix of `text` whose display width is <= maxw.
pub fn tail_to_width(text: &str, maxw: i64) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut w: i64 = 0;
    for ch in text.chars().rev() {
        let cw: i64 = if is_wide(ch as u32) { 2 } else { 1 };
        if w + cw > maxw {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.reverse();
    out.into_iter().collect()
}

fn at(b: &[u8], i: usize, want: u8) -> bool {
    b.get(i) == Some(&want)
}

/// Python `re` `\d` is the unicode Nd category; every number this file
/// sees comes from yt-dlp's ASCII progress output, and the harness
/// protocol is UTF-8/ASCII, so the scanners match ASCII digits only.
/// That is a recorded port note, not an accident.
///
/// `^\d+(\.\d+)?[KMGT]?i?B$`
fn is_size(word: &str) -> bool {
    let b = word.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return false;
    }
    if at(b, i, b'.') {
        i += 1;
        let digits = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'K' | b'M' | b'G' | b'T')) {
        i += 1;
    }
    if at(b, i, b'i') {
        i += 1;
    }
    at(b, i, b'B') && i + 1 == b.len()
}

/// `^\d+(\.\d+)?[KMGT]?i?B/s$`
fn is_speed(word: &str) -> bool {
    let b = word.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return false;
    }
    if at(b, i, b'.') {
        i += 1;
        let digits = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'K' | b'M' | b'G' | b'T')) {
        i += 1;
    }
    if at(b, i, b'i') {
        i += 1;
    }
    i + 3 == b.len() && at(b, i, b'B') && at(b, i + 1, b'/') && at(b, i + 2, b's')
}

/// `^\d{1,2}:\d{2}(:\d{2})?$`
fn is_time(word: &str) -> bool {
    let b = word.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if !(1..=2).contains(&i) || !at(b, i, b':') {
        return false;
    }
    i += 1;
    if b.len() < i + 2 || !b[i].is_ascii_digit() || !b[i + 1].is_ascii_digit() {
        return false;
    }
    i += 2;
    if i == b.len() {
        return true;
    }
    if !at(b, i, b':') {
        return false;
    }
    i += 1;
    b.len() == i + 2 && b[i].is_ascii_digit() && b[i + 1].is_ascii_digit()
}

/// Numbers print whole, the fraction truncated: yt-dlp emits two
/// (`format_bytes -> %.2f%sB` in the pinned source) and neither a
/// percentage nor a byte count needs them. `9.94MiB/s` -> `9MiB/s`.
pub fn whole_num(word: &str) -> String {
    let b = word.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'~'));
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == int_start || !at(b, i, b'.') {
        return word.to_string();
    }
    let dot = i;
    i += 1;
    let dec_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == dec_start {
        return word.to_string();
    }
    let tail = i;
    while i < b.len() && (b[i].is_ascii_alphabetic() || matches!(b[i], b'/' | b'%')) {
        i += 1;
    }
    if i == tail || i != b.len() {
        return word.to_string();
    }
    let mut out = String::with_capacity(word.len());
    out.push_str(&word[..dot]);
    out.push_str(&word[tail..]);
    out
}

/// The speed keeps a three-digit integer field so digit crossings never
/// shift the rest of the line; the field is space-padded the way yt-dlp
/// right-aligns its own speed (`format_speed: >10s`).
pub fn pad_speed(word: &str) -> String {
    if !is_speed(word) {
        return word.to_string();
    }
    let whole = whole_num(word);
    let b = whole.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return word.to_string();
    }
    let int_end = i;
    let tail = i;
    while i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'/') {
        i += 1;
    }
    if i == tail || i != b.len() {
        return word.to_string();
    }
    let mut out = String::with_capacity(whole.len() + 3);
    for _ in int_end..3 {
        out.push(' ');
    }
    out.push_str(&whole);
    out
}

/// Color one `[download]` body: percent amber, size mint, speed light
/// blue, ETA purple, the plain words grey; numbers print whole and the
/// speed keeps a three-digit field. Pure; the caller skips it off a tty.
pub fn color_progress(body: &str) -> String {
    const PLAIN_WORDS: [&str; 4] = ["of", "at", "in", "ETA"];
    let mut out = String::with_capacity(body.len() + 64);
    for (n, word) in body.split(' ').enumerate() {
        if n > 0 {
            out.push(' ');
        }
        if word.contains('%') {
            out.push_str(A_AMBER);
            out.push_str(&whole_num(word));
            out.push_str(A_RESET);
        } else if is_size(word) || word.strip_prefix('~').is_some_and(is_size) {
            out.push_str(A_MINT);
            out.push_str(&whole_num(word));
            out.push_str(A_RESET);
        } else if is_speed(word) {
            out.push_str(A_LBLUE);
            out.push_str(&pad_speed(word));
            out.push_str(A_RESET);
        } else if is_time(word) {
            out.push_str(A_PURPLE);
            out.push_str(word);
            out.push_str(A_RESET);
        } else if PLAIN_WORDS.contains(&word) {
            out.push_str(A_GREY);
            out.push_str(word);
            out.push_str(A_RESET);
        } else {
            out.push_str(word);
        }
    }
    out
}

/// Console-safe copy of yt-dlp output (NFR-6): nine punctuation pairs
/// mapped to ASCII, then every remaining non-ASCII character becomes
/// `?`, so a cp1252 console cannot crash the print.
pub fn ascii_safe(text: &str) -> String {
    let s = text
        .replace('\u{ff5c}', "|")
        .replace(['\u{2013}', '\u{2014}'], "-")
        .replace(['\u{2018}', '\u{2019}'], "'")
        .replace(['\u{201c}', '\u{201d}'], "\"")
        .replace('\u{2026}', "...")
        .replace('\u{00a0}', " ");
    s.chars()
        .map(|ch| if ch.is_ascii() { ch } else { '?' })
        .collect()
}

/// ANSI render of one cell frame: a color code only when the color
/// changes, reset at the end.
pub fn render_cells(frame: &[(String, u8)]) -> String {
    let mut out = String::with_capacity(frame.len() * 8 + A_RESET.len());
    let mut last: Option<u8> = None;
    for (ch, col) in frame {
        if last != Some(*col) {
            out.push_str("\x1b[38;5;");
            out.push_str(&col.to_string());
            out.push('m');
            last = Some(*col);
        }
        out.push_str(ch);
    }
    out.push_str(A_RESET);
    out
}

/// The frame without color, trimmed to column 0, for a piped run's
/// frozen line.
pub fn plain_cells(frame: &[(String, u8)]) -> String {
    frame
        .iter()
        .map(|(ch, _)| ch.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Flat frames ("...", ">>>") as cell frames, one cell per character.
pub fn flat_frames(frames: &[String], color: u8) -> Vec<Vec<(String, u8)>> {
    frames
        .iter()
        .map(|f| f.chars().map(|ch| (ch.to_string(), color)).collect())
        .collect()
}

/// A frozen flat marker as one wide cell, rendered exactly as before.
pub fn flat_freeze(text: &str, color: u8) -> Vec<(String, u8)> {
    vec![(text.to_string(), color)]
}

pub const FRAME_SECS: f64 = 0.1;
pub const DOTS: [&str; 5] = [".  ", ".. ", "...", ".. ", ".  "];

// The eight waiting animations, ported from the pre-rust python
// implementation (the reference). Each frame is six
// (char, color) cells so one frame can mix colors; each animation carries
// its own tick and its own settled freeze frame. The parity test below
// pins the whole table against a digest recorded from the reference.

const FIRE: [u8; 8] = [231, 223, 216, 214, 208, 202, 196, 88];
const GRAY: [u8; 8] = [255, 252, 249, 246, 243, 240, 236, 232];
const CYAN: [u8; 8] = [123, 87, 81, 51, 45, 39, 31, 30];
const MAGENTA: [u8; 8] = [213, 177, 141, 105, 99, 63, 62, 60];
const SPECTRUM: [u8; 6] = [196, 214, 226, 46, 33, 129];
const HEAT: [u8; 9] = [234, 238, 244, 250, 215, 208, 202, 196, 231];

pub struct Spinner {
    pub name: &'static str,
    pub frames: Vec<Vec<(String, u8)>>,
    pub tick: f64,
    pub freeze: Vec<(String, u8)>,
}

/// Six-cell frame from `(pos, (char, color))` entries; unset cells stay
/// blank (python `_cells`).
fn cells(mapping: &[(usize, (&str, u8))]) -> Vec<(String, u8)> {
    let mut frame = vec![(" ".to_string(), 0u8); 6];
    for (pos, (ch, col)) in mapping {
        frame[*pos] = (ch.to_string(), *col);
    }
    frame
}

fn comet() -> Vec<Vec<(String, u8)>> {
    const TAIL: [&str; 6] = [".", ":", "-", "=", "+", "*"];
    let path = [0i64, 1, 2, 3, 4, 5, 4, 3, 2, 1];
    let mut frames = Vec::with_capacity(path.len());
    let mut prev = 0i64;
    for (i, &p) in path.iter().enumerate() {
        let step: i64 = if i == 0 || p > prev { 1 } else { -1 };
        prev = p;
        let mut m = vec![(p as usize, ("@", 231))];
        for k in 1..=5i64 {
            let q = p - k * step;
            if (0..=5).contains(&q) {
                m.push((
                    q as usize,
                    (
                        TAIL[(k - 1) as usize],
                        GRAY[(k as usize).min(GRAY.len() - 1)],
                    ),
                ));
            }
        }
        frames.push(cells(&m));
    }
    frames
}

fn pulse() -> Vec<Vec<(String, u8)>> {
    let mut frames = Vec::new();
    for ring in ["*", "+", "."] {
        let mut bloom: Vec<Vec<(String, u8)>> = Vec::with_capacity(3);
        for t in 0..3i64 {
            let mut m = Vec::new();
            for p in 0..6i64 {
                let d = (p as f64 - 2.5).abs();
                if d <= t as f64 + 0.5 {
                    let idx = ((d - 0.5) * 2.0) as usize;
                    if d > t as f64 {
                        m.push((p as usize, (ring, FIRE[idx])));
                    } else {
                        m.push((p as usize, ("o", FIRE[(idx + 2).min(FIRE.len() - 1)])));
                    }
                }
            }
            bloom.push(cells(&m));
        }
        frames.extend(bloom.iter().cloned());
        frames.push(bloom[1].clone());
        frames.push(bloom[0].clone());
    }
    frames
}

fn orbit() -> Vec<Vec<(String, u8)>> {
    let mut frames = Vec::with_capacity(12);
    for ((cl, cr), hl, hr) in [((123u8, 213u8), "o", "O"), ((46, 226), "e", "0")] {
        for d in [1i64, 3, 5, 5, 3, 1] {
            let mut m = Vec::new();
            for p in 0..6i64 {
                let dd = ((p as f64 - 2.5).abs() * 2.0).round() as i64;
                if dd < d {
                    m.push((p as usize, (":", 238)));
                } else if dd == d {
                    m.push((p as usize, if p < 3 { (hl, cl) } else { (hr, cr) }));
                }
            }
            frames.push(cells(&m));
        }
    }
    frames
}

fn scanner() -> Vec<Vec<(String, u8)>> {
    let path = [0i64, 1, 2, 3, 4, 5, 4, 3, 2, 1];
    let mut frames = Vec::with_capacity(path.len());
    for (i, &p) in path.iter().enumerate() {
        let mut m = vec![(p as usize, ("|", SPECTRUM[i % 6]))];
        for k in 1..=3i64 {
            let ch = match k {
                1 => ".",
                2 => ":",
                _ => "-",
            };
            for q in [p - k, p + k] {
                if (0..=5).contains(&q) {
                    m.push((q as usize, (ch, GRAY[(k + 1) as usize])));
                }
            }
        }
        frames.push(cells(&m));
    }
    frames
}

fn twinkle() -> Vec<Vec<(String, u8)>> {
    let glyph = ["*", "+", "."];
    let mut frames = Vec::with_capacity(8);
    for t in 0..8i64 {
        let mut m = Vec::new();
        for p in 0..6i64 {
            let seed = (p * 4 + t * 3) % 11;
            if seed < 3 {
                m.push((
                    p as usize,
                    (glyph[seed as usize], SPECTRUM[((p + t) % 6) as usize]),
                ));
            }
        }
        frames.push(cells(&m));
    }
    frames
}

fn eq() -> Vec<Vec<(String, u8)>> {
    const RAMP: [&str; 9] = [" ", ".", ":", "-", "=", "+", "*", "#", "@"];
    let mut frames = Vec::with_capacity(12);
    for t in 0..12i64 {
        let mut m = Vec::new();
        for p in 0..6i64 {
            // python round() is banker's rounding and f64::round is half
            // away from zero; the reference's closest value to any .5
            // boundary is 2.2e-3 away (scratch probe 2026-10-14), so the
            // two can never disagree here.
            let raw = 4.0 + 3.6 * (t as f64 * 0.8 + p as f64 * 1.25).sin();
            let lvl = raw.round() as i64;
            let lvl = lvl.clamp(0, RAMP.len() as i64 - 1) as usize;
            m.push((p as usize, (RAMP[lvl], HEAT[lvl])));
        }
        frames.push(cells(&m));
    }
    frames
}

fn fill() -> Vec<Vec<(String, u8)>> {
    let edge = |t: usize| {
        let mut m = Vec::new();
        for p in 0..t {
            m.push((p, ("=", CYAN[t])));
            m.push((5 - p, ("=", MAGENTA[t])));
        }
        cells(&m)
    };
    let full = |ch: &'static str| cells(&(0..6usize).map(|p| (p, (ch, 231))).collect::<Vec<_>>());
    let mut frames = vec![edge(0), edge(1), edge(2)];
    frames.push(cells(
        &(0..6usize)
            .map(|p| (p, ("=", if p < 3 { CYAN[3] } else { MAGENTA[3] })))
            .collect::<Vec<_>>(),
    ));
    frames.push(full("#"));
    frames.push(full("#"));
    frames.extend([edge(2), edge(1), edge(0)]);
    frames
}

fn bounce() -> Vec<Vec<(String, u8)>> {
    let path = [0i64, 0, 1, 2, 3, 4, 5, 5, 4, 3, 2, 1];
    let mut frames = Vec::with_capacity(path.len() * 2);
    for hue in [SPECTRUM[0], SPECTRUM[3]] {
        let mut step = 1i64;
        for (i, &p) in path.iter().enumerate() {
            if i > 0 && p != path[i - 1] {
                step = if p > path[i - 1] { 1 } else { -1 };
            }
            let head = if i > 0 && p == path[i - 1] { "O" } else { "o" };
            let mut m = vec![(p as usize, (head, hue))];
            for k in 1..=3i64 {
                let q = p - k * step;
                if (0..=5).contains(&q) {
                    m.push((q as usize, (".", GRAY[(k + 1) as usize])));
                }
            }
            frames.push(cells(&m));
        }
    }
    frames
}

fn build_spinners() -> Vec<Spinner> {
    vec![
        Spinner {
            name: "comet",
            frames: comet(),
            tick: 0.08,
            freeze: cells(&[(2, ("@", 231))]),
        },
        Spinner {
            name: "pulse",
            frames: pulse(),
            tick: 0.08,
            freeze: cells(&[(2, ("o", FIRE[2])), (3, ("o", FIRE[2]))]),
        },
        Spinner {
            name: "orbit",
            frames: orbit(),
            tick: 0.08,
            freeze: cells(&[(2, ("o", CYAN[0]))]),
        },
        Spinner {
            name: "scanner",
            frames: scanner(),
            tick: 0.08,
            freeze: cells(&[(2, ("|", SPECTRUM[3]))]),
        },
        Spinner {
            name: "twinkle",
            frames: twinkle(),
            tick: 0.12,
            freeze: cells(&[(2, ("*", 226))]),
        },
        Spinner {
            name: "eq",
            frames: eq(),
            tick: 0.09,
            freeze: cells(&(0..6usize).map(|p| (p, ("=", HEAT[5]))).collect::<Vec<_>>()),
        },
        Spinner {
            name: "fill",
            frames: fill(),
            tick: 0.08,
            freeze: cells(
                &(0..6usize)
                    .map(|p| (p, ("=", if p < 3 { CYAN[3] } else { MAGENTA[3] })))
                    .collect::<Vec<_>>(),
            ),
        },
        Spinner {
            name: "bounce",
            frames: bounce(),
            tick: 0.08,
            freeze: cells(&[(2, ("o", SPECTRUM[3]))]),
        },
    ]
}

/// The eight animations, built once per process.
pub fn spinners() -> &'static [Spinner] {
    static SPINNERS: OnceLock<Vec<Spinner>> = OnceLock::new();
    SPINNERS.get_or_init(build_spinners)
}

thread_local! {
    static SPINNER_RNG: RefCell<Pcg32> = RefCell::new(crate::rng::seeded());
}

/// A different waiting animation per download run, chosen at random.
pub fn pick_spinner() -> &'static Spinner {
    let all = spinners();
    let idx = SPINNER_RNG.with(|r| r.borrow_mut().below(all.len() as u32)) as usize;
    &all[idx]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha256::sha256_hex;

    #[test]
    fn width_table_matches_the_pinned_reference() {
        // The digest is over the same serialization the generator and the
        // reference probe used: %08x%08x per range (`%08x` hex matches).
        let mut blob: Vec<u8> = Vec::new();
        for &(a, b) in WIDE_RANGES {
            blob.extend_from_slice(format!("{a:08x}{b:08x}\n").as_bytes());
        }
        assert_eq!(
            sha256_hex(&blob),
            "73ee3105a481eed7e96b61663338a0f21b600c970b35f728e2b38a55384d06d5"
        );
        assert_eq!(WIDE_RANGES.len(), 122);
    }

    #[test]
    fn dwidth_counts_columns_not_code_points() {
        // F10 pins, copied from the python selftest expectations.
        assert_eq!(dwidth("ab"), 2);
        assert_eq!(dwidth("\u{4e00}"), 2);
        assert_eq!(dwidth("a\u{4e00}"), 3);
        assert_eq!(dwidth(""), 0);
        assert_eq!(dwidth("\u{1f600}"), 2); // emoji, Wide in unicodedata 16
        assert_eq!(dwidth("\u{ff21}"), 2); // fullwidth A
        assert_eq!(dwidth("\u{0301}"), 1); // combining acute, not W/F
        assert_eq!(dwidth("\u{3000}"), 2); // ideographic space
    }

    #[test]
    fn cut_and_tail_count_columns() {
        let wide = "\u{4e00}";
        assert_eq!(
            cut_to_width(&format!("ab{wide}{wide}{wide}"), 6),
            format!("ab{wide}{wide}")
        );
        assert_eq!(cut_to_width("ab", 1), "a");
        assert_eq!(cut_to_width("ab", 0), "");
        assert_eq!(
            tail_to_width(&format!("ab{wide}{wide}{wide}"), 6),
            wide.repeat(3)
        );
        assert_eq!(tail_to_width("ab", 1), "b");
    }

    #[test]
    fn whole_truncation_matches_selftest_pins() {
        assert_eq!(whole_num("15.2%"), "15%");
        assert_eq!(whole_num("9.94MiB/s"), "9MiB/s");
        assert_eq!(whole_num("~4.63MiB"), "~4MiB");
        assert_eq!(whole_num("198.15MiB"), "198MiB");
        assert_eq!(whole_num("plain"), "plain");
        assert_eq!(whole_num("5.6"), "5.6"); // no unit tail, untouched
    }

    #[test]
    fn pad_speed_matches_selftest_pins() {
        assert_eq!(pad_speed("999.9MiB/s"), "999MiB/s");
        assert_eq!(pad_speed("99.99MiB/s"), " 99MiB/s");
        assert_eq!(pad_speed("2.32MiB/s"), "  2MiB/s");
        assert_eq!(pad_speed("not a speed"), "not a speed");
    }

    /// The rendered strings for the sample bodies recorded from the
    /// reference via a scratch probe on 2026-10-14; the numbers print
    /// whole (USE_COLOR is irrelevant: this function emits codes
    /// unconditionally).
    #[test]
    fn color_progress_matches_the_pinned_strings() {
        let cp1 = color_progress("15.2% of 198.15MiB at 2.32MiB/s ETA 01:12");
        assert_eq!(
            cp1,
            "\x1b[38;5;214m15%\x1b[0m \x1b[38;5;250mof\x1b[0m \x1b[38;5;121m198MiB\x1b[0m \
             \x1b[38;5;250mat\x1b[0m \x1b[38;5;117m  2MiB/s\x1b[0m \x1b[38;5;250mETA\x1b[0m \
             \x1b[38;5;176m01:12\x1b[0m"
        );
        let cp2 = color_progress("5.0% of ~4.63MiB at 9.94MiB/s ETA 00:57");
        assert_eq!(
            cp2,
            "\x1b[38;5;214m5%\x1b[0m \x1b[38;5;250mof\x1b[0m \x1b[38;5;121m~4MiB\x1b[0m \
             \x1b[38;5;250mat\x1b[0m \x1b[38;5;117m  9MiB/s\x1b[0m \x1b[38;5;250mETA\x1b[0m \
             \x1b[38;5;176m00:57\x1b[0m"
        );
        let cp3 = color_progress("100% of 198.15MiB in 00:45");
        assert_eq!(
            cp3,
            "\x1b[38;5;214m100%\x1b[0m \x1b[38;5;250mof\x1b[0m \x1b[38;5;121m198MiB\x1b[0m \
             \x1b[38;5;250min\x1b[0m \x1b[38;5;176m00:45\x1b[0m"
        );
    }

    #[test]
    fn ascii_safe_maps_the_nine_pairs_then_replaces() {
        assert_eq!(ascii_safe("\u{ff5c}"), "|");
        assert_eq!(ascii_safe("\u{2013}\u{2014}"), "--");
        assert_eq!(ascii_safe("\u{2018}\u{2019}"), "''");
        assert_eq!(ascii_safe("\u{201c}\u{201d}"), "\"\"");
        assert_eq!(ascii_safe("\u{2026}"), "...");
        assert_eq!(ascii_safe("a\u{00a0}b"), "a b");
        assert_eq!(ascii_safe("\u{4e2d}\u{6587}"), "??");
        assert_eq!(ascii_safe("plain"), "plain");
    }

    #[test]
    fn render_and_plain_cells_match_selftest_pins() {
        let f1 = vec![("@".to_string(), 231), (".".to_string(), 231)];
        assert_eq!(render_cells(&f1), "\x1b[38;5;231m@.\x1b[0m");
        let f2 = vec![("@".to_string(), 231), (".".to_string(), 202)];
        assert_eq!(render_cells(&f2), "\x1b[38;5;231m@\x1b[38;5;202m.\x1b[0m");
        assert_eq!(
            render_cells(&[("...".to_string(), 214)]),
            "\x1b[38;5;214m...\x1b[0m"
        );
        assert_eq!(plain_cells(spinners()[0].freeze.as_slice()), "@");
        assert_eq!(plain_cells(&flat_freeze("...", 214)), "...");
        let flat = flat_frames(&[".  ".to_string()], 214);
        assert_eq!(
            flat,
            vec![vec![
                (".".to_string(), 214),
                (" ".to_string(), 214),
                (" ".to_string(), 214)
            ]]
        );
    }

    #[test]
    fn eight_spinners_and_every_shape() {
        let all = spinners();
        assert_eq!(all.len(), 8);
        let names: Vec<&str> = all.iter().map(|s| s.name).collect();
        let mut uniq = names.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), 8, "names must be unique: {names:?}");
        for sp in all {
            assert!(!sp.frames.is_empty(), "{}", sp.name);
            assert!(sp.tick > 0.0 && sp.tick <= 0.2, "{}", sp.name);
            assert_eq!(sp.freeze.len(), 6, "{}", sp.name);
            for frame in &sp.frames {
                assert_eq!(frame.len(), 6, "{}", sp.name);
                for (ch, _col) in frame {
                    assert!(!ch.is_empty(), "{}", sp.name);
                }
            }
        }
        assert_eq!(
            all.iter().map(|s| s.frames.len()).collect::<Vec<_>>(),
            [10, 15, 12, 10, 8, 12, 9, 24]
        );
        let pick = pick_spinner();
        assert!(all.iter().any(|s| std::ptr::eq(s, pick)));
        // freeze frames render in color end to end
        for sp in all {
            assert!(render_cells(&sp.freeze).ends_with(A_RESET), "{}", sp.name);
        }
    }

    /// The recorded reference digest over the exact serialization the scratch
    /// probe used (name|tick|cells;cells;freeze\n per spinner). This is
    /// what makes "ported from the reference" checkable: any wrong cell,
    /// color, tick or frame order moves the digest.
    #[test]
    fn spinner_table_matches_the_reference_digest() {
        let mut blob: Vec<u8> = Vec::new();
        for sp in spinners() {
            let tick = format!("{:.6}", sp.tick);
            let tick = tick.trim_end_matches('0').trim_end_matches('.');
            blob.extend_from_slice(format!("{}|{}|", sp.name, tick).as_bytes());
            for frame in &sp.frames {
                for (ch, col) in frame {
                    blob.extend_from_slice(format!("{ch}:{col},").as_bytes());
                }
                blob.push(b';');
            }
            for (ch, col) in &sp.freeze {
                blob.extend_from_slice(format!("{ch}:{col},").as_bytes());
            }
            blob.push(b'\n');
        }
        assert_eq!(
            sha256_hex(&blob),
            "cba9f09dde5804089d343f9eedc1635ca474f6dd93a0c8ce9e3b54d825c089ac"
        );
    }

    #[test]
    fn c_gates_on_color() {
        assert_eq!(c(false, A_AMBER, "x"), "x");
        assert_eq!(c(true, A_AMBER, "x"), "\x1b[38;5;214mx\x1b[0m");
    }
}
