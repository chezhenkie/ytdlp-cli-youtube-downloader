//! Configuration (specsheet section 4, layer 2): the settings schema as
//! values, the shipped defaults, and the audit F1/F2 boundary coercion.
//! Load and save (atomic write, LF, skip-when-unchanged) land in this
//! module with their phase.
//!
//! The settings object is modelled as an ordered JSON object, mirroring
//! the python dict: user files may carry any value on any key, and the
//! coercion below is the one place raw values are validated before they
//! reach int()/argv/renders.

use crate::settings_json;
use std::fmt;
use std::io;
use std::path::Path;

/// A settings value; structurally the JSON data model. State that the
/// settings file cannot hold is not representable, and nothing here is
/// tuned to shapes the files happen to use (pitfalls doc, item 8).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Obj),
}

pub type Obj = Vec<(String, Value)>;

/// Object field read; the python `s.get(key)`.
pub fn get<'a>(obj: &'a Obj, key: &str) -> Option<&'a Value> {
    obj.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Object field write, position-preserving for an existing key exactly
/// like a python dict assignment.
pub fn set(obj: &mut Obj, key: &str, value: Value) {
    if let Some(slot) = obj.iter_mut().find(|(k, _)| k == key) {
        slot.1 = value;
    } else {
        obj.push((key.to_string(), value));
    }
}

fn str_value(text: &str) -> Value {
    Value::Str(text.to_string())
}

/// The shipped defaults, in the python dict's order (TYPES et al. below
/// are pinned by the specsheet FR-2/FR-8).
pub fn defaults() -> Obj {
    [
        ("settings_version", Value::Int(1)),
        ("yt_dlp_path", str_value("")),
        ("ffmpeg_path", str_value("")),
        ("auto_update", Value::Bool(true)),
        ("auto_update_last_check", Value::Int(0)),
        ("type", str_value("video")),
        ("video_container", str_value("mp4")),
        ("video_resolution", str_value("1080")),
        ("audio_format", str_value("mp3")),
        ("audio_quality", str_value("0")),
        ("output_dir", str_value("")),
        ("filename_template", str_value("%(title)s")),
        ("subtitles", Value::Bool(true)),
        ("subtitle_langs", str_value("en")),
        ("playlist", Value::Bool(false)),
        ("playlist_max_items", Value::Int(0)),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect()
}

pub const RESOLUTIONS: [&str; 7] = ["2160", "1440", "1080", "720", "480", "360", "best"];
pub const CONTAINERS: [&str; 4] = ["mp4", "mkv", "webm", "best"];
pub const AUDIO_FORMATS: [&str; 5] = ["mp3", "m4a", "opus", "wav", "best"];
pub const TYPES: [&str; 3] = ["video", "video no audio", "audio only"];

/// Python `%r` for the values the defaults carry (int and plain string;
/// the bool/null/float arms are spelled out for completeness - none of
/// the coerced keys has such a default, and no default string contains a
/// quote or backslash that would need escaping).
fn py_repr(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("'{s}'"),
        Value::Int(n) => n.to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Null => "None".to_string(),
        Value::Float(x) => py_float_repr(*x),
        Value::Arr(_) => "[]".to_string(),
        Value::Obj(_) => "{}".to_string(),
    }
}

