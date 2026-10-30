//! Argument construction (specsheet section 4, layer 4): url cleaning,
//! JS-runtime detection mapping, subtitle language input, and the yt-dlp
//! argv builders for the download and subtitle passes.
//!
//! Ported one to one from `deprecated-py-src/src/ytdlp.py` (the reference for the
//! differential harness). Where the python original reads process state
//! (`shutil.which`) it takes it as a parameter so the pure layer stays
//! pure; the impure lookup lands in its own layer. Settings values that
//! `coerce_settings` repairs (type, container, resolution, audio format
//! and quality, max items) arrive as their coerced shapes; values that
//! stay raw in the file and can raise in python (a truthy non-string
//! `filename_template` is `ABS_RE.match`'s TypeError, a truthy
//! non-string `output_dir` is `resolve_outdir`'s AttributeError, a
//! non-numeric `playlist_max_items` string is `int()`'s ValueError)
//! carry typed branches.

use crate::config::{self, Obj, Value};
use crate::pytext::py_space;
use crate::resolve::{self, ShapeKind};

/// First http(s) url in `text`, punctuation-trimmed; None when there is
/// none. Python: `URL_RE.search(text or "")` then `rstrip(".,;:!?)]\"'")`.
pub fn clean_url(text: Option<&str>) -> Option<String> {
    let text = text.unwrap_or("");
    let mut found: Option<&str> = None;
    for (i, _) in text.match_indices('h') {
        let rest = &text[i..];
        let scheme_len = if rest.starts_with("https://") {
            8
        } else if rest.starts_with("http://") {
            7
        } else {
            continue;
        };
        // `\S+` needs at least one non-space right after the scheme
        let body = &rest[scheme_len..];
        if body.chars().next().is_none_or(py_space) {
            continue;
        }
        let end = scheme_len
            + body
                .char_indices()
                .find(|(_, c)| py_space(*c))
                .map_or(body.len(), |(j, _)| j);
        found = Some(&text[i..i + end]);
        break;
    }
    let m = found?;
    Some(
        m.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '"', '\''])
            .to_string(),
    )
}

/// `--js-runtimes node` when a node is on PATH, empty otherwise. The
/// python original calls `shutil.which`; the lookup belongs to the
/// resolution layer, so the pure mapping takes the answer.
pub fn js_runtime_args(has_node: bool) -> Vec<String> {
    if has_node {
        vec!["--js-runtimes".to_string(), "node".to_string()]
    } else {
        Vec::new()
    }
}

/// Every language code the menu offers and validates against, 191 in all
/// (specsheet FR-9). The 190 two-letter primary subtags of the IANA BCP 47
/// registry plus `fil` from the pinned yt-dlp source. Sorted, so it is
/// usable with binary search and `sub_lang_lines` renders it directly.
pub const KNOWN_LANG_CODES: &[&str] = &[
    "aa", "ab", "ae", "af", "ak", "am", "an", "ar", "as", "av", "ay", "az", "ba", "be", "bg", "bh",
    "bi", "bm", "bn", "bo", "br", "bs", "ca", "ce", "ch", "co", "cr", "cs", "cu", "cv", "cy", "da",
    "de", "dv", "dz", "ee", "el", "en", "eo", "es", "et", "eu", "fa", "ff", "fi", "fil", "fj",
    "fo", "fr", "fy", "ga", "gd", "gl", "gn", "gu", "gv", "ha", "he", "hi", "ho", "hr", "ht", "hu",
    "hy", "hz", "ia", "id", "ie", "ig", "ii", "ik", "in", "io", "is", "it", "iu", "iw", "ja", "ji",
    "jv", "jw", "ka", "kg", "ki", "kj", "kk", "kl", "km", "kn", "ko", "kr", "ks", "ku", "kv", "kw",
    "ky", "la", "lb", "lg", "li", "ln", "lo", "lt", "lu", "lv", "mg", "mh", "mi", "mk", "ml", "mn",
    "mo", "mr", "ms", "mt", "my", "na", "nb", "nd", "ne", "ng", "nl", "nn", "no", "nr", "nv", "ny",
    "oc", "oj", "om", "or", "os", "pa", "pi", "pl", "ps", "pt", "qu", "rm", "rn", "ro", "ru", "rw",
    "sa", "sc", "sd", "se", "sg", "sh", "si", "sk", "sl", "sm", "sn", "so", "sq", "sr", "ss", "st",
    "su", "sv", "sw", "ta", "te", "tg", "th", "ti", "tk", "tl", "tn", "to", "tr", "ts", "tt", "tw",
    "ty", "ug", "uk", "ur", "uz", "ve", "vi", "vo", "wa", "wo", "xh", "yi", "yo", "za", "zh", "zu",
];

