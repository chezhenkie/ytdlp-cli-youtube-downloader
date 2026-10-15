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
use ytdlp_cli::resolve::{self, ShapeKind};
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
fn map_which_closure<'a>(
    dir: &'a TempCaseDir,
    map: Option<&'a Json>,
    calls: &'a mut Vec<String>,
) -> impl FnMut(&str) -> Option<String> + 'a {
    move |name: &str| {
        calls.push(name.to_string());
        match map.and_then(|m| obj_field(m, name)) {
            Some(Json::Str(rel)) => Some(resolve_scratch(rel, dir)),
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
