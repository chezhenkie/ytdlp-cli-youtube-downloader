//! Resolution and diagnostics (specsheet section 4, layer 3): platform
//! name, PATH lookup, the pathlib lexical model, environment expansion,
//! folder resolution, binary discovery, version probing and the --check
//! content.
//!
//! Ported one to one from `app-python/src/ytdlp.py` (the oracle). Every
//! process-state read (which answers, PATH/PATHEXT, registry values,
//! home, env map, mtimes, subprocess outcomes) is a parameter so the
//! differential can inject identical state on both sides; the real
//! lookups live at the bottom of this module (specsheet section 4,
//! phase-4 port notes). Shapes where python raises carry typed branches
//! (`ShapeKind`), and messages that embed payloads stay shape-gated.

use std::collections::BTreeMap;
#[cfg(unix)]
use std::path::Path;

use crate::config::{self, Obj, Value};
use crate::pytext::{py_space, py_splitlines_first, py_strip};

// ------------------------------------------------------------ platform

/// Python `platform_name()`: os.name first, then sys.platform.
pub fn platform_name(os_name: &str, sys_platform: &str) -> String {
    if os_name == "nt" {
        return "windows".to_string();
    }
    if sys_platform.starts_with("linux") {
        return "linux".to_string();
    }
    if sys_platform == "darwin" {
        return "macos".to_string();
    }
    os_name.to_string()
}

/// Python `PLATFORM_BIN.get(platform_name(), "yt-dlp")`.
pub fn platform_bin_name(platform: &str) -> &'static str {
    match platform {
        "windows" => "yt-dlp.exe",
        "linux" | "macos" => "yt-dlp",
        _ => "yt-dlp",
    }
}

/// Python `"%s" % bool` for the `exists=` field.
pub fn py_bool(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

// -------------------------------------------------------- path model
//
// pathlib 3.14's lexical model, pinned against probed oracle matrices
// (the parsers are C builtins; behaviour, not source, is the reference).

#[cfg(windows)]
const SEP: char = '\\';
#[cfg(windows)]
const ALTSEP: Option<char> = Some('/');
#[cfg(not(windows))]
const SEP: char = '/';
#[cfg(not(windows))]
const ALTSEP: Option<char> = None;

#[cfg(windows)]
fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}
#[cfg(not(windows))]
fn is_sep(c: char) -> bool {
    c == '/'
}

/// `os.path.splitroot` (drive, root, tail). Windows follows the probed
/// forms: drive letters, UNC server+share, verbatim and device prefixes,
/// rooted paths; posix: `//` is the special root, `///` folds to `/`.
#[cfg(windows)]
pub fn splitroot(path: &str) -> (String, String, String) {
    let chars: Vec<char> = path.chars().collect();
    let n = chars.len();
    if n == 0 {
        return (String::new(), String::new(), String::new());
    }
    let first = chars[0];
    let second = chars.get(1).copied();
    if !is_sep(first) {
        // Relative, possibly with a drive: "C:x", "C:\x", "a".
        if n >= 2 && chars[1] == ':' && chars[0].is_ascii_alphabetic() {
            let drive: String = chars[..2].iter().collect();
            let rest: String = chars[2..].iter().collect();
            return match chars.get(2) {
                Some(&c) if is_sep(c) => {
                    (drive, c.to_string(), chars[3..].iter().collect::<String>())
                }
                _ => (drive, String::new(), rest),
            };
        }
        return (String::new(), String::new(), path.to_string());
    }
    if second.is_none_or(|c| !is_sep(c)) {
        // Single leading separator: rooted.
        return (
            String::new(),
            first.to_string(),
            chars[1..].iter().collect::<String>(),
        );
    }
    // Two leading separators: device/verbatim or UNC.
    let body: Vec<char> = chars[2..].to_vec();
    let b0 = body.first().copied();
    let b1 = body.get(1).copied();
    if matches!(b0, Some('?') | Some('.')) && b1.is_some_and(is_sep) {
        let after: Vec<char> = body[2..].to_vec();
        if let Some(j) = after.iter().position(|&c| is_sep(c)) {
            let drive_end = 4 + j;
            let drive: String = chars[..drive_end].iter().collect();
            let root = chars[drive_end].to_string();
            let tail: String = chars[drive_end + 1..].iter().collect();
            return (drive, root, tail);
        }
        return (path.to_string(), String::new(), String::new());
    }
    // UNC: server [share [more]].
    match body.iter().position(|&c| is_sep(c)) {
        None => (path.to_string(), String::new(), String::new()),
        Some(sep1) => {
            let rest: Vec<char> = body[sep1 + 1..].to_vec();
            match rest.iter().position(|&c| is_sep(c)) {
                None => (path.to_string(), String::new(), String::new()),
                Some(sep2) => {
                    let drive_end = 2 + sep1 + 1 + sep2;
                    let drive: String = chars[..drive_end].iter().collect();
                    let root = chars[drive_end].to_string();
                    let tail: String = chars[drive_end + 1..].iter().collect();
                    (drive, root, tail)
                }
            }
        }
    }
}

/// `posixpath.splitroot`: `//` stays the root, `///` folds to `/`.
#[cfg(not(windows))]
pub fn splitroot(path: &str) -> (String, String, String) {
    if path.is_empty() {
        return (String::new(), String::new(), String::new());
    }
    if !path.starts_with('/') {
        return (String::new(), String::new(), path.to_string());
    }
    if path.starts_with("//") && !path[2..].starts_with('/') {
        return (String::new(), "//".to_string(), path[2..].to_string());
    }
    (String::new(), "/".to_string(), path[1..].to_string())
}

/// `pathlib.PurePath._format_parsed_parts` plus the `.` fallback.
fn render_parts(drv: &str, root: &str, tail: &[String]) -> String {
    let sep = SEP.to_string();
    if !drv.is_empty() || !root.is_empty() {
        let mut out = String::with_capacity(drv.len() + root.len() + 8);
        out.push_str(drv);
        out.push_str(root);
        out.push_str(&tail.join(&sep));
        return out;
    }
    if let Some(first) = tail.first()
        && !splitroot(first).0.is_empty()
    {
        let mut out = String::with_capacity(8);
        out.push('.');
        out.push(SEP);
        out.push_str(&tail.join(&sep));
        return out;
    }
    tail.join(&sep)
}

