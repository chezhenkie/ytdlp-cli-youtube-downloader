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
//! (PY_CALL_HOOKS). Phase-4 cases carry their fixtures in the case
//! object; each side materializes them in its own scratch dir and
//! projects that dir as `{scratch}` in the record.
//!
//! pick_spinner has no arm on purpose: it returns a random draw, so a
//! byte comparison is impossible; its twins live in cli/src/term.rs.

use crate::json::Json;
use std::cell::RefCell;
use std::rc::Rc;
use ytdlp_cli::cli_args;
use ytdlp_cli::config::{Obj, Value};
use ytdlp_cli::menu::{self, MenuErr};
use ytdlp_cli::resolve::{self, ShapeKind};

use ytdlp_cli::net::HttpRt;
use ytdlp_cli::tty::{self, ScriptItem};
use ytdlp_cli::{archive, args, config, exec, net, sha256, term, ui, update};

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
        // Phase 3, state layer: the case carries the fixture, the arm
        // materializes it in a private scratch dir and drives the state
        // functions over it. Outcome tokens are port-domain shapes
        // (payloads stay shape-gated, RUST-PORT-PLAN.md port rules).
        "load_settings" => {
            let case = arg(args, 0, "settings case")?;
            let dir = TempCaseDir::new("load")?;
            let rel = field_str(case, "path")?;
            let target = dir.path.join(&rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("scratch setup: {e}"))?;
            }
            if let Some(text) = field_opt_str(case, "seed_text")? {
                std::fs::write(&target, text.as_bytes())
                    .map_err(|e| format!("scratch seed: {e}"))?;
            } else if let Some(hex) = field_opt_str(case, "seed_hex")? {
                std::fs::write(&target, hex_decode(hex)?)
                    .map_err(|e| format!("scratch seed: {e}"))?;
            }
            let (load, settings, notes) = match config::load_settings(&target) {
                Ok(config::LoadOutcome::Loaded { settings, notes }) => {
                    ("ok", Some(settings), notes)
                }
                Ok(config::LoadOutcome::Unreadable { settings, .. }) => {
                    ("unreadable", Some(settings), Vec::new())
                }
                Err(e) => (
                    match e.class_name {
                        "TypeError" => "type_error",
                        "AttributeError" => "attribute_error",
                        other => return Err(format!("unexpected shape class {other}")),
                    },
                    None,
                    Vec::new(),
                ),
            };
            let settings_file = match &settings {
                Some(s) => Json::Str(config::render_settings_file(s)),
                None => Json::Null,
            };
            let file_after = match std::fs::read(&target) {
                Ok(bytes) => Json::Str(hex_encode(&bytes)),
                Err(_) => Json::Null,
            };
            Ok(Json::Obj(vec![
                ("load".to_string(), Json::Str(load.to_string())),
                ("settings_file".to_string(), settings_file),
                ("notes".to_string(), str_arr(notes)),
                ("file_after_hex".to_string(), file_after),
            ]))
        }
        "save_settings" => {
            let case = arg(args, 0, "settings case")?;
            let dir = TempCaseDir::new("save")?;
            let rel = field_str(case, "path")?;
            let target = dir.path.join(&rel);
            if field_opt_bool(case, "parents")?.unwrap_or(true)
                && let Some(parent) = target.parent()
            {
                std::fs::create_dir_all(parent).map_err(|e| format!("scratch setup: {e}"))?;
            }
            if field_opt_bool(case, "block_parent")?.unwrap_or(false)
                && let Some(parent) = target.parent()
            {
                std::fs::write(parent, b"blocked").map_err(|e| format!("scratch setup: {e}"))?;
            }
            if let Some(text) = field_opt_str(case, "seed_text")? {
                std::fs::write(&target, text.as_bytes())
                    .map_err(|e| format!("scratch seed: {e}"))?;
            } else if let Some(hex) = field_opt_str(case, "seed_hex")? {
                std::fs::write(&target, hex_decode(hex)?)
                    .map_err(|e| format!("scratch seed: {e}"))?;
            }
            let sentinel = field_opt_bool(case, "sentinel")?.unwrap_or(false);
            if sentinel {
                std::fs::write(target.with_extension("tmp"), SENTINEL)
                    .map_err(|e| format!("scratch sentinel: {e}"))?;
            }
            let settings_json =
                obj_field(case, "settings").ok_or("case needs a settings object")?;
            let Value::Obj(settings) = json_to_value(settings_json)? else {
                return Err("the settings field must be an object".to_string());
            };
            let save = match config::save_settings(&target, &settings) {
                Ok(()) => "ok",
                Err(config::SaveError::NotUtf8) => "not_utf8",
                Err(config::SaveError::Io(_)) => "io_error",
            };
            let skipped = if sentinel {
                let tmp = target.with_extension("tmp");
                match std::fs::read(&tmp) {
                    Ok(bytes) => Json::Bool(bytes == SENTINEL),
                    Err(_) => Json::Bool(false),
                }
            } else {
                Json::Null
            };
            let (file, file_hex) = match std::fs::read(&target) {
                Ok(bytes) => (
                    String::from_utf8(bytes.clone())
                        .map(Json::Str)
                        .unwrap_or(Json::Null),
                    Json::Str(hex_encode(&bytes)),
                ),
                Err(_) => (Json::Null, Json::Null),
            };
            Ok(Json::Obj(vec![
                ("save".to_string(), Json::Str(save.to_string())),
                ("file".to_string(), file),
                ("file_after_hex".to_string(), file_hex),
                ("skipped".to_string(), skipped),
            ]))
        }
        // Phase 4, resolution and diagnostics: cases carry their world
        // (fixtures, injected env/registry/which/probe state); each side
        // materializes it in a private scratch dir and projects that dir
        // as {scratch} in the record.
        "platform_name" => {
            let os_name = arg_str(args, 0)?;
            let sys_platform = arg_str(args, 1)?;
            Ok(Json::Str(resolve::platform_name(os_name, sys_platform)))
        }
        "path_render" => Ok(Json::Str(resolve::path_render(arg_str(args, 0)?))),
        "path_is_absolute" => Ok(Json::Bool(resolve::path_is_absolute(arg_str(args, 0)?))),
        "splitroot" => {
            let (drv, root, tail) = resolve::splitroot(arg_str(args, 0)?);
            Ok(Json::Arr(vec![
                Json::Str(drv),
                Json::Str(root),
                Json::Str(tail),
            ]))
        }
        "expandvars" => {
            let case = arg(args, 0, "expandvars case")?;
            let text = field_str(case, "text")?;
            let env = env_map_from_json(case, None)?;
            Ok(Json::Str(resolve::expandvars(&text, &env)))
        }
        "which" => {
            let case = arg(args, 0, "which case")?;
            let dir = TempCaseDir::new("which")?;
            materialize_files(&dir, obj_field(case, "files"))?;
            let cmd = resolve_scratch(&field_str(case, "cmd")?, &dir);
            let entries = entries_str(case, &dir)?;
            let pathext = field_opt_str(case, "pathext")?.map(str::to_string);
            let found = resolve::which(&cmd, Some(&entries), pathext.as_deref());
            Ok(match found {
                Some(p) => Json::Str(project(&dir, &p)),
                None => Json::Null,
            })
        }
        "find_yt_dlp" => {
            let case = arg(args, 0, "find_yt_dlp case")?;
            let dir = TempCaseDir::new("ytdlp")?;
            materialize_files(&dir, obj_field(case, "files"))?;
            let saved = settings_from_case(case)?;
            let os_name = field_str(case, "os_name")?;
            let sys_platform = field_str(case, "sys_platform")?;
            let platform = resolve::platform_name(&os_name, &sys_platform);
            let bin_dir = resolve::py_path_join(&resolve_scratch("{scratch}", &dir), "bin");
            let mut which_calls: Vec<String> = Vec::new();
            let result = {
                let mut which = map_which_closure(&dir, obj_field(case, "which"), &mut which_calls);
                resolve::find_yt_dlp(&saved, &bin_dir, &platform, &mut which)
            };
            let (found, shape) = shape_record(result.map(|p| p.map(|s| project(&dir, &s))));
            Ok(Json::Obj(vec![
                ("found".to_string(), found),
                (
                    "shape".to_string(),
                    shape
                        .map(|s| Json::Str(s.to_string()))
                        .unwrap_or(Json::Null),
                ),
                ("which_calls".to_string(), str_arr(which_calls)),
            ]))
        }
        "find_ffmpeg_dir" => {
            let case = arg(args, 0, "find_ffmpeg_dir case")?;
            let dir = TempCaseDir::new("ffmpeg")?;
            materialize_files(&dir, obj_field(case, "files"))?;
            let saved = settings_from_case(case)?;
            let is_windows = field_str(case, "os_name")? == "nt";
            let bin_dir = resolve::py_path_join(&resolve_scratch("{scratch}", &dir), "bin");
            let mut which_calls: Vec<String> = Vec::new();
            let result = {
                let mut which = map_which_closure(&dir, obj_field(case, "which"), &mut which_calls);
                resolve::find_ffmpeg_dir(&saved, &bin_dir, is_windows, &mut which)
            };
            let (found, shape) = shape_record(result.map(|p| p.map(|s| project(&dir, &s))));
            Ok(Json::Obj(vec![
                ("found".to_string(), found),
                (
                    "shape".to_string(),
                    shape
                        .map(|s| Json::Str(s.to_string()))
                        .unwrap_or(Json::Null),
                ),
                ("which_calls".to_string(), str_arr(which_calls)),
            ]))
        }
        "probe_version" => {
            let case = arg(args, 0, "probe_version case")?;
            let calls = match obj_field(case, "calls") {
                Some(Json::Arr(items)) => items,
                _ => return Err("case needs a calls array".to_string()),
            };
            let prompts: Vec<Json> = match obj_field(case, "spawns") {
                Some(Json::Arr(items)) => items.clone(),
                _ => return Err("case needs a spawns array".to_string()),
            };
            let mut spawn_log: Vec<String> = Vec::new();
            let mut queue = prompts.into_iter();
            let mut cache = resolve::VersionCache::default();
            let mut results: Vec<Json> = Vec::new();
            for call in calls {
                let binary = field_str(call, "binary")?;
                let mtime = match obj_field(call, "mtime") {
                    None | Some(Json::Null) => None,
                    Some(Json::Float(x)) => Some(*x),
                    Some(Json::Int(n)) => Some(*n as f64),
                    Some(other) => {
                        return Err(format!("mtime must be a number or null: {other:?}"));
                    }
                };
                let mut spawn =
                    |_binary: &str, flag: &str| -> Result<resolve::SpawnOutcome, String> {
                        spawn_log.push(flag.to_string());
                        match queue.next() {
                            Some(item) => match obj_field(&item, "raise") {
                                Some(Json::Str(msg)) => Err(msg.clone()),
                                _ => Ok(resolve::SpawnOutcome {
                                    stdout: field_str(&item, "stdout").unwrap_or_default(),
                                    stderr: field_str(&item, "stderr").unwrap_or_default(),
                                    rc: obj_field(&item, "rc")
                                        .and_then(|v| match v {
                                            Json::Int(n) => Some(*n),
                                            _ => None,
                                        })
                                        .unwrap_or(0),
                                }),
                            },
                            None => Err("spawn queue exhausted".to_string()),
                        }
                    };
                let (line, rc) = resolve::probe_version(&binary, mtime, &mut cache, &mut spawn);
                results.push(Json::Arr(vec![Json::Str(line), Json::Int(rc)]));
            }
            let used = spawn_log.len();
            Ok(Json::Obj(vec![
                ("calls".to_string(), Json::Arr(results)),
                ("spawn_log".to_string(), str_arr(spawn_log)),
                ("spawns_used".to_string(), Json::Int(used as i64)),
            ]))
        }
        "auto_outdir" => {
            let case = arg(args, 0, "auto_outdir case")?;
            let dir = TempCaseDir::new("outdir")?;
            materialize_files(&dir, obj_field(case, "files"))?;
            let home = resolve::py_path_join(&resolve_scratch("{scratch}", &dir), "home");
            if let Some(text) = field_opt_str(case, "user_dirs")? {
                let target = resolve::py_path_join(&home, ".config/user-dirs.dirs");
                if let Some(parent) = std::path::Path::new(&target).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("scratch setup: {e}"))?;
                }
                std::fs::write(&target, text.as_bytes())
                    .map_err(|e| format!("scratch seed: {e}"))?;
            }
            if let Some(hex) = field_opt_str(case, "user_dirs_hex")? {
                let target = resolve::py_path_join(&home, ".config/user-dirs.dirs");
                if let Some(parent) = std::path::Path::new(&target).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("scratch setup: {e}"))?;
                }
                std::fs::write(&target, hex_decode(hex)?)
                    .map_err(|e| format!("scratch seed: {e}"))?;
            }
            let mut env = env_map_from_json(case, Some(&dir))?;
            env.push(("HOME".to_string(), home.clone()));
            env.push(("USERPROFILE".to_string(), home.clone()));
            let is_windows = field_str(case, "os_name")? == "nt";
            let win = obj_field(case, "win");
            let win_value = |key: &str| -> Result<Option<String>, String> {
                match win.and_then(|w| obj_field(w, key)) {
                    None | Some(Json::Null) => Ok(None),
                    Some(Json::Str(s)) => {
                        let resolved = resolve_scratch(s, &dir);
                        Ok(if resolved.is_empty() {
                            None
                        } else {
                            Some(resolved)
                        })
                    }
                    Some(other) => Err(format!("win.{key} must be a string or null: {other:?}")),
                }
            };
            let win_desktop = win_value("Desktop")?;
            let win_downloads = win_value("{374DE290-123F-4565-9164-39C4925E467B}")?;
            let n_calls = match obj_field(case, "calls") {
                Some(Json::Int(n)) => *n,
                _ => 2,
            };
            let mut cache = resolve::OutdirCache::default();
            let mut lookups: i64 = 0;
            let mut results: Vec<Json> = Vec::new();
            let mut error: Option<&'static str> = None;
            for _ in 0..n_calls {
                let outcome = cache.get(|| {
                    lookups += 1;
                    resolve::auto_outdir_lookup(
                        is_windows,
                        win_desktop.as_deref(),
                        win_downloads.as_deref(),
                        &home,
                        &env,
                    )
                });
                match outcome {
                    Ok(Some(path)) => results.push(Json::Str(project(&dir, &path))),
                    Ok(None) => results.push(Json::Null),
                    Err(shape) => {
                        error = Some(shape.token());
                        break;
                    }
                }
            }
            Ok(Json::Obj(vec![
                (
                    "error".to_string(),
                    error
                        .map(|s| Json::Str(s.to_string()))
                        .unwrap_or(Json::Null),
                ),
                ("lookups".to_string(), Json::Int(lookups)),
                (
                    "results".to_string(),
                    if error.is_some() {
                        Json::Null
                    } else {
                        Json::Arr(results)
                    },
                ),
            ]))
        }
        "resolve_outdir" => {
            let case = arg(args, 0, "resolve_outdir case")?;
            let dir = TempCaseDir::new("resolve")?;
            let mut saved: Obj = Vec::new();
            if let Some(v) = obj_field(case, "output_dir") {
                saved.push(("output_dir".to_string(), json_to_value(v)?));
            }
            let root = resolve_scratch("{scratch}/root", &dir);
            let auto = field_opt_str(case, "auto")?.map(|rel| resolve_scratch(rel, &dir));
            let result = resolve::resolve_outdir(&saved, auto.as_deref(), &root);
            let (outdir, shape) = match result {
                Ok(s) => (Json::Str(project(&dir, &s)), None),
                Err(shape) => (Json::Null, Some(shape)),
            };
            Ok(Json::Obj(vec![
                ("outdir".to_string(), outdir),
                (
                    "shape".to_string(),
                    shape
                        .map(|s| Json::Str(s.token().to_string()))
                        .unwrap_or(Json::Null),
                ),
            ]))
        }
        "show_check" => {
            let case = arg(args, 0, "show_check case")?;
            let dir = TempCaseDir::new("check")?;
            materialize_files(&dir, obj_field(case, "files"))?;
            let scratch_root = resolve_scratch("{scratch}", &dir);
            let root = resolve::py_path_join(&scratch_root, "root");
            let bin_dir = resolve::py_path_join(&root, "bin");
            let config_path = resolve::py_path_join(
                &resolve::py_path_join(&scratch_root, "config"),
                "settings.json",
            );
            let config_exists = if let Some(text) = field_opt_str(case, "config_text")? {
                if let Some(parent) = std::path::Path::new(&config_path).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("scratch setup: {e}"))?;
                }
                std::fs::write(&config_path, text.as_bytes())
                    .map_err(|e| format!("scratch seed: {e}"))?;
                true
            } else {
                false
            };
            let home = resolve::py_path_join(&scratch_root, "home");
            if let Some(text) = field_opt_str(case, "user_dirs")? {
                let target = resolve::py_path_join(&home, ".config/user-dirs.dirs");
                if let Some(parent) = std::path::Path::new(&target).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("scratch setup: {e}"))?;
                }
                std::fs::write(&target, text.as_bytes())
                    .map_err(|e| format!("scratch seed: {e}"))?;
            }
            let mut env = env_map_from_json(case, Some(&dir))?;
            env.push(("HOME".to_string(), home.clone()));
            env.push(("USERPROFILE".to_string(), home.clone()));
            let os_name = field_str(case, "os_name")?;
            let sys_platform = field_str(case, "sys_platform")?;
            let platform = resolve::platform_name(&os_name, &sys_platform);
            let is_windows = os_name == "nt";
            let win = obj_field(case, "win");
            let win_value = |key: &str| -> Result<Option<String>, String> {
                match win.and_then(|w| obj_field(w, key)) {
                    None | Some(Json::Null) => Ok(None),
                    Some(Json::Str(s)) => {
                        let resolved = resolve_scratch(s, &dir);
                        Ok(if resolved.is_empty() {
                            None
                        } else {
                            Some(resolved)
                        })
                    }
                    Some(other) => Err(format!("win.{key} must be a string or null: {other:?}")),
                }
            };
            let win_desktop = win_value("Desktop")?;
            let win_downloads = win_value("{374DE290-123F-4565-9164-39C4925E467B}")?;
            let saved = settings_from_case(case)?;
            let world = resolve::CheckWorld {
                platform: &platform,
                root: &root,
                bin_dir: &bin_dir,
                config_path: &config_path,
                config_exists,
                saved: &saved,
                is_windows,
                home: &home,
                env: &env,
                win_desktop: win_desktop.as_deref(),
                win_downloads: win_downloads.as_deref(),
            };
            let mut which_calls: Vec<String> = Vec::new();
            let probe_case = obj_field(case, "probe").cloned();
            let mut probe_calls: Vec<String> = Vec::new();
            let lines = {
                let mut which = map_which_closure(&dir, obj_field(case, "which"), &mut which_calls);
                let mut probe = |binary: &str| -> (String, i64) {
                    probe_calls.push(project(&dir, binary));
                    match &probe_case {
                        Some(p) => (
                            field_str(p, "version").unwrap_or_default(),
                            obj_field(p, "rc")
                                .and_then(|v| match v {
                                    Json::Int(n) => Some(*n),
                                    _ => None,
                                })
                                .unwrap_or(0),
                        ),
                        None => ("no version output".to_string(), -1),
                    }
                };
                resolve::show_check(&world, &mut which, &mut probe).map_err(|e| {
                    format!("show_check shape {}: python would raise here", e.token())
                })?
            };
            let mut text = lines.join("\n");
            text.push('\n');
            Ok(Json::Obj(vec![
                ("probe_calls".to_string(), str_arr(probe_calls)),
                ("stdout".to_string(), Json::Str(project(&dir, &text))),
                ("which_calls".to_string(), str_arr(which_calls)),
            ]))
        }
        // Phase 5, argument construction: the case carries the settings
        // object, the url, the ffmpeg answer, the auto-outdir answer and
        // the which map (node on PATH); each side materializes its own
        // scratch dir and records the argv element list (scratch
        // projected), the shape token, or the which call log.
        "output_args" => {
            let case = arg(args, 0, "output_args case")?;
            let dir = TempCaseDir::new("outargs")?;
            materialize_files(&dir, obj_field(case, "files"))?;
            let s = obj_from_field(case, "s")?;
            let root = resolve_scratch("{scratch}/root", &dir);
            let auto = field_opt_str(case, "auto")?
                .map(|rel| resolve_scratch(rel, &dir))
                .filter(|s| !s.is_empty());
            let result = args::output_args(&s, &root, auto.as_deref());
            Ok(argv_record(result, &dir, None))
        }
        "build_args" => {
            let case = arg(args, 0, "build_args case")?;
            let dir = TempCaseDir::new("buildargs")?;
            materialize_files(&dir, obj_field(case, "files"))?;
            let s = obj_from_field(case, "s")?;
            let url = field_str(case, "url")?;
            let root = resolve_scratch("{scratch}/root", &dir);
            let ffdir = case_dir_from_field(case, "ffdir", &dir)?;
            let auto = field_opt_str(case, "auto")?
                .map(|rel| resolve_scratch(rel, &dir))
                .filter(|s| !s.is_empty());
            let mut which_calls: Vec<String> = Vec::new();
            let result = {
                let mut which = map_which_closure(&dir, obj_field(case, "which"), &mut which_calls);
                args::build_args(
                    &s,
                    &url,
                    ffdir.as_deref(),
                    &root,
                    auto.as_deref(),
                    &mut || which("node").is_some(),
                )
            };
            Ok(argv_record(result, &dir, Some(which_calls)))
        }
        "build_sub_args" => {
            let case = arg(args, 0, "build_sub_args case")?;
            let dir = TempCaseDir::new("subargs")?;
            materialize_files(&dir, obj_field(case, "files"))?;
            let s = obj_from_field(case, "s")?;
            let url = field_str(case, "url")?;
            let root = resolve_scratch("{scratch}/root", &dir);
            let ffdir = case_dir_from_field(case, "ffdir", &dir)?;
            let auto = field_opt_str(case, "auto")?
                .map(|rel| resolve_scratch(rel, &dir))
                .filter(|s| !s.is_empty());
            let mut which_calls: Vec<String> = Vec::new();
            let result = {
                let mut which = map_which_closure(&dir, obj_field(case, "which"), &mut which_calls);
                args::build_sub_args(
                    &s,
                    &url,
                    ffdir.as_deref(),
                    &root,
                    auto.as_deref(),
                    &mut || which("node").is_some(),
                )
            };
            Ok(argv_record(result, &dir, Some(which_calls)))
        }
        // Phase 6, terminal layer: the menu arms drive the menu through
        // the recording sink and a scripted reader. Cases carry the
        // settings object and the world (root implied, auto, color, tty
        // flags, terminal size, is_windows, version); each side
        // materializes its own scratch dir and records project it as
        // {scratch}.
        "menu_summary" => {
            let s = value_obj(arg(args, 0, "settings object")?)?;
            match ui::menu_summary(&s) {
                Ok(text) => Ok(str_shape(Some(text), None)),
                Err(shape) => Ok(str_shape(None, Some(shape))),
            }
        }
        "menu_items" => {
            let case = arg(args, 0, "menu_items case")?;
            let dir = TempCaseDir::new("menu-items")?;
            let s = obj_from_field(case, "s")?;
            let auto = case_auto(case, &dir)?;
            let root = case_root(&dir);
            let is_windows = field_opt_bool(case, "is_windows")?.unwrap_or(false);
            let outdir = match resolve::resolve_outdir(&s, auto.as_deref(), &root) {
                Ok(outdir) => outdir,
                Err(shape) => return Ok(rows_shape(None, Some(shape))),
            };
            match ui::menu_items(&s, &outdir, is_windows) {
                Ok(rows) => {
                    let rows: Vec<Json> = rows
                        .into_iter()
                        .map(|row| {
                            Json::Arr(vec![
                                Json::Int(row.idx),
                                Json::Str(row.label),
                                Json::Str(project(&dir, &row.value)),
                                Json::Bool(row.grey),
                            ])
                        })
                        .collect();
                    Ok(rows_shape(Some(rows), None))
                }
                Err(shape) => Ok(rows_shape(None, Some(shape))),
            }
        }
        "header" => {
            let version = arg_str(args, 0)?;
            let use_color = arg_bool(args, 1)?;
            let lines = ui::header_lines(version, use_color);
            Ok(Json::Obj(vec![(
                "stdout".to_string(),
                Json::Str(transcript(&lines)),
            )]))
        }
        "hint" => {
            let case = arg(args, 0, "hint case")?;
            let dir = TempCaseDir::new("hint")?;
            let s = obj_from_field(case, "s")?;
            let auto = case_auto(case, &dir)?;
            let root = case_root(&dir);
            let use_color = field_opt_bool(case, "use_color")?.unwrap_or(false);
            let outdir = resolve::resolve_outdir(&s, auto.as_deref(), &root).map_err(|shape| {
                format!("hint shape {}: python would raise here", shape.token())
            })?;
            let lines = ui::hint_lines(&outdir, use_color);
            Ok(Json::Obj(vec![(
                "stdout".to_string(),
                Json::Str(project(&dir, &transcript(&lines))),
            )]))
        }
        "render" => {
            let case = arg(args, 0, "render case")?;
            let dir = TempCaseDir::new("render")?;
            let s = obj_from_field(case, "s")?;
            let auto = case_auto(case, &dir)?;
            let root = case_root(&dir);
            let version = field_str(case, "version")?;
            let is_windows = field_opt_bool(case, "is_windows")?.unwrap_or(false);
            let use_color = field_opt_bool(case, "use_color")?.unwrap_or(false);
            let outdir = match resolve::resolve_outdir(&s, auto.as_deref(), &root) {
                Ok(outdir) => outdir,
                Err(shape) => {
                    return Ok(Json::Obj(vec![
                        ("stdout".to_string(), Json::Null),
                        ("shape".to_string(), Json::Str(shape.token().to_string())),
                    ]));
                }
            };
            match ui::render_lines(&version, &s, &outdir, is_windows, use_color) {
                Ok(lines) => Ok(Json::Obj(vec![
                    (
                        "stdout".to_string(),
                        Json::Str(project(&dir, &transcript(&lines))),
                    ),
                    ("shape".to_string(), Json::Null),
                ])),
                Err(shape) => Ok(Json::Obj(vec![
                    ("stdout".to_string(), Json::Null),
                    ("shape".to_string(), Json::Str(shape.token().to_string())),
                ])),
            }
        }
        "block_under_input" => {
            let block = arg_str_arr(args, 0)?;
            let use_color = arg_bool(args, 1)?;
            let stdin_tty = arg_bool(args, 2)?;
            let terminal = match arg(args, 3, "terminal size")? {
                Json::Null => None,
                Json::Arr(items) if items.len() == 2 => Some((
                    json_int(&items[0], "columns")?,
                    json_int(&items[1], "lines")?,
                )),
                other => return Err(format!("terminal must be null or [cols, lines]: {other:?}")),
            };
            let result = tty::block_under_input(&block, use_color, stdin_tty, terminal);
            Ok(Json::Obj(vec![("result".to_string(), Json::Bool(result))]))
        }
        "read_esc_piped" => {
            let prompt = opt_str(args, 0)?.unwrap_or("");
            let below = match arg(args, 1, "below block")? {
                Json::Null => None,
                Json::Arr(items) => {
                    let mut lines = Vec::with_capacity(items.len());
                    for item in items {
                        let Json::Str(line) = item else {
                            return Err(format!("below lines must be strings: {item:?}"));
                        };
                        lines.push(line.clone());
                    }
                    Some(lines)
                }
                other => return Err(format!("below must be null or an array: {other:?}")),
            };
            let stdin_text = arg_str(args, 2)?;
            let mut source = tty::LineSource::from_bytes(stdin_text.as_bytes());
            let mut rec = tty::RecordingOut::default();
            let outcome = tty::read_esc_piped(prompt, below.as_deref(), &mut source, &mut rec);
            let (line, eof) = match outcome {
                tty::ReadOutcome::Line(text) => (Json::Str(text), Json::Bool(false)),
                _ => (Json::Null, Json::Bool(true)),
            };
            Ok(Json::Obj(vec![
                ("stdout".to_string(), Json::Str(rec.text())),
                ("line".to_string(), line),
                ("eof".to_string(), eof),
            ]))
        }
        "line_edit" => {
            let case = arg(args, 0, "line_edit case")?;
            let prompt = field_opt_str(case, "prompt")?.unwrap_or("> ");
            let mut history = tty::History::new();
            for item in str_array_field(case, "history")? {
                history.entries.push(item);
            }
            let ops = match obj_field(case, "ops") {
                Some(Json::Arr(items)) => items,
                _ => return Err("case needs an ops array".to_string()),
            };
            let mut ed = tty::LineEdit::new(prompt, &history);
            let mut rec = tty::RecordingOut::default();
            let mut snapshots: Vec<String> = Vec::new();
            for op in ops {
                match field_str(op, "op")?.as_str() {
                    "add" => {
                        let ch = field_str(op, "ch")?;
                        let Some(ch) = ch.chars().next() else {
                            return Err("add needs one char".to_string());
                        };
                        ed.add(ch, &mut rec);
                    }
                    "back" => ed.back(&mut rec),
                    "recall" => {
                        let older = field_opt_bool(op, "older")?.unwrap_or(true);
                        ed.recall(older, &history, &mut rec);
                    }
                    "remember" => ed.remember(&mut history),
                    "text" => {}
                    other => return Err(format!("unknown line_edit op {other:?}")),
                }
                snapshots.push(ed.text());
            }
            Ok(Json::Obj(vec![
                ("stdout".to_string(), Json::Str(rec.text())),
                ("snapshots".to_string(), str_arr(snapshots)),
                ("history_after".to_string(), str_arr(history.entries)),
            ]))
        }
        "pick_list" => {
            let case = arg(args, 0, "pick_list case")?;
            let world_dir = TempCaseDir::new("pick")?;
            let title = field_str(case, "title")?;
            let options = str_array_field(case, "options")?;
            let current = obj_field(case, "current").cloned();
            let current_value = match &current {
                Some(j) => Some(json_to_value(j)?),
                None => None,
            };
            let use_color = field_opt_bool(case, "use_color")?.unwrap_or(false);
            let mut rec = tty::RecordingOut::default();
            let mut reader = scripted_reader(case)?;
            let world = menu::World {
                version: "1.5.0",
                root: &case_root(&world_dir),
                config_path: "",
                is_windows: false,
                use_color,
                stdin_tty: false,
                stdout_tty: false,
                terminal: None,
                auto: None,
            };
            let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
            let outcome = {
                let mut io = menu::Io {
                    out: &mut rec,
                    reader: &mut reader,
                };
                match menu::pick_list(
                    &title,
                    &option_refs,
                    current_value.as_ref(),
                    &mut io,
                    &world,
                ) {
                    Ok(None) => "kept".to_string(),
                    Ok(Some(Value::Str(text))) => format!("picked:{text}"),
                    Ok(Some(other)) => {
                        return Err(format!("pick_list returned {other:?}, port bug"));
                    }
                    Err(MenuErr::GoBack) => "goback".to_string(),
                    Err(MenuErr::Eof) => "eof".to_string(),
                    Err(MenuErr::Interrupted) => "interrupted".to_string(),
                    Err(other) => return Err(format!("pick_list error {other:?}")),
                }
            };
            Ok(Json::Obj(vec![
                ("stdout".to_string(), Json::Str(rec.text())),
                ("outcome".to_string(), Json::Str(outcome)),
                (
                    "prompts".to_string(),
                    str_arr(
                        reader
                            .prompts
                            .iter()
                            .map(|p| project(&world_dir, p))
                            .collect(),
                    ),
                ),
            ]))
        }
        "ask_bool" => {
            let case = arg(args, 0, "ask_bool case")?;
            let label = field_str(case, "label")?;
            let current = obj_field(case, "current").cloned();
            let current_value = match &current {
                Some(j) => Some(json_to_value(j)?),
                None => None,
            };
            let mut rec = tty::RecordingOut::default();
            let mut reader = scripted_reader(case)?;
            let outcome = {
                let mut io = menu::Io {
                    out: &mut rec,
                    reader: &mut reader,
                };
                match menu::ask_bool(&label, current_value.as_ref(), &mut io) {
                    Ok(Some(Value::Bool(true))) => "true".to_string(),
                    Ok(Some(Value::Bool(false))) => "false".to_string(),
                    Ok(None) => match current_value.as_ref() {
                        Some(Value::Bool(true)) => "true".to_string(),
                        Some(Value::Bool(false)) => "false".to_string(),
                        _ => "other".to_string(),
                    },
                    Ok(Some(other)) => {
                        return Err(format!("ask_bool returned {other:?}, port bug"));
                    }
                    Err(MenuErr::GoBack) => "goback".to_string(),
                    Err(MenuErr::Eof) => "eof".to_string(),
                    Err(MenuErr::Interrupted) => "interrupted".to_string(),
                    Err(other) => return Err(format!("ask_bool error {other:?}")),
                }
            };
            Ok(Json::Obj(vec![
                ("stdout".to_string(), Json::Str(rec.text())),
                ("outcome".to_string(), Json::Str(outcome)),
                ("prompts".to_string(), str_arr(reader.prompts)),
            ]))
        }
        "act" | "main_menu" => {
            let case = arg(args, 0, "menu case")?;
            let dir = TempCaseDir::new("menu")?;
            let mut s = obj_from_field(case, "settings")?;
            let root = case_root(&dir);
            let auto = case_auto(case, &dir)?;
            let version = field_str(case, "version")?;
            // pathlib-rendered like python's work / "config" / "settings.json"
            // (windows backslashes), so the bye line matches on every OS
            let config_path = resolve::py_path_join(&root, "config/settings.json");
            if let Some(parent) = std::path::Path::new(&config_path).parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("scratch setup: {e}"))?;
            }
            std::fs::write(&config_path, config::render_settings_file(&s).as_bytes())
                .map_err(|e| format!("scratch seed: {e}"))?;
            let world = menu::World {
                version: &version,
                root: &root,
                config_path: &config_path,
                is_windows: field_opt_bool(case, "is_windows")?.unwrap_or(false),
                use_color: field_opt_bool(case, "use_color")?.unwrap_or(false),
                stdin_tty: field_opt_bool(case, "stdin_tty")?.unwrap_or(false),
                stdout_tty: false,
                terminal: case_terminal(case)?,
                auto: auto.as_deref(),
            };
            let mut rec = tty::RecordingOut::default();
            let mut reader = scripted_reader(case)?;
            let mut download = |_s: &mut Obj, _url: &str, _io: &mut menu::Io| -> i32 { 0 };
            // the phase-8 updater pair: the harness drives act and
            // main_menu, not the network, so both stay inert closures
            // unless a case scripts them (see the update arms)
            let mut update = |_s: &mut Obj, _io: &mut menu::Io| -> Result<(), MenuErr> { Ok(()) };
            let mut quiet_check =
                |_s: &mut Obj, _io: &mut menu::Io| -> Result<(), MenuErr> { Ok(()) };
            let (outcome, exit) = {
                let mut io = menu::Io {
                    out: &mut rec,
                    reader: &mut reader,
                };
                if name == "act" {
                    let choice = field_str(case, "choice")?;
                    match menu::act(&choice, &mut s, &mut io, &world, &mut download, &mut update) {
                        Ok(menu::ActOutcome::Repaint(true)) => {
                            ("repaint_true".to_string(), Json::Null)
                        }
                        Ok(menu::ActOutcome::Repaint(false)) => {
                            ("repaint_false".to_string(), Json::Null)
                        }
                        Ok(menu::ActOutcome::Exit) => ("exit".to_string(), Json::Null),
                        Err(MenuErr::GoBack) => ("goback".to_string(), Json::Null),
                        Err(MenuErr::Eof) => ("eof".to_string(), Json::Null),
                        Err(MenuErr::Interrupted) => ("interrupted".to_string(), Json::Null),
                        Err(other) => return Err(format!("act error {other:?}")),
                    }
                } else {
                    match menu::main_menu(
                        &mut s,
                        &mut io,
                        &world,
                        &mut download,
                        &mut update,
                        &mut quiet_check,
                    ) {
                        Ok(code) => ("return".to_string(), Json::Int(code as i64)),
                        Err(MenuErr::Eof) => ("eof".to_string(), Json::Null),
                        Err(MenuErr::Interrupted) => ("interrupted".to_string(), Json::Null),
                        Err(other) => return Err(format!("main_menu error {other:?}")),
                    }
                }
            };
            let settings_after = value_to_json(&Value::Obj(project_obj(&dir, &s)))?;
            let settings_file = std::fs::read_to_string(&config_path)
                .ok()
                .map(|text| project(&dir, &text));
            Ok(Json::Obj(vec![
                ("stdout".to_string(), Json::Str(project(&dir, &rec.text()))),
                ("outcome".to_string(), Json::Str(outcome)),
                ("exit".to_string(), exit),
                ("settings_after".to_string(), settings_after),
                ("settings_file".to_string(), str_or_null(settings_file)),
                (
                    "prompts".to_string(),
                    str_arr(reader.prompts.iter().map(|p| project(&dir, p)).collect()),
                ),
            ]))
        }
        // ------------------------------------ phase 8: network, updaters
        //
        // Every arm runs offline: the network is a scripted seam, the
        // archives are built here from the case's member list (so a vector
        // carries no bytes and both sides read the same shape), and the
        // records project the scratch dir, the temp-dir suffix, the product
        // version and the update source - the two lines the native port is
        // documented to spell differently (FR-13).
        "http_text" | "http_file" => {
            let case = arg(args, 0, "network case")?;
            let dir = TempCaseDir::new("http")?;
            let url = field_str(case, "url")?;
            let mut http = HttpScript::from_case(case, "url")?;
            if name == "http_text" {
                let timeout = case_int_or(case, "timeout", net::FORCED_TIMEOUT as i64)?;
                let (ok, text, error) = match http.text(&url, timeout as u64) {
                    Ok(text) => (true, Json::Str(text), Json::Null),
                    Err(e) => (false, Json::Null, Json::Str(project(&dir, &e))),
                };
                return Ok(Json::Obj(vec![
                    ("ok".to_string(), Json::Bool(ok)),
                    ("text".to_string(), text),
                    ("error".to_string(), error),
                    ("calls".to_string(), http.call_log()),
                ]));
            }
            let dest = resolve_scratch(&field_str(case, "dest")?, &dir);
            mkdir_parent(&dest)?;
            let with_stage = field_opt_bool(case, "stage")?.unwrap_or(false);
            let timeout = case_int_or(case, "timeout", net::DL_TIMEOUT as i64)?;
            let mut live: Option<String> = None;
            let result = if with_stage {
                let mut report = |text: &str| live = Some(text.to_string());
                http.file(&url, &dest, timeout as u64, Some(&mut report))
            } else {
                http.file(&url, &dest, timeout as u64, None)
            };
            let part = format!("{dest}.part");
            let (ok, got, final_url, total, error) = match result {
                Ok(out) => (
                    true,
                    Json::Int(out.bytes),
                    Json::Str(project(&dir, &out.final_url)),
                    Json::Int(out.content_length),
                    Json::Null,
                ),
                Err(e) => (
                    false,
                    Json::Null,
                    Json::Null,
                    Json::Null,
                    Json::Str(project(&dir, &e)),
                ),
            };
            Ok(Json::Obj(vec![
                ("ok".to_string(), Json::Bool(ok)),
                ("error".to_string(), error),
                ("got".to_string(), got),
                ("final".to_string(), final_url),
                ("total".to_string(), total),
                (
                    "live".to_string(),
                    str_or_null(live.map(|t| project(&dir, &t))),
                ),
                ("dest".to_string(), Json::Str(project(&dir, &dest))),
                ("dest_hex".to_string(), file_hex(&dest)),
                (
                    "part_exists".to_string(),
                    Json::Bool(std::path::Path::new(&part).exists()),
                ),
                ("part_hex".to_string(), file_hex(&part)),
                ("calls".to_string(), http.call_log()),
            ]))
        }
        "sha256_file" => {
            let case = arg(args, 0, "sha256_file case")?;
            let dir = TempCaseDir::new("sha")?;
            let path = resolve_scratch(&field_str(case, "path")?, &dir);
            if field_opt_bool(case, "missing")?.unwrap_or(false) {
                return Ok(Json::Obj(vec![
                    ("ok".to_string(), Json::Bool(false)),
                    ("digest".to_string(), Json::Null),
                ]));
            }
            mkdir_parent(&path)?;
            std::fs::write(&path, body_bytes(case)?).map_err(|e| format!("scratch write: {e}"))?;
            let digest = sha256::sha256_file(&path).map_err(|e| format!("sha256_file: {e}"))?;
            Ok(Json::Obj(vec![
                ("ok".to_string(), Json::Bool(true)),
                ("digest".to_string(), Json::Str(digest)),
            ]))
        }
        "expected_sha" => {
            let case = arg(args, 0, "expected_sha case")?;
            let rec = tty::RecordingOut::default();
            let sink = tty::SharedOut::new(rec.clone()).sink();
            let mut http = HttpScript::from_case(case, "sha_url")?;
            let digest = update::expected_sha(
                &field_str(case, "url")?,
                &field_str(case, "sha_url")?,
                &field_str(case, "asset")?,
                &mut http,
                &sink,
            );
            Ok(Json::Obj(vec![
                ("digest".to_string(), str_or_null(digest)),
                ("stdout".to_string(), Json::Str(rec.text())),
                ("calls".to_string(), http.call_log()),
            ]))
        }
        "extract_member" => {
            let case = arg(args, 0, "extract_member case")?;
            let dir = TempCaseDir::new("archive")?;
            let how = field_str(case, "how")?;
            let binname = field_str(case, "binname")?;
            let blob = build_archive(
                &dir,
                obj_field(case, "archive").ok_or("case needs an archive")?,
            )?;
            let outdir = resolve_scratch(&field_str(case, "outdir")?, &dir);
            std::fs::create_dir_all(&outdir).map_err(|e| format!("scratch mkdir: {e}"))?;
            let rec = tty::RecordingOut::default();
            let sink = tty::SharedOut::new(rec.clone()).sink();
            let result = archive::extract_member(&blob, &how, &binname, &outdir, &sink);
            let listed = listing(&outdir, &dir);
            Ok(Json::Obj(vec![
                (
                    "result".to_string(),
                    str_or_null(result.map(|path| project_install(&dir, &path))),
                ),
                (
                    "stdout".to_string(),
                    Json::Str(project_unreadable(&rec.text())),
                ),
                ("listing".to_string(), listed),
            ]))
        }
        "install_ffmpeg" => {
            let case = arg(args, 0, "install_ffmpeg case")?;
            let dir = TempCaseDir::new("ffmpeg")?;
            let platform = field_str(case, "platform")?;
            let bin_dir = resolve_scratch(
                field_opt_str(case, "bin_dir")?.unwrap_or("{scratch}/bin"),
                &dir,
            );
            std::fs::create_dir_all(&bin_dir).map_err(|e| format!("scratch mkdir: {e}"))?;
            let blob = build_archive(
                &dir,
                obj_field(case, "archive").ok_or("case needs an archive")?,
            )?;
            let digest = sha256::sha256_file(&blob).map_err(|e| format!("sha256_file: {e}"))?;
            // the scripted provider: its checksum file and its archive
            let mut http = HttpScript::new();
            http.sends_agent = false;
            if let Some(source) = exec::ffmpeg_source(&platform) {
                let asset = source.url.rsplit('/').next().unwrap_or("").to_string();
                http.add_text(source.sha_url, &format!("{digest}  {asset}\n"));
                http.add_file(source.url, std::fs::read(&blob).unwrap_or_default());
                if let Some(err) = field_opt_str(case, "sha_error")? {
                    http.text_error = Some(err.to_string());
                }
                if let Some(err) = field_opt_str(case, "download_error")? {
                    http.file_error = Some(err.to_string());
                    http.error_after = None;
                }
            }
            let probes = probe_queue(case)?;
            let probed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let queue = Rc::new(RefCell::new(probes));
            let mut probe = arm_probe(probed.clone(), queue);
            let slept: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
            let mut sleep = arm_sleep(slept.clone());
            let rec = tty::RecordingOut::default();
            let sink = tty::SharedOut::new(rec.clone()).sink();
            let mut out_rec = rec.clone();
            let mut reader = tty::ScriptedReader::new(Vec::new());
            let root = dir.path.to_string_lossy().into_owned();
            // python's `if os.name != "nt"` is the host, not the platform
            // the case installs for
            let is_windows = cfg!(windows);
            let mut which = |_: &str| None;
            let mut ctx = exec::DownloadCtx {
                root: &root,
                is_windows,
                use_color: false,
                stdin_tty: true,
                sink,
                out: &mut out_rec,
                reader: &mut reader,
                auto: None,
                http: &mut http,
                probe: &mut probe,
                which: &mut which,
                sleep: &mut sleep,
            };
            let result = exec::install_ffmpeg(None, &platform, &bin_dir, is_windows, &mut ctx);
            let listed = listing(&bin_dir, &dir);
            let calls = http.call_log();
            Ok(Json::Obj(vec![
                (
                    "result".to_string(),
                    str_or_null(result.map(|path| project_install(&dir, &path))),
                ),
                (
                    "stdout".to_string(),
                    Json::Str(project_unreadable(&project_install(
                        &dir,
                        &project_product(&rec.text()),
                    ))),
                ),
                (
                    "probes".to_string(),
                    str_arr(
                        probed
                            .borrow()
                            .iter()
                            .map(|path| project_install(&dir, path))
                            .collect(),
                    ),
                ),
                (
                    "sleeps".to_string(),
                    str_arr(
                        slept
                            .borrow()
                            .iter()
                            .map(|secs| format!("{secs:.1}"))
                            .collect(),
                    ),
                ),
                ("listing".to_string(), listed),
                ("calls".to_string(), calls),
            ]))
        }
        "update_yt_dlp" => {
            let case = arg(args, 0, "update_yt_dlp case")?;
            let dir = TempCaseDir::new("upd")?;
            let s = settings_from_case(case)?;
            exec::set_interrupt(false);
            let binary = seed_binary(case, &dir)?;
            let probes = probe_queue(case)?;
            let probed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let queue = Rc::new(RefCell::new(probes));
            let mut probe = arm_probe(probed.clone(), queue);
            let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
            let mut call = arm_call(calls.clone(), case)?;
            let rec = tty::RecordingOut::default();
            let sink = tty::SharedOut::new(rec.clone()).sink();
            let mut out_rec = rec.clone();
            let mut reader = tty::ScriptedReader::new(Vec::new());
            let answer = binary.clone();
            let mut which = move |_: &str| answer.clone();
            let mut http = HttpScript::new();
            let platform = ARM_PLATFORM.to_string();
            let root = dir.path.to_string_lossy().into_owned();
            let mut selftest = |_: &str| -1;
            let mut clock = || 0.0;
            let self_path = format!("{root}/ytdlp");
            std::fs::write(&self_path, b"the running image")
                .map_err(|e| format!("scratch write: {e}"))?;
            let mut ctx = updater_ctx(
                &root,
                &platform,
                &self_path,
                true,
                sink,
                &mut http,
                &mut probe,
                &mut selftest,
                &mut call,
                &mut which,
                &mut clock,
                &mut out_rec,
                &mut reader,
            );
            let result = update::update_yt_dlp(&s, &mut ctx);
            Ok(Json::Obj(vec![
                ("result".to_string(), Json::Bool(result)),
                ("stdout".to_string(), Json::Str(project(&dir, &rec.text()))),
                (
                    "probes".to_string(),
                    str_arr(
                        probed
                            .borrow()
                            .iter()
                            .map(|path| project_install(&dir, path))
                            .collect(),
                    ),
                ),
                (
                    "calls".to_string(),
                    Json::Arr(
                        calls
                            .borrow()
                            .iter()
                            .map(|argv| {
                                str_arr(
                                    argv.iter()
                                        .map(|part| project_install(&dir, part))
                                        .collect(),
                                )
                            })
                            .collect(),
                    ),
                ),
            ]))
        }
        "auto_update_check" | "update_all" => {
            let case = arg(args, 0, "updater case")?;
            let dir = TempCaseDir::new("upd")?;
            let mut s = obj_from_field(case, "settings")?;
            exec::set_interrupt(false);
            let binary = seed_binary(case, &dir)?;
            let probes = probe_queue(case)?;
            let probed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let queue = Rc::new(RefCell::new(probes));
            let mut probe = arm_probe(probed.clone(), queue);
            let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
            let mut call = arm_call(calls.clone(), case)?;
            let mut reader = scripted_reader(case)?;
            let fetches: Rc<RefCell<Vec<i64>>> = Rc::new(RefCell::new(Vec::new()));
            let version = field_opt_str(case, "version")?.map(|v| v.to_string());
            let fetch_error = field_opt_str(case, "fetch_error")?.map(|v| v.to_string());
            let mut http = HttpScript::new();
            let fetch_log = Rc::clone(&fetches);
            http.answer = Some(Rc::new(move |_url: &str, timeout: u64| {
                fetch_log.borrow_mut().push(timeout as i64);
                match &fetch_error {
                    Some(e) => Err(e.clone()),
                    None => Ok(release_answer(version.as_deref())),
                }
            }));
            let rec = tty::RecordingOut::default();
            let sink = tty::SharedOut::new(rec.clone()).sink();
            let mut out_rec = rec.clone();
            let platform = ARM_PLATFORM.to_string();
            let root = dir.path.to_string_lossy().into_owned();
            let self_path = format!("{root}/ytdlp");
            std::fs::write(&self_path, b"the running image")
                .map_err(|e| format!("scratch write: {e}"))?;
            let answer = binary.clone();
            let mut which = move |_: &str| answer.clone();
            let mut selftest = |_: &str| -1;
            let can_ask = field_opt_bool(case, "can_ask")?.unwrap_or(true);
            let now = case_f64_or(case, "now", 0.0)?;
            let mut clock = move || now;
            let mut ctx = updater_ctx(
                &root,
                &platform,
                &self_path,
                can_ask,
                sink,
                &mut http,
                &mut probe,
                &mut selftest,
                &mut call,
                &mut which,
                &mut clock,
                &mut out_rec,
                &mut reader,
            );
            let (result, error) = if name == "update_all" {
                match update::update_all(&mut s, &mut ctx) {
                    Ok(moved) => (Json::Bool(moved), Json::Null),
                    Err(e) => (Json::Null, Json::Str(upd_err_token(&e))),
                }
            } else {
                let force = field_opt_bool(case, "force")?.unwrap_or(false);
                match update::auto_update_check(&mut s, force, &mut ctx) {
                    Ok(moved) => (Json::Bool(moved), Json::Null),
                    Err(e) => (Json::Null, Json::Str(upd_err_token(&e))),
                }
            };
            Ok(Json::Obj(vec![
                ("result".to_string(), result),
                ("error".to_string(), error),
                (
                    "stdout".to_string(),
                    Json::Str(project(&dir, &project_product(&rec.text()))),
                ),
                (
                    "fetches".to_string(),
                    Json::Arr(
                        fetches
                            .borrow()
                            .iter()
                            .map(|timeout| {
                                Json::Obj(vec![("timeout".to_string(), Json::Int(*timeout))])
                            })
                            .collect(),
                    ),
                ),
                ("installs".to_string(), Json::Arr(Vec::new())),
                (
                    "probes".to_string(),
                    str_arr(
                        probed
                            .borrow()
                            .iter()
                            .map(|path| project_install(&dir, path))
                            .collect(),
                    ),
                ),
                (
                    "prompts".to_string(),
                    str_arr(reader.prompts.iter().map(|p| project(&dir, p)).collect()),
                ),
                (
                    "settings_after".to_string(),
                    value_to_json(&Value::Obj(project_obj(&dir, &s)))?,
                ),
            ]))
        }
        "newest_since" | "clean_partials" => {
            let case = arg(args, 0, "download-support case")?;
            let dir = TempCaseDir::new("dlfs")?;
            materialize_files(&dir, obj_field(case, "files"))?;
            let folder = resolve_scratch(&field_str(case, "folder")?, &dir);
            // the gate is relative to each side's own clock, so the two
            // runs cannot disagree about a fixture's freshness
            let since = now_secs() + field_f64(case, "since_offset")?;
            let slack = field_f64(case, "slack")?;
            if name == "newest_since" {
                Ok(match exec::newest_since(&folder, since, slack) {
                    Some(path) => Json::Str(project(&dir, &path)),
                    None => Json::Null,
                })
            } else {
                let mut removed = exec::clean_partials(&folder, since, slack);
                removed.sort();
                Ok(str_arr(removed))
            }
        }
        "run_download" => {
            let case = arg(args, 0, "run_download case")?;
            let stdout = case_lines(case, "stdout")?;
            let stderr = case_lines(case, "stderr")?;
            let polls: std::collections::VecDeque<Option<i64>> =
                case_polls(case)?.into_iter().collect();
            let escapes: std::collections::VecDeque<bool> =
                case_bools(case, "escapes")?.into_iter().collect();
            let interrupt_after = match obj_field(case, "interrupt_after") {
                None | Some(Json::Null) => None,
                Some(Json::Int(n)) => Some(*n),
                Some(other) => {
                    return Err(format!("interrupt_after must be int or null: {other:?}"));
                }
            };
            let use_color = field_opt_bool(case, "use_color")?.unwrap_or(false);
            let with_stage = field_opt_bool(case, "stage")?.unwrap_or(false);
            let kill_calls = std::rc::Rc::new(std::cell::Cell::new(0i64));
            let kill_tick = std::rc::Rc::clone(&kill_calls);
            let sleeps = std::rc::Rc::new(std::cell::Cell::new(0i64));
            let stage = with_stage.then(|| {
                tty::Stage::new(
                    "downloading (escape to stop)",
                    Vec::new(),
                    0.0,
                    vec![(".".to_string(), 214u8)],
                    use_color,
                    std::sync::Arc::new(std::sync::Mutex::new(tty::StdoutOut::new()))
                        as tty::SharedSink,
                )
            });
            let (sleep_tick, sleep_check) = (std::rc::Rc::clone(&sleeps), sleeps);
            let mut rt = exec::RunRt {
                spawn: Box::new(move |_binary, _opts| {
                    Ok(exec::Spawned {
                        stdout: Box::new(std::io::Cursor::new(joined_lines(&stdout))),
                        stderr: Box::new(std::io::Cursor::new(joined_lines(&stderr))),
                        child: Box::new(ArmChild {
                            polls: polls.clone(),
                        }),
                    })
                }),
                watch: Box::new(move || {
                    Box::new(ArmWatch {
                        hits: escapes.clone(),
                    }) as Box<dyn exec::EscapeWatch>
                }),
                kill: Box::new(move |_child| kill_tick.set(kill_tick.get() + 1)),
                interrupted: Box::new(move || {
                    interrupt_after.is_some_and(|n| sleep_check.get() > n)
                }),
                sleep_frame: Box::new(move || sleep_tick.set(sleep_tick.get() + 1)),
                interrupt_scope: None,
            };
            let (rc, stopped, err, sub_codes) =
                exec::run_download("yt-dlp", &[], stage.as_ref(), use_color, &mut rt);
            // the live text is read after the pump threads joined
            let live = stage.as_ref().and_then(|s| s.live());
            Ok(Json::Obj(vec![
                ("rc".to_string(), Json::Int(rc)),
                ("stopped".to_string(), Json::Bool(stopped)),
                ("err".to_string(), Json::Str(err)),
                ("sub_codes".to_string(), str_arr(sub_codes)),
                ("live".to_string(), str_or_null(live)),
                ("kill_calls".to_string(), Json::Int(kill_calls.get())),
            ]))
        }
        "cli_args" => {
            let case = arg(args, 0, "cli_args case")?;
            let prog = field_str(case, "prog")?;
            let width = field_i64(case, "width")?;
            let argv: Vec<String> = match obj_field(case, "argv") {
                Some(Json::Arr(items)) => {
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        match item {
                            Json::Str(s) => out.push(s.clone()),
                            other => {
                                return Err(format!("argv elements must be strings: {other:?}"));
                            }
                        }
                    }
                    out
                }
                other => return Err(format!("argv must be an array: {other:?}")),
            };
            match cli_args::parse(&prog, &argv, width) {
                cli_args::Outcome::Help(text) => Ok(Json::Obj(vec![
                    ("kind".to_string(), Json::Str("help".to_string())),
                    ("exit".to_string(), Json::Int(0)),
                    ("text".to_string(), Json::Str(text)),
                ])),
                cli_args::Outcome::Error(text) => Ok(Json::Obj(vec![
                    ("kind".to_string(), Json::Str("error".to_string())),
                    ("exit".to_string(), Json::Int(2)),
                    ("text".to_string(), Json::Str(text)),
                ])),
                cli_args::Outcome::Parsed(inv) => Ok(Json::Obj(vec![
                    ("kind".to_string(), Json::Str("parsed".to_string())),
                    ("url".to_string(), str_or_null(inv.url)),
                    ("audio".to_string(), Json::Bool(inv.audio)),
                    ("format".to_string(), str_or_null(inv.format)),
                    ("resolution".to_string(), str_or_null(inv.resolution)),
                    ("af".to_string(), str_or_null(inv.af)),
                    ("outdir".to_string(), str_or_null(inv.outdir)),
                    ("template".to_string(), str_or_null(inv.template)),
                    ("subs".to_string(), Json::Bool(inv.subs)),
                    ("playlist".to_string(), Json::Bool(inv.playlist)),
                    ("menu".to_string(), Json::Bool(inv.menu)),
                    ("check".to_string(), Json::Bool(inv.check)),
                    ("update".to_string(), Json::Bool(inv.update)),
                    ("selftest".to_string(), Json::Bool(inv.selftest)),
                ])),
            }
        }
        _ => Err(format!("unknown function: {name}")),
    }
}

