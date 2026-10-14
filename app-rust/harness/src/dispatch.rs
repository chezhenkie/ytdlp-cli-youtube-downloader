//! Dispatcher arms for the differential harness: one arm per ported
//! function, mapping vector args onto the cli crate and the result back
//! to JSON.
//!
//! Record shapes mirror each python return value exactly (NFR-4):
//! `Option<String>` is null|string, python tuples are arrays, and a case
//! with `"capture_args": true` wraps the record as
//! `{"args_after": ..., "result": ...}` exactly like tools/harness.py.
//!
//! Functions whose python source reads process or module state take that
//! state as a trailing arg; tools/harness.py binds it on the python side
//! (PY_CALL_HOOKS).
//!
//! pick_spinner has no arm on purpose: it returns a random draw, so a
//! byte comparison is impossible; its twins live in cli/src/term.rs.

use crate::json::Json;
use ytdlp_cli::config::{Obj, Value};
use ytdlp_cli::{args, config, exec, term, ui, update};

pub fn dispatch(name: &str, args: &[Json]) -> Result<Json, String> {
    match name {
        "clean_url" => {
            let text = opt_str(args, 0)?;
            Ok(str_or_null(args::clean_url(text)))
        }
        "js_runtime_args" => {
            let has_node = arg_bool(args, 0)?;
            Ok(str_arr(args::js_runtime_args(has_node)))
        }
        "sub_lang_patterns" => {
            let langs = opt_str(args, 0)?;
            Ok(Json::Str(args::sub_lang_patterns(langs)))
        }
        "parse_lang_input" => {
            let raw = opt_str(args, 0)?;
            let (kept, bad, unknown) = args::parse_lang_input(raw);
            Ok(Json::Arr(vec![
                str_arr(kept),
                str_arr(bad),
                str_arr(unknown),
            ]))
        }
        "one_dec_num" => Ok(Json::Str(term::one_dec_num(arg_str(args, 0)?))),
        "pad_speed" => Ok(Json::Str(term::pad_speed(arg_str(args, 0)?))),
        "color_progress" => Ok(Json::Str(term::color_progress(arg_str(args, 0)?))),
        "ascii_safe" => Ok(Json::Str(term::ascii_safe(arg_str(args, 0)?))),
        "dwidth" => Ok(Json::Int(term::dwidth(arg_str(args, 0)?))),
        "cut_to_width" => Ok(Json::Str(term::cut_to_width(
            arg_str(args, 0)?,
            arg_int(args, 1)?,
        ))),
        "tail_to_width" => Ok(Json::Str(term::tail_to_width(
            arg_str(args, 0)?,
            arg_int(args, 1)?,
        ))),
        "flat_frames" => {
            let frames = arg_str_arr(args, 0)?;
            let color = arg_color(args, 1)?;
            Ok(frames_to_json(&term::flat_frames(&frames, color)))
        }
        "render_cells" => Ok(Json::Str(term::render_cells(&arg_cells(args, 0)?))),
        "plain_cells" => Ok(Json::Str(term::plain_cells(&arg_cells(args, 0)?))),
        "fail_sentence" => Ok(Json::Str(exec::fail_sentence(
            arg_int(args, 0)?,
            arg_str(args, 1)?,
        ))),
        "sub_fail_reason" => {
            let err = opt_str(args, 0)?;
            Ok(str_or_null(exec::sub_fail_reason(err)))
        }
        "sub_success_lines" => {
            let codes = arg_str_arr(args, 0)?;
            let (verdict, miss) = exec::sub_success_lines(&codes);
            Ok(Json::Arr(vec![Json::Str(verdict), Json::Str(miss)]))
        }
        "sub_fail_lines" => {
            let err = opt_str(args, 0)?;
            Ok(str_arr(exec::sub_fail_lines(err)))
        }
        "version_tuple" => {
            let v = opt_str(args, 0)?;
            Ok(Json::Arr(
                update::version_tuple(v)
                    .into_iter()
                    .map(Json::Int)
                    .collect(),
            ))
        }
        "parse_sha256" => Ok(str_or_null(update::parse_sha256(arg_str(args, 0)?))),
        "pick_checksum" => Ok(str_or_null(update::pick_checksum(
            arg_str(args, 0)?,
            arg_str(args, 1)?,
        ))),
        "menu_line" => Ok(Json::Str(ui::menu_line(
            arg_int(args, 0)?,
            arg_str(args, 1)?,
            arg_str(args, 2)?,
            arg_bool(args, 3)?,
            arg_bool(args, 4)?,
        ))),
        "short_path" => Ok(Json::Str(ui::short_path(
            arg_str(args, 0)?,
            arg_int(args, 1)?,
            arg_bool(args, 2)?,
        ))),
        "sub_lang_lines" => {
            let width = match args.len() {
                0 => ui::WIDTH,
                1 => arg_int(args, 0)?,
                _ => return Err("sub_lang_lines takes at most one arg".to_string()),
            };
            Ok(str_arr(ui::sub_lang_lines(width)))
        }
        "batch_row" => Ok(Json::Str(ui::batch_row(
            arg_int(args, 0)?,
            arg_str(args, 1)?,
            arg_bool(args, 2)?,
        ))),
        "coerce_settings" => {
            let input = json_to_value(arg(args, 0, "settings object")?)?;
            let Value::Obj(mut obj) = input else {
                return Err("arg 0 must be a settings object".to_string());
            };
            let notes = config::coerce_settings(&mut obj);
            let mut args_after = Vec::with_capacity(args.len());
            for (i, a) in args.iter().enumerate() {
                if i == 0 {
                    args_after.push(value_to_json(&Value::Obj(std::mem::take(&mut obj)))?);
                } else {
                    args_after.push(value_to_json(&json_to_value(a)?)?);
                }
            }
            Ok(Json::Obj(vec![
                ("args_after".to_string(), Json::Arr(args_after)),
                ("result".to_string(), str_arr(notes)),
            ]))
        }
        _ => Err(format!("unknown function: {name}")),
    }
}