/// `pathlib.PurePath._parse_path` (altsep folding, splitroot, the UNC
/// drive/root post-processing, empty and `.` components dropped).
fn parse_parts(path: &str) -> (String, String, Vec<String>) {
    if path.is_empty() {
        return (String::new(), String::new(), Vec::new());
    }
    let folded: String = match ALTSEP {
        Some(alt) => path.replace(alt, &SEP.to_string()),
        None => path.to_string(),
    };
    let (drv, mut root, rel) = splitroot(&folded);
    if root.is_empty() && drv.starts_with(SEP) && !drv.ends_with(SEP) {
        // Python: `drv_parts[2] not in '?.'` - and `'' in '?.'` is True,
        // so an empty component also suppresses the root.
        let parts: Vec<&str> = drv.split(SEP).collect();
        if (parts.len() == 4 && !matches!(parts[2], "?" | "." | "")) || parts.len() == 6 {
            root = SEP.to_string();
        }
    }
    let tail = rel
        .split(SEP)
        .filter(|c| !c.is_empty() && *c != ".")
        .map(str::to_string)
        .collect();
    (drv, root, tail)
}

/// `str(Path(text))`: python's lexical render, or `.` for an empty result.
pub fn path_render(path: &str) -> String {
    let (drv, root, tail) = parse_parts(path);
    let out = render_parts(&drv, &root, &tail);
    if out.is_empty() { ".".to_string() } else { out }
}

/// `pathlib.PurePath.is_absolute`: posix reads the raw string, windows
/// asks `ntpath.isabs` of the rendered one (pathlib 3.14 source).
pub fn path_is_absolute(path: &str) -> bool {
    #[cfg(windows)]
    {
        nt_isabs(&path_render(path))
    }
    #[cfg(not(windows))]
    {
        path.starts_with('/')
    }
}

/// `ntpath.isabs` on the first three characters, char-safe.
#[cfg(windows)]
fn nt_isabs(path: &str) -> bool {
    let head: Vec<char> = path
        .chars()
        .take(3)
        .map(|c| if c == '/' { '\\' } else { c })
        .collect();
    (head.get(1) == Some(&':') && head.get(2) == Some(&'\\'))
        || (head.first() == Some(&'\\') && head.get(1) == Some(&'\\'))
}

/// `os.path.join(a, b)` for the two-argument form.
pub fn parser_join(a: &str, b: &str) -> String {
    #[cfg(windows)]
    {
        nt_join(a, b)
    }
    #[cfg(not(windows))]
    {
        if b.starts_with(SEP) || a.is_empty() {
            b.to_string()
        } else if a.ends_with(SEP) {
            format!("{a}{b}")
        } else {
            format!("{a}{SEP}{b}")
        }
    }
}

/// `ntpath.join` for two arguments, ported from the 3.14 source.
#[cfg(windows)]
fn nt_join(a: &str, b: &str) -> String {
    let seps = ['\\', '/'];
    let (mut result_drive, mut result_root, mut result_path) = splitroot(a);
    let (p_drive, p_root, p_path) = splitroot(b);
    if !p_root.is_empty() {
        // Second path is absolute.
        if !p_drive.is_empty() || result_drive.is_empty() {
            result_drive = p_drive;
        }
        result_root = p_root;
        result_path = p_path;
    } else if !p_drive.is_empty() && p_drive != result_drive {
        if p_drive.to_lowercase() != result_drive.to_lowercase() {
            // Different drives: ignore the first path entirely.
            result_drive = p_drive;
            result_root = String::new();
            result_path = p_path;
        } else {
            result_drive = p_drive;
            if !result_path.is_empty() && !result_path.ends_with(seps) {
                result_path.push(SEP);
            }
            result_path.push_str(&p_path);
        }
    } else {
        if !result_path.is_empty() && !result_path.ends_with(seps) {
            result_path.push(SEP);
        }
        result_path.push_str(&p_path);
    }
    if !result_path.is_empty() && result_root.is_empty() && !result_drive.is_empty() {
        let last = result_drive.chars().last();
        if !matches!(last, Some(':') | Some('\\') | Some('/')) {
            return format!("{result_drive}{SEP}{result_path}");
        }
    }
    format!("{result_drive}{result_root}{result_path}")
}

/// `Path(a) / b` rendered: `parser_join` then the lexical render.
pub fn py_path_join(a: &str, b: &str) -> String {
    path_render(&parser_join(a, b))
}

/// `Path(path).parent` rendered.
pub fn py_path_parent(path: &str) -> String {
    let (drv, root, mut tail) = parse_parts(path);
    if tail.is_empty() {
        let out = render_parts(&drv, &root, &tail);
        return if out.is_empty() { ".".to_string() } else { out };
    }
    tail.pop();
    let out = render_parts(&drv, &root, &tail);
    if out.is_empty() { ".".to_string() } else { out }
}

// ------------------------------------------------------- expandvars

pub type EnvMap = BTreeMap<String, String>;

/// Environment lookup with python's semantics: `os.environ` on windows
/// compares keys case-insensitively via `str.upper`; posix exactly.
fn env_lookup(env: &EnvMap, name: &str) -> Option<String> {
    if cfg!(windows) {
        let upper = name.to_uppercase();
        env.iter()
            .find(|(k, _)| k.to_uppercase() == upper)
            .map(|(_, v)| v.clone())
    } else {
        env.get(name).cloned()
    }
}

fn is_ascii_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `os.path.expandvars`: posix `$name`/`${name}` only; windows adds
/// `%name%`, `$$ -> $`, `%% -> %` and the single-quoted-chunk skip.
/// Single pass; unknown names stay verbatim (probed oracle matrices).
pub fn expandvars(text: &str, env: &EnvMap) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let mut consumed: Option<(usize, String)> = None;
        #[cfg(windows)]
        if c == '\'' {
            // `'[^']*'?`: single-quoted chunks pass through verbatim.
            let mut j = i + 1;
            while j < n && chars[j] != '\'' {
                j += 1;
            }
            if j < n {
                j += 1; // include the closing quote
            }
            let chunk: String = chars[i..j].iter().collect();
            consumed = Some((j, replacement_quote(&chunk, env)));
        }
        #[cfg(windows)]
        if consumed.is_none() && c == '%' {
            let (j, group) = if chars.get(i + 1) == Some(&'%') {
                (i + 2, "%".to_string())
            } else {
                let mut j = i + 1;
                while j < n && chars[j] != '%' {
                    j += 1;
                }
                if j < n {
                    j += 1;
                }
                (j, chars[i + 1..j].iter().collect::<String>())
            };
            consumed = Some((j, replacement_percent(&chars[i..j], &group, env)));
        }
        if consumed.is_none()
            && c == '$'
            && (chars.get(i + 1) == Some(&'{')
                || chars.get(i + 1).copied().is_some_and(is_dollar_word))
        {
            let (j, group) = dollar_span(&chars, i);
            consumed = Some((j, replacement_dollar(&chars[i..j], &group, env)));
        }
        match consumed {
            Some((j, repl)) => {
                out.push_str(&repl);
                i = j;
            }
            None => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(windows)]
fn is_dollar_word(c: char) -> bool {
    is_ascii_word(c) || c == '-'
}
#[cfg(not(windows))]
fn is_dollar_word(c: char) -> bool {
    is_ascii_word(c)
}