/// `'en,nl'` -> `'(?-i:en(?:-[A-Z]{2})?)$,(?-i:nl(?:-[A-Z]{2})?)$'`.
/// yt-dlp compiles each entry with `re.I` and `fullmatch`, so a case-blind
/// regional class would also select translated keys (`en-de`, `en-de-DE`),
/// and those requests answer 429; the scoped case-sensitive group selects
/// the native code and its regional variants (`en-GB`, `en-US`) only.
/// Entries that already are patterns (`* ? ( [`) and the `all` alias pass
/// through untouched.
pub fn sub_lang_patterns(langs: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    for part in langs.unwrap_or("").split(',') {
        let code = part.trim();
        if code.is_empty() {
            continue;
        }
        if code == "all" || code.contains(['*', '?', '(', '[']) {
            out.push(code.to_string());
        } else {
            out.push(format!("(?-i:{code}(?:-[A-Z]{{2}})?)$"));
        }
    }
    if out.is_empty() {
        "(?-i:en(?:-[A-Z]{2})?)$".to_string()
    } else {
        out.join(",")
    }
}

/// A language code with optional subtags: en, fil, en-GB, sr-Latn,
/// es-419. Python `^[a-z]{2,3}(?:-[a-z0-9]{2,8})*$`.
fn is_lang_code(code: &str) -> bool {
    fn ascii_alpha(c: char) -> bool {
        c.is_ascii_lowercase()
    }
    fn ascii_alnum(c: char) -> bool {
        c.is_ascii_lowercase() || c.is_ascii_digit()
    }
    let mut parts = code.split('-');
    let first = match parts.next() {
        Some(p) if (2..=3).contains(&p.chars().count()) => p,
        _ => return false,
    };
    if !first.chars().all(ascii_alpha) {
        return false;
    }
    parts.all(|p| (2..=8).contains(&p.chars().count()) && p.chars().all(ascii_alnum))
}

/// Split a typed language list into (kept, bad, unknown). `bad` cannot be
/// a language code at all; `unknown` is well formed but absent from
/// KNOWN_LANG_CODES and is kept anyway (FR-9: the reference never gates).
pub fn parse_lang_input(raw: Option<&str>) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut kept = Vec::new();
    let mut bad = Vec::new();
    let mut unknown = Vec::new();
    for part in raw.unwrap_or("").split(',') {
        let code = part.trim().to_lowercase();
        if code.is_empty() {
            continue;
        }
        let wild = code.contains(['*', '?', '(', '[']);
        if wild || is_lang_code(&code) {
            let primary = code.split('-').next().unwrap_or("");
            if !wild && KNOWN_LANG_CODES.binary_search(&primary).is_err() {
                unknown.push(code.clone());
            }
            kept.push(code);
        } else {
            bad.push(code);
        }
    }
    (kept, bad, unknown)
}

// ------------------------------------------------- argv construction

/// The shipped filename template (`DEFAULTS["filename_template"]`); a
/// rust test pins it against `config::defaults()` so the two cannot
/// drift. `menu` reuses it for the template preset list.
pub(crate) const TEMPLATE_DEFAULT: &str = "%(title)s";

/// Python `ABS_RE = re.compile(r"^([A-Za-z]:[\\/]|[\\/])")` (L317): a
/// drive-absolute or rooted template suppresses both `-P` entries. A
/// pure string test, so `\x` counts as absolute on posix too.
fn abs_re_match(template: &str) -> bool {
    let mut chars = template.chars();
    match chars.next() {
        Some('\\') | Some('/') => true,
        Some(c) if c.is_ascii_alphabetic() => {
            matches!(chars.next(), Some(':')) && matches!(chars.next(), Some('\\' | '/'))
        }
        _ => false,
    }
}

