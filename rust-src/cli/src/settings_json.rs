//! CPython-faithful JSON codec for the settings file (phase 3, state layer).
//!
//! The retired harness interchange codec (ensure_ascii=False, records
//! only) was a different dialect; this one mirrors the `json` module as
//! the settings file sees it.
//! Read: `json.loads` semantics - floats, the `NaN`/`Infinity`/`-Infinity`
//! constants, last-wins duplicate keys; integers saturate at i64 per the
//! recorded port note; a lone-surrogate `\u` escape, unrepresentable in a
//! rust String, maps to U+FFFD (recorded; python keeps the lone surrogate).
//! Write: `json.dumps(obj, indent=2, sort_keys=True)` semantics -
//! ensure_ascii=True escaping (0x00-0x1f short forms and `\uXXXX`, 0x7f
//! and every char above as `\uXXXX`, astral chars as surrogate pairs) and
//! the json float spellings (`NaN`, `Infinity`, `-Infinity` for non-finite
//! values; `repr` semantics through `py_float_repr` for finite ones).
//!
//! Pins recorded 2026-10-14 from CPython 3.14.4 (the pinned reference; probe
//! output `.scratch-rust-p3-01/pins.txt`, deleted after use).

use crate::config::{Obj, Value, py_float_repr, set};
use std::fmt;

#[derive(Debug)]
pub struct ParseError {
    pos: usize,
    msg: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parse error at byte {}: {}", self.pos, self.msg)
    }
}

const MAX_DEPTH: usize = 128;

pub fn parse(text: &str) -> Result<Value, ParseError> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    p.skip_ws();
    let v = p.value(0)?;
    p.skip_ws();
    if p.i != p.b.len() {
        return Err(p.err("trailing data"));
    }
    Ok(v)
}