/// The run_download arm's scripted child handle.
struct ArmChild {
    polls: std::collections::VecDeque<Option<i64>>,
}

impl exec::ChildHandle for ArmChild {
    fn poll(&mut self) -> Option<i64> {
        self.polls.pop_front().unwrap_or(Some(0))
    }
    fn wait(&mut self) -> i64 {
        0
    }
    fn pid(&self) -> u32 {
        4242
    }
}

/// The run_download arm's scripted escape watch.
struct ArmWatch {
    hits: std::collections::VecDeque<bool>,
}

impl exec::EscapeWatch for ArmWatch {
    fn escape_hit(&mut self) -> bool {
        self.hits.pop_front().unwrap_or(false)
    }
}

/// Case lines joined the way python's line iteration yields them: one
/// `\n` per line, always terminated.
fn joined_lines(lines: &[String]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for line in lines {
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn case_lines(case: &Json, key: &str) -> Result<Vec<String>, String> {
    match obj_field(case, key) {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Arr(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Json::Str(s) => out.push(s.clone()),
                    other => return Err(format!("{key} lines must be strings: {other:?}")),
                }
            }
            Ok(out)
        }
        Some(other) => Err(format!("{key} must be an array: {other:?}")),
    }
}

fn case_polls(case: &Json) -> Result<Vec<Option<i64>>, String> {
    match obj_field(case, "polls") {
        Some(Json::Arr(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Json::Null => out.push(None),
                    Json::Int(n) => out.push(Some(*n)),
                    other => return Err(format!("polls must be int or null: {other:?}")),
                }
            }
            Ok(out)
        }
        other => Err(format!("polls must be an array: {other:?}")),
    }
}

