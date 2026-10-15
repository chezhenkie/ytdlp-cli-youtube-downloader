//! Argument construction (specsheet section 4, layer 4): url cleaning,
//! JS-runtime detection mapping, subtitle language input.
//!
//! Ported one to one from `app-python/src/ytdlp.py` (the oracle for the
//! differential harness). Where the python original reads process state
//! (`shutil.which`), the port takes it as a parameter so the pure layer
//! stays pure; the impure lookup lands in its own layer.

use crate::pytext::py_space;

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

/// `'en,nl'` -> `'en(-.*)?,nl(-.*)?'` so en, en-GB, en-US and en-orig all
/// match; entries that already are patterns (`* ? ( [`) pass through.
pub fn sub_lang_patterns(langs: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    for part in langs.unwrap_or("").split(',') {
        let code = part.trim();
        if code.is_empty() {
            continue;
        }
        if code.contains(['*', '?', '(', '[']) {
            out.push(code.to_string());
        } else {
            out.push(format!("{code}(-.*)?"));
        }
    }
    if out.is_empty() {
        "en(-.*)?".to_string()
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
