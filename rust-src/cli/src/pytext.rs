//! Python string semantics the port cannot get from rust's stdlib
//! equivalents: the `str.strip`/`str.splitlines`/`\s` character sets,
//! and the `repr()` of a string (quote choice and escape spellings),
//! which the `str()` of container values renders element-wise.
//! Shared by the layers that must split text exactly like the reference
//! (specsheet section 4 port notes: python `str.strip` whitespace
//! includes `\x1c`-`\x1f`, `\x85` and `\xa0`; `str.splitlines` breaks on
//! `\n`, `\r`, `\r\n`, `\v`, `\f`, `\x1c`-`\x1e`, `\x85`, U+2028, U+2029).

use crate::printable_table::PRINTABLE_RANGES;

/// CPython `Py_UNICODE_ISPRINTABLE` from the generated reference table: the
/// characters `repr()` keeps literal.
pub fn py_printable(c: char) -> bool {
    let cp = c as u32;
    let idx = PRINTABLE_RANGES.partition_point(|&(start, _)| start <= cp);
    idx > 0 && cp < PRINTABLE_RANGES[idx - 1].1
}

/// CPython `repr()` of a string: quote choice (double quotes only when
/// the text carries a single quote and no double quote), the special
/// spellings for tab/newline/CR, then the non-printable escapes
/// (`\xNN`/`\uNNNN`/`\UNNNNNNNN`, lowercase hex). Escape predicates
/// pinned against CPython 3.14.6 and 3.14.4 probes (2026-10-15).
pub fn py_repr_str(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        if c == quote {
            out.push('\\');
            out.push(c);
        } else if c == '\\' {
            out.push_str("\\\\");
        } else if c == '\t' {
            out.push_str("\\t");
        } else if c == '\n' {
            out.push_str("\\n");
        } else if c == '\r' {
            out.push_str("\\r");
        } else if py_printable(c) {
            out.push(c);
        } else {
            let cp = c as u32;
            if cp < 0x100 {
                out.push_str(&format!("\\x{cp:02x}"));
            } else if cp < 0x10000 {
                out.push_str(&format!("\\u{cp:04x}"));
            } else {
                out.push_str(&format!("\\U{cp:08x}"));
            }
        }
    }
    out.push(quote);
    out
}

/// One character of python's whitespace set: rust `char::is_whitespace`
/// (the Unicode White_Space property) plus the four ASCII controls
/// `\x1c`-`\x1f` that python counts and the property does not.
pub fn py_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\x1c'..='\x1f')
}

/// Python `str.strip()` with no argument.
pub fn py_strip(text: &str) -> &str {
    text.trim_matches(py_space)
}

/// Python `str.rstrip()` with no argument (the download pump's line
/// cleanup).
pub fn py_rstrip(text: &str) -> &str {
    text.trim_end_matches(py_space)
}

/// Python `str.split()` with no argument over the python whitespace set
/// (`" ".join(line.split())` in the download pump).
pub fn py_split_ws(text: &str) -> Vec<&str> {
    text.split(py_space).filter(|s| !s.is_empty()).collect()
}

/// Python `str.splitlines()` break set, as the first line only: the text
/// up to the first break character (`\r\n` counts as one break, which is
/// the same prefix).
pub fn py_splitlines_first(text: &str) -> &str {
    for (i, c) in text.char_indices() {
        if matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            return &text[..i];
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn py_strip_matches_python_whitespace() {
        assert_eq!(py_strip("  a  "), "a");
        assert_eq!(py_strip("\u{1c}a\u{85}"), "a"); // controls python counts
        assert_eq!(py_strip("\u{a0}a\u{a0}"), "a"); // nbsp is whitespace
        assert_eq!(py_strip("a"), "a");
        assert_eq!(py_strip(""), "");
        assert_eq!(py_strip(" \t\r\n "), "");
    }

    #[test]
    fn splitlines_first_matches_python_breaks() {
        assert_eq!(py_splitlines_first("a\nb"), "a");
        assert_eq!(py_splitlines_first("a\r\nb"), "a");
        assert_eq!(py_splitlines_first("a\rb"), "a");
        assert_eq!(py_splitlines_first("a\u{0b}b"), "a");
        assert_eq!(py_splitlines_first("a\u{85}b"), "a");
        assert_eq!(py_splitlines_first("a\u{2028}b"), "a");
        assert_eq!(py_splitlines_first("ab"), "ab");
        assert_eq!(py_splitlines_first(""), "");
    }

    /// Pins from CPython 3.14.6 repr() probes (2026-10-15); the same
    /// matrix was re-run on 3.14.4 with identical output.
    #[test]
    fn py_repr_str_matches_cpython() {
        for (text, expect) in [
            ("abc", "'abc'"),
            ("", "''"),
            ("a'b", "\"a'b\""),
            ("a\"b", "'a\"b'"),
            ("a'b\"c", "'a\\'b\"c'"),
            ("back\\slash", "'back\\\\slash'"),
            ("tab\tx", "'tab\\tx'"),
            ("nl\nx", "'nl\\nx'"),
            ("cr\rx", "'cr\\rx'"),
            ("\x00\x1f\x7f", "'\\x00\\x1f\\x7f'"),
            ("\u{ad}\u{a0}", "'\\xad\\xa0'"),
            ("\u{378}", "'\\u0378'"),
            ("\u{200b}", "'\\u200b'"),
            ("zhong\u{4e2d}", "'zhong\u{4e2d}'"),
            ("emoji\u{1f600}", "'emoji\u{1f600}'"),
            ("\u{e0001}", "'\\U000e0001'"),
            ("caf\u{e9}", "'caf\u{e9}'"),
        ] {
            assert_eq!(py_repr_str(text), expect, "repr of {text:?}");
        }
    }

    #[test]
    fn py_printable_spot_pins() {
        // reference edges, probed 2026-10-15
        assert!(py_printable(' '));
        assert!(py_printable('~'));
        assert!(!py_printable('\u{1f}')); // below space is not printable
        assert!(!py_printable('\u{7f}'));
        assert!(!py_printable('\u{a0}'));
        assert!(!py_printable('\u{ad}'));
        assert!(!py_printable('\u{378}')); // unassigned
        assert!(!py_printable('\u{200b}'));
        assert!(py_printable('\u{4e2d}'));
        assert!(py_printable('\u{1f600}'));
        assert!(!py_printable('\u{e0001}'));
        assert!(!py_printable('\u{10ffff}')); // unassigned top
    }
}