/// Span of the `$...` alternative at `i` (caller checked the head) plus
/// its group text.
fn dollar_span(chars: &[char], i: usize) -> (usize, String) {
    let n = chars.len();
    if chars.get(i + 1) == Some(&'{') {
        let mut j = i + 2;
        while j < n && chars[j] != '}' {
            j += 1;
        }
        if j < n {
            j += 1; // include the closing brace
        }
        let group: String = chars[i + 1..j].iter().collect();
        (j, group)
    } else {
        let mut j = i + 2; // chars[i+1] is a word char
        while j < n && is_dollar_word(chars[j]) {
            j += 1;
        }
        let group: String = chars[i + 1..j].iter().collect();
        (j, group)
    }
}

/// Windows quoted alternative: verbatim, with python's odd `%`-trim path.
#[cfg(windows)]
fn replacement_quote(chunk: &str, env: &EnvMap) -> String {
    if chunk == "%" {
        return chunk.to_string();
    }
    if !chunk.ends_with('%') {
        return chunk.to_string();
    }
    let name = &chunk[..chunk.len() - 1];
    env_lookup(env, name).unwrap_or_else(|| chunk.to_string())
}

/// Windows `%name%` alternative.
#[cfg(windows)]
fn replacement_percent(whole: &[char], group: &str, env: &EnvMap) -> String {
    if group == "%" {
        return "%".to_string();
    }
    let original: String = whole.iter().collect();
    if !group.ends_with('%') {
        return original;
    }
    let name = &group[..group.len() - 1];
    env_lookup(env, name).unwrap_or(original)
}

/// `$name` / `${name}` alternative.
fn replacement_dollar(whole: &[char], group: &str, env: &EnvMap) -> String {
    let original: String = whole.iter().collect();
    let name = if group.starts_with('{') {
        if !group.ends_with('}') {
            return original;
        }
        &group[1..group.len() - 1]
    } else {
        group
    };
    #[cfg(windows)]
    if name == "$" {
        return "$".to_string();
    }
    env_lookup(env, name).unwrap_or(original)
}

/// `str(Path.home() / name)` for the auto-outdir candidates.
pub fn home_dir(home: &str, name: &str) -> String {
    py_path_join(home, name)
}

/// Python `~` expansion of a bare `~`, env-source only: HOME on posix;
/// USERPROFILE else HOMEDRIVE+HOMEPATH on windows. Python applies
/// `rstrip(sep)` and an `or sep` fallback; the passwd and `~user`
/// fallbacks are recorded as not ported.
pub fn home_from_env(env: &EnvMap) -> Option<String> {
    #[cfg(windows)]
    {
        let raw = if let Some(profile) = env.get("USERPROFILE") {
            profile.clone()
        } else if let Some(homepath) = env.get("HOMEPATH") {
            let drive = env.get("HOMEDRIVE").cloned().unwrap_or_default();
            nt_join(&drive, homepath)
        } else {
            return None;
        };
        let stripped = raw.trim_end_matches('\\').to_string();
        Some(if stripped.is_empty() {
            "\\".to_string()
        } else {
            stripped
        })
    }
    #[cfg(not(windows))]
    {
        let raw = env.get("HOME")?;
        let stripped = raw.trim_end_matches('/').to_string();
        Some(if stripped.is_empty() {
            "/".to_string()
        } else {
            stripped
        })
    }
}

// -------------------------------------------------------------- which

/// `os.path.split` for the two shapes this runs on (bare names and
/// absolute paths): rightmost separator; a lone leading separator is the
/// head; the drive-relative form falls back to splitroot.
fn path_split_name(path: &str) -> (String, String) {
    let chars: Vec<char> = path.chars().collect();
    match chars.iter().rposition(|&c| is_sep(c)) {
        Some(i) => {
            let head: String = chars[..i].iter().collect();
            let head = if head.is_empty() && i == 0 {
                chars[0].to_string()
            } else {
                head
            };
            let tail: String = chars[i + 1..].iter().collect();
            (head, tail)
        }
        None => {
            #[cfg(windows)]
            {
                let (d, _r, t) = splitroot(path);
                if !d.is_empty() {
                    return (d, t);
                }
            }
            (String::new(), path.to_string())
        }
    }
}