/// CPython `str(float)` / `repr(float)` for finite, inf and nan values:
/// shortest round-trip digits, fixed notation while the decimal point
/// sits in `-3..=16`, scientific with a two-digit minimum exponent
/// otherwise, and always a `.0` tail in fixed notation. Used by the
/// coercion path (`str()` of a raw settings value) and, later, by the
/// settings writer.
pub fn py_float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x == f64::INFINITY {
        return "inf".to_string();
    }
    if x == f64::NEG_INFINITY {
        return "-inf".to_string();
    }
    let neg = x.is_sign_negative();
    let ax = x.abs();
    if ax == 0.0 {
        return if neg { "-0.0" } else { "0.0" }.to_string();
    }
    // shortest round-trip digits through Rust's scientific formatting
    let sci = format!("{ax:e}");
    let (mant, exp) = sci.split_once('e').expect("scientific format carries an e");
    let exp: i32 = exp.parse().expect("exponent is an integer");
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1;
    let mut out = String::with_capacity(24);
    if neg {
        out.push('-');
    }
    if decpt <= -4 || decpt > 16 {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        let exp_val = decpt - 1;
        out.push(if exp_val < 0 { '-' } else { '+' });
        let mag = exp_val.unsigned_abs();
        if mag < 10 {
            out.push('0');
        }
        out.push_str(&mag.to_string());
    } else if decpt <= 0 {
        out.push_str("0.");
        for _ in 0..-decpt {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let n = decpt as usize;
        if digits.len() > n {
            out.push_str(&digits[..n]);
            out.push('.');
            out.push_str(&digits[n..]);
        } else {
            out.push_str(&digits);
            for _ in 0..n - digits.len() {
                out.push('0');
            }
            out.push_str(".0");
        }
    }
    out
}

/// Python `str(value)` for the values that can reach the audio-quality
/// check. Only the numeric and string arms need exact representation:
/// every other shape starts with a non-digit regardless of era, so the
/// quality pattern cannot match it either way (arr/obj reprs included).
fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(x) => py_float_repr(*x),
        Value::Str(s) => s.clone(),
        Value::Arr(_) => "[]".to_string(),
        Value::Obj(_) => "{}".to_string(),
    }
}

/// Python `int(str_value)` for the playlist_max_items path: optional
/// single `+`/`-`, ASCII digit runs with at most one underscore between
/// them, surrounding whitespace trimmed. Values beyond i64 saturate
/// (recorded port note: python keeps a bignum, but the harness protocol
/// is i64 and every consumer - a `> 0` test and a `1-<n>` argv - cannot
/// observe the difference). Unicode digits and other int() niceties are
/// deliberately not accepted; see the module port notes in the spec.
fn py_int(text: &str) -> Option<i64> {
    let t = text.trim();
    let (neg, body) = match t.strip_prefix(['+', '-']) {
        Some(rest) => (t.starts_with('-'), rest),
        None => (false, t),
    };
    if body.is_empty() {
        return None;
    }
    let mut digits = String::with_capacity(body.len());
    for part in body.split('_') {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.push_str(part);
    }
    let mut num = String::with_capacity(digits.len() + 1);
    if neg {
        num.push('-');
    }
    num.push_str(&digits);
    match num.parse::<i64>() {
        Ok(n) => Some(n),
        Err(e) => match e.kind() {
            std::num::IntErrorKind::PosOverflow => Some(i64::MAX),
            std::num::IntErrorKind::NegOverflow => Some(i64::MIN),
            _ => None,
        },
    }
}

/// yt-dlp `--audio-quality SPEC`: `^\d+(?:\.\d+)?K?$`, case-insensitive
/// on the K (pinned from `yt_dlp/options.py`).
fn is_quality(q: &str) -> bool {
    let b = q.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return false;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'K' | b'k')) {
        i += 1;
    }
    i == b.len()
}

/// Audit F1/F2: settings.json is user-editable and its values flow raw
/// into int() and yt-dlp argv. Coerce at this one boundary so a mistyped
/// value can never raise and never reaches argv unvalidated. In memory
/// only - the file is never rewritten (user settings are theirs).
/// Returns the note lines, in python's order.
pub fn coerce_settings(s: &mut Obj) -> Vec<String> {
    let d = defaults();
    let mut notes = Vec::new();

    fn keep_default(s: &mut Obj, d: &Obj, notes: &mut Vec<String>, key: &str, why: &str) {
        let default = get(d, key).expect("coerced keys have defaults").clone();
        notes.push(format!(
            "setting {key} {why} - keeping default {}",
            py_repr(&default)
        ));
        set(s, key, default);
    }

    match get(s, "playlist_max_items") {
        Some(Value::Int(_)) => {}
        Some(Value::Str(text)) => match py_int(text) {
            Some(n) => set(s, "playlist_max_items", Value::Int(n)),
            None => keep_default(s, &d, &mut notes, "playlist_max_items", "is not a number"),
        },
        _ => keep_default(s, &d, &mut notes, "playlist_max_items", "is not a number"),
    }

    for (key, vocab) in [
        ("type", TYPES.as_slice()),
        ("video_container", CONTAINERS.as_slice()),
        ("video_resolution", RESOLUTIONS.as_slice()),
        ("audio_format", AUDIO_FORMATS.as_slice()),
    ] {
        let ok = matches!(get(s, key), Some(Value::Str(v)) if vocab.contains(&v.as_str()));
        if !ok {
            let why = format!("is not one of {}", vocab.join("|"));
            keep_default(s, &d, &mut notes, key, &why);
        }
    }

    let q = py_str(get(s, "audio_quality").unwrap_or(&Value::Str(String::new())));
    let q = q.trim();
    if !is_quality(q) {
        keep_default(s, &d, &mut notes, "audio_quality", "is not a quality value");
    } else {
        set(s, "audio_quality", Value::Str(q.to_string()));
    }
    notes
}