/// True when the template's last path component already ends in a literal
/// suffix such as `.mp4`. A trailing yt-dlp field (`.%(upload_date)s`) is
/// not one: its rendered value is not a fixed extension.
fn has_literal_ext(template: &str) -> bool {
    let last = template.rsplit(['/', '\\']).next().unwrap_or(template);
    let Some((_, ext)) = last.rsplit_once('.') else {
        return false;
    };
    !ext.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric())
}

/// yt-dlp only appends the media extension when the output template names
/// `%(ext)s`. Without it a video-only download (no merge/postprocess step)
/// keeps the bare template name and lands with no suffix, so the argv
/// builder adds the placeholder instead of rewriting stored settings.
fn with_ext(template: &str) -> String {
    if template.contains("%(ext)s") || has_literal_ext(template) {
        template.to_string()
    } else {
        format!("{template}.%(ext)s")
    }
}

/// `output_args(s)`: the shared `-o`/`-P` set for the video and subtitle
/// passes. Both `-P` entries are skipped when the template is absolute.
/// `auto` is the `auto_outdir()` answer for the caller (memoized in the
/// real glue).
pub fn output_args(s: &Obj, root: &str, auto: Option<&str>) -> Result<Vec<String>, ShapeKind> {
    let template = match config::get(s, "filename_template") {
        Some(v) if config::py_truthy(v) => match v {
            Value::Str(t) => t.clone(),
            // a truthy non-string template dies in ABS_RE.match
            // (reachable: coerce_settings does not touch this key)
            _ => return Err(ShapeKind::TypeError),
        },
        Some(_) => TEMPLATE_DEFAULT.to_string(),
        // `s["filename_template"]` on a missing key is python's KeyError;
        // a loaded settings object always carries it
        None => return Err(ShapeKind::TypeError),
    };
    let template = with_ext(&template);
    let mut opts = vec!["-o".to_string(), template.clone()];
    if !abs_re_match(&template) {
        let outdir = resolve::resolve_outdir(s, auto, root)?;
        let temp = resolve::py_path_join(&outdir, resolve::TEMP_DIR_NAME);
        opts.push("-P".to_string());
        opts.push(format!("home:{outdir}"));
        opts.push("-P".to_string());
        opts.push(format!("temp:{temp}"));
    }
    Ok(opts)
}

/// A key whose consumer takes the raw value (`--audio-format`,
/// `--merge-output-format`): coerced to a string before it can reach
/// here, so any other shape is out of contract. python would carry the
/// raw value into argv and fail at spawn; the port refuses instead
/// (recorded in the phase-5 port notes).
fn str_field(s: &Obj, key: &str) -> Result<String, ShapeKind> {
    match config::get(s, key) {
        Some(Value::Str(t)) => Ok(t.clone()),
        _ => Err(ShapeKind::TypeError),
    }
}

/// The `[height<=N]` clause: `"best"` means no cap, everything else
/// renders through python `%s` (faithful even for non-string values;
/// coercion repairs the whitelist in the real flow). Missing key would
/// be python's KeyError - a loaded settings object always carries it.
fn height_clause(s: &Obj) -> Result<String, ShapeKind> {
    match config::get(s, "video_resolution") {
        Some(Value::Str(r)) if r == "best" => Ok(String::new()),
        Some(v) => Ok(format!("[height<={}]", config::py_str(v))),
        None => Err(ShapeKind::TypeError),
    }
}

/// `int(s.get("playlist_max_items") or 0)`: falsy and absent read as 0,
/// ints pass, strings go through python's `int()` (a non-numeric string
/// is the ValueError shape). Bool and float shapes are refused rather
/// than mirrored - a loaded settings object always carries the coerced
/// int (recorded in the phase-5 port notes). `menu_items` reuses this
/// for the playlist row.
pub(crate) fn playlist_max_items(s: &Obj) -> Result<i64, ShapeKind> {
    let Some(v) = config::get(s, "playlist_max_items") else {
        return Ok(0);
    };
    if !config::py_truthy(v) {
        return Ok(0);
    }
    match v {
        Value::Int(n) => Ok(*n),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Str(t) => config::py_int(t).ok_or(ShapeKind::ValueError),
        _ => Err(ShapeKind::TypeError),
    }
}