/// `shutil._winapi.NeedCurrentDirectoryForExePath` refinement is not
/// ported (only system-executable names differ; this product queries
/// four fixed non-system names), so a bare name always gets the `.`
/// candidate on windows - exactly what the python does for these names.
fn access_check(path: &str) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let Ok(cpath) = CString::new(Path::new(path).as_os_str().as_bytes()) else {
            return false;
        };
        unsafe { libc::access(cpath.as_ptr(), libc::X_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolve a candidate list against the real filesystem; first hit wins.
fn first_existing(cands: impl IntoIterator<Item = String>) -> Option<String> {
    cands
        .into_iter()
        .find(|c| !c.is_empty() && std::fs::metadata(c).map(|m| m.is_file()).unwrap_or(false))
}

/// `shutil.which(cmd)` with default mode: posix PATH scan with the real
/// `access(2)` X_OK; windows PATHEXT machinery, the extension-suffix
/// direct-match rule and the `.` curdir candidate.
pub fn which(cmd: &str, path_env: Option<&str>, pathext_env: Option<&str>) -> Option<String> {
    let pathsep = if cfg!(windows) { ';' } else { ':' };
    #[cfg(not(windows))]
    let _ = pathext_env;
    let (dirname, cmd) = path_split_name(cmd);
    let dirs: Vec<String> = if !dirname.is_empty() {
        vec![dirname]
    } else {
        let path = match path_env {
            Some(p) => p.to_string(),
            None => default_path(),
        };
        if path.is_empty() {
            return None;
        }
        let base: Vec<String> = path.split(pathsep).map(str::to_string).collect();
        #[cfg(windows)]
        {
            let mut dirs = base;
            dirs.insert(0, ".".to_string());
            dirs
        }
        #[cfg(not(windows))]
        {
            base
        }
    };
    let files: Vec<String> = {
        #[cfg(windows)]
        {
            const WIN_DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.WS;.MSC";
            let source = pathext_env.unwrap_or(WIN_DEFAULT_PATHEXT);
            let exts: Vec<String> = source
                .split(pathsep)
                .filter(|e| !e.is_empty())
                .map(|e| e.trim_end_matches('.').to_string())
                .collect();
            let mut files: Vec<String> = exts.iter().map(|e| format!("{cmd}{e}")).collect();
            let upper = cmd.to_uppercase();
            if exts.iter().any(|e| upper.ends_with(&e.to_uppercase())) {
                files.insert(0, cmd.to_string());
            }
            files
        }
        #[cfg(not(windows))]
        {
            vec![cmd.to_string()]
        }
    };
    let normcase = |d: &str| -> String {
        #[cfg(windows)]
        {
            d.replace('/', "\\").to_lowercase()
        }
        #[cfg(not(windows))]
        {
            d.to_string()
        }
    };
    let mut seen: Vec<String> = Vec::new();
    for dir in &dirs {
        let key = normcase(dir);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        for file in &files {
            let name = parser_join(dir, file);
            if access_check(&name) {
                return Some(name);
            }
        }
    }
    None
}

/// `os.environ.get("PATH")` closing fallbacks: `os.confstr("CS_PATH")`
/// then `os.defpath` on posix; `os.defpath` on windows (confstr raises
/// AttributeError there inside python).
fn default_path() -> String {
    #[cfg(unix)]
    {
        let mut buf = vec![0u8; 4096];
        let rc = unsafe {
            libc::confstr(
                libc::_CS_PATH,
                buf.as_mut_ptr() as *mut std::os::raw::c_char,
                buf.len(),
            )
        };
        if rc > 0 {
            let len = rc - 1;
            if len < buf.len() {
                buf.truncate(len);
                if let Ok(text) = String::from_utf8(buf)
                    && !text.is_empty()
                {
                    return text;
                }
            }
        }
        "/bin:/usr/bin".to_string()
    }
    #[cfg(not(unix))]
    {
        ".;C:\\bin".to_string()
    }
}

// ------------------------------------------------------- folder resolve

/// Python `re.search(r'^\s*KEY\s*=\s*"([^"]*)"', re.M)` over the XDG
/// file text: `^` anchors after `\n` only, `\s*` runs across lines.
pub fn xdg_dir(key: &str, text: &str, home: &str, env: &EnvMap) -> Option<String> {
    let starts = std::iter::once(0).chain(
        text.char_indices()
            .filter(|(_, c)| *c == '\n')
            .map(|(i, c)| i + c.len_utf8()),
    );
    for start in starts {
        let rest = &text[start..];
        let mut pos = 0usize;
        // \s* across lines
        while let Some(c) = rest[pos..].chars().next() {
            if py_space(c) {
                pos += c.len_utf8();
            } else {
                break;
            }
        }
        if !rest[pos..].starts_with(key) {
            continue;
        }
        pos += key.len();
        while let Some(c) = rest[pos..].chars().next() {
            if py_space(c) {
                pos += c.len_utf8();
            } else {
                break;
            }
        }
        if !rest[pos..].starts_with('=') {
            continue;
        }
        pos += 1;
        while let Some(c) = rest[pos..].chars().next() {
            if py_space(c) {
                pos += c.len_utf8();
            } else {
                break;
            }
        }
        if !rest[pos..].starts_with('"') {
            continue;
        }
        pos += 1;
        let Some(end) = rest[pos..].find('"') else {
            continue;
        };
        let value = &rest[pos..pos + end];
        let replaced = value.replace("$HOME", home);
        return Some(expandvars(&replaced, env));
    }
    None
}

/// `_auto_outdir_lookup`: registry candidates on windows (expandvars'd
/// like `_win_shell_folder`), the XDG file on posix, then `~/Desktop`
/// and `~/Downloads`, each gated on `os.path.isdir`.
pub fn auto_outdir_lookup(
    is_windows: bool,
    win_desktop: Option<&str>,
    win_downloads: Option<&str>,
    home: &str,
    env: &EnvMap,
) -> Result<Option<String>, ShapeKind> {
    let mut cands: Vec<Option<String>> = Vec::new();
    if is_windows {
        cands.push(win_desktop.map(|v| expandvars(v, env)));
        cands.push(win_downloads.map(|v| expandvars(v, env)));
    } else {
        let xdg_path = py_path_join(home, ".config/user-dirs.dirs");
        let text = match std::fs::read(&xdg_path) {
            Err(_) => None,
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(t) => Some(t),
                Err(_) => return Err(ShapeKind::UnicodeError),
            },
        };
        let text = text.as_deref();
        cands.push(text.and_then(|t| xdg_dir("XDG_DESKTOP_DIR", t, home, env)));
        cands.push(text.and_then(|t| xdg_dir("XDG_DOWNLOAD_DIR", t, home, env)));
    }
    cands.push(Some(home_dir(home, "Desktop")));
    cands.push(Some(home_dir(home, "Downloads")));
    for c in cands.into_iter().flatten() {
        if c.is_empty() {
            continue;
        }
        if std::fs::metadata(&c).map(|m| m.is_dir()).unwrap_or(false) {
            return Ok(Some(c));
        }
    }
    Ok(None)
}

/// Python `_outdir_cache`: filled once per process, failures included
/// (`None` answers are memoized; a raise is not).
#[derive(Default)]
pub struct OutdirCache {
    filled: Option<Option<String>>,
}

impl OutdirCache {
    pub fn get(
        &mut self,
        lookup: impl FnOnce() -> Result<Option<String>, ShapeKind>,
    ) -> Result<Option<String>, ShapeKind> {
        if let Some(v) = &self.filled {
            return Ok(v.clone());
        }
        let value = lookup()?;
        self.filled = Some(value.clone());
        Ok(value)
    }
}

// ------------------------------------------------------ binary lookup

/// `find_yt_dlp`: saved path, then `bin/` + the platform name, then
/// `which("yt-dlp") or which("yt-dlp.exe")`; first existing file wins.
pub fn find_yt_dlp(
    saved: &Obj,
    bin_dir: &str,
    platform: &str,
    which: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<Option<String>, ShapeKind> {
    let mut cands: Vec<String> = Vec::new();
    if let Some(v) = config::get(saved, "yt_dlp_path")
        && config::py_truthy(v)
    {
        match v {
            Value::Str(s) => cands.push(path_render(s)),
            _ => return Err(ShapeKind::TypeError),
        }
    }
    cands.push(py_path_join(bin_dir, platform_bin_name(platform)));
    let found = which("yt-dlp");
    let found = if found.is_some() {
        found
    } else {
        which("yt-dlp.exe")
    };
    if let Some(f) = found {
        cands.push(path_render(&f));
    }
    Ok(first_existing(cands))
}

/// `find_ffmpeg_dir`: saved path used as a dir, else its parent when the
/// parent exists; then `bin/ffmpeg[.exe]`; then `which("ffmpeg")`'s
/// parent directory.
pub fn find_ffmpeg_dir(
    saved: &Obj,
    bin_dir: &str,
    is_windows: bool,
    which: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<Option<String>, ShapeKind> {
    if let Some(v) = config::get(saved, "ffmpeg_path")
        && config::py_truthy(v)
    {
        let Value::Str(s) = v else {
            return Err(ShapeKind::TypeError);
        };
        let p = path_render(s);
        if std::fs::metadata(&p).map(|m| m.is_dir()).unwrap_or(false) {
            return Ok(Some(p));
        }
        let parent = py_path_parent(&p);
        if std::fs::metadata(&parent)
            .map(|m| m.is_dir())
            .unwrap_or(false)
        {
            return Ok(Some(parent));
        }
    }
    let binname = if is_windows { "ffmpeg.exe" } else { "ffmpeg" };
    let binfile = py_path_join(bin_dir, binname);
    if std::fs::metadata(&binfile)
        .map(|m| m.is_file())
        .unwrap_or(false)
    {
        return Ok(Some(bin_dir.to_string()));
    }
    let bindir_named = py_path_join(bin_dir, "ffmpeg");
    if std::fs::metadata(&bindir_named)
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        return Ok(Some(bin_dir.to_string()));
    }
    if let Some(hit) = which("ffmpeg") {
        return Ok(Some(py_path_parent(&path_render(&hit))));
    }
    Ok(None)
}

// ------------------------------------------------------ probe_version

#[derive(Clone, Debug)]
pub struct SpawnOutcome {
    pub stdout: String,
    pub stderr: String,
    pub rc: i64,
}

/// The process-lifetime memo of successful version probes; the key is
/// `(path string, mtime seconds)` exactly like python's
/// `(str(binary), os.path.getmtime(binary))`, with `None` for the
/// `OSError` case. Failures are never stored (audit F4).
#[derive(Default)]
pub struct VersionCache {
    map: std::collections::HashMap<(String, Option<u64>), (String, i64)>,
}

impl VersionCache {
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

fn mtime_bits(mtime: Option<f64>) -> Option<u64> {
    mtime.map(f64::to_bits)
}

/// `probe_version`: `--version` then `-version`, first rc 0 with a line
/// wins, both streams joined; the spawn outcome arrives through the
/// callback, a raised exception collapses to the shape-gated `error:`.
pub fn probe_version(
    binary: &str,
    mtime: Option<f64>,
    cache: &mut VersionCache,
    spawn: &mut dyn FnMut(&str, &str) -> Result<SpawnOutcome, String>,
) -> (String, i64) {
    let key = (binary.to_string(), mtime_bits(mtime));
    if let Some(hit) = cache.map.get(&key) {
        return hit.clone();
    }
    let mut last = ("no version output".to_string(), -1i64);
    for flag in ["--version", "-version"] {
        let outcome = match spawn(binary, flag) {
            Err(payload) => return (format!("error: {payload}"), -1),
            Ok(o) => o,
        };
        let mut joined = outcome.stdout;
        joined.push_str(&outcome.stderr);
        let blob = py_strip(&joined).to_string();
        let line = if blob.is_empty() {
            String::new()
        } else {
            py_strip(py_splitlines_first(&blob)).to_string()
        };
        if outcome.rc == 0 && !line.is_empty() {
            cache.map.insert(key.clone(), (line.clone(), 0));
            return (line, 0);
        }
        if !line.is_empty() {
            last = (line, outcome.rc);
        }
    }
    last
}

// ----------------------------------------------------- resolve_outdir

/// `resolve_outdir`: the saved folder when set, else the OS default,
/// else `downloads/` under the root; a truthy non-string saved value is
/// python's `.strip()` AttributeError carried as a shape.
pub fn resolve_outdir(saved: &Obj, auto: Option<&str>, root: &str) -> Result<String, ShapeKind> {
    let out: String = match config::get(saved, "output_dir") {
        None | Some(Value::Null) => String::new(),
        Some(Value::Str(s)) => py_strip(s).to_string(),
        Some(other) => {
            if config::py_truthy(other) {
                return Err(ShapeKind::AttributeError);
            }
            String::new()
        }
    };
    let chosen = if out.is_empty() {
        auto.map(str::to_string)
            .unwrap_or_else(|| "downloads".to_string())
    } else {
        out
    };
    Ok(if path_is_absolute(&chosen) {
        path_render(&chosen)
    } else {
        py_path_join(root, &chosen)
    })
}

// ---------------------------------------------------------- shapes

/// The shapes where python raises out of the resolution functions; the
/// tokens mirror the phase-3 harness vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    TypeError,
    AttributeError,
    UnicodeError,
}

impl ShapeKind {
    pub fn token(self) -> &'static str {
        match self {
            ShapeKind::TypeError => "type_error",
            ShapeKind::AttributeError => "attribute_error",
            ShapeKind::UnicodeError => "not_utf8",
        }
    }
}

impl std::fmt::Display for ShapeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "python shape {}", self.token())
    }
}