fn arg<'a>(args: &'a [Json], i: usize, what: &str) -> Result<&'a Json, String> {
    args.get(i)
        .ok_or_else(|| format!("missing arg {i} ({what})"))
}

fn arg_str(args: &[Json], i: usize) -> Result<&str, String> {
    match arg(args, i, "string")? {
        Json::Str(s) => Ok(s.as_str()),
        other => Err(format!("arg {i} must be a string, got {other:?}")),
    }
}

fn arg_int(args: &[Json], i: usize) -> Result<i64, String> {
    match arg(args, i, "int")? {
        Json::Int(n) => Ok(*n),
        other => Err(format!("arg {i} must be an int, got {other:?}")),
    }
}

/// A color: int in 0..=255, the range the frames use.
fn arg_color(args: &[Json], i: usize) -> Result<u8, String> {
    let n = arg_int(args, i)?;
    u8::try_from(n).map_err(|_| format!("arg {i} color {n} outside 0..=255"))
}

fn arg_bool(args: &[Json], i: usize) -> Result<bool, String> {
    match arg(args, i, "bool")? {
        Json::Bool(b) => Ok(*b),
        other => Err(format!("arg {i} must be a bool, got {other:?}")),
    }
}

/// Null reads as None (python `text or ""`), a string as itself.
fn opt_str(args: &[Json], i: usize) -> Result<Option<&str>, String> {
    match arg(args, i, "string")? {
        Json::Null => Ok(None),
        Json::Str(s) => Ok(Some(s.as_str())),
        other => Err(format!("arg {i} must be a string or null, got {other:?}")),
    }
}

fn arg_str_arr(args: &[Json], i: usize) -> Result<Vec<String>, String> {
    match arg(args, i, "array of strings")? {
        Json::Arr(items) => items
            .iter()
            .map(|item| match item {
                Json::Str(s) => Ok(s.clone()),
                other => Err(format!("arg {i} items must be strings, got {other:?}")),
            })
            .collect(),
        other => Err(format!(
            "arg {i} must be an array of strings, got {other:?}"
        )),
    }
}

/// A cell frame: array of [text, color] pairs.
fn arg_cells(args: &[Json], i: usize) -> Result<Vec<(String, u8)>, String> {
    match arg(args, i, "cell frame")? {
        Json::Arr(items) => items
            .iter()
            .map(|item| match item {
                Json::Arr(pair) if pair.len() == 2 => {
                    let (Json::Str(text), Json::Int(col)) = (&pair[0], &pair[1]) else {
                        return Err(format!("arg {i} cells must be [string, int]"));
                    };
                    let col = u8::try_from(*col)
                        .map_err(|_| format!("arg {i} color {col} outside 0..=255"))?;
                    Ok((text.clone(), col))
                }
                other => Err(format!(
                    "arg {i} cells must be [string, int], got {other:?}"
                )),
            })
            .collect(),
        other => Err(format!("arg {i} must be a cell frame, got {other:?}")),
    }
}

fn str_or_null(v: Option<String>) -> Json {
    match v {
        Some(s) => Json::Str(s),
        None => Json::Null,
    }
}

fn str_arr(items: Vec<String>) -> Json {
    Json::Arr(items.into_iter().map(Json::Str).collect())
}

fn frames_to_json(frames: &[Vec<(String, u8)>]) -> Json {
    Json::Arr(
        frames
            .iter()
            .map(|frame| {
                Json::Arr(
                    frame
                        .iter()
                        .map(|(text, col)| {
                            Json::Arr(vec![Json::Str(text.clone()), Json::Int(i64::from(*col))])
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

/// JSON -> settings value. Floats cannot appear (the harness protocol and
/// its parser carry integers only).
fn json_to_value(j: &Json) -> Result<Value, String> {
    Ok(match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Int(n) => Value::Int(*n),
        Json::Str(s) => Value::Str(s.clone()),
        Json::Arr(items) => Value::Arr(
            items
                .iter()
                .map(json_to_value)
                .collect::<Result<Vec<Value>, String>>()?,
        ),
        Json::Obj(fields) => Value::Obj(
            fields
                .iter()
                .map(|(k, v)| Ok((k.clone(), json_to_value(v)?)))
                .collect::<Result<Obj, String>>()?,
        ),
    })
}

/// Settings value -> JSON. A float is outside the interchange format
/// (json.rs); reaching this is an instrument problem, not a verdict.
fn value_to_json(v: &Value) -> Result<Json, String> {
    Ok(match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(n) => Json::Int(*n),
        Value::Float(_) => {
            return Err("float value outside the harness protocol".to_string());
        }
        Value::Str(s) => Json::Str(s.clone()),
        Value::Arr(items) => Json::Arr(
            items
                .iter()
                .map(value_to_json)
                .collect::<Result<Vec<Json>, String>>()?,
        ),
        Value::Obj(fields) => Json::Obj(
            fields
                .iter()
                .map(|(k, v)| Ok((k.clone(), value_to_json(v)?)))
                .collect::<Result<Vec<(String, Json)>, String>>()?,
        ),
    })
}