fn case_bools(case: &Json, key: &str) -> Result<Vec<bool>, String> {
    match obj_field(case, key) {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Arr(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Json::Bool(b) => out.push(*b),
                    other => return Err(format!("{key} must be bools: {other:?}")),
                }
            }
            Ok(out)
        }
        Some(other) => Err(format!("{key} must be an array: {other:?}")),
    }
}

fn field_i64(case: &Json, key: &str) -> Result<i64, String> {
    match obj_field(case, key) {
        Some(Json::Int(n)) => Ok(*n),
        other => Err(format!("case needs an int field {key:?}: {other:?}")),
    }
}

fn field_f64(case: &Json, key: &str) -> Result<f64, String> {
    match obj_field(case, key) {
        Some(Json::Float(x)) => Ok(*x),
        Some(Json::Int(n)) => Ok(*n as f64),
        other => Err(format!("case needs a number field {key:?}: {other:?}")),
    }
}

/// The sentinel bytes the save cases plant at the tmp path; python plants
/// the same literal (tools/harness.py).
const SENTINEL: &[u8] = b"ytdlp-sentinel";

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

/// JSON -> settings value.
fn json_to_value(j: &Json) -> Result<Value, String> {
    Ok(match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Int(n) => Value::Int(*n),
        Json::Float(x) => Value::Float(*x),
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

fn obj_field<'a>(j: &'a Json, key: &str) -> Option<&'a Json> {
    match j {
        Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn field_str(case: &Json, key: &str) -> Result<String, String> {
    match obj_field(case, key) {
        Some(Json::Str(s)) => Ok(s.clone()),
        Some(other) => Err(format!("field {key:?} must be a string, got {other:?}")),
        None => Err(format!("case needs a string field {key:?}")),
    }
}

fn field_opt_str<'a>(case: &'a Json, key: &str) -> Result<Option<&'a str>, String> {
    match obj_field(case, key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(s)) => Ok(Some(s.as_str())),
        Some(other) => Err(format!(
            "field {key:?} must be a string or null, got {other:?}"
        )),
    }
}