// --------------------------------------------------------- show_check

/// Everything `show_check` reads from its world; the arm and the CLI
/// fill it from injected state and from the real system respectively.
pub struct CheckWorld<'a> {
    pub platform: &'a str,
    pub root: &'a str,
    pub bin_dir: &'a str,
    pub config_path: &'a str,
    pub config_exists: bool,
    pub saved: &'a Obj,
    pub is_windows: bool,
    pub home: &'a str,
    pub env: &'a EnvMap,
    pub win_desktop: Option<&'a str>,
    pub win_downloads: Option<&'a str>,
}

/// `show_check` line by line, in python's print order.
pub fn show_check(
    world: &CheckWorld,
    which: &mut dyn FnMut(&str) -> Option<String>,
    probe: &mut dyn FnMut(&str) -> (String, i64),
) -> Result<Vec<String>, ShapeKind> {
    let binary = find_yt_dlp(world.saved, world.bin_dir, world.platform, which)?;
    let ffdir = find_ffmpeg_dir(world.saved, world.bin_dir, world.is_windows, which)?;
    let mut lines = Vec::with_capacity(7);
    lines.push(format!("platform      : {}", world.platform));
    lines.push(format!("project root  : {}", world.root));
    lines.push(format!(
        "yt-dlp binary : {}",
        binary.as_deref().unwrap_or("MISSING")
    ));
    if let Some(b) = &binary {
        let (ver, rc) = probe(b);
        lines.push(format!("yt-dlp version: {ver} (rc={rc})"));
    }
    lines.push(format!(
        "ffmpeg dir    : {}",
        ffdir
            .as_deref()
            .unwrap_or("MISSING - single files / no convert")
    ));
    let auto = auto_outdir_lookup(
        world.is_windows,
        world.win_desktop,
        world.win_downloads,
        world.home,
        world.env,
    )?;
    let outdir = resolve_outdir(world.saved, auto.as_deref(), world.root)?;
    lines.push(format!("output folder : {outdir}"));
    lines.push(format!(
        "settings file : {} (exists={})",
        world.config_path,
        py_bool(world.config_exists)
    ));
    Ok(lines)
}

// ------------------------------------------------------- real glue
//
// The impure lookups the CLI calls; the pure functions above stay
// injection-friendly.

