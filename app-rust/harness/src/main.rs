//! Differential harness dispatcher for the ytdlp native port.
//!
//! Not shipped: the bundle carries the product binary only. Usage:
//!
//!     ytdlp-harness <function> --vectors <file.json>
//!
//! Reads a vector file `{"fn": "...", "cases": [{"args": [...]}]}` and
//! prints one canonical JSON line per case (CPython json.dumps format,
//! see json.rs). The arms live in dispatch.rs; instrument errors exit 2
//! with one stderr line. The python side and the differ live in
//! tools/harness.py.

mod dispatch;
mod json;

use json::Json;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("ytdlp-harness: {msg}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let fn_name = args
        .next()
        .ok_or("usage: ytdlp-harness <function> --vectors <file.json>")?;
    let mut vectors_path: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--vectors" => {
                vectors_path = Some(args.next().ok_or("--vectors needs a path")?);
            }
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    let vectors_path = vectors_path.ok_or("--vectors <file.json> is required")?;

    let text = fs::read_to_string(&vectors_path)
        .map_err(|e| format!("cannot read {vectors_path}: {e}"))?;
    let root = json::parse(&text).map_err(|e| format!("{vectors_path}: {e}"))?;

    let Json::Obj(fields) = &root else {
        return Err(format!("{vectors_path}: top level must be an object"));
    };
    let file_fn = match get(fields, "fn") {
        Some(Json::Str(s)) => s.as_str(),
        _ => return Err(format!("{vectors_path}: missing string field \"fn\"")),
    };
    if file_fn != fn_name {
        return Err(format!(
            "{vectors_path} is for \"{file_fn}\", requested \"{fn_name}\""
        ));
    }
    let cases = match get(fields, "cases") {
        Some(Json::Arr(items)) => items,
        _ => return Err(format!("{vectors_path}: missing array field \"cases\"")),
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut buf: Vec<u8> = Vec::with_capacity(256);
    for (i, case) in cases.iter().enumerate() {
        let case_fields = match case {
            Json::Obj(f) => f,
            _ => return Err(format!("{vectors_path}: case {i} must be an object")),
        };
        let args = match get(case_fields, "args") {
            Some(Json::Arr(items)) => items.as_slice(),
            _ => {
                return Err(format!(
                    "{vectors_path}: case {i} missing array field \"args\""
                ));
            }
        };
        let value = dispatch::dispatch(&fn_name, args).map_err(|e| format!("case {i}: {e}"))?;
        buf.clear();
        json::write_canonical(&value, &mut buf);
        buf.push(b'\n');
        out.write_all(&buf).map_err(|e| format!("stdout: {e}"))?;
    }
    out.flush().map_err(|e| format!("stdout: {e}"))?;
    Ok(())
}

fn get<'a>(fields: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}