fn field_opt_bool(case: &Json, key: &str) -> Result<Option<bool>, String> {
    match obj_field(case, key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(format!("field {key:?} must be a bool, got {other:?}")),
    }
}

fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("hex payload must have an even length".to_string());
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        let hi = hex_val(pair[0])?;
        let lo = hex_val(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_val(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!("bad hex digit {c:?}")),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xF) as usize] as char);
    }
    out
}

/// Per-case scratch directory, removed on drop, so a failed case (even a
/// panic) cannot leave fixture files behind.
struct TempCaseDir {
    path: std::path::PathBuf,
}

impl TempCaseDir {
    fn new(kind: &str) -> Result<Self, String> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("ytdlp-harness-{kind}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|e| format!("scratch dir: {e}"))?;
        Ok(TempCaseDir { path })
    }
}

impl Drop for TempCaseDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ------------------------------------------------- phase 8 helpers
//
// The scripted network, the archive builder and the small projections the
// network and updater arms share. Nothing here dials: an answer that is not
// scripted is an error, exactly like the python hooks.

// the platform and arch the updater arms run as; both sides resolve the
// same bare-binary asset name from them
const ARM_PLATFORM: &str = "linux";
const ARM_ARCH: &str = "x86_64";

/// The scripted answer for a text call: `(url, timeout) -> body`.
type AnswerHook = Rc<dyn Fn(&str, u64) -> Result<String, String>>;