/// Project root from the executable's own path (IF-1): the python gets
/// `Path(__file__).resolve().parent.parent`; the exe sits at the root.
pub fn real_root() -> Result<String, String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot resolve the executable path: {e}"))?;
    let parent = exe
        .parent()
        .ok_or_else(|| "the executable path has no parent directory".to_string())?;
    Ok(parent.to_string_lossy().to_string())
}

/// The process environment as the injected map (unicode entries only).
pub fn env_map_from_process() -> EnvMap {
    std::env::vars().collect()
}

/// `Path.home()`: env-source expansion (see `home_from_env`).
pub fn home_from_process(env: &EnvMap) -> Result<String, String> {
    home_from_env(env).ok_or_else(|| "could not determine the home directory".to_string())
}

/// `shutil.which(cmd)` against the real process environment.
pub fn which_from_process(cmd: &str) -> Option<String> {
    let path = std::env::var("PATH").ok();
    let pathext = std::env::var("PATHEXT").ok();
    which(cmd, path.as_deref(), pathext.as_deref())
}

/// `os.path.getmtime(path)` in seconds, `None` on OSError.
pub fn mtime_seconds(path: &str) -> Option<f64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    match modified.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => Some(d.as_secs_f64()),
        Err(e) => Some(-(e.duration().as_secs_f64())),
    }
}

/// One `subprocess.run([binary, flag], capture_output=True, text=True,
/// timeout=60)`; a timeout kills the child and lands in the `error:`
/// shape branch like python's `TimeoutExpired`.
pub fn spawn_version_probe(binary: &str, flag: &str) -> Result<SpawnOutcome, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let mut child = Command::new(binary)
        .arg(flag)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdout_pipe = child.stdout.take().ok_or("no stdout pipe")?;
    let mut stderr_pipe = child.stderr.take().ok_or("no stderr pipe")?;
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("Command '{}' timed out after 60 seconds", binary));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    let decode = |bytes: Vec<u8>, what: &str| -> Result<String, String> {
        String::from_utf8(bytes)
            .map_err(|_| format!("'utf-8' codec can't decode the {what} of {binary}"))
    };
    let rc = match status.code() {
        Some(code) => {
            #[cfg(windows)]
            {
                // Python reads the windows exit code unsigned (0xABAFB008
                // survives as a positive number).
                i64::from(code as u32)
            }
            #[cfg(not(windows))]
            {
                i64::from(code)
            }
        }
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                -(i64::from(status.signal().unwrap_or(0)))
            }
            #[cfg(not(unix))]
            {
                0
            }
        }
    };
    Ok(SpawnOutcome {
        stdout: decode(stdout, "stdout")?,
        stderr: decode(stderr, "stderr")?,
        rc,
    })
}

/// The two `_win_shell_folder` answers, registry value + expandvars,
/// `None` when the value is absent or empty; windows only.
#[cfg(windows)]
pub fn shell_folder_values(env: &EnvMap) -> (Option<String>, Option<String>) {
    let key = match registry_open_shell_folders() {
        Some(k) => k,
        None => return (None, None),
    };
    let desktop = registry_query_string(key, "Desktop");
    let downloads = registry_query_string(key, "{374DE290-123F-4565-9164-39C4925E467B}");
    unsafe { windows_sys::Win32::System::Registry::RegCloseKey(key) };
    let wrap = |v: Option<String>| v.filter(|s| !s.is_empty()).map(|s| expandvars(&s, env));
    (wrap(desktop), wrap(downloads))
}

#[cfg(windows)]
fn registry_open_shell_folders() -> Option<windows_sys::Win32::System::Registry::HKEY> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, KEY_READ, RegOpenKeyExW};

    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Shell Folders"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut key = std::ptr::null_mut();
    let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut key) };
    if rc == ERROR_SUCCESS { Some(key) } else { None }
}

#[cfg(windows)]
fn registry_query_string(
    key: windows_sys::Win32::System::Registry::HKEY,
    name: &str,
) -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::RegQueryValueExW;

    let name_w: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut len: u32 = 0;
    let mut ty: u32 = 0;
    unsafe {
        let rc = RegQueryValueExW(
            key,
            name_w.as_ptr(),
            std::ptr::null(),
            &mut ty,
            std::ptr::null_mut(),
            &mut len,
        );
        if rc != ERROR_SUCCESS || len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let rc = RegQueryValueExW(
            key,
            name_w.as_ptr(),
            std::ptr::null(),
            &mut ty,
            buf.as_mut_ptr(),
            &mut len,
        );
        if rc != ERROR_SUCCESS {
            return None;
        }
        let units: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
        Some(String::from_utf16_lossy(&units[..end]))
    }
}