/// The settings file as text: `json.dumps(obj, indent=2, sort_keys=True)`,
/// no trailing newline (the caller adds the LF the file carries).
pub fn render(obj: &Obj) -> String {
    let mut out = String::with_capacity(256);
    write_obj(obj, 0, &mut out);
    out
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, msg: &'static str) -> ParseError {
        ParseError { pos: self.i, msg }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), ParseError> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(())
        } else {
            Err(self.err("bad literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        match self.peek() {
            Some(b'n') => {
                self.literal("null")?;
                Ok(Value::Null)
            }
            Some(b't') => {
                self.literal("true")?;
                Ok(Value::Bool(true))
            }
            Some(b'f') => {
                self.literal("false")?;
                Ok(Value::Bool(false))
            }
            Some(b'N') => {
                self.literal("NaN")?;
                Ok(Value::Float(f64::NAN))
            }
            Some(b'I') => {
                self.literal("Infinity")?;
                Ok(Value::Float(f64::INFINITY))
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Value::Arr(items));
                }
                loop {
                    self.skip_ws();
                    items.push(self.value(depth + 1)?);
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Arr(items));
                        }
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut fields: Obj = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(Value::Obj(fields));
                }
                loop {
                    self.skip_ws();
                    if self.peek() != Some(b'"') {
                        return Err(self.err("expected object key"));
                    }
                    let key = self.string()?;
                    self.skip_ws();
                    if self.peek() != Some(b':') {
                        return Err(self.err("expected ':'"));
                    }
                    self.i += 1;
                    self.skip_ws();
                    let val = self.value(depth + 1)?;
                    // python dict assignment: duplicate keys keep the first
                    // position and the last value
                    set(&mut fields, &key, val);
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Obj(fields));
                        }
                        _ => return Err(self.err("expected ',' or '}'")),
                    }
                }
            }
            Some(b'-') => {
                if self.b[self.i..].starts_with(b"-Infinity") {
                    self.i += 9;
                    Ok(Value::Float(f64::NEG_INFINITY))
                } else {
                    self.number()
                }
            }
            Some(b'0'..=b'9') => self.number(),
            _ => Err(self.err("unexpected token")),
        }
    }

    /// JSON number, the `json.loads` grammar: optional '-', no leading
    /// zeros, optional fraction and exponent; integers saturate at i64
    /// (recorded port note), floats parse to f64 (overflow goes to inf
    /// like python).
    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        let digits = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        if self.i == digits {
            return Err(self.err("expected digits"));
        }
        if self.i - digits > 1 && self.b[digits] == b'0' {
            return Err(self.err("leading zero"));
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            self.i += 1;
            let frac = self.i;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            if self.i == frac {
                return Err(self.err("expected fraction digits"));
            }
            is_float = true;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            let exp = self.i;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            if self.i == exp {
                return Err(self.err("expected exponent digits"));
            }
            is_float = true;
        }
        let text =
            std::str::from_utf8(&self.b[start..self.i]).map_err(|_| self.err("invalid utf-8"))?;
        if is_float {
            match text.parse::<f64>() {
                Ok(x) => Ok(Value::Float(x)),
                Err(_) => Err(self.err("bad float")),
            }
        } else {
            match text.parse::<i64>() {
                Ok(n) => Ok(Value::Int(n)),
                Err(e) => match e.kind() {
                    std::num::IntErrorKind::PosOverflow => Ok(Value::Int(i64::MAX)),
                    std::num::IntErrorKind::NegOverflow => Ok(Value::Int(i64::MIN)),
                    _ => Err(self.err("bad integer")),
                },
            }
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.i += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    self.escape(&mut out)?;
                }
                Some(c) if c < 0x20 => return Err(self.err("raw control character in string")),
                Some(_) => {
                    // one plain run up to the next ", \ or control byte:
                    // none of those bytes can sit inside a multi-byte
                    // utf-8 sequence, so the run ends at a char boundary
                    // and the validation cost stays linear in the
                    // document (the old per-character tail revalidation
                    // made a long string quadratic)
                    let mut end = self.i;
                    while let Some(b) = self.b.get(end) {
                        if *b == b'"' || *b == b'\\' || *b < 0x20 {
                            break;
                        }
                        end += 1;
                    }
                    let run = std::str::from_utf8(&self.b[self.i..end])
                        .map_err(|_| self.err("invalid utf-8"))?;
                    out.push_str(run);
                    self.i = end;
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), ParseError> {
        match self.peek() {
            Some(b'"') => {
                out.push('"');
                self.i += 1;
            }
            Some(b'\\') => {
                out.push('\\');
                self.i += 1;
            }
            Some(b'/') => {
                out.push('/');
                self.i += 1;
            }
            Some(b'b') => {
                out.push('\u{8}');
                self.i += 1;
            }
            Some(b'f') => {
                out.push('\u{c}');
                self.i += 1;
            }
            Some(b'n') => {
                out.push('\n');
                self.i += 1;
            }
            Some(b'r') => {
                out.push('\r');
                self.i += 1;
            }
            Some(b't') => {
                out.push('\t');
                self.i += 1;
            }
            Some(b'u') => {
                self.i += 1;
                let first = self.hex4()?;
                if (0xD800..=0xDBFF).contains(&first) {
                    // High surrogate: combine with a following low-surrogate
                    // escape when one is there; otherwise - and for a lone
                    // low surrogate below - the recorded port note applies
                    // and U+FFFD stands in (python keeps the lone surrogate).
                    let save = self.i;
                    if self.peek() == Some(b'\\') {
                        self.i += 1;
                        if self.peek() == Some(b'u') {
                            self.i += 1;
                            let second = self.hex4()?;
                            if (0xDC00..=0xDFFF).contains(&second) {
                                let cp = 0x10000
                                    + ((u32::from(first) - 0xD800) << 10)
                                    + (u32::from(second) - 0xDC00);
                                out.push(
                                    char::from_u32(cp).ok_or_else(|| self.err("bad code point"))?,
                                );
                            } else {
                                self.i = save;
                                out.push('\u{FFFD}');
                            }
                        } else {
                            self.i = save;
                            out.push('\u{FFFD}');
                        }
                    } else {
                        out.push('\u{FFFD}');
                    }
                } else if (0xDC00..=0xDFFF).contains(&first) {
                    out.push('\u{FFFD}');
                } else {
                    out.push(
                        char::from_u32(u32::from(first))
                            .ok_or_else(|| self.err("bad code point"))?,
                    );
                }
            }
            _ => return Err(self.err("bad escape")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u16, ParseError> {
        let mut v: u16 = 0;
        for _ in 0..4 {
            let c = self
                .peek()
                .ok_or_else(|| self.err("truncated \\u escape"))?;
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return Err(self.err("invalid \\u escape")),
            };
            v = (v << 4) | u16::from(d);
            self.i += 1;
        }
        Ok(v)
    }
}

fn push_indent(level: usize, out: &mut String) {
    for _ in 0..2 * level {
        out.push(' ');
    }
}

fn write_value(v: &Value, level: usize, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(n) => {
            let _ = fmt::Write::write_fmt(out, format_args!("{n}"));
        }
        Value::Float(x) => out.push_str(&float_spelling(*x)),
        Value::Str(s) => write_escaped(s, out),
        Value::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                push_indent(level + 1, out);
                write_value(item, level + 1, out);
            }
            out.push('\n');
            push_indent(level, out);
            out.push(']');
        }
        Value::Obj(fields) => write_obj(fields, level, out),
    }
}