/// A scripted `HttpRt`. `answer` wins over the per-url tables, so one arm
/// can drive the releases API while another drives a provider.
struct HttpScript {
    texts: Vec<(String, String)>,
    files: Vec<(String, Vec<u8>)>,
    /// fails before any byte is read (urlopen itself raised)
    open_error: Option<String>,
    /// the text call fails with this text
    text_error: Option<String>,
    /// the file call fails with this text, after `error_after` reads
    file_error: Option<String>,
    error_after: Option<usize>,
    /// false when the answer comes from a replaced helper rather than a
    /// real request, so no User-Agent was ever sent
    sends_agent: bool,
    content_length: Option<i64>,
    final_url: Option<String>,
    /// `Some` answers every text call from the closure, whatever the url
    answer: Option<AnswerHook>,
    calls: Vec<Json>,
}

impl HttpScript {
    fn new() -> Self {
        Self {
            texts: Vec::new(),
            files: Vec::new(),
            open_error: None,
            text_error: None,
            file_error: None,
            error_after: None,
            sends_agent: true,
            content_length: None,
            final_url: None,
            answer: None,
            calls: Vec::new(),
        }
    }

    fn add_text(&mut self, url: &str, body: &str) -> &mut Self {
        self.texts.push((url.to_string(), body.to_string()));
        self
    }

