//! Minimal JSON codec for the differential harness.
//!
//! The writer is byte-for-byte identical to CPython 3.14
//! `json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)`,
//! which is the interchange format `tools/harness.py` produces, and the
//! parser mirrors `json.loads`, so phase-3 vector inputs may carry floats
//! and the `NaN`/`Infinity`/`-Infinity` constants. Generated records stay
//! i64-only, enforced on both sides. Traps and rules:
//! python-to-rust-refactor-pitfalls.md items 4 and 5.

use std::fmt;
use ytdlp_cli::config::py_float_repr;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

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

pub fn parse(text: &str) -> Result<Json, ParseError> {
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

    fn value(&mut self, depth: usize) -> Result<Json, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        match self.peek() {
            Some(b'n') => {
                self.literal("null")?;
                Ok(Json::Null)
            }
            Some(b't') => {
                self.literal("true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.literal("false")?;
                Ok(Json::Bool(false))
            }
            Some(b'N') => {
                self.literal("NaN")?;
                Ok(Json::Float(f64::NAN))
            }
            Some(b'I') => {
                self.literal("Infinity")?;
                Ok(Json::Float(f64::INFINITY))
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    self.skip_ws();
                    items.push(self.value(depth + 1)?);
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut fields = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(fields));
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
                    fields.push((key, val));
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(fields));
                        }
                        _ => return Err(self.err("expected ',' or '}'")),
                    }
                }
            }
            Some(b'-') => {
                if self.b[self.i..].starts_with(b"-Infinity") {
                    self.i += 9;
                    Ok(Json::Float(f64::NEG_INFINITY))
                } else {
                    self.number()
                }
            }
            Some(b'0'..=b'9') => self.number(),
            _ => Err(self.err("unexpected token")),
        }
    }

    fn number(&mut self) -> Result<Json, ParseError> {
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
            return match text.parse::<f64>() {
                Ok(x) => Ok(Json::Float(x)),
                Err(_) => Err(self.err("bad float")),
            };
        }
        match text.parse::<i64>() {
            Ok(n) => Ok(Json::Int(n)),
            Err(_) => Err(self.err("integer out of i64 range")),
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
                    let rest = std::str::from_utf8(&self.b[self.i..])
                        .map_err(|_| self.err("invalid utf-8"))?;
                    let ch = rest.chars().next().ok_or(self.err("truncated utf-8"))?;
                    out.push(ch);
                    self.i += ch.len_utf8();
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
                    if self.peek() == Some(b'\\') {
                        self.i += 1;
                    } else {
                        return Err(self.err("unpaired surrogate"));
                    }
                    if self.peek() == Some(b'u') {
                        self.i += 1;
                    } else {
                        return Err(self.err("unpaired surrogate"));
                    }
                    let second = self.hex4()?;
                    if !(0xDC00..=0xDFFF).contains(&second) {
                        return Err(self.err("unpaired surrogate"));
                    }
                    let cp = 0x10000
                        + ((u32::from(first) - 0xD800) << 10)
                        + (u32::from(second) - 0xDC00);
                    out.push(char::from_u32(cp).ok_or(self.err("bad code point"))?);
                } else if (0xDC00..=0xDFFF).contains(&first) {
                    return Err(self.err("unpaired surrogate"));
                } else {
                    out.push(char::from_u32(u32::from(first)).ok_or(self.err("bad code point"))?);
                }
            }
            _ => return Err(self.err("bad escape")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u16, ParseError> {
        let mut v: u16 = 0;
        for _ in 0..4 {
            let c = self.peek().ok_or(self.err("truncated \\u escape"))?;
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

const HEX: [u8; 16] = *b"0123456789abcdef";

/// Canonical serialization, byte-identical to the CPython call named in
/// the module docs. Callers own the surrounding framing (trailing '\n').
pub fn write_canonical(v: &Json, out: &mut Vec<u8>) {
    match v {
        Json::Null => out.extend_from_slice(b"null"),
        Json::Bool(true) => out.extend_from_slice(b"true"),
        Json::Bool(false) => out.extend_from_slice(b"false"),
        Json::Int(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Json::Float(x) => out.extend_from_slice(float_spelling(*x).as_bytes()),
        Json::Str(s) => write_string(s, out),
        Json::Arr(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        Json::Obj(fields) => {
            let mut order: Vec<usize> = (0..fields.len()).collect();
            order.sort_by(|&a, &b| fields[a].0.cmp(&fields[b].0));
            out.push(b'{');
            for (n, &idx) in order.iter().enumerate() {
                if n > 0 {
                    out.push(b',');
                }
                write_string(&fields[idx].0, out);
                out.push(b':');
                write_canonical(&fields[idx].1, out);
            }
            out.push(b'}');
        }
    }
}

/// `json.dumps` float spelling under ensure_ascii=False: the constants for
/// non-finite values, `repr` semantics (through `py_float_repr`) for finite
/// ones. Record outputs never carry floats; this serves vector round-trips.
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

fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for ch in s.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\r' => out.extend_from_slice(b"\\r"),
            c if (c as u32) < 0x20 => {
                let v = c as u32;
                out.extend_from_slice(b"\\u00");
                out.push(HEX[((v >> 4) & 0xF) as usize]);
                out.push(HEX[(v & 0xF) as usize]);
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(v: &Json) -> String {
        let mut out = Vec::new();
        write_canonical(v, &mut out);
        String::from_utf8(out).unwrap()
    }

    fn s(text: &str) -> Json {
        Json::Str(text.to_string())
    }

    // Expected strings recorded 2026-10-14 from CPython 3.14.6 (probe
    // output, .scratch-rust-p1-01/pins.txt, deleted after use); the float
    // spellings also recorded from the 3.14.4 oracle
    // (.scratch-rust-p3-01/pins.txt, deleted after use):
    //   json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
    #[allow(clippy::type_complexity)]
    fn writer_pins() -> Vec<(Json, &'static str)> {
        vec![
            (s("hello"), "\"hello\""),
            (s("a\"b\\c"), "\"a\\\"b\\\\c\""),
            (
                s("\u{0}\u{1}\u{1f}\u{8}\t\n\u{c}\r"),
                "\"\\u0000\\u0001\\u001f\\b\\t\\n\\f\\r\"",
            ),
            (s("\u{e9}\u{4e2d}\u{1f600}"), "\"\u{e9}\u{4e2d}\u{1f600}\""),
            (Json::Int(-5), "-5"),
            (Json::Int(0), "0"),
            (Json::Int(i64::MAX), "9223372036854775807"),
            (Json::Int(i64::MIN), "-9223372036854775808"),
            (Json::Float(1.5), "1.5"),
            (Json::Float(1e16), "1e+16"),
            (Json::Float(-0.0), "-0.0"),
            (Json::Float(0.0001), "0.0001"),
            (Json::Float(f64::NAN), "NaN"),
            (Json::Float(f64::INFINITY), "Infinity"),
            (Json::Float(f64::NEG_INFINITY), "-Infinity"),
            (Json::Bool(true), "true"),
            (Json::Bool(false), "false"),
            (Json::Null, "null"),
            (
                Json::Arr(vec![Json::Null, Json::Bool(true), Json::Int(1), s("x")]),
                "[null,true,1,\"x\"]",
            ),
            (
                Json::Obj(vec![
                    ("b".to_string(), Json::Int(1)),
                    (
                        "a".to_string(),
                        Json::Arr(vec![Json::Bool(true), Json::Bool(false), Json::Null]),
                    ),
                    ("A".to_string(), s("z")),
                    ("\u{e9}".to_string(), s("u")),
                ]),
                "{\"A\":\"z\",\"a\":[true,false,null],\"b\":1,\"\u{e9}\":\"u\"}",
            ),
            (
                Json::Arr(vec![Json::Arr(vec![Json::Arr(vec![])])]),
                "[[[]]]",
            ),
            (Json::Obj(vec![]), "{}"),
            (Json::Arr(vec![]), "[]"),
        ]
    }

    #[test]
    fn writer_matches_cpython_pins() {
        for (value, expected) in writer_pins() {
            assert_eq!(canon(&value), expected);
        }
    }

    #[test]
    fn parser_roundtrips_cpython_pins() {
        for (value, expected) in writer_pins() {
            let parsed = parse(expected).expect("pin must parse");
            assert_eq!(
                canon(&parsed),
                canon(&value),
                "parse must preserve the value"
            );
            assert_eq!(canon(&parsed), expected);
        }
    }

    #[test]
    fn parser_accepts_escapes_pairs_and_whitespace() {
        assert_eq!(
            parse("\"\\u0041\\u00e9\\u4e2d\\ud83d\\ude00\"").unwrap(),
            s("A\u{e9}\u{4e2d}\u{1f600}")
        );
        assert_eq!(parse("\"\\/\"").unwrap(), s("/"));
        assert_eq!(
            parse(" \t[ 1 , 2 ]\n ").unwrap(),
            Json::Arr(vec![Json::Int(1), Json::Int(2)])
        );
        assert_eq!(parse("-0").unwrap(), Json::Int(0));
    }

    #[test]
    fn parser_rejects_bad_input() {
        let bad = [
            "",
            "01",
            "01.5",
            "1.",
            "1e",
            ".5",
            "+1",
            "1 2",
            "\"\u{1}\"",
            "\"\\ud800\"",
            "\"\\q\"",
            "{}x",
            "tru",
            "[1,]",
            "{\"a\":}",
            "-",
            "{1:2}",
            "9223372036854775808",
        ];
        for text in bad {
            assert!(parse(text).is_err(), "should reject: {text}");
        }
        let deep = "[".repeat(200) + &"]".repeat(200);
        assert!(parse(&deep).is_err(), "should reject deep nesting");
    }
}