fn write_obj(fields: &Obj, level: usize, out: &mut String) {
    // python dicts cannot hold duplicates; a hand-built Obj can, so the
    // render view dedupes last-wins like dict assignment would, and the
    // sorted order erases the first-position nuance sort_keys removes.
    let mut uniq: std::collections::BTreeMap<&str, &Value> = std::collections::BTreeMap::new();
    for (k, v) in fields {
        uniq.insert(k.as_str(), v);
    }
    if uniq.is_empty() {
        out.push_str("{}");
        return;
    }
    out.push_str("{\n");
    let last = uniq.len() - 1;
    for (i, (k, v)) in uniq.iter().enumerate() {
        push_indent(level + 1, out);
        write_escaped(k, out);
        out.push_str(": ");
        write_value(v, level + 1, out);
        if i < last {
            out.push(',');
        }
        out.push('\n');
    }
    push_indent(level, out);
    out.push('}');
}

/// `json.dumps` float spelling: the constants for non-finite values,
/// `repr` semantics (through `py_float_repr`) for finite ones.
fn float_spelling(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == f64::INFINITY {
        return "Infinity".to_string();
    }
    if x == f64::NEG_INFINITY {
        return "-Infinity".to_string();
    }
    py_float_repr(x)
}

/// ensure_ascii=True string escaping, pins in the module docs.
fn write_escaped(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => push_u_hex(c as u32, out),
            c if !c.is_ascii() => {
                let cp = c as u32;
                if cp <= 0xFFFF {
                    push_u_hex(cp, out);
                } else {
                    let v = cp - 0x10000;
                    push_u_hex(0xD800 + (v >> 10), out);
                    push_u_hex(0xDC00 + (v & 0x3FF), out);
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_u_hex(cp: u32, out: &mut String) {
    let _ = fmt::Write::write_fmt(out, format_args!("\\u{cp:04x}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::get;

    fn obj(fields: &[(&str, Value)]) -> Obj {
        fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    // Parse pins recorded 2026-10-14 from CPython 3.14.4
    // (json.loads answers, .scratch-rust-p3-01/pins.txt).
    #[test]
    fn parser_int_float_grammar() {
        for (text, expect) in [
            ("42", Value::Int(42)),
            ("-0", Value::Int(0)),
            ("-17", Value::Int(-17)),
            ("1.5", Value::Float(1.5)),
            ("1e2", Value::Float(100.0)),
            ("1E+2", Value::Float(100.0)),
            ("1e-2", Value::Float(0.01)),
            ("-0.0", Value::Float(-0.0)),
            ("-Infinity", Value::Float(f64::NEG_INFINITY)),
            ("Infinity", Value::Float(f64::INFINITY)),
            ("NaN", Value::Float(f64::NAN)),
        ] {
            let got = parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
            match (&got, &expect) {
                (Value::Float(a), Value::Float(b)) if a.is_nan() && b.is_nan() => {}
                _ => assert_eq!(got, expect, "text {text:?}"),
            }
        }
        // 1e999 overflows to inf like python's float()
        let Value::Float(big) = parse("1e999").unwrap() else {
            panic!("1e999 must be a float");
        };
        assert!(big.is_infinite() && big > 0.0);
    }

    #[test]
    fn parser_rejects_bad_numbers_and_tokens() {
        for text in [
            "01",
            "-01",
            "1.",
            ".5",
            "+1",
            "1e",
            "1e+",
            "nan",
            "inf",
            "tru",
            "-NaN",
            "--1",
            "{1:2}",
            "[1,]",
            "\"\u{1}\"",
        ] {
            assert!(parse(text).is_err(), "should reject: {text}");
        }
    }

    #[test]
    fn parser_saturates_integers() {
        let big = "9".repeat(30);
        assert_eq!(parse(&big).unwrap(), Value::Int(i64::MAX));
        let small = format!("-{big}");
        assert_eq!(parse(&small).unwrap(), Value::Int(i64::MIN));
    }

    #[test]
    fn parser_dedupes_keys_last_wins_first_position() {
        let Value::Obj(fields) = parse(r#"{"a": 1, "b": 2, "a": 3}"#).unwrap() else {
            panic!("object expected");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, "a");
        assert_eq!(get(&fields, "a"), Some(&Value::Int(3)));
        assert_eq!(get(&fields, "b"), Some(&Value::Int(2)));
    }

    #[test]
    fn parser_lone_surrogates_become_replacement() {
        assert_eq!(
            parse(r#""\ud800""#).unwrap(),
            Value::Str("\u{FFFD}".to_string())
        );
        assert_eq!(
            parse(r#""\udc00""#).unwrap(),
            Value::Str("\u{FFFD}".to_string())
        );
        // a proper pair still combines
        assert_eq!(
            parse(r#""\ud83d\ude00""#).unwrap(),
            Value::Str("\u{1F600}".to_string())
        );
        // high surrogate followed by a non-low escape: replacement, then
        // the second escape stands alone
        assert_eq!(
            parse(r#""\ud800\u0041""#).unwrap(),
            Value::Str("\u{FFFD}A".to_string())
        );
        // high surrogate followed by a plain char
        assert_eq!(
            parse(r#""\ud800x""#).unwrap(),
            Value::Str("\u{FFFD}x".to_string())
        );
        // an invalid second escape is still a parse error
        assert!(parse(r#""\ud800\uzz""#).is_err());
    }

    #[test]
    fn parser_bom_is_an_error_like_json_loads() {
        assert!(parse("\u{FEFF}{}").is_err());
    }

    // audit perf 4: the old per-character tail revalidation made a long
    // string quadratic; a 100k-char document must still decode, in one
    // run per plain span
    #[test]
    fn parser_decodes_a_long_string_in_linear_runs() {
        let plain = format!("\"{}\"", "a".repeat(100_000));
        let Value::Str(got) = parse(&plain).unwrap() else {
            panic!("string expected");
        };
        assert_eq!(got.chars().count(), 100_000);
        let mut text = String::from("\"");
        for _ in 0..20_000 {
            text.push_str("x\u{e9}\\n");
        }
        text.push('"');
        let Value::Str(got) = parse(&text).unwrap() else {
            panic!("string expected");
        };
        assert_eq!(got, "x\u{e9}\n".repeat(20_000));
    }

    // Render pins recorded 2026-10-14 from CPython 3.14.4:
    //   json.dumps(value, indent=2, sort_keys=True)
    #[test]
    fn render_pins_small() {
        assert_eq!(render(&obj(&[])), "{}");
        assert_eq!(
            render(&obj(&[
                ("zz", Value::Int(1)),
                ("aa", Value::Str("x".into()))
            ])),
            "{\n  \"aa\": \"x\",\n  \"zz\": 1\n}"
        );
        assert_eq!(
            render(&obj(&[("arr", Value::Arr(vec![]))])),
            "{\n  \"arr\": []\n}"
        );
        assert_eq!(
            render(&obj(&[("obj", Value::Obj(Obj::new()))])),
            "{\n  \"obj\": {}\n}"
        );
        // probe deep_nest
        assert_eq!(
            render(&obj(&[(
                "a",
                Value::Arr(vec![Value::Obj(obj(&[(
                    "b",
                    Value::Arr(vec![Value::Int(1), Value::Int(2)]),
                )]))])
            )])),
            "{\n  \"a\": [\n    {\n      \"b\": [\n        1,\n        2\n      ]\n    }\n  ]\n}"
        );
        // probe indent: arr with mixed empties
        assert_eq!(
            render(&obj(&[(
                "arr",
                Value::Arr(vec![
                    Value::Int(1),
                    Value::Arr(vec![]),
                    Value::Obj(Obj::new()),
                    Value::Float(2.5),
                ])
            )])),
            "{\n  \"arr\": [\n    1,\n    [],\n    {},\n    2.5\n  ]\n}"
        );
    }

    #[test]
    fn render_float_pins() {
        assert_eq!(
            render(&obj(&[
                ("a", Value::Float(1.5)),
                ("b", Value::Float(1e16)),
                ("c", Value::Float(-0.0)),
                ("d", Value::Float(f64::NAN)),
                ("e", Value::Float(f64::INFINITY)),
                ("f", Value::Float(f64::NEG_INFINITY)),
                ("g", Value::Float(0.0001)),
                ("h", Value::Int(1791925032)),
            ])),
            "{\n  \"a\": 1.5,\n  \"b\": 1e+16,\n  \"c\": -0.0,\n  \"d\": NaN,\n  \"e\": Infinity,\n  \"f\": -Infinity,\n  \"g\": 0.0001,\n  \"h\": 1791925032\n}"
        );
    }

    #[test]
    fn render_string_escaping_pins() {
        // probe t_ctrl (inside indent output)
        assert_eq!(
            render(&obj(&[("t", Value::Str("a\tb\n\"q\\".into()))])),
            "{\n  \"t\": \"a\\tb\\n\\\"q\\\\\"\n}"
        );
        // probe u_uni: e-acute, CJK, emoji as escapes
        assert_eq!(
            render(&obj(&[("u", Value::Str("\u{e9}\u{4e2d}\u{1f600}".into()))])),
            "{\n  \"u\": \"\\u00e9\\u4e2d\\ud83d\\ude00\"\n}"
        );
        // probe del_and_specials: 0x7f, 0x1f and U+2028/U+2029 all escaped
        assert_eq!(
            render(&obj(&[
                ("\u{7f}", Value::Int(1)),
                ("\u{1f}", Value::Int(2)),
                ("\u{2028}\u{2029}", Value::Int(3)),
            ])),
            "{\n  \"\\u001f\": 2,\n  \"\\u007f\": 1,\n  \"\\u2028\\u2029\": 3\n}"
        );
        // short forms
        assert_eq!(
            render(&obj(&[("s", Value::Str("\u{8}\u{c}\r".into()))])),
            "{\n  \"s\": \"\\b\\f\\r\"\n}"
        );
    }

    #[test]
    fn render_dedupes_duplicate_keys_last_wins() {
        let dup = obj(&[
            ("a", Value::Int(1)),
            ("b", Value::Int(3)),
            ("a", Value::Int(2)),
        ]);
        assert_eq!(render(&dup), "{\n  \"a\": 2,\n  \"b\": 3\n}");
    }

    #[test]
    fn render_python_json_spelling_for_non_finite() {
        // 'nan'/'inf' (repr spellings) are wrong here; json spells them
        assert_eq!(float_spelling(f64::NAN), "NaN");
        assert_eq!(float_spelling(f64::INFINITY), "Infinity");
        assert_eq!(float_spelling(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(float_spelling(-0.0), "-0.0");
        assert_eq!(float_spelling(1.0), "1.0");
    }
}