// ------------------------------------------------------------------ state

/// Outcome of `load_settings`, mirroring the python returns: the loaded
/// object plus the coercion notes, or the "settings unreadable" path,
/// which yields the defaults with a message and runs no coercion (python
/// returns before `coerce_settings` there; the defaults are already valid).
#[derive(Debug)]
pub enum LoadOutcome {
    Loaded { settings: Obj, notes: Vec<String> },
    Unreadable { settings: Obj, message: String },
}

/// The shapes where python raises out of `load_settings` (a settings file
/// whose JSON is not an object). Carries the python exception class name
/// as the shape token for the harness record; messages stay shape-gated
/// (RUST-PORT-PLAN.md port rules).
#[derive(Debug)]
pub struct ShapeError {
    pub class_name: &'static str,
}

impl fmt::Display for ShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "settings file holds a JSON value that is not an object (python raises {})",
            self.class_name
        )
    }
}

#[derive(Debug)]
pub enum SaveError {
    /// The existing file is not valid UTF-8; python raises UnicodeDecodeError
    /// in the skip-compare read.
    NotUtf8,
    Io(std::io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::NotUtf8 => write!(f, "settings file is not valid UTF-8"),
            SaveError::Io(e) => write!(f, "could not write the settings file: {e}"),
        }
    }
}

/// Python `bool(value)` over the settings shapes; the legacy audio_only
/// line is the only caller today.
pub fn py_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(x) => *x != 0.0,
        Value::Str(s) => !s.is_empty(),
        Value::Arr(a) => !a.is_empty(),
        Value::Obj(o) => !o.is_empty(),
    }
}

/// The settings file content as the writer leaves it: render plus one LF.
pub fn render_settings_file(s: &Obj) -> String {
    let mut out = settings_json::render(s);
    out.push('\n');
    out
}