/// `build_args(s, url, ffdir)`: the video-run argv (binary first, url
/// last). `ffdir` is explicit: `Some(dir)` or `None` for "no ffmpeg",
/// which forces the single-file / no-convert branches. Subtitles are NOT
/// part of this run - `build_sub_args` builds the pass that runs first.
/// `has_node` is the `js_runtime_args` lookup, injected lazily because
/// the python calls it only after `output_args` has succeeded (an early
/// shape branch must not have queried PATH).
pub fn build_args(
    s: &Obj,
    url: &str,
    ffdir: Option<&str>,
    root: &str,
    auto: Option<&str>,
    has_node: &mut dyn FnMut() -> bool,
) -> Result<Vec<String>, ShapeKind> {
    let mut opts = output_args(s, root, auto)?;
    opts.push("--retries".to_string());
    opts.push("5".to_string());
    opts.extend(js_runtime_args(has_node()));

    let kind = config::get(s, "type");
    let audio_only = matches!(kind, Some(Value::Str(t)) if t == "audio only");
    let video_no_audio = matches!(kind, Some(Value::Str(t)) if t == "video no audio");
    if audio_only {
        if ffdir.is_some() {
            opts.push("-f".to_string());
            opts.push("bestaudio/best".to_string());
            opts.push("-x".to_string());
            opts.push("--audio-format".to_string());
            opts.push(str_field(s, "audio_format")?);
            opts.push("--audio-quality".to_string());
            opts.push(match config::get(s, "audio_quality") {
                Some(v) => config::py_str(v),
                None => return Err(ShapeKind::TypeError),
            });
        } else {
            opts.push("-f".to_string());
            opts.push("bestaudio".to_string());
        }
    } else if video_no_audio {
        opts.push("-f".to_string());
        opts.push(format!("bv*{}", height_clause(s)?));
    } else if ffdir.is_some() {
        opts.push("-f".to_string());
        opts.push(format!("bv*{}+ba/b", height_clause(s)?));
        let container = match config::get(s, "video_container") {
            Some(Value::Str(c)) => c.clone(),
            // raw non-string value is not in ("best", ""): python would
            // append it to argv raw; out of contract here
            Some(_) => return Err(ShapeKind::TypeError),
            None => return Err(ShapeKind::TypeError),
        };
        if container != "best" && !container.is_empty() {
            opts.push("--merge-output-format".to_string());
            opts.push(container);
        }
    } else {
        opts.push("-f".to_string());
        opts.push("b".to_string());
    }

    if let Some(dir) = ffdir {
        opts.push("--ffmpeg-location".to_string());
        opts.push(dir.to_string());
    }

    if config::get(s, "playlist").is_some_and(config::py_truthy) {
        let n = playlist_max_items(s)?;
        if n > 0 {
            opts.push("--playlist-items".to_string());
            opts.push(format!("1-{n}"));
        }
    } else {
        opts.push("--no-playlist".to_string());
    }

    // FR-21: when the info field setting is on, the video run writes the
    // description (the info field below the video) as yt-dlp's
    // "<name> [<id>].description" file next to the media
    if config::get(s, "info_field").is_some_and(config::py_truthy) {
        opts.push("--write-description".to_string());
    }

    opts.push("--newline".to_string());
    opts.push(url.to_string());
    Ok(opts)
}