// ------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("ytdlp-resolve-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn env(pairs: &[(&str, &str)]) -> EnvMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn platform_name_matrix() {
        assert_eq!(platform_name("nt", "win32"), "windows");
        assert_eq!(platform_name("posix", "linux"), "linux");
        assert_eq!(platform_name("posix", "linux2"), "linux");
        assert_eq!(platform_name("posix", "darwin"), "macos");
        assert_eq!(platform_name("nt", "darwin"), "windows");
        assert_eq!(platform_name("posix", "freebsd13"), "posix");
        assert_eq!(platform_name("java", "linux"), "linux");
    }

    #[test]
    fn path_render_spot_pins() {
        // pinned against the probed oracle matrices (2026-10-15)
        #[cfg(windows)]
        {
            assert_eq!(path_render("//x"), "\\\\x");
            assert_eq!(path_render("//x/y"), "\\\\x\\y\\");
            assert_eq!(path_render("c:/a/b"), "c:\\a\\b");
            assert_eq!(path_render("\\\\.\\COM1"), "\\\\.\\COM1");
            assert_eq!(path_render("a\\"), "a");
            assert_eq!(path_render("a/./b"), "a\\b");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(path_render("//x"), "//x");
            assert_eq!(path_render("///x"), "/x");
            assert_eq!(path_render("x//y"), "x/y");
            assert_eq!(path_render("x/./y"), "x/y");
            assert_eq!(path_render(""), ".");
            assert_eq!(path_render("/x/"), "/x");
        }
    }

    #[test]
    fn path_is_absolute_spot_pins() {
        #[cfg(windows)]
        {
            assert!(path_is_absolute("C:\\x"));
            assert!(path_is_absolute("//x/y"));
            assert!(!path_is_absolute("/x"));
            assert!(!path_is_absolute("\\x"));
            assert!(!path_is_absolute("C:x"));
        }
        #[cfg(not(windows))]
        {
            assert!(path_is_absolute("/x"));
            assert!(path_is_absolute("//x"));
            assert!(!path_is_absolute("C:\\x"));
            assert!(!path_is_absolute("x"));
        }
    }

    #[test]
    fn splitroot_spot_pins() {
        #[cfg(windows)]
        {
            assert_eq!(
                splitroot("C:\\x"),
                ("C:".to_string(), "\\".to_string(), "x".to_string())
            );
            assert_eq!(
                splitroot("C:x"),
                ("C:".to_string(), String::new(), "x".to_string())
            );
            assert_eq!(
                splitroot("\\\\server\\share\\a"),
                (
                    "\\\\server\\share".to_string(),
                    "\\".to_string(),
                    "a".to_string()
                )
            );
            assert_eq!(
                splitroot("\\\\.\\COM1"),
                ("\\\\.\\COM1".to_string(), String::new(), String::new())
            );
        }
        #[cfg(not(windows))]
        {
            assert_eq!(
                splitroot("//x"),
                (String::new(), "//".to_string(), "x".to_string())
            );
            assert_eq!(
                splitroot("///x"),
                (String::new(), "/".to_string(), "//x".to_string())
            );
            assert_eq!(
                splitroot("/x/y"),
                (String::new(), "/".to_string(), "x/y".to_string())
            );
        }
    }

    #[test]
    fn parser_join_spot_pins() {
        // Path(a)/b renders, pinned against the probed matrices
        assert_eq!(py_path_join("/root", "a//b"), "/root/a/b");
        assert_eq!(py_path_join("/root", "../x"), "/root/../x");
        assert_eq!(py_path_join("/root", "/x"), "/x");
        assert_eq!(py_path_join("/root", ""), "/root");
        #[cfg(windows)]
        {
            assert_eq!(py_path_join("C:\\root", "C:x"), "C:\\root\\x");
            assert_eq!(py_path_join("C:\\root", "C:\\x"), "C:\\x");
            assert_eq!(py_path_join("C:\\root", "/x"), "C:\\x");
            assert_eq!(py_path_join("C:\\root", ""), "C:\\root");
            assert_eq!(py_path_join("C:\\root", "bin"), "C:\\root\\bin");
        }
        assert_eq!(py_path_parent("/a/b/c"), "/a/b");
        assert_eq!(py_path_parent("a"), ".");
        assert_eq!(py_path_parent("/"), "/");
    }

    #[test]
    fn expandvars_spot_pins() {
        let e = env(&[("A", "1"), ("B", "two words"), ("EMPTY", "")]);
        assert_eq!(expandvars("$A", &e), "1");
        assert_eq!(expandvars("${A}", &e), "1");
        assert_eq!(expandvars("$A$B", &e), "1two words");
        assert_eq!(expandvars("$MISSING", &e), "$MISSING");
        assert_eq!(expandvars("$EMPTY.", &e), ".");
        assert_eq!(expandvars("$A", &e).len(), 1);
        #[cfg(windows)]
        {
            assert_eq!(expandvars("$$", &e), "$");
            assert_eq!(expandvars("%A%", &e), "1");
            assert_eq!(expandvars("%%", &e), "%");
            assert_eq!(expandvars("100%", &e), "100%");
            assert_eq!(expandvars("'$A'", &e), "'$A'");
            assert_eq!(expandvars("$A%%B", &env(&[("A", "1"), ("B", "b")])), "1%B");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(expandvars("$$", &e), "$$");
            assert_eq!(expandvars("%A%", &e), "%A%");
            assert_eq!(expandvars("$A-x", &e), "1-x", "the dash ends the name");
            assert_eq!(expandvars("'$A'", &e), "'1'");
        }
    }

    #[test]
    fn home_from_env_strips_trailing_seps() {
        #[cfg(not(windows))]
        {
            assert_eq!(
                home_from_env(&env(&[("HOME", "/home/u/")])),
                Some("/home/u".to_string())
            );
            assert_eq!(home_from_env(&env(&[("HOME", "/")])), Some("/".to_string()));
            assert_eq!(home_from_env(&env(&[])), None);
        }
        #[cfg(windows)]
        {
            assert_eq!(
                home_from_env(&env(&[("USERPROFILE", "C:\\Users\\u")])),
                Some("C:\\Users\\u".to_string())
            );
            assert_eq!(
                home_from_env(&env(&[("HOMEDRIVE", "C:"), ("HOMEPATH", "\\Users\\u")])),
                Some("C:\\Users\\u".to_string())
            );
            assert_eq!(home_from_env(&env(&[])), None);
        }
    }

    #[test]
    fn xdg_parse_semantics() {
        let e = env(&[("HOME", "/home/u")]);
        let text = "\n\n   XDG_DESKTOP_DIR   =   \"$HOME/Desk\"\n";
        assert_eq!(
            xdg_dir("XDG_DESKTOP_DIR", text, "/home/u", &e),
            Some("/home/u/Desk".to_string())
        );
        assert_eq!(
            xdg_dir(
                "XDG_DESKTOP_DIR",
                "XDG_DESKTOP_DIR=\"$HOME/D\"\nXDG_DOWNLOAD_DIR=\"$HOME/Dl\"\n",
                "/home/u",
                &e
            ),
            Some("/home/u/D".to_string())
        );
        assert_eq!(
            xdg_dir(
                "XDG_DESKTOP_DIR",
                "XDG_DESKTOP_DIR = $HOME/D\n",
                "/home/u",
                &e
            ),
            None
        );
        assert_eq!(
            xdg_dir(
                "XDG_DESKTOP_DIR",
                "xdg_desktop_dir=\"$HOME/D\"\n",
                "/home/u",
                &e
            ),
            None
        );
        assert_eq!(
            xdg_dir("XDG_DESKTOP_DIR", "XDG_DESKTOP_DIR=\"\"\n", "/home/u", &e),
            Some(String::new())
        );
    }

    #[cfg(unix)]
    #[test]
    fn which_posix_uses_the_exec_bit() {
        let dir = scratch("which");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("bin dir");
        let exe = bin.join("tool");
        std::fs::write(&exe, b"x").expect("file");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let path = bin.to_string_lossy().to_string();
        assert_eq!(
            which("tool", Some(&path), None),
            None,
            "0644 is not executable"
        );
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let expected = format!("{path}/tool");
        assert_eq!(which("tool", Some(&path), None), Some(expected.clone()));
        let dir_named = bin.join("ffmpeg");
        std::fs::create_dir_all(&dir_named).expect("dir");
        assert_eq!(which("ffmpeg", Some(&path), None), None, "dirs never hit");
        assert_eq!(which("tool", Some(""), None), None, "empty PATH");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn which_windows_pathext_machinery() {
        let dir = scratch("which");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("bin dir");
        std::fs::write(bin.join("tool.exe"), b"x").expect("file");
        let path = bin.to_string_lossy().to_string();
        // default PATHEXT: the .EXE spelling is constructed, not the file's
        let hit = which("tool", Some(&path), None).expect("found");
        assert!(hit.ends_with("tool.EXE"), "got {hit}");
        // a directory never hits
        std::fs::create_dir_all(bin.join("node")).expect("dir");
        assert_eq!(which("node", Some(&path), None), None);
        // direct match needs a PATHEXT extension
        let direct = which("tool.exe", Some(&path), None).expect("found");
        assert!(direct.ends_with("tool.exe"), "got {direct}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_yt_dlp_precedence_and_shapes() {
        let dir = scratch("find");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        std::fs::write(bin.join("yt-dlp"), b"x").expect("bin file");
        let bin_dir = bin.to_string_lossy().to_string();
        let mut calls: Vec<String> = Vec::new();
        let mut no_which = |name: &str| {
            calls.push(name.to_string());
            None
        };
        let found = find_yt_dlp(&Vec::new(), &bin_dir, "linux", &mut no_which)
            .expect("no shape")
            .expect("found");
        assert!(found.ends_with("yt-dlp"));
        assert_eq!(calls, vec!["yt-dlp".to_string(), "yt-dlp.exe".to_string()]);
        let saved = vec![("yt_dlp_path".to_string(), Value::Int(5))];
        let mut nothing = |_: &str| None;
        assert_eq!(
            find_yt_dlp(&saved, &bin_dir, "linux", &mut nothing),
            Err(ShapeKind::TypeError)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_outdir_shapes_and_fallbacks() {
        let mut none = Vec::new();
        assert_eq!(
            resolve_outdir(&none, None, "/root").expect("ok"),
            "/root/downloads"
        );
        assert_eq!(
            resolve_outdir(&none, Some("/auto"), "/root").expect("ok"),
            "/auto"
        );
        none.push(("output_dir".to_string(), Value::Str(" rel ".to_string())));
        assert_eq!(
            resolve_outdir(&none, None, "/root").expect("ok"),
            "/root/rel"
        );
        let junk = vec![("output_dir".to_string(), Value::Int(5))];
        assert_eq!(
            resolve_outdir(&junk, None, "/root"),
            Err(ShapeKind::AttributeError)
        );
    }

    #[test]
    fn f4_success_probes_memoize_but_failures_never_do() {
        // audit F4 (RUST-PORT-PLAN named regression)
        let mut cache = VersionCache::default();
        let spawns = std::cell::Cell::new(0);
        let mut ok_spawn = |_b: &str, _f: &str| {
            spawns.set(spawns.get() + 1);
            Ok(SpawnOutcome {
                stdout: "v1\n".to_string(),
                stderr: String::new(),
                rc: 0,
            })
        };
        let first = probe_version("bin/x", Some(1.5), &mut cache, &mut ok_spawn);
        let second = probe_version("bin/x", Some(1.5), &mut cache, &mut ok_spawn);
        assert_eq!(first, ("v1".to_string(), 0));
        assert_eq!(second, first);
        assert_eq!(
            spawns.get(),
            1,
            "one spawn for two identical successful probes"
        );
        let third = probe_version("bin/x", Some(2.5), &mut cache, &mut ok_spawn);
        assert_eq!(third.0, "v1");
        assert_eq!(spawns.get(), 2, "a new mtime misses the cache");

        let mut fail_cache = VersionCache::default();
        let fail_spawns = std::cell::Cell::new(0);
        let mut bad_spawn = |_b: &str, _f: &str| {
            fail_spawns.set(fail_spawns.get() + 1);
            Ok(SpawnOutcome {
                stdout: "A".to_string(),
                stderr: String::new(),
                rc: 1,
            })
        };
        let _ = probe_version("bin/y", Some(1.0), &mut fail_cache, &mut bad_spawn);
        let _ = probe_version("bin/y", Some(1.0), &mut fail_cache, &mut bad_spawn);
        assert_eq!(
            fail_spawns.get(),
            4,
            "two failing calls spawn both flags twice"
        );
        assert!(fail_cache.is_empty(), "no failure is ever stored");
    }

    #[test]
    fn f6_outdir_lookup_runs_once_across_calls() {
        // audit F6 (named regression): the process memo answers every call
        let mut cache = OutdirCache::default();
        let lookups = std::cell::Cell::new(0);
        let dir = scratch("outdir-f6");
        let desktop = dir.join("Desktop");
        std::fs::create_dir_all(&desktop).expect("desktop");
        let answer = desktop.to_string_lossy().to_string();
        for _ in 0..3 {
            let value = cache
                .get(|| {
                    lookups.set(lookups.get() + 1);
                    Ok(Some(answer.clone()))
                })
                .expect("ok");
            assert_eq!(value.as_deref(), Some(answer.as_str()));
        }
        assert_eq!(lookups.get(), 1, "one lookup answers three calls");
        let mut none_cache = OutdirCache::default();
        let none_lookups = std::cell::Cell::new(0);
        for _ in 0..2 {
            let value = none_cache
                .get(|| {
                    none_lookups.set(none_lookups.get() + 1);
                    Ok(None)
                })
                .expect("ok");
            assert_eq!(value, None);
        }
        assert_eq!(none_lookups.get(), 1, "a None answer is memoized too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn show_check_line_shapes() {
        let dir = scratch("check");
        let root = dir.join("root");
        let bin = root.join("bin");
        let home = dir.join("home");
        std::fs::create_dir_all(&bin).expect("bin");
        std::fs::create_dir_all(&home).expect("home");
        let saved = vec![("output_dir".to_string(), Value::Str(String::new()))];
        let root_s = root.to_string_lossy().to_string();
        let bin_s = bin.to_string_lossy().to_string();
        let home_s = home.to_string_lossy().to_string();
        let env_map = env(&[("HOME", &home_s)]);
        let world = CheckWorld {
            platform: "linux",
            root: &root_s,
            bin_dir: &bin_s,
            config_path: "/nowhere/settings.json",
            config_exists: false,
            saved: &saved,
            is_windows: false,
            home: &home_s,
            env: &env_map,
            win_desktop: None,
            win_downloads: None,
        };
        let mut which = |_: &str| None;
        let mut probe = |_: &str| ("unused".to_string(), -1);
        let lines = show_check(&world, &mut which, &mut probe).expect("lines");
        assert_eq!(lines[0], "platform      : linux");
        assert!(lines[2].starts_with("yt-dlp binary : "));
        assert!(lines[2].ends_with("MISSING"));
        assert!(lines[3].starts_with("ffmpeg dir    : MISSING - single files"));
        assert!(lines[4].starts_with("output folder : "));
        assert!(lines[4].ends_with("downloads"));
        assert!(lines[5].ends_with("(exists=False)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mtime_seconds_reads_a_file() {
        let dir = scratch("mtime");
        let file = dir.join("f");
        std::fs::write(&file, b"x").expect("file");
        let mtime = mtime_seconds(&file.to_string_lossy());
        assert!(mtime.is_some());
        assert!(mtime.expect("mtime") > 0.0);
        assert_eq!(mtime_seconds("/definitely/not/there"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
