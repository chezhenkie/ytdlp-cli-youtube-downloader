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
use ytdlp_cli::config::{Obj, Value};
use ytdlp_cli::menu::{self, MenuErr};
use ytdlp_cli::resolve::{self, ShapeKind};
use ytdlp_cli::tty::{self, ScriptItem};
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
            let config_path = resolve_scratch("{scratch}/config/settings.json", &dir);
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
            let mut download = |_s: &mut Obj, _url: &str| -> i32 { 0 };
            let mut quiet_check = |_s: &mut Obj| {};
            let (outcome, exit) = {
                let mut io = menu::Io {
                    out: &mut rec,
                    reader: &mut reader,
                };
                if name == "act" {
                    let choice = field_str(case, "choice")?;
                    match menu::act(&choice, &mut s, &mut io, &world, &mut download) {
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
                    match menu::main_menu(&mut s, &mut io, &world, &mut download, &mut quiet_check)
                    {
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
        _ => Err(format!("unknown function: {name}")),
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
    }
    Ok(())
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
