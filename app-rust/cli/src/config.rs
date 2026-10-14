//! Configuration (specsheet section 4, layer 2): the settings schema as
//! values, the shipped defaults, and the audit F1/F2 boundary coercion.
//! Load and save (atomic write, LF, skip-when-unchanged) land in this
//! module with their phase.
//!
//! The settings object is modelled as an ordered JSON object, mirroring
//! the python dict: user files may carry any value on any key, and the
//! coercion below is the one place raw values are validated before they
//! reach int()/argv/renders.

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
}