    fn add_file(&mut self, url: &str, body: Vec<u8>) -> &mut Self {
        self.files.push((url.to_string(), body));
        self
    }

    /// The http_text / http_file / expected_sha arms: one answer keyed on
    /// `key` (the url the call under test reads), and the failure shapes
    /// both sides print.
    fn from_case(case: &Json, key: &str) -> Result<Self, String> {
        let url = field_str(case, key)?;
        let mut script = Self::new();
        let body = body_bytes(case)?;
        if let Some(err) = field_opt_str(case, "open_error")? {
            script.open_error = Some(err.to_string());
        }
        if let Some(err) = field_opt_str(case, "error")? {
            script.text_error = Some(err.to_string());
            script.file_error = Some(err.to_string());
        }
        script.error_after = match obj_field(case, "error_after_reads") {
            Some(Json::Int(n)) => Some(*n as usize),
            _ => None,
        };
        script.content_length = match obj_field(case, "content_length") {
            // absent: the fake publishes the real length, like a real answer
            None => Some(body.len() as i64),
            // null: no header at all, so python's total stays 0
            Some(Json::Null) => None,
            Some(Json::Int(n)) => Some(*n),
            Some(other) => return Err(format!("content_length must be int or null: {other:?}")),
        };
        script.final_url = field_opt_str(case, "final_url")?.map(|v| v.to_string());
        let text = String::from_utf8_lossy(&body).into_owned();
        script.add_text(&url, &text);
        script.add_file(&url, body);
        Ok(script)
    }

    fn call_log(&self) -> Json {
        Json::Arr(self.calls.clone())
    }

    fn record(&mut self, kind: &str, url: &str, timeout: u64) {
        self.calls.push(Json::Obj(vec![
            ("kind".to_string(), Json::Str(kind.to_string())),
            ("url".to_string(), Json::Str(url.to_string())),
            ("timeout".to_string(), Json::Int(timeout as i64)),
            (
                "user_agent".to_string(),
                if self.sends_agent {
                    Json::Str(net::USER_AGENT.to_string())
                } else {
                    Json::Null
                },
            ),
        ]));
    }
}

impl net::HttpRt for HttpScript {
    fn text(&mut self, url: &str, timeout: u64) -> Result<String, String> {
        self.record("text", url, timeout);
        if let Some(answer) = &self.answer {
            return answer(url, timeout);
        }
        if let Some(err) = self.open_error.clone().or_else(|| self.text_error.clone()) {
            return Err(err);
        }
        self.texts
            .iter()
            .find(|(key, _)| key == url)
            .map(|(_, body)| body.clone())
            .ok_or_else(|| format!("no scripted text for {url}"))
    }

    fn file(
        &mut self,
        url: &str,
        dest: &str,
        timeout: u64,
        mut live: Option<&mut dyn FnMut(&str)>,
    ) -> Result<net::FileOutcome, String> {
        self.record("file", url, timeout);
        if let Some(err) = &self.open_error {
            return Err(err.clone());
        }
        let body = self
            .files
            .iter()
            .find(|(key, _)| key == url)
            .map(|(_, body)| body.clone())
            .ok_or_else(|| format!("no scripted file for {url}"))?;
        // python's staging and its read loop: the .part sibling, then
        // 64 KiB reads until the body runs out. the scripted failure
        // fires on a read number, so a mid-stream case lands the same
        // partial .part python leaves behind.
        let part = format!("{dest}.part");
        let mut written: i64 = 0;
        {
            let mut fh = std::fs::File::create(&part).map_err(|e| e.to_string())?;
            let mut cursor = 0usize;
            let mut reads = 0usize;
            loop {
                if self.file_error.is_some() && self.error_after.is_none_or(|after| reads >= after)
                {
                    return Err(self.file_error.clone().unwrap_or_default());
                }
                let end = (cursor + (1 << 16)).min(body.len());
                if end == cursor {
                    break;
                }
                std::io::Write::write_all(&mut fh, &body[cursor..end])
                    .map_err(|e| e.to_string())?;
                written += (end - cursor) as i64;
                cursor = end;
                reads += 1;
                if let Some(report) = live.as_deref_mut()
                    && let Some(total) = self.content_length
                    && total > 0
                {
                    report(&format!(
                        "downloading ffmpeg - {:.1} / {:.1} MB",
                        written as f64 / 1048576.0,
                        total as f64 / 1048576.0
                    ));
                }
            }
        }
        std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
        Ok(net::FileOutcome {
            bytes: written,
            final_url: self.final_url.clone().unwrap_or_else(|| url.to_string()),
            // python keeps 0 when the answer published no length
            content_length: self.content_length.unwrap_or(0),
        })
    }
}

/// The releases-API answer the updater arms script: the newest tag with a
/// bare-binary asset and its digest, so both sides reach the same gates.
fn release_answer(version: Option<&str>) -> String {
    let asset = update::release_asset(ARM_PLATFORM, ARM_ARCH).unwrap_or("");
    let digest = "0".repeat(64);
    let tag = match version {
        Some(v) => format!("\"tag_name\": \"v{v}\", "),
        // python's fetch_cli answers None when the published file carries
        // no version line, which is a missing key, not an empty string
        None => String::new(),
    };
    format!(
        "{{{tag}\"assets\": [{{\"name\": \"{asset}\", \
         \"browser_download_url\": \"https://example.invalid/{asset}\", \
         \"digest\": \"sha256:{digest}\"}}]}}"
    )
}