/// `build_sub_args(s, url, ffdir)`: the subtitle-only pass, same output
/// template, `--skip-download`; never counts as a failure for the
/// download. `srt` conversion needs ffmpeg, else `vtt` in place. The
/// `has_node` lookup is injected lazily like in `build_args` (python
/// queries PATH at the very end of this function).
pub fn build_sub_args(
    s: &Obj,
    url: &str,
    ffdir: Option<&str>,
    root: &str,
    auto: Option<&str>,
    has_node: &mut dyn FnMut() -> bool,
) -> Result<Vec<String>, ShapeKind> {
    let mut opts = output_args(s, root, auto)?;
    // `sub_lang_patterns(s["subtitle_langs"])`: missing key is python's
    // KeyError; the value goes through `str(langs or "")` first
    let langs = match config::get(s, "subtitle_langs") {
        Some(v) => {
            if config::py_truthy(v) {
                config::py_str(v)
            } else {
                String::new()
            }
        }
        None => return Err(ShapeKind::TypeError),
    };
    opts.push("--skip-download".to_string());
    opts.push("--write-subs".to_string());
    opts.push("--write-auto-subs".to_string());
    opts.push("--sub-langs".to_string());
    opts.push(sub_lang_patterns(Some(&langs)));
    opts.push("--no-playlist".to_string());
    opts.push("--newline".to_string());
    opts.push("--convert-subs".to_string());
    opts.push(if ffdir.is_some() { "srt" } else { "vtt" }.to_string());
    if let Some(dir) = ffdir {
        opts.push("--ffmpeg-location".to_string());
        opts.push(dir.to_string());
    }
    opts.extend(js_runtime_args(has_node()));
    opts.push(url.to_string());
    Ok(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(pairs: &[(&str, Value)]) -> Obj {
        let mut s = config::defaults();
        for (k, v) in pairs {
            config::set(&mut s, k, v.clone());
        }
        s
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn default_template_matches_config_defaults() {
        assert_eq!(
            config::get(&config::defaults(), "filename_template").unwrap(),
            &Value::Str(TEMPLATE_DEFAULT.to_string())
        );
    }

    #[test]
    fn abs_re_match_matrix() {
        assert!(abs_re_match("/x"));
        assert!(abs_re_match("\\x"));
        assert!(abs_re_match("C:/x"));
        assert!(abs_re_match("C:\\x"));
        assert!(abs_re_match("z:/x"));
        assert!(!abs_re_match("z:X"));
        assert!(!abs_re_match("C:x"));
        assert!(!abs_re_match("rel/x"));
        assert!(!abs_re_match(""));
        assert!(!abs_re_match("1:/x"));
        assert!(!abs_re_match("./x"));
    }

    #[test]
    fn sub_lang_patterns_select_native_codes_only() {
        // a bare code becomes a case-sensitive native matcher: en, en-GB,
        // en-US match; en-de (translated) does not
        assert_eq!(
            sub_lang_patterns(Some("en,nl")),
            "(?-i:en(?:-[A-Z]{2})?)$,(?-i:nl(?:-[A-Z]{2})?)$"
        );
        // explicit patterns and the all alias pass through
        assert_eq!(sub_lang_patterns(Some("en.*")), "en.*");
        assert_eq!(sub_lang_patterns(Some("all")), "all");
        // empty and missing settings fall back to native english
        assert_eq!(sub_lang_patterns(Some("")), "(?-i:en(?:-[A-Z]{2})?)$");
        assert_eq!(sub_lang_patterns(None), "(?-i:en(?:-[A-Z]{2})?)$");
    }

    #[test]
    fn with_ext_matrix() {
        // the plain shipped template gains the placeholder
        assert_eq!(with_ext("%(title)s"), "%(title)s.%(ext)s");
        // a template that already names the extension is untouched
        assert_eq!(with_ext("%(title)s.%(ext)s"), "%(title)s.%(ext)s");
        assert_eq!(with_ext("/abs/%(title)s"), "/abs/%(title)s.%(ext)s");
        assert_eq!(with_ext("C:\\out\\%(title)s"), "C:\\out\\%(title)s.%(ext)s");
        // literal suffixes keep their meaning
        assert_eq!(with_ext("%(title)s.mp4"), "%(title)s.mp4");
        assert_eq!(with_ext("C:/out/show.mkv"), "C:/out/show.mkv");
        // a trailing field is not a literal extension
        assert_eq!(
            with_ext("%(title)s.%(upload_date)s"),
            "%(title)s.%(upload_date)s.%(ext)s"
        );
    }

    #[test]
    fn output_args_default_and_absolute_templates() {
        let s = settings(&[("output_dir", Value::Str("rel".into()))]);
        let outdir = resolve::py_path_join("root", "rel");
        let argv = output_args(&s, "root", None).unwrap();
        assert_eq!(
            argv,
            strings(&[
                "-o",
                "%(title)s.%(ext)s",
                "-P",
                &format!("home:{outdir}"),
                "-P",
                &format!(
                    "temp:{}",
                    resolve::py_path_join(&outdir, resolve::TEMP_DIR_NAME)
                ),
            ])
        );

        // absolute templates suppress both -P entries, \x counts too
        let s = settings(&[("filename_template", Value::Str("/abs/%(title)s".into()))]);
        assert_eq!(
            output_args(&s, "root", None).unwrap(),
            strings(&["-o", "/abs/%(title)s.%(ext)s"])
        );
        let s = settings(&[("filename_template", Value::Str("C:\\tpl".into()))]);
        assert_eq!(output_args(&s, "root", None).unwrap().len(), 2);

        // falsy template uses the shipped default
        let s = settings(&[("filename_template", Value::Null)]);
        assert_eq!(
            output_args(&s, "root", None).unwrap()[1],
            "%(title)s.%(ext)s"
        );

        // truthy non-string template is ABS_RE.match's TypeError
        let s = settings(&[("filename_template", Value::Int(5))]);
        assert_eq!(output_args(&s, "root", None), Err(ShapeKind::TypeError));

        // truthy non-string output_dir is resolve_outdir's AttributeError
        let s = settings(&[("output_dir", Value::Int(5))]);
        assert_eq!(
            output_args(&s, "root", None),
            Err(ShapeKind::AttributeError)
        );

        // auto answer fills an empty output_dir (relative joins the root)
        let s = settings(&[]);
        let argv = output_args(&s, "root", Some("auto")).unwrap();
        let outdir = resolve::py_path_join("root", "auto");
        assert_eq!(argv[3], format!("home:{outdir}"));
        assert_eq!(
            argv[5],
            format!(
                "temp:{}",
                resolve::py_path_join(&outdir, resolve::TEMP_DIR_NAME)
            )
        );
    }

    #[test]
    fn build_args_branch_tree() {
        let base = settings(&[("output_dir", Value::Str("out".into()))]);
        let head = output_args(&base, "root", None).unwrap();
        let tail = strings(&["--newline", "u"]);

        // video + ffmpeg: merge flag from the container
        let mut s = base.clone();
        config::set(&mut s, "video_container", Value::Str("mkv".into()));
        let argv = build_args(&s, "u", Some("/ff"), "root", None, &mut || false).unwrap();
        let mut expect = head.clone();
        expect.extend(strings(&[
            "--retries",
            "5",
            "-f",
            "bv*[height<=1080]+ba/b",
            "--merge-output-format",
            "mkv",
            "--ffmpeg-location",
            "/ff",
            "--no-playlist",
        ]));
        expect.extend(tail.clone());
        assert_eq!(argv, expect);

        // container best skips the merge flag; resolution best drops the cap
        let mut s = base.clone();
        config::set(&mut s, "video_container", Value::Str("best".into()));
        config::set(&mut s, "video_resolution", Value::Str("best".into()));
        let argv = build_args(&s, "u", Some("/ff"), "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"bv*+ba/b".to_string()));
        assert!(!argv.contains(&"--merge-output-format".to_string()));

        // video without ffmpeg is a single best file
        let argv = build_args(&base, "u", None, "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"b".to_string()));
        assert!(!argv.contains(&"bv*[height<=1080]+ba/b".to_string()));

        // video no audio keeps its selector with and without ffmpeg
        let mut s = base.clone();
        config::set(&mut s, "type", Value::Str("video no audio".into()));
        let argv = build_args(&s, "u", None, "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"bv*[height<=1080]".to_string()));
        assert!(!argv.contains(&"--merge-output-format".to_string()));

        // audio only + ffmpeg converts; audio only without is a single stream
        let mut s = base.clone();
        config::set(&mut s, "type", Value::Str("audio only".into()));
        config::set(&mut s, "audio_quality", Value::Int(7));
        let argv = build_args(&s, "u", Some("/ff"), "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"bestaudio/best".to_string()));
        assert!(argv.contains(&"--audio-format".to_string()));
        assert!(argv.contains(&"7".to_string()));
        let argv = build_args(&s, "u", None, "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"bestaudio".to_string()));
        assert!(!argv.contains(&"-x".to_string()));

        // node on PATH adds the flag pair after --retries 5
        let argv = build_args(&base, "u", None, "root", None, &mut || true).unwrap();
        let at = argv.iter().position(|a| a == "--js-runtimes").unwrap();
        assert_eq!(argv[at + 1], "node");
        assert_eq!(argv[at - 2], "--retries");

        // playlist: capped, uncapped (no flag either way), uncapped-junk
        let mut s = base.clone();
        config::set(&mut s, "playlist", Value::Bool(true));
        config::set(&mut s, "playlist_max_items", Value::Int(5));
        let argv = build_args(&s, "u", None, "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"1-5".to_string()));
        assert!(!argv.contains(&"--no-playlist".to_string()));
        config::set(&mut s, "playlist_max_items", Value::Int(0));
        let argv = build_args(&s, "u", None, "root", None, &mut || false).unwrap();
        assert!(!argv.contains(&"--playlist-items".to_string()));
        assert!(!argv.contains(&"--no-playlist".to_string()));
        config::set(&mut s, "playlist_max_items", Value::Str("3".into()));
        let argv = build_args(&s, "u", None, "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"1-3".to_string()));
        config::set(&mut s, "playlist_max_items", Value::Str("abc".into()));
        assert_eq!(
            build_args(&s, "u", None, "root", None, &mut || false),
            Err(ShapeKind::ValueError)
        );

        // playlist off is --no-playlist (absent and falsy alike)
        let mut s = base.clone();
        config::set(&mut s, "playlist", Value::Null);
        let argv = build_args(&s, "u", None, "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"--no-playlist".to_string()));
    }

    /// FR-21: the info field flag rides the video run only - on adds
    /// --write-description before --newline; off, absent and falsy carry
    /// no description flag.
    #[test]
    fn build_args_info_field_flag() {
        let base = settings(&[("output_dir", Value::Str("out".into()))]);
        let argv = build_args(&base, "u", None, "root", None, &mut || false).unwrap();
        assert!(!argv.contains(&"--write-description".to_string()));

        let mut s = base.clone();
        config::set(&mut s, "info_field", Value::Bool(true));
        let argv = build_args(&s, "u", None, "root", None, &mut || false).unwrap();
        let at = argv
            .iter()
            .position(|a| a == "--write-description")
            .unwrap();
        assert_eq!(argv[at + 1], "--newline");
        assert_eq!(argv[at + 2], "u");

        config::set(&mut s, "info_field", Value::Null);
        let argv = build_args(&s, "u", None, "root", None, &mut || false).unwrap();
        assert!(!argv.contains(&"--write-description".to_string()));
    }

    #[test]
    fn build_sub_args_tree() {
        let base = settings(&[("output_dir", Value::Str("out".into()))]);
        let head = output_args(&base, "root", None).unwrap();

        let argv = build_sub_args(&base, "u", Some("/ff"), "root", None, &mut || false).unwrap();
        let mut expect = head.clone();
        expect.extend(strings(&[
            "--skip-download",
            "--write-subs",
            "--write-auto-subs",
            "--sub-langs",
            "(?-i:en(?:-[A-Z]{2})?)$",
            "--no-playlist",
            "--newline",
            "--convert-subs",
            "srt",
            "--ffmpeg-location",
            "/ff",
            "u",
        ]));
        assert_eq!(argv, expect);

        // no ffmpeg: vtt in place, no location flag
        let argv = build_sub_args(&base, "u", None, "root", None, &mut || false).unwrap();
        let at = argv.iter().position(|a| a == "--convert-subs").unwrap();
        assert_eq!(argv[at + 1], "vtt");
        assert!(!argv.contains(&"--ffmpeg-location".to_string()));

        // node flag lands after the ffmpeg location, before the url
        let argv = build_sub_args(&base, "u", Some("/ff"), "root", None, &mut || true).unwrap();
        assert_eq!(argv[argv.len() - 3], "--js-runtimes");
        assert_eq!(argv[argv.len() - 1], "u");

        // lang values render through str() first
        let mut s = base.clone();
        config::set(&mut s, "subtitle_langs", Value::Int(5));
        let argv = build_sub_args(&s, "u", None, "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"(?-i:5(?:-[A-Z]{2})?)$".to_string()));
        config::set(
            &mut s,
            "subtitle_langs",
            Value::Arr(vec![Value::Str("en".into())]),
        );
        let argv = build_sub_args(&s, "u", None, "root", None, &mut || false).unwrap();
        // the repr carries "[", which is a wildcard pass-through: no suffix
        assert!(argv.contains(&"['en']".to_string()));

        // empty list is falsy: the default pattern returns
        config::set(&mut s, "subtitle_langs", Value::Arr(vec![]));
        let argv = build_sub_args(&s, "u", None, "root", None, &mut || false).unwrap();
        assert!(argv.contains(&"(?-i:en(?:-[A-Z]{2})?)$".to_string()));
    }
}
