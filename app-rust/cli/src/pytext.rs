//! Python string semantics the port cannot get from rust's stdlib
//! equivalents: the `str.strip`/`str.splitlines`/`\s` character sets.
//! Shared by the layers that must split text exactly like the oracle
//! (specsheet section 4 port notes: python `str.strip` whitespace
//! includes `\x1c`-`\x1f`, `\x85` and `\xa0`; `str.splitlines` breaks on
//! `\n`, `\r`, `\r\n`, `\v`, `\f`, `\x1c`-`\x1e`, `\x85`, U+2028, U+2029).

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
}