/// `tempfile.mkdtemp` and the port's mkdtemp both hide a suffix behind
/// `.ff-`; the record pins the shape, not the name.
fn project_rand(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(".ff-") {
        out.push_str(&rest[..at]);
        out.push_str(".ff-<rand>");
        let tail = &rest[at + 4..];
        let end = tail.find(['/', '\\']).unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn project_install(dir: &TempCaseDir, text: &str) -> String {
    project_rand(&project(dir, text))
}

/// The archive-fault payload is exception text, so it is shape-gated like
/// every other one: the line shape is the pin, the wording is each
/// library's own.
fn project_unreadable(text: &str) -> String {
    // the whole line goes: the payload is each library's own wording
    let prefix = "  the downloaded archive was unreadable: ";
    let token = "  the downloaded archive was unreadable: {archive-error}";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(prefix) {
        out.push_str(&rest[..at]);
        out.push_str(token);
        let tail = &rest[at + prefix.len()..];
        let end = tail.find('\n').unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The two lines the native port spells differently by design: the product
/// version and the update source (FR-13). Both are projected to tokens, so
/// the arm pins the line shapes and the spec pins the values.
fn project_product(text: &str) -> String {
    text.replace(
        &format!("this is v{}", ytdlp_cli::VERSION),
        "this is v{version}",
    )
    .replace(ytdlp_cli::VERSION, "{version}")
    .replace(update::RELEASES_PAGE, "{update-url}")
}

fn upd_err_token(err: &update::UpdErr) -> String {
    match err {
        update::UpdErr::Eof => "eof".to_string(),
        update::UpdErr::Interrupted => "interrupted".to_string(),
    }
}

/// `probes`: one `[version, rc]` per call, in order.
fn probe_queue(case: &Json) -> Result<std::collections::VecDeque<(String, i64)>, String> {
    let items = match obj_field(case, "probes") {
        None | Some(Json::Null) => return Ok(std::collections::VecDeque::new()),
        Some(Json::Arr(items)) => items,
        Some(other) => return Err(format!("probes must be an array: {other:?}")),
    };
    let mut queue = std::collections::VecDeque::new();
    for item in items {
        let Json::Arr(pair) = item else {
            return Err("each probe is [version, rc]".to_string());
        };
        if pair.len() != 2 {
            return Err("each probe is [version, rc]".to_string());
        }
        queue.push_back((str_or_empty(&pair[0]), json_int(&pair[1], "probe rc")?));
    }
    Ok(queue)
}

fn str_or_empty(value: &Json) -> String {
    match value {
        Json::Str(text) => text.clone(),
        Json::Null => String::new(),
        other => {
            let mut out = Vec::new();
            crate::json::write_canonical(other, &mut out);
            String::from_utf8_lossy(&out).into_owned()
        }
    }
}

fn arm_probe(
    log: Rc<RefCell<Vec<String>>>,
    queue: Rc<RefCell<std::collections::VecDeque<(String, i64)>>>,
) -> impl FnMut(&str) -> (String, i64) {
    move |binary: &str| {
        log.borrow_mut().push(binary.to_string());
        queue
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| (String::new(), -1))
    }
}

/// The retry waits, recorded as the floats python's repr would spell.
fn arm_sleep(slept: Rc<RefCell<Vec<f64>>>) -> impl FnMut(f64) {
    move |secs: f64| slept.borrow_mut().push(secs)
}

/// `subprocess.call([binary, "-U"])`: the argv is logged, the rc comes
/// from the case's queue.
fn arm_call(
    calls: Rc<RefCell<Vec<Vec<String>>>>,
    case: &Json,
) -> Result<impl FnMut(&str, &[String]) -> i64, String> {
    let rcs = RefCell::new(std::collections::VecDeque::from(case_int_list(
        case, "call_rcs",
    )?));
    let interrupt = field_opt_bool(case, "call_interrupt")?.unwrap_or(false);
    Ok(move |binary: &str, args: &[String]| {
        let mut argv = vec![binary.to_string()];
        argv.extend(args.iter().cloned());
        calls.borrow_mut().push(argv);
        if interrupt {
            // python raises KeyboardInterrupt here; the port reads its
            // own flag after the call
            exec::set_interrupt(true);
        }
        let mut queue = rcs.borrow_mut();
        if queue.is_empty() {
            0
        } else {
            queue.remove(0).unwrap_or(0)
        }
    })
}

fn case_int_list(case: &Json, key: &str) -> Result<Vec<i64>, String> {
    match obj_field(case, key) {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Arr(items)) => items.iter().map(|item| json_int(item, key)).collect(),
        Some(other) => Err(format!("{key} must be an array: {other:?}")),
    }
}

fn case_int_or(case: &Json, key: &str, fallback: i64) -> Result<i64, String> {
    match obj_field(case, key) {
        None | Some(Json::Null) => Ok(fallback),
        Some(value) => json_int(value, key),
    }
}

fn case_f64_or(case: &Json, key: &str, fallback: f64) -> Result<f64, String> {
    match obj_field(case, key) {
        None | Some(Json::Null) => Ok(fallback),
        Some(Json::Int(n)) => Ok(*n as f64),
        Some(Json::Float(x)) => Ok(*x),
        Some(other) => Err(format!("{key} must be a number: {other:?}")),
    }
}

/// The case's binary fixture, written so `find_yt_dlp` can resolve it.
fn seed_binary(case: &Json, dir: &TempCaseDir) -> Result<Option<String>, String> {
    let Some(rel) = field_opt_str(case, "binary")? else {
        return Ok(None);
    };
    let path = resolve_scratch(rel, dir);
    mkdir_parent(&path)?;
    std::fs::write(&path, b"yt-dlp").map_err(|e| format!("scratch write: {e}"))?;
    Ok(Some(path))
}

fn mkdir_parent(path: &str) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("scratch mkdir: {e}"))?;
    }
    Ok(())
}

/// The body a case carries: `chunks` (hex strings) or `text`.
fn body_bytes(case: &Json) -> Result<Vec<u8>, String> {
    if let Some(Json::Arr(chunks)) = obj_field(case, "chunks") {
        let mut out = Vec::new();
        for chunk in chunks {
            out.extend_from_slice(&hex_decode(&str_or_empty(chunk))?);
        }
        return Ok(out);
    }
    Ok(field_opt_str(case, "text")?
        .unwrap_or("")
        .as_bytes()
        .to_vec())
}

fn file_hex(path: &str) -> Json {
    match std::fs::read(path) {
        Ok(bytes) => Json::Str(hex_encode(&bytes)),
        Err(_) => Json::Null,
    }
}

/// `[{"name", "dir", "hex", "mode"}]` sorted, so a case can assert what a
/// run left on disk. The mode is posix-only: python's os.stat and rust's
/// PermissionsExt report different things on windows, so the record does
/// not ask there.
fn listing(folder: &str, dir: &TempCaseDir) -> Json {
    let mut names: Vec<(String, bool)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.path().is_dir();
            names.push((name, is_dir));
        }
    }
    names.sort();
    Json::Arr(
        names
            .into_iter()
            .map(|(name, is_dir)| {
                let path = resolve_scratch(&format!("{folder}/{name}"), dir);
                Json::Obj(vec![
                    ("name".to_string(), Json::Str(name)),
                    ("dir".to_string(), Json::Bool(is_dir)),
                    (
                        "hex".to_string(),
                        if is_dir { Json::Null } else { file_hex(&path) },
                    ),
                    (
                        "mode".to_string(),
                        if is_dir { Json::Null } else { file_mode(&path) },
                    ),
                ])
            })
            .collect(),
    )
}

fn file_mode(path: &str) -> Json {
    if cfg!(windows) {
        return Json::Null;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(meta) => Json::Str(format!("{:#o}", meta.permissions().mode() & 0o7777)),
            Err(_) => Json::Null,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Json::Null
    }
}

/// Materialise one archive from the case's member list. Each side builds
/// its own bytes from the same shape; the record never carries the archive.
fn build_archive(dir: &TempCaseDir, spec: &Json) -> Result<String, String> {
    let rel = field_str(spec, "path")?;
    let path = resolve_scratch(&rel, dir);
    mkdir_parent(&path)?;
    let members = match obj_field(spec, "members") {
        Some(Json::Arr(items)) => items,
        _ => return Err("archive members must be an array".to_string()),
    };
    let how = field_opt_str(spec, "how")?.unwrap_or("zip");
    let pairs: Vec<(String, Vec<u8>)> = members
        .iter()
        .map(|item| {
            let Json::Arr(fields) = item else {
                return Err("each archive member is [name, text]".to_string());
            };
            let name = str_or_empty(fields.first().unwrap_or(&Json::Null));
            let body = fields
                .get(1)
                .map(|v| str_or_empty(v).into_bytes())
                .unwrap_or_default();
            Ok((name, body))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if how == "zip" {
        use zip::write::SimpleFileOptions;
        use zip::{CompressionMethod, ZipWriter};
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            let mut writer = ZipWriter::new(&mut buf);
            for (name, body) in &pairs {
                writer.start_file(name.as_str(), options).map_err(zip_err)?;
                std::io::Write::write_all(&mut writer, body).map_err(io_err)?;
            }
            writer.finish().map_err(zip_err)?;
        }
        std::fs::write(&path, buf.into_inner()).map_err(|e| format!("archive write: {e}"))?;
    } else {
        let mut raw = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut raw);
            for (name, body) in &pairs {
                let mut header = tar::Header::new_gnu();
                header.set_size(body.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder
                    .append_data(&mut header, name.as_str(), body.as_slice())
                    .map_err(tar_err)?;
            }
            builder.finish().map_err(tar_err)?;
        }
        let mut out = Vec::new();
        let mut enc = lzma_rust2::XzWriter::new(&mut out, lzma_rust2::XzOptions::with_preset(1))
            .map_err(tar_err)?;
        std::io::Write::write_all(&mut enc, &raw).map_err(tar_err)?;
        enc.finish().map_err(tar_err)?;
        std::fs::write(&path, out).map_err(|e| format!("archive write: {e}"))?;
    }
    if let Some(keep) = field_opt_f64(spec, "truncate")? {
        let data = std::fs::read(&path).map_err(|e| format!("archive read: {e}"))?;
        let cut = ((data.len() as f64) * keep).max(1.0) as usize;
        std::fs::write(&path, &data[..cut.min(data.len())])
            .map_err(|e| format!("archive truncate: {e}"))?;
    }
    Ok(path)
}

fn zip_err(e: zip::result::ZipError) -> String {
    format!("archive: {e}")
}

fn io_err(e: std::io::Error) -> String {
    format!("archive: {e}")
}

fn tar_err(e: std::io::Error) -> String {
    format!("archive: {e}")
}

/// The updater context for one arm, with every seam a local.
#[allow(clippy::too_many_arguments)]
fn updater_ctx<'a>(
    root: &'a str,
    platform: &'a str,
    self_path: &'a str,
    stdin_tty: bool,
    sink: tty::SharedSink,
    http: &'a mut HttpScript,
    probe: &'a mut dyn FnMut(&str) -> (String, i64),
    selftest: &'a mut dyn FnMut(&str) -> i64,
    call: &'a mut dyn FnMut(&str, &[String]) -> i64,
    which: &'a mut dyn FnMut(&str) -> Option<String>,
    now: &'a mut dyn FnMut() -> f64,
    out: &'a mut tty::RecordingOut,
    reader: &'a mut tty::ScriptedReader,
) -> update::UpdateCtx<'a> {
    update::UpdateCtx {
        platform,
        arch: ARM_ARCH,
        self_path,
        root,
        is_windows: false,
        use_color: false,
        stdin_tty,
        sink,
        http,
        probe,
        selftest,
        call,
        now,
        which,
        out,
        reader,
    }
}