/// `Path.read_text(encoding="utf-8")`: strict UTF-8 with python's universal
/// newline translation (CRLF and lone CR become LF). Used for the load
/// parse and the save skip-compare so both sides see python's text.
fn read_text(path: &Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not valid UTF-8"))?;
    if !text.contains('\r') {
        return Ok(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Python: `"settings unreadable (%s), using defaults" % exc` - the payload
/// is shape-gated, the wording around it is pinned.
fn unreadable_message(payload: &str) -> String {
    format!("settings unreadable ({payload}), using defaults")
}

/// Python `load_settings`: defaults, merge of known keys, the legacy
/// audio_only line, then the boundary coercion. The file is never written.
pub fn load_settings(path: &Path) -> Result<LoadOutcome, ShapeError> {
    let mut s = defaults();
    if !path.exists() {
        let notes = coerce_settings(&mut s);
        return Ok(LoadOutcome::Loaded { settings: s, notes });
    }
    let text = match read_text(path) {
        Ok(t) => t,
        Err(e) => {
            return Ok(LoadOutcome::Unreadable {
                settings: s,
                message: unreadable_message(&e.to_string()),
            });
        }
    };
    let data = match settings_json::parse(&text) {
        Ok(v) => v,
        Err(e) => {
            return Ok(LoadOutcome::Unreadable {
                settings: s,
                message: unreadable_message(&e.to_string()),
            });
        }
    };
    match data {
        Value::Obj(fields) => {
            let raw_has_type = get(&fields, "type").is_some();
            let raw_audio_only = matches!(get(&fields, "audio_only"), Some(v) if py_truthy(v));
            for (k, v) in fields {
                if get(&s, &k).is_some() {
                    set(&mut s, &k, v);
                }
            }
            // The legacy check reads the raw file data, not the merged
            // object (the defaults always carry "type").
            if !raw_has_type && raw_audio_only {
                set(&mut s, "type", Value::Str("audio only".to_string()));
            }
        }
        // Every other shape is where python walks off the object protocol.
        // The walk below mirrors what it does before it raises; the class
        // name is the shape token the harness compares.
        Value::Str(t) => {
            for ch in t.chars() {
                if s.iter().any(|(k, _)| k.chars().eq(std::iter::once(ch))) {
                    // s[ch] = data[ch] with a str index raises TypeError
                    return Err(ShapeError {
                        class_name: "TypeError",
                    });
                }
            }
            if !t.contains("type") {
                // data.get on a str raises AttributeError
                return Err(ShapeError {
                    class_name: "AttributeError",
                });
            }
        }
        Value::Arr(items) => {
            for item in &items {
                match item {
                    Value::Str(k) => {
                        if get(&s, k).is_some() {
                            // s[k] = data[k] with a list index raises TypeError
                            return Err(ShapeError {
                                class_name: "TypeError",
                            });
                        }
                    }
                    Value::Arr(_) | Value::Obj(_) => {
                        // `key in s` on an unhashable element raises TypeError
                        return Err(ShapeError {
                            class_name: "TypeError",
                        });
                    }
                    _ => {}
                }
            }
            // `"type" not in data` is False here only via an element equal
            // to a settings key, which already crashed above; the membership
            // and the .get that follows are still modelled shape for shape.
            let has_type = items
                .iter()
                .any(|v| matches!(v, Value::Str(t) if t == "type"));
            if !has_type {
                return Err(ShapeError {
                    class_name: "AttributeError",
                });
            }
        }
        _ => {
            // `for key in data` on a non-iterable raises TypeError
            return Err(ShapeError {
                class_name: "TypeError",
            });
        }
    }
    let notes = coerce_settings(&mut s);
    Ok(LoadOutcome::Loaded { settings: s, notes })
}

/// Python `save_settings`: render, skip when the newline-translated
/// existing text already matches, create parents, write the `.tmp` file,
/// rename it over the target. The render carries LF; the tmp step is a
/// create-or-truncate write exactly like python's `open(tmp, "w")`.
pub fn save_settings(path: &Path, s: &Obj) -> Result<(), SaveError> {
    let data = render_settings_file(s);
    match read_text(path) {
        Ok(existing) => {
            if existing == data {
                return Ok(());
            }
        }
        Err(e) if e.kind() == io::ErrorKind::InvalidData => return Err(SaveError::NotUtf8),
        // everything else is python's `except OSError: pass`: fall through
        // to the write
        Err(_) => {}
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(SaveError::Io)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data.as_bytes()).map_err(SaveError::Io)?;
    std::fs::rename(&tmp, path).map_err(SaveError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn junk_from_defaults(overrides: &[(&str, Value)]) -> Obj {
        let mut s = defaults();
        for (k, v) in overrides {
            set(&mut s, k, v.clone());
        }
        s
    }

    fn to_map(s: &Obj) -> HashMap<String, Value> {
        s.iter().cloned().collect()
    }

    /// F1/F2 junk set from the python selftest: coerce fixes the values,
    /// never raises, and emits one note per repair in python's order.
    #[test]
    fn coerce_fixes_the_junk_set() {
        let mut junk = junk_from_defaults(&[
            ("playlist", Value::Bool(true)),
            ("playlist_max_items", Value::Str("abc".to_string())),
            ("video_resolution", Value::Str("not-a-number".to_string())),
            ("audio_quality", Value::Str("banana".to_string())),
            ("video_container", Value::Str("avi".to_string())),
            ("type", Value::Str("tunnel".to_string())),
        ]);
        let notes = coerce_settings(&mut junk);
        let m = to_map(&junk);
        assert_eq!(m["playlist_max_items"], Value::Int(0));
        assert_eq!(m["video_resolution"], Value::Str("1080".to_string()));
        assert_eq!(m["video_container"], Value::Str("mp4".to_string()));
        assert_eq!(m["audio_format"], Value::Str("mp3".to_string()));
        assert_eq!(m["type"], Value::Str("video".to_string()));
        assert_eq!(
            notes,
            vec![
                "setting playlist_max_items is not a number - keeping default 0".to_string(),
                "setting type is not one of video|video no audio|audio only - keeping default 'video'"
                    .to_string(),
                "setting video_container is not one of mp4|mkv|webm|best - keeping default 'mp4'"
                    .to_string(),
                "setting video_resolution is not one of 2160|1440|1080|720|480|360|best - keeping default '1080'"
                    .to_string(),
                "setting audio_quality is not a quality value - keeping default '0'".to_string(),
            ]
        );
    }

    #[test]
    fn coerce_keeps_the_boundary_values() {
        let mut q9 = junk_from_defaults(&[("audio_quality", Value::Str("9".to_string()))]);
        assert_eq!(coerce_settings(&mut q9), Vec::<String>::new());
        assert_eq!(to_map(&q9)["audio_quality"], Value::Str("9".to_string()));

        let mut q320 = junk_from_defaults(&[("audio_quality", Value::Str("320K".to_string()))]);
        coerce_settings(&mut q320);
        assert_eq!(
            to_map(&q320)["audio_quality"],
            Value::Str("320K".to_string())
        );

        let mut n5 = junk_from_defaults(&[("playlist_max_items", Value::Str("5".to_string()))]);
        coerce_settings(&mut n5);
        assert_eq!(to_map(&n5)["playlist_max_items"], Value::Int(5));
    }

    #[test]
    fn coerce_matches_python_int_forms() {
        for (raw, expect) in [
            ("1_000", Some(1000i64)),
            ("+5", Some(5)),
            ("-3", Some(-3)),
            (" 7 ", Some(7)),
            ("007", Some(7)),
            ("_1", None),
            ("1_", None),
            ("1__0", None),
            ("abc", None),
            ("0x10", None),
            ("", None),
        ] {
            assert_eq!(py_int(raw), expect, "raw {raw:?}");
        }
        // beyond i64 saturates (recorded port note)
        assert_eq!(py_int(&"9".repeat(30)), Some(i64::MAX));
    }

    #[test]
    fn coerce_accepts_the_unsigned_float_shape_python_would() {
        // python str(5.0) is "5.0", which passes the quality pattern
        let mut s = junk_from_defaults(&[("audio_quality", Value::Float(5.0))]);
        let notes = coerce_settings(&mut s);
        assert_eq!(notes, Vec::<String>::new());
        assert_eq!(to_map(&s)["audio_quality"], Value::Str("5.0".to_string()));
        // a float that prints with an exponent cannot match and falls back
        let mut s = junk_from_defaults(&[("audio_quality", Value::Float(1e16))]);
        let notes = coerce_settings(&mut s);
        assert_eq!(notes.len(), 1);
        assert_eq!(to_map(&s)["audio_quality"], Value::Str("0".to_string()));
    }

    #[test]
    fn coerce_quality_grammar() {
        for (q, ok) in [
            ("0", true),
            ("9.5", true),
            ("320k", true),
            ("320K", true),
            ("10", true),
            ("5.", false),
            (".5", false),
            ("", false),
            ("5K5", false),
            ("best", false),
        ] {
            let mut s = junk_from_defaults(&[("audio_quality", Value::Str(q.to_string()))]);
            let notes = coerce_settings(&mut s);
            assert_eq!(notes.is_empty(), ok, "q {q:?}");
        }
    }

    /// Pins recorded from CPython 3.14.6 repr() on 2026-10-14.
    #[test]
    fn py_float_repr_matches_cpython() {
        for (x, expect) in [
            (0.0f64, "0.0"),
            (-0.0, "-0.0"),
            (5.0, "5.0"),
            (3.5, "3.5"),
            (0.5, "0.5"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (123.456, "123.456"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1.5e-7, "1.5e-07"),
            (1e300, "1e+300"),
            (-2.5, "-2.5"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
            (f64::NAN, "nan"),
        ] {
            assert_eq!(py_float_repr(x), expect, "x {x:?}");
        }
    }

    // ------------------------------------------------------- state tests

    fn temp_case(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("ytdlp-cfg-test-{}-{}-{n}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_file(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("seed file");
        path
    }

    #[test]
    fn py_truthy_matches_python() {
        for (v, expect) in [
            (Value::Null, false),
            (Value::Bool(true), true),
            (Value::Bool(false), false),
            (Value::Int(0), false),
            (Value::Int(-1), true),
            (Value::Float(0.0), false),
            (Value::Float(-0.0), false),
            (Value::Float(0.5), true),
            (Value::Float(f64::NAN), true),
            (Value::Str(String::new()), false),
            (Value::Str("x".into()), true),
            (Value::Arr(vec![]), false),
            (Value::Arr(vec![Value::Null]), true),
            (Value::Obj(Obj::new()), false),
            (Value::Obj(vec![("k".into(), Value::Null)]), true),
        ] {
            assert_eq!(py_truthy(&v), expect, "value {v:?}");
        }
    }

    #[test]
    fn load_missing_file_gives_defaults_without_notes() {
        let dir = temp_case("missing");
        let path = dir.join("settings.json");
        match load_settings(&path).unwrap() {
            LoadOutcome::Loaded { settings, notes } => {
                assert_eq!(settings, defaults());
                assert!(notes.is_empty());
            }
            other => panic!("unexpected outcome {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_merges_known_keys_and_drops_unknown() {
        let dir = temp_case("merge");
        let path = write_file(
            &dir,
            "settings.json",
            br#"{"video_resolution": "720", "zz": 9, "audio_only": true}"#,
        );
        let LoadOutcome::Loaded { settings, notes } = load_settings(&path).unwrap() else {
            panic!("loaded expected");
        };
        assert!(notes.is_empty());
        assert_eq!(
            get(&settings, "video_resolution").unwrap(),
            &Value::Str("720".into())
        );
        assert_eq!(get(&settings, "zz"), None);
        assert_eq!(to_map(&settings).len(), 16);
        assert_eq!(
            get(&settings, "type").unwrap(),
            &Value::Str("audio only".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_legacy_audio_only_truthiness() {
        for (literal, expect) in [
            ("true", "audio only"),
            ("1", "audio only"),
            ("-2", "audio only"),
            ("1.5", "audio only"),
            ("\"x\"", "audio only"),
            ("[0]", "audio only"),
            ("{\"a\": 0}", "audio only"),
            ("false", "video"),
            ("0", "video"),
            ("\"\"", "video"),
            ("[]", "video"),
            ("{}", "video"),
            ("null", "video"),
        ] {
            let dir = temp_case("legacy");
            let path = write_file(
                &dir,
                "settings.json",
                format!("{{\"audio_only\": {literal}}}").as_bytes(),
            );
            let LoadOutcome::Loaded { settings, .. } = load_settings(&path).unwrap() else {
                panic!("loaded expected for {literal}");
            };
            assert_eq!(
                get(&settings, "type").unwrap(),
                &Value::Str(expect.to_string()),
                "literal {literal}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn load_type_present_blocks_legacy_line() {
        let dir = temp_case("typeline");
        let path = write_file(
            &dir,
            "settings.json",
            br#"{"type": "audio only", "audio_only": true}"#,
        );
        let LoadOutcome::Loaded { settings, notes } = load_settings(&path).unwrap() else {
            panic!("loaded expected");
        };
        assert!(notes.is_empty());
        assert_eq!(
            get(&settings, "type").unwrap(),
            &Value::Str("audio only".into())
        );

        let path2 = write_file(
            &dir,
            "settings2.json",
            br#"{"type": "x", "audio_only": true}"#,
        );
        let LoadOutcome::Loaded { settings, notes } = load_settings(&path2).unwrap() else {
            panic!("loaded expected");
        };
        assert_eq!(get(&settings, "type").unwrap(), &Value::Str("video".into()));
        assert_eq!(
            notes,
            vec![
                "setting type is not one of video|video no audio|audio only - keeping default 'video'"
                    .to_string()
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_junk_emits_coercion_notes_in_python_order() {
        let dir = temp_case("junknote");
        let path = write_file(
            &dir,
            "settings.json",
            br#"{"playlist_max_items": "abc", "video_resolution": "not-a-number", "audio_quality": "banana", "video_container": "avi", "type": "tunnel"}"#,
        );
        let LoadOutcome::Loaded { settings, notes } = load_settings(&path).unwrap() else {
            panic!("loaded expected");
        };
        assert_eq!(
            notes,
            vec![
                "setting playlist_max_items is not a number - keeping default 0".to_string(),
                "setting type is not one of video|video no audio|audio only - keeping default 'video'".to_string(),
                "setting video_container is not one of mp4|mkv|webm|best - keeping default 'mp4'".to_string(),
                "setting video_resolution is not one of 2160|1440|1080|720|480|360|best - keeping default '1080'".to_string(),
                "setting audio_quality is not a quality value - keeping default '0'".to_string(),
            ]
        );
        assert_eq!(
            get(&settings, "playlist_max_items").unwrap(),
            &Value::Int(0)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_unreadable_shapes() {
        for (name, bytes) in [
            ("bad_json", b"not json{".to_vec()),
            ("invalid_utf8", vec![0x7b, 0xbd, 0x7d]),
            ("bom", b"\xef\xbb\xbf{}".to_vec()),
        ] {
            let dir = temp_case("unreadable");
            let path = write_file(&dir, name, &bytes);
            match load_settings(&path).unwrap() {
                LoadOutcome::Unreadable { settings, message } => {
                    assert_eq!(settings, defaults());
                    assert!(message.starts_with("settings unreadable ("), "{message}");
                    assert!(message.ends_with("), using defaults"), "{message}");
                }
                other => panic!("{name}: unexpected outcome {other:?}"),
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn load_dir_path_is_unreadable() {
        let dir = temp_case("dirpath");
        let path = dir.join("settings.json");
        std::fs::create_dir(&path).unwrap();
        match load_settings(&path).unwrap() {
            LoadOutcome::Unreadable { .. } => {}
            other => panic!("unexpected outcome {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_non_object_shapes_are_typed_errors() {
        for (text, class) in [
            ("42", "TypeError"),
            ("4.5", "TypeError"),
            ("true", "TypeError"),
            ("null", "TypeError"),
            ("\"hello\"", "AttributeError"),
            ("\"\"", "AttributeError"),
            ("\"type\"", ""),      // substring short-circuit: loads defaults
            ("\"printtype\"", ""), // substring anywhere counts
            ("[]", "AttributeError"),
            ("[\"hello\", 1]", "AttributeError"),
            ("[\"output_dir\"]", "TypeError"),
            ("[{}]", "TypeError"),
            ("{}", ""),
        ] {
            let dir = temp_case("shape");
            let path = write_file(&dir, "settings.json", text.as_bytes());
            match (load_settings(&path), class) {
                (Ok(LoadOutcome::Loaded { settings, notes }), "") => {
                    assert_eq!(settings, defaults(), "text {text}");
                    assert!(notes.is_empty(), "text {text}");
                }
                (Err(e), want) => assert_eq!(e.class_name, want, "text {text}"),
                (other, want) => panic!("text {text}: expected class {want:?}, got {other:?}"),
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn load_crlf_file_translates_and_parses() {
        let dir = temp_case("crlf");
        let path = write_file(
            &dir,
            "settings.json",
            b"{\r\n  \"audio_format\": \"wav\"\r\n}\r\n",
        );
        let LoadOutcome::Loaded { settings, notes } = load_settings(&path).unwrap() else {
            panic!("loaded expected");
        };
        assert!(notes.is_empty());
        assert_eq!(
            get(&settings, "audio_format").unwrap(),
            &Value::Str("wav".into())
        );
        // the loader never writes: bytes untouched
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"{\r\n  \"audio_format\": \"wav\"\r\n}\r\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_bignum_on_unknown_key_is_dropped_before_it_matters() {
        // The one divergence (i64 saturation) cannot be observed here: the
        // key is dropped before its value could differ.
        let dir = temp_case("bignum");
        let path = write_file(
            &dir,
            "settings.json",
            br#"{"zz": 99999999999999999999999999999999}"#,
        );
        let LoadOutcome::Loaded { settings, notes } = load_settings(&path).unwrap() else {
            panic!("loaded expected");
        };
        assert!(notes.is_empty());
        assert_eq!(settings, defaults());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_writes_the_pinned_render_and_lf() {
        let dir = temp_case("savewrite");
        let path = dir.join("settings.json");
        let mut s = Obj::new();
        set(&mut s, "zz", Value::Int(1));
        set(&mut s, "aa", Value::Str("x".into()));
        save_settings(&path, &s).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"{\n  \"aa\": \"x\",\n  \"zz\": 1\n}\n"
        );
        assert!(!path.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_skips_equal_content_without_touching_tmp() {
        let dir = temp_case("skips");
        let path = dir.join("settings.json");
        let mut s = Obj::new();
        set(&mut s, "aa", Value::Str("x".into()));
        let data = render_settings_file(&s);
        std::fs::write(&path, &data).unwrap();
        // A save that decided to write would truncate this directory and
        // fail; the skip path never touches the tmp path.
        std::fs::create_dir(path.with_extension("tmp")).unwrap();
        save_settings(&path, &s).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), data.as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_skips_crlf_file_equal_after_translation() {
        let dir = temp_case("skips_crlf");
        let path = dir.join("settings.json");
        let mut s = Obj::new();
        set(&mut s, "aa", Value::Str("x".into()));
        let crlf = render_settings_file(&s).replace('\n', "\r\n");
        std::fs::write(&path, &crlf).unwrap();
        save_settings(&path, &s).unwrap();
        // python's read_text translation made these equal, so the skip
        // leaves the CRLF file exactly as it was
        assert_eq!(std::fs::read(&path).unwrap(), crlf.as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_overwrites_different_content() {
        let dir = temp_case("overwrite");
        let path = write_file(&dir, "settings.json", b"{\n  \"aa\": \"old\"\n}\n");
        let mut s = Obj::new();
        set(&mut s, "aa", Value::Str("new".into()));
        save_settings(&path, &s).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\n  \"aa\": \"new\"\n}\n");
        assert!(!path.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_creates_missing_parents() {
        let dir = temp_case("parents");
        let path = dir.join("sub").join("inner").join("settings.json");
        let mut s = Obj::new();
        set(&mut s, "aa", Value::Int(1));
        save_settings(&path, &s).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\n  \"aa\": 1\n}\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_refuses_invalid_utf8_existing_file() {
        let dir = temp_case("notutf8");
        let path = write_file(&dir, "settings.json", &[0xbd]);
        let mut s = Obj::new();
        set(&mut s, "aa", Value::Int(1));
        match save_settings(&path, &s) {
            Err(SaveError::NotUtf8) => {}
            other => panic!("unexpected result {other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), [0xbd]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_parent_file_is_an_io_error() {
        let dir = temp_case("parentfile");
        write_file(&dir, "sub", b"x"); // "sub" is a file, not a directory
        let path = dir.join("sub").join("settings.json");
        let mut s = Obj::new();
        set(&mut s, "aa", Value::Int(1));
        match save_settings(&path, &s) {
            Err(SaveError::Io(_)) => {}
            other => panic!("unexpected result {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_roundtrips_clean_defaults() {
        let dir = temp_case("roundtrip");
        let path = dir.join("settings.json");
        let s = defaults();
        save_settings(&path, &s).unwrap();
        let LoadOutcome::Loaded { settings, notes } = load_settings(&path).unwrap() else {
            panic!("loaded expected");
        };
        assert!(notes.is_empty());
        assert_eq!(settings, defaults());
        let before = std::fs::read(&path).unwrap();
        save_settings(&path, &settings).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