// ------------------------------------------------- phase 4 helpers

/// Replace the `{scratch}` token with this side's scratch dir.
fn resolve_scratch(text: &str, dir: &TempCaseDir) -> String {
    text.replace("{scratch}", &dir.path.to_string_lossy())
}

/// Replace this side's scratch dir with the `{scratch}` token.
fn project(dir: &TempCaseDir, text: &str) -> String {
    let needle = dir.path.to_string_lossy().to_string();
    text.replace(&needle, "{scratch}")
}

/// Create the case's fixture files in the scratch dir: directories,
/// content files, and posix exec bits (windows ignores `exec`).
fn materialize_files(dir: &TempCaseDir, files: Option<&Json>) -> Result<(), String> {
    let items = match files {
        None | Some(Json::Null) => return Ok(()),
        Some(Json::Arr(items)) => items,
        Some(other) => return Err(format!("files must be an array: {other:?}")),
    };
    for item in items {
        let rel = field_str(item, "path")?;
        let target = dir.path.join(&rel);
        if field_opt_bool(item, "dir")?.unwrap_or(false) {
            std::fs::create_dir_all(&target).map_err(|e| format!("scratch mkdir {rel}: {e}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("scratch mkdir {rel}: {e}"))?;
        }
        std::fs::write(&target, b"x").map_err(|e| format!("scratch write {rel}: {e}"))?;
        let exec = field_opt_bool(item, "exec")?.unwrap_or(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if exec { 0o755 } else { 0o644 };
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))
                .map_err(|e| format!("scratch chmod {rel}: {e}"))?;
        }
        #[cfg(not(unix))]
        let _ = exec;
        // the download-support arms backdate fixtures (mtime = now - age)
        if let Some(age) = field_opt_f64(item, "age_secs")? {
            let when = std::time::SystemTime::now()
                .checked_sub(std::time::Duration::from_secs_f64(age))
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if let Ok(handle) = std::fs::OpenOptions::new().write(true).open(&target) {
                let _ = handle.set_modified(when);
            }
        }
    }
    Ok(())
}

fn field_opt_f64(case: &Json, key: &str) -> Result<Option<f64>, String> {
    match obj_field(case, key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Float(x)) => Ok(Some(*x)),
        Some(Json::Int(n)) => Ok(Some(*n as f64)),
        other => Err(format!("{key} must be a number field: {other:?}")),
    }
}

/// The case's `saved` object as the settings value model.
fn settings_from_case(case: &Json) -> Result<Obj, String> {
    match obj_field(case, "saved") {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Obj(fields)) => fields
            .iter()
            .map(|(k, v)| Ok((k.clone(), json_to_value(v)?)))
            .collect(),
        Some(other) => Err(format!("saved must be an object or null: {other:?}")),
    }
}

/// The case's settings object under `key` (phase-5 argument cases use
/// "s" and it must be present).
fn obj_from_field(case: &Json, key: &str) -> Result<Obj, String> {
    match obj_field(case, key) {
        Some(Json::Obj(fields)) => fields
            .iter()
            .map(|(k, v)| Ok((k.clone(), json_to_value(v)?)))
            .collect(),
        Some(other) => Err(format!("{key} must be an object: {other:?}")),
        None => Err(format!("case needs an object field {key:?}")),
    }
}

/// A directory-valued case field (`ffdir`): absent/null/empty reads as
/// no-ffmpeg, everything else is scratch-resolved and pathlib-rendered
/// like python's `str(Path(...))` (windows backslash forms).
fn case_dir_from_field(
    case: &Json,
    key: &str,
    dir: &TempCaseDir,
) -> Result<Option<String>, String> {
    Ok(field_opt_str(case, key)?
        .map(|rel| resolve_scratch(rel, dir))
        .filter(|p| !p.is_empty())
        .map(|p| resolve::path_render(&p)))
}

/// The phase-5 argv record: `args` is the element list (scratch
/// projected) or null when the call took a shape branch; `shape` carries
/// the port-domain token; `which_calls` is present for the builder arms.
fn argv_record(
    result: Result<Vec<String>, ShapeKind>,
    dir: &TempCaseDir,
    which_calls: Option<Vec<String>>,
) -> Json {
    let (args, shape) = match result {
        Ok(argv) => (
            str_arr(argv.into_iter().map(|a| project(dir, &a)).collect()),
            Json::Null,
        ),
        Err(shape) => (Json::Null, Json::Str(shape.token().to_string())),
    };
    let mut fields = vec![("args".to_string(), args), ("shape".to_string(), shape)];
    if let Some(calls) = which_calls {
        fields.push(("which_calls".to_string(), str_arr(calls)));
    }
    Json::Obj(fields)
}

/// The case's `env` object, `{scratch}`-resolved when a dir is given.
fn env_map_from_json(case: &Json, dir: Option<&TempCaseDir>) -> Result<resolve::EnvMap, String> {
    let mut map: resolve::EnvMap = Vec::new();
    match obj_field(case, "env") {
        None | Some(Json::Null) => {}
        Some(Json::Obj(fields)) => {
            for (k, v) in fields {
                let Json::Str(s) = v else {
                    return Err(format!("env values must be strings, {k} is {v:?}"));
                };
                let value = match dir {
                    Some(d) => resolve_scratch(s, d),
                    None => s.clone(),
                };
                map.push((k.clone(), value));
            }
        }
        Some(other) => return Err(format!("env must be an object: {other:?}")),
    }
    Ok(map)
}

/// `os.pathsep`-joined, `{scratch}`-resolved PATH entries.
fn entries_str(case: &Json, dir: &TempCaseDir) -> Result<String, String> {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let items = match obj_field(case, "entries") {
        Some(Json::Arr(items)) => items,
        _ => return Err("case needs an entries array".to_string()),
    };
    let mut parts: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        let Json::Str(s) = item else {
            return Err(format!("entries must be strings, got {item:?}"));
        };
        parts.push(resolve_scratch(s, dir));
    }
    Ok(parts.join(sep))
}

/// A fake `shutil.which` driven by the case's answer map; records calls.
/// A falsy answer (`""`) reads as a miss, exactly like the python hook's
/// `if rel else None`.
fn map_which_closure<'a>(
    dir: &'a TempCaseDir,
    map: Option<&'a Json>,
    calls: &'a mut Vec<String>,
) -> impl FnMut(&str) -> Option<String> + 'a {
    move |name: &str| {
        calls.push(name.to_string());
        match map.and_then(|m| obj_field(m, name)) {
            Some(Json::Str(rel)) if !rel.is_empty() => Some(resolve_scratch(rel, dir)),
            _ => None,
        }
    }
}

/// Split a function result into the record pair (value, shape token).
fn shape_record(result: Result<Option<String>, ShapeKind>) -> (Json, Option<&'static str>) {
    match result {
        Ok(Some(p)) => (Json::Str(p), None),
        Ok(None) => (Json::Null, None),
        Err(s) => (Json::Null, Some(s.token())),
    }
}

// ------------------------------------------------- phase 6 helpers

/// The transcript text python's `print()`-per-line produces: lines joined
/// with LF plus one trailing LF (io.StringIO capture has no translation).
fn transcript(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn value_obj(j: &Json) -> Result<Obj, String> {
    match json_to_value(j)? {
        Value::Obj(obj) => Ok(obj),
        other => Err(format!("settings must be an object, got {other:?}")),
    }
}

/// The scratch dir itself is the case root (python binds ROOT the same
/// way), so every resolved path projects to `{scratch}`.
fn case_root(dir: &TempCaseDir) -> String {
    resolve_scratch("{scratch}", dir)
}

fn case_auto(case: &Json, dir: &TempCaseDir) -> Result<Option<String>, String> {
    Ok(field_opt_str(case, "auto")?
        .map(|rel| resolve_scratch(rel, dir))
        .filter(|s| !s.is_empty()))
}

fn case_terminal(case: &Json) -> Result<Option<(i64, i64)>, String> {
    match obj_field(case, "terminal") {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Arr(items)) if items.len() == 2 => Ok(Some((
            json_int(&items[0], "columns")?,
            json_int(&items[1], "lines")?,
        ))),
        Some(other) => Err(format!("terminal must be null or [cols, lines]: {other:?}")),
    }
}

fn json_int(j: &Json, what: &str) -> Result<i64, String> {
    match j {
        Json::Int(n) => Ok(*n),
        other => Err(format!("{what} must be an int, got {other:?}")),
    }
}

/// A string array case field (line_edit history, pick_list options).
fn str_array_field(case: &Json, key: &str) -> Result<Vec<String>, String> {
    match obj_field(case, key) {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Arr(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let Json::Str(text) = item else {
                    return Err(format!("{key} entries must be strings: {item:?}"));
                };
                out.push(text.clone());
            }
            Ok(out)
        }
        Some(other) => Err(format!("{key} must be an array: {other:?}")),
    }
}

/// The scripted reader replies: a string is a submitted line, null is
/// escape, {"eof": true} and {"ctrl_c": true} are the read failures.
/// The python hooks encode the same vocabulary.
fn scripted_reader(case: &Json) -> Result<tty::ScriptedReader, String> {
    let items = match obj_field(case, "replies") {
        Some(Json::Arr(items)) => items.clone(),
        None | Some(Json::Null) => Vec::new(),
        Some(other) => return Err(format!("replies must be an array: {other:?}")),
    };
    let mut replies = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Json::Str(line) => replies.push(ScriptItem::Line(line)),
            Json::Null => replies.push(ScriptItem::Escape),
            Json::Obj(_) => {
                let entry = &item;
                if field_opt_bool(entry, "eof")?.unwrap_or(false) {
                    replies.push(ScriptItem::Eof);
                } else if field_opt_bool(entry, "ctrl_c")?.unwrap_or(false) {
                    replies.push(ScriptItem::Interrupted);
                } else if field_opt_bool(entry, "esc")?.unwrap_or(false) {
                    replies.push(ScriptItem::Escape);
                } else {
                    return Err(format!("unknown reply object: {entry:?}"));
                }
            }
            other => return Err(format!("reply must be a string, null or object: {other:?}")),
        }
    }
    Ok(tty::ScriptedReader::new(replies))
}

fn str_shape(value: Option<String>, shape: Option<ShapeKind>) -> Json {
    Json::Obj(vec![
        ("value".to_string(), str_or_null(value)),
        (
            "shape".to_string(),
            match shape {
                Some(shape) => Json::Str(shape.token().to_string()),
                None => Json::Null,
            },
        ),
    ])
}

fn rows_shape(rows: Option<Vec<Json>>, shape: Option<ShapeKind>) -> Json {
    Json::Obj(vec![
        (
            "rows".to_string(),
            match rows {
                Some(rows) => Json::Arr(rows),
                None => Json::Null,
            },
        ),
        (
            "shape".to_string(),
            match shape {
                Some(shape) => Json::Str(shape.token().to_string()),
                None => Json::Null,
            },
        ),
    ])
}

/// Project string leaves of a settings object onto `{scratch}`.
fn project_obj(dir: &TempCaseDir, obj: &Obj) -> Obj {
    obj.iter()
        .map(|(k, v)| (k.clone(), project_value(dir, v)))
        .collect()
}

fn project_value(dir: &TempCaseDir, value: &Value) -> Value {
    match value {
        Value::Str(s) => Value::Str(project(dir, s)),
        Value::Arr(items) => Value::Arr(items.iter().map(|v| project_value(dir, v)).collect()),
        Value::Obj(o) => Value::Obj(project_obj(dir, o)),
        other => other.clone(),
    }
}
