//! Quick-mode command line (specsheet IF-2): an argparse-compatible
//! parser for the fixed flag set plus the argparse help/usage/error text
//! at any width, because python's flag set, accepted values, help text
//! and rejection behaviour are part of the product contract and are
//! identical in both implementations.
//!
//! Every rule here is a port of CPython 3.14 `argparse` (verbs taken from
//! the stdlib source and from probes on 2026-10-15): the token
//! classification (exact, `=`, unambiguous abbreviation, short clusters,
//! unknown option-like -> extras, negative numbers and tokens with a
//! space -> positional), the pattern interleave for one `nargs="?"`
//! positional, the consume rules (`expected one argument`, `ignored
//! explicit argument`, `ambiguous option`, choice validation), the usage
//! wrapping algorithm and the action-help wrapping (a `textwrap.wrap`
//! port; the width sweep 11..=200 is digest-pinned against the oracle).
//!
//! Schema in `_actions` order: help, url, audio, format, resolution, af,
//! outdir, template, subs, playlist, menu, check, update, selftest.

use crate::config::{AUDIO_FORMATS, CONTAINERS, RESOLUTIONS};
use crate::pytext::py_repr_str;
use crate::tty;

/// The parser's description line.
pub const DESCRIPTION: &str = "yt-dlp CLI wrapper";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dest {
    Help,
    Audio,
    Format,
    Resolution,
    Af,
    Outdir,
    Template,
    Subs,
    Playlist,
    Menu,
    Check,
    Update,
    Selftest,
}

#[derive(Clone, Copy)]
struct OptionDef {
    /// Registration order; the first entry is the usage/help spelling.
    names: &'static [&'static str],
    /// False for store_true/help actions (argparse `nargs=0`).
    takes_value: bool,
    choices: Option<&'static [&'static str]>,
    /// Default metavar (argparse: `dest.upper()` for value options).
    metavar: &'static str,
    help: &'static str,
    dest: Dest,
}

const OPTIONS: &[OptionDef] = &[
    OptionDef {
        names: &["-h", "--help"],
        takes_value: false,
        choices: None,
        metavar: "",
        help: "show this help message and exit",
        dest: Dest::Help,
    },
    OptionDef {
        names: &["-a", "--audio"],
        takes_value: false,
        choices: None,
        metavar: "",
        help: "audio only",
        dest: Dest::Audio,
    },
    OptionDef {
        names: &["-f", "--format"],
        takes_value: true,
        choices: Some(&CONTAINERS),
        metavar: "",
        help: "container",
        dest: Dest::Format,
    },
    OptionDef {
        names: &["-r", "--resolution"],
        takes_value: true,
        choices: Some(&RESOLUTIONS),
        metavar: "",
        help: "max height",
        dest: Dest::Resolution,
    },
    OptionDef {
        names: &["--af"],
        takes_value: true,
        choices: Some(&AUDIO_FORMATS),
        metavar: "",
        help: "audio format",
        dest: Dest::Af,
    },
    OptionDef {
        names: &["-o", "--outdir"],
        takes_value: true,
        choices: None,
        metavar: "OUTDIR",
        help: "output folder",
        dest: Dest::Outdir,
    },
    OptionDef {
        names: &["--template"],
        takes_value: true,
        choices: None,
        metavar: "TEMPLATE",
        help: "output filename template",
        dest: Dest::Template,
    },
    OptionDef {
        names: &["--subs"],
        takes_value: false,
        choices: None,
        metavar: "",
        help: "subtitles",
        dest: Dest::Subs,
    },
    OptionDef {
        names: &["--playlist"],
        takes_value: false,
        choices: None,
        metavar: "",
        help: "playlist mode",
        dest: Dest::Playlist,
    },
    OptionDef {
        names: &["--menu"],
        takes_value: false,
        choices: None,
        metavar: "",
        help: "force menu even with URL",
        dest: Dest::Menu,
    },
    OptionDef {
        names: &["--check"],
        takes_value: false,
        choices: None,
        metavar: "",
        help: "show resolved paths/versions",
        dest: Dest::Check,
    },
    OptionDef {
        names: &["--update"],
        takes_value: false,
        choices: None,
        metavar: "",
        help: "update the yt-dlp binary in place",
        dest: Dest::Update,
    },
    OptionDef {
        names: &["--selftest"],
        takes_value: false,
        choices: None,
        metavar: "",
        help: "run built-in logic tests",
        dest: Dest::Selftest,
    },
];

/// The parsed namespace (python's `argparse.Namespace` fields).
#[derive(Default, Debug, PartialEq)]
pub struct Invocation {
    pub url: Option<String>,
    pub audio: bool,
    pub format: Option<String>,
    pub resolution: Option<String>,
    pub af: Option<String>,
    pub outdir: Option<String>,
    pub template: Option<String>,
    pub subs: bool,
    pub playlist: bool,
    pub menu: bool,
    pub check: bool,
    pub update: bool,
    pub selftest: bool,
}

/// One parse outcome: the namespace, the help text (stdout, exit 0), or
/// the usage + error text (stderr, exit 2).
#[derive(Debug)]
pub enum Outcome {
    Parsed(Invocation),
    Help(String),
    Error(String),
}

/// The argparse formatter width: `COLUMNS` when it parses positive, else
/// the stdout terminal size, else the `shutil.get_terminal_size` fallback
/// of 80, minus 2 (argparse's own default).
pub fn cli_width() -> i64 {
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|c| *c > 0);
    let columns = match columns {
        Some(c) => c,
        None => match tty::terminal_size_stdout() {
            Some((c, _)) if c > 0 => c,
            _ => 80,
        },
    };
    columns - 2
}

// ------------------------------------------------------------ usage text

/// One action invocation as argparse renders it: `-h, --help`,
/// `-f, --format {mp4,mkv,webm,best}`, `-o, --outdir OUTDIR`, `url`.
/// (3.14 joins the option strings and appends the value form once.)
fn action_invocation(def: &OptionDef) -> String {
    let names = def.names.join(", ");
    if def.takes_value {
        format!("{names} {}", args_string(def))
    } else {
        names
    }
}

/// `_format_args`: choices in braces, else the metavar.
fn args_string(def: &OptionDef) -> String {
    match def.choices {
        Some(choices) => format!("{{{}}}", choices.join(",")),
        None => def.metavar.to_string(),
    }
}

/// The usage form of one optional: first name, value appended, in
/// brackets.
fn usage_part(def: &OptionDef) -> String {
    let name = def.names[0];
    if def.takes_value {
        format!("[{name} {}]", args_string(def))
    } else {
        format!("[{name}]")
    }
}

/// `_format_usage` (wrap branch included), ending with one newline like
/// `format_usage()`.
pub fn format_usage(prog: &str, width: i64) -> String {
    let prefix = "usage: ";
    let opt_parts: Vec<String> = OPTIONS.iter().map(usage_part).collect();
    let opt_usage = opt_parts.join(" ");
    let pos_usage = "[url]";
    let usage = format!("{prog} {opt_usage} {pos_usage}");
    let text_width = width;
    let prefix_len = prefix.chars().count() as i64;
    let usage = if prefix_len + usage.chars().count() as i64 > text_width {
        let opt_split = split_usage_parts(&opt_usage);
        let pos_split = split_usage_parts(pos_usage);
        let prog_len = prog.chars().count() as i64;
        if (prefix_len + prog_len) as f64 <= 0.75 * text_width as f64 {
            let indent = " ".repeat((prefix_len + prog_len + 1) as usize);
            let mut first: Vec<String> = vec![prog.to_string()];
            first.extend(opt_split);
            let mut lines = get_lines(&first, &indent, Some(prefix), text_width);
            lines.extend(get_lines(&pos_split, &indent, None, text_width));
            lines.join("\n")
        } else {
            let indent = " ".repeat(prefix_len as usize);
            let mut parts = opt_split.clone();
            parts.extend(pos_split.clone());
            let mut lines = get_lines(&parts, &indent, None, text_width);
            if lines.len() > 1 {
                lines = get_lines(&opt_split, &indent, None, text_width);
                lines.extend(get_lines(&pos_split, &indent, None, text_width));
            }
            // 3.14 puts the long prog on its own line
            let mut all: Vec<String> = vec![prog.to_string()];
            all.extend(lines);
            all.join("\n")
        }
    } else {
        usage
    };
    format!("{prefix}{usage}\n")
}

/// argparse's `part_regexp = r'\(.*?\)+|\[.*?\]+|\S+'` split.
fn split_usage_parts(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == '[' || c == '(' {
            let close = if c == '[' { ']' } else { ')' };
            let mut part = String::new();
            part.push(c);
            chars.next();
            for c2 in chars.by_ref() {
                part.push(c2);
                if c2 == close {
                    break;
                }
            }
            while chars.peek() == Some(&close) {
                part.push(close);
                chars.next();
            }
            out.push(part);
        } else {
            let mut part = String::new();
            while let Some(&c2) = chars.peek() {
                if c2.is_whitespace() {
                    break;
                }
                part.push(c2);
                chars.next();
            }
            out.push(part);
        }
    }
    out
}

/// argparse's `get_lines` helper for usage wrapping.
fn get_lines(parts: &[String], indent: &str, prefix: Option<&str>, text_width: i64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line: Vec<String> = Vec::new();
    let indent_len = indent.chars().count() as i64;
    let mut line_len = match prefix {
        Some(p) => p.chars().count() as i64 - 1,
        None => indent_len - 1,
    };
    for part in parts {
        let part_len = part.chars().count() as i64;
        if line_len + 1 + part_len > text_width && !line.is_empty() {
            lines.push(format!("{indent}{}", line.join(" ")));
            line.clear();
            line_len = indent_len - 1;
        }
        line.push(part.clone());
        line_len += 1 + part_len;
    }
    if !line.is_empty() {
        lines.push(format!("{indent}{}", line.join(" ")));
    }
    if let Some(p) = prefix
        && let Some(first) = lines.first_mut()
    {
        let cut = indent_len as usize;
        *first = first.chars().skip(cut).collect();
        let _ = p;
    }
    lines
}

// ------------------------------------------------------------- help text

/// `format_help()` for this schema, ending with one newline.
pub fn format_help(prog: &str, width: i64) -> String {
    let actions: Vec<(String, &'static str)> = {
        let mut v: Vec<(String, &'static str)> =
            vec![("url".to_string(), "download this URL with saved defaults")];
        for def in OPTIONS {
            v.push((action_invocation(def), def.help));
        }
        v
    };
    // argparse: action_max_length counts the 2-space section indent
    let action_max_length = actions
        .iter()
        .map(|(inv, _)| inv.chars().count() as i64 + 2)
        .max()
        .unwrap_or(0);
    let max_help_position = (width - 20).clamp(4, 24);
    let help_position = std::cmp::min(action_max_length + 2, max_help_position);

    let mut out = String::new();
    out.push_str(&format_usage(prog, width));
    out.push('\n');
    // argparse's add_text wraps the description at the formatter width
    let description_lines = wrap_text(DESCRIPTION, width);
    out.push_str(&description_lines.join("\n"));
    out.push_str("\n\n");
    // the positional section and the options section, each as
    // '\n' + heading + items + '\n' (argparse's _Section)
    out.push('\n');
    out.push_str("positional arguments:\n");
    out.push_str(&action_block(
        "url",
        "download this URL with saved defaults",
        help_position,
        width,
    ));
    out.push('\n');
    out.push('\n');
    out.push_str("options:\n");
    for def in OPTIONS {
        out.push_str(&action_block(
            &action_invocation(def),
            def.help,
            help_position,
            width,
        ));
    }
    out.push('\n');
    let collapsed = collapse_blank_runs(&out);
    format!("{collapsed}\n")
}

/// argparse's `_long_break_matcher.sub('\n\n', help).strip('\n')`.
fn collapse_blank_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = 0usize;
    for c in text.chars() {
        if c == '\n' {
            run += 1;
            if run <= 2 {
                out.push('\n');
            }
        } else {
            run = 0;
            out.push(c);
        }
    }
    out.trim_matches('\n').to_string()
}

/// argparse `_format_action` for one action with help text.
fn action_block(invocation: &str, help: &str, help_position: i64, width: i64) -> String {
    let indent = 2i64;
    let action_width = help_position - indent - 2;
    let inv_len = invocation.chars().count() as i64;
    let help_width = std::cmp::max(width - help_position, 11);
    let help_lines = wrap_text(help, help_width);
    let mut out = String::new();
    if inv_len <= action_width {
        out.push_str(&format!(
            "{:indent$}{:<action_width$}  ",
            "",
            invocation,
            indent = indent as usize,
            action_width = action_width as usize
        ));
        if let Some(first) = help_lines.first() {
            out.push_str(first);
            out.push('\n');
        }
        for line in &help_lines[1..] {
            out.push_str(&" ".repeat(help_position as usize));
            out.push_str(line);
            out.push('\n');
        }
    } else {
        out.push_str(&format!(
            "{:indent$}{}\n",
            "",
            invocation,
            indent = indent as usize
        ));
        for line in &help_lines {
            out.push_str(&" ".repeat(help_position.max(0) as usize));
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

// -------------------------------------------------------------- textwrap

fn is_letter_like(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

/// `textwrap.wrap(text, width)` with the argparse defaults
/// (break_long_words, break_on_hyphens, drop_whitespace on).
pub fn wrap_text(text: &str, width: i64) -> Vec<String> {
    let width = width.max(1) as usize;
    let mut chunks: Vec<String> = Vec::new();
    for word in text.split_inclusive(|c: char| c.is_whitespace()) {
        let (word_part, ws_part) = split_trailing_whitespace(word);
        if !word_part.is_empty() {
            chunks.extend(split_word_hyphens(word_part));
        }
        if !ws_part.is_empty() {
            // python munges every whitespace char to a single space
            chunks.push(" ".repeat(ws_part.chars().count()));
        }
    }
    wrap_chunks(&mut chunks, width)
}

fn split_trailing_whitespace(word: &str) -> (&str, &str) {
    match word.find(|c: char| c.is_whitespace()) {
        Some(i) => (&word[..i], &word[i..]),
        None => (word, ""),
    }
}

/// The `wordsep_re` hyphen rule: a hyphen splits the word when the two
/// preceding characters are letters, or the pattern letter-`-`-letter
/// precedes it, and a letter (optionally `-` + letter) follows.
fn split_word_hyphens(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;
    for i in 0..chars.len() {
        if chars[i] != '-' {
            continue;
        }
        let before_ok = i >= 2 && is_letter_like(chars[i - 1]) && is_letter_like(chars[i - 2])
            || (i >= 3
                && is_letter_like(chars[i - 1])
                && chars[i - 2] == '-'
                && is_letter_like(chars[i - 3]));
        let after_ok = i + 1 < chars.len()
            && is_letter_like(chars[i + 1])
            && (i + 2 >= chars.len()
                || chars[i + 2] != '-'
                || (i + 3 < chars.len() && is_letter_like(chars[i + 3])));
        if before_ok && after_ok {
            out.push(chars[start..=i].iter().collect());
            start = i + 1;
        }
    }
    out.push(chars[start..].iter().collect());
    out
}

/// textwrap's `_wrap_chunks` core for one positional, no indents, no
/// max_lines.
fn wrap_chunks(chunks: &mut Vec<String>, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    chunks.reverse();
    while !chunks.is_empty() {
        let mut cur_line: Vec<String> = Vec::new();
        let mut cur_len = 0usize;
        if !lines.is_empty() && chunks.last().is_none_or(|c| c.trim().is_empty()) {
            chunks.pop();
        }
        while let Some(last) = chunks.last() {
            let l = last.chars().count();
            if cur_len + l <= width {
                cur_line.push(chunks.pop().unwrap());
                cur_len += l;
            } else {
                break;
            }
        }
        if let Some(mut chunk) = chunks.pop_if(|last| last.chars().count() > width) {
            let space_left = width - cur_len;
            if space_left > 0 {
                let chunk_chars: Vec<char> = chunk.chars().collect();
                let mut end = space_left;
                if chunk_chars.len() > space_left
                    && let Some(h) = chunk_chars[..space_left].iter().rposition(|c| *c == '-')
                    && h > 0
                    && chunk_chars[..h].iter().any(|c| *c != '-')
                {
                    end = h + 1;
                }
                let head: String = chunk_chars[..end].iter().collect();
                chunk = chunk_chars[end..].iter().collect();
                cur_line.push(head);
                chunks.push(chunk);
            } else if cur_line.is_empty() {
                cur_line.push(chunk);
            } else {
                chunks.push(chunk);
            }
        }
        if let Some(last) = cur_line.last()
            && last.trim().is_empty()
        {
            cur_line.pop();
        }
        if !cur_line.is_empty() {
            lines.push(cur_line.concat());
        }
    }
    lines
}

// --------------------------------------------------------------- parser

#[derive(Clone, Debug)]
enum TokenClass {
    Argument,
    Terminator,
    Unknown,
    Known {
        def: usize,
        /// `(explicit value, came from an `=` split)`
        explicit: Option<(String, bool)>,
    },
    Ambiguous(Vec<&'static str>),
}

fn negative_number(token: &str) -> bool {
    let body = &token[1..];
    if body.is_empty() {
        return false;
    }
    if let Some((a, b)) = body.split_once('.') {
        a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit())
    } else {
        body.chars().all(|c| c.is_ascii_digit())
    }
}

fn exact_option(token: &str) -> Option<usize> {
    for (i, def) in OPTIONS.iter().enumerate() {
        if def.names.contains(&token) {
            return Some(i);
        }
    }
    None
}

/// `_parse_optional` + `_get_option_tuples` classification.
fn classify(token: &str) -> TokenClass {
    if token.is_empty() || !token.starts_with('-') {
        return TokenClass::Argument;
    }
    if let Some(def) = exact_option(token) {
        return TokenClass::Known {
            def,
            explicit: None,
        };
    }
    if token.chars().count() == 1 {
        return TokenClass::Argument;
    }
    if let Some((head, tail)) = token.split_once('=')
        && let Some(def) = exact_option(head)
    {
        return TokenClass::Known {
            def,
            explicit: Some((tail.to_string(), true)),
        };
    }
    // candidate tuples
    let mut cands: Vec<(usize, Option<(String, bool)>)> = Vec::new();
    let mut names: Vec<&'static str> = Vec::new();
    let mut chars = token.chars();
    let first = chars.next().unwrap_or(' ');
    let second = chars.next().unwrap_or(' ');
    if first == '-' && second == '-' {
        let (prefix, explicit) = match token.split_once('=') {
            Some((p, v)) => (p, Some((v.to_string(), true))),
            None => (token, None),
        };
        for (i, def) in OPTIONS.iter().enumerate() {
            for name in def.names {
                if name.starts_with(prefix) && name.starts_with("--") {
                    cands.push((i, explicit.clone()));
                    names.push(name);
                }
            }
        }
    } else if first == '-' && second != '-' {
        let (prefix, explicit) = match token.split_once('=') {
            Some((p, v)) => (p, Some((v.to_string(), true))),
            None => (token, None),
        };
        let short_prefix: String = token.chars().take(2).collect();
        let short_explicit: String = token.chars().skip(2).collect();
        for (i, def) in OPTIONS.iter().enumerate() {
            for name in def.names {
                if *name == short_prefix {
                    // argparse: sep '' and the raw tail (which may hold '=')
                    cands.push((i, Some((short_explicit.clone(), false))));
                    names.push(name);
                } else if name.starts_with(prefix) {
                    cands.push((i, explicit.clone()));
                    names.push(name);
                }
            }
        }
    }
    if cands.len() > 1 {
        return TokenClass::Ambiguous(names);
    }
    if let Some((def, explicit)) = cands.into_iter().next() {
        return TokenClass::Known { def, explicit };
    }
    if negative_number(token) {
        return TokenClass::Argument;
    }
    if token.contains(' ') {
        return TokenClass::Argument;
    }
    TokenClass::Unknown
}

fn action_name(def: &OptionDef) -> String {
    def.names.join("/")
}

fn invalid_choice(def: &OptionDef, value: &str) -> String {
    // the pinned 3.14.4 oracle renders the choices with str(); 3.14.6
    // switched to repr (measured 2026-10-15) - the port follows the
    // pinned oracle and CI pins 3.14.4
    let choices: Vec<&str> = def.choices.unwrap_or(&[]).to_vec();
    format!(
        "argument {}: invalid choice: {} (choose from {})",
        action_name(def),
        py_repr_str(value),
        choices.join(", ")
    )
}

/// `parse_args`: the namespace, the help text, or the error text (already
/// including the usage block), matching python's stdout/stderr split and
/// exit codes 0/2.
pub fn parse(prog: &str, args: &[String], width: i64) -> Outcome {
    let classes: Vec<TokenClass> = {
        let mut v: Vec<TokenClass> = Vec::with_capacity(args.len());
        let mut after_terminator = false;
        for token in args {
            if after_terminator {
                v.push(TokenClass::Argument);
                continue;
            }
            if token == "--" {
                v.push(TokenClass::Terminator);
                after_terminator = true;
                continue;
            }
            v.push(classify(token));
        }
        v
    };

    let mut parsed = Invocation::default();
    let mut extras: Vec<String> = Vec::new();
    let mut i = 0usize;
    let n = args.len();
    while i < n {
        match &classes[i] {
            TokenClass::Terminator => {
                if parsed.url.is_none() && matches!(classes.get(i + 1), Some(TokenClass::Argument))
                {
                    parsed.url = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            TokenClass::Argument => {
                if parsed.url.is_none() {
                    parsed.url = Some(args[i].clone());
                } else {
                    extras.push(args[i].clone());
                }
                i += 1;
            }
            TokenClass::Unknown => {
                extras.push(args[i].clone());
                i += 1;
            }
            TokenClass::Ambiguous(names) => {
                let message = format!(
                    "ambiguous option: {} could match {}",
                    args[i],
                    names.join(", ")
                );
                return Outcome::Error(usage_and_error(prog, width, &message));
            }
            TokenClass::Known { def, explicit } => {
                // python's consume_optional cluster loop: a store_true
                // option with a tail ("-af", "-az") continues on
                // "-" + tail, or files the tail as an extra.
                let mut current_idx = *def;
                let mut pending: Option<(String, bool)> = explicit.clone();
                loop {
                    let def = &OPTIONS[current_idx];
                    if def.dest == Dest::Help && pending.is_none() {
                        return Outcome::Help(format_help(prog, width));
                    }
                    match pending.take() {
                        Some((value, from_eq)) => {
                            if def.takes_value {
                                if let Err(message) = store_value(&mut parsed, def, &value) {
                                    return Outcome::Error(usage_and_error(prog, width, &message));
                                }
                                break;
                            }
                            if value.is_empty() || from_eq || value.starts_with('-') {
                                let message = format!(
                                    "argument {}: ignored explicit argument {}",
                                    action_name(def),
                                    py_repr_str(&value)
                                );
                                return Outcome::Error(usage_and_error(prog, width, &message));
                            }
                            if let Err(message) = store_value(&mut parsed, def, "") {
                                return Outcome::Error(usage_and_error(prog, width, &message));
                            }
                            let head = value.chars().next().unwrap_or(' ');
                            let next_token = format!("-{head}");
                            match exact_option(&next_token) {
                                None => {
                                    extras.push(format!("-{value}"));
                                    break;
                                }
                                Some(idx) => {
                                    let rest: String = value.chars().skip(1).collect();
                                    pending = if rest.is_empty() {
                                        None
                                    } else if let Some(after) = rest.strip_prefix('=') {
                                        Some((after.to_string(), true))
                                    } else {
                                        Some((rest, false))
                                    };
                                    current_idx = idx;
                                    continue;
                                }
                            }
                        }
                        None => {
                            if def.takes_value {
                                if !matches!(classes.get(i + 1), Some(TokenClass::Argument)) {
                                    let message = format!(
                                        "argument {}: expected one argument",
                                        action_name(def)
                                    );
                                    return Outcome::Error(usage_and_error(prog, width, &message));
                                }
                                let value = args[i + 1].clone();
                                if let Err(message) = store_value(&mut parsed, def, &value) {
                                    return Outcome::Error(usage_and_error(prog, width, &message));
                                }
                                i += 1;
                                break;
                            }
                            if let Err(message) = store_value(&mut parsed, def, "") {
                                return Outcome::Error(usage_and_error(prog, width, &message));
                            }
                            break;
                        }
                    }
                }
                i += 1;
            }
        }
    }

    if !extras.is_empty() {
        let message = format!("unrecognized arguments: {}", extras.join(" "));
        return Outcome::Error(usage_and_error(prog, width, &message));
    }
    Outcome::Parsed(parsed)
}

/// Store one option's value (or set a flag), with choice validation.
fn store_value(parsed: &mut Invocation, def: &OptionDef, value: &str) -> Result<(), String> {
    if let Some(choices) = def.choices
        && !choices.contains(&value)
    {
        return Err(invalid_choice(def, value));
    }
    match def.dest {
        Dest::Help => {}
        Dest::Audio => parsed.audio = true,
        Dest::Format => parsed.format = Some(value.to_string()),
        Dest::Resolution => parsed.resolution = Some(value.to_string()),
        Dest::Af => parsed.af = Some(value.to_string()),
        Dest::Outdir => parsed.outdir = Some(value.to_string()),
        Dest::Template => parsed.template = Some(value.to_string()),
        Dest::Subs => parsed.subs = true,
        Dest::Playlist => parsed.playlist = true,
        Dest::Menu => parsed.menu = true,
        Dest::Check => parsed.check = true,
        Dest::Update => parsed.update = true,
        Dest::Selftest => parsed.selftest = true,
    }
    Ok(())
}

fn usage_and_error(prog: &str, width: i64, message: &str) -> String {
    let mut out = format_usage(prog, width);
    out.push_str(&format!("{prog}: error: {message}\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help_78() -> String {
        format_help("ytdlp.py", 78)
    }

    #[test]
    fn help_at_78_matches_the_oracle_capture() {
        let expected = concat!(
            "usage: ytdlp.py [-h] [-a] [-f {mp4,mkv,webm,best}]\n",
            "                [-r {2160,1440,1080,720,480,360,best}]\n",
            "                [--af {mp3,m4a,opus,wav,best}] [-o OUTDIR]\n",
            "                [--template TEMPLATE] [--subs] [--playlist] [--menu] [--check]\n",
            "                [--update] [--selftest]\n",
            "                [url]\n",
            "\n",
            "yt-dlp CLI wrapper\n",
            "\n",
            "positional arguments:\n",
            "  url                   download this URL with saved defaults\n",
            "\n",
            "options:\n",
            "  -h, --help            show this help message and exit\n",
            "  -a, --audio           audio only\n",
            "  -f, --format {mp4,mkv,webm,best}\n",
            "                        container\n",
            "  -r, --resolution {2160,1440,1080,720,480,360,best}\n",
            "                        max height\n",
            "  --af {mp3,m4a,opus,wav,best}\n",
            "                        audio format\n",
            "  -o, --outdir OUTDIR   output folder\n",
            "  --template TEMPLATE   output filename template\n",
            "  --subs                subtitles\n",
            "  --playlist            playlist mode\n",
            "  --menu                force menu even with URL\n",
            "  --check               show resolved paths/versions\n",
            "  --update              update the yt-dlp binary in place\n",
            "  --selftest            run built-in logic tests\n",
        );
        assert_eq!(help_78(), expected);
    }

    #[test]
    fn help_at_38_matches_the_oracle_capture() {
        let expected = concat!(
            "usage: ytdlp.py [-h] [-a]\n",
            "                [-f {mp4,mkv,webm,best}]\n",
            "                [-r {2160,1440,1080,720,480,360,best}]\n",
            "                [--af {mp3,m4a,opus,wav,best}]\n",
            "                [-o OUTDIR]\n",
            "                [--template TEMPLATE]\n",
            "                [--subs] [--playlist]\n",
            "                [--menu] [--check]\n",
            "                [--update]\n",
            "                [--selftest]\n",
            "                [url]\n",
            "\n",
            "yt-dlp CLI wrapper\n",
            "\n",
            "positional arguments:\n",
            "  url             download this URL\n",
            "                  with saved defaults\n",
            "\n",
            "options:\n",
            "  -h, --help      show this help\n",
            "                  message and exit\n",
            "  -a, --audio     audio only\n",
            "  -f, --format {mp4,mkv,webm,best}\n",
            "                  container\n",
            "  -r, --resolution {2160,1440,1080,720,480,360,best}\n",
            "                  max height\n",
            "  --af {mp3,m4a,opus,wav,best}\n",
            "                  audio format\n",
            "  -o, --outdir OUTDIR\n",
            "                  output folder\n",
            "  --template TEMPLATE\n",
            "                  output filename\n",
            "                  template\n",
            "  --subs          subtitles\n",
            "  --playlist      playlist mode\n",
            "  --menu          force menu even with\n",
            "                  URL\n",
            "  --check         show resolved\n",
            "                  paths/versions\n",
            "  --update        update the yt-dlp\n",
            "                  binary in place\n",
            "  --selftest      run built-in logic\n",
            "                  tests\n",
        );
        assert_eq!(format_help("ytdlp.py", 38), expected);
    }

    /// The full width sweep digest: the oracle's `-h` output for every
    /// formatter width 11..=200, LF-normalized, separated by `==W==\n`;
    /// probed from the real CLI 2026-10-15 (CPython 3.14.6 COLUMNS=w+2,
    /// `\r\n` normalized to `\n`).
    #[test]
    fn help_width_sweep_matches_the_oracle_digest() {
        let mut blob: Vec<u8> = Vec::new();
        for w in 11..=200i64 {
            blob.extend_from_slice(format_help("ytdlp.py", w).as_bytes());
            blob.extend_from_slice(format!("=={w}==\n").as_bytes());
        }
        // dev aid: dump the local blob to diff against the oracle probe
        if let Ok(path) = std::env::var("SWEEP_BLOB_OUT") {
            let _ = std::fs::write(path, &blob);
        }
        assert_eq!(
            crate::sha256::sha256_hex(&blob),
            "68a3325d83e53008fd041656c5d2613dbb0e1aa21f824bd1dbd15b0ce0debef7"
        );
    }

    fn parse_at(args: &[&str]) -> Outcome {
        let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        parse("ytdlp.py", &owned, 78)
    }

    fn error_of(args: &[&str]) -> String {
        match parse_at(args) {
            Outcome::Error(text) => text
                .trim_end_matches('\n')
                .rsplit_once('\n')
                .map(|(_, last)| last.to_string())
                .unwrap_or_default(),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn error_texts_match_the_oracle_probes() {
        assert_eq!(
            error_of(&["-f", "junk"]).trim_end(),
            "ytdlp.py: error: argument -f/--format: invalid choice: 'junk' \
             (choose from mp4, mkv, webm, best)"
        );
        assert_eq!(
            error_of(&["--nope", "notaurl"]).trim_end(),
            "ytdlp.py: error: unrecognized arguments: --nope"
        );
        assert_eq!(
            error_of(&["a", "b"]).trim_end(),
            "ytdlp.py: error: unrecognized arguments: b"
        );
        assert_eq!(
            error_of(&["-weird"]).trim_end(),
            "ytdlp.py: error: unrecognized arguments: -weird"
        );
        assert_eq!(
            error_of(&["-f"]).trim_end(),
            "ytdlp.py: error: argument -f/--format: expected one argument"
        );
        assert_eq!(
            error_of(&["-o", "-zzz", "notaurl"]).trim_end(),
            "ytdlp.py: error: argument -o/--outdir: expected one argument"
        );
        assert_eq!(
            error_of(&["--template", "-a", "notaurl"]).trim_end(),
            "ytdlp.py: error: argument --template: expected one argument"
        );
        assert_eq!(
            error_of(&["--template", "-nope", "notaurl"]).trim_end(),
            "ytdlp.py: error: argument --template: expected one argument"
        );
        assert_eq!(
            error_of(&["--template", "--", "notaurl"]).trim_end(),
            "ytdlp.py: error: argument --template: expected one argument"
        );
        assert_eq!(
            error_of(&["--subs=1"]).trim_end(),
            "ytdlp.py: error: argument --subs: ignored explicit argument '1'"
        );
        assert_eq!(
            error_of(&["--a", "notaurl"]).trim_end(),
            "ytdlp.py: error: ambiguous option: --a could match --audio, --af"
        );
        assert_eq!(
            error_of(&["--s", "notaurl"]).trim_end(),
            "ytdlp.py: error: ambiguous option: --s could match --subs, --selftest"
        );
        assert_eq!(
            error_of(&["-f", "junk", "-h"]).trim_end(),
            "ytdlp.py: error: argument -f/--format: invalid choice: 'junk' \
             (choose from mp4, mkv, webm, best)"
        );
    }

    #[test]
    fn parse_results_match_the_oracle_probes() {
        // hostile argv: argparse rejects before any url is taken
        assert_eq!(
            error_of(&["--nope", "notaurl"]).trim_end(),
            "ytdlp.py: error: unrecognized arguments: --nope"
        );
        assert_eq!(
            error_of(&["-weird"]).trim_end(),
            "ytdlp.py: error: unrecognized arguments: -weird"
        );
        // accepted argv: the url the parser hands over (probed through
        // the CLI's not-a-URL echo)
        for (args, url) in [
            (vec!["--"], None),
            (vec!["--", "-weird"], Some("-weird")),
            (vec!["notaurl", "-a"], Some("notaurl")),
            (vec!["-fmp4", "notaurl"], Some("notaurl")),
            (vec!["--format=mp4", "notaurl"], Some("notaurl")),
            (vec!["-af", "mp4", "notaurl"], Some("notaurl")),
            (vec!["--au", "notaurl"], Some("notaurl")),
            (vec!["--che", "notaurl"], Some("notaurl")),
        ] {
            match parse_at(&args) {
                Outcome::Parsed(inv) => {
                    assert_eq!(inv.url.as_deref(), url, "args {args:?}");
                }
                other => panic!("args {args:?}: expected parsed, got {other:?}"),
            }
        }
        let Outcome::Parsed(inv) = parse_at(&["-af", "mp4", "notaurl"]) else {
            panic!("cluster parse failed");
        };
        assert!(inv.audio && inv.format.as_deref() == Some("mp4"));
        let Outcome::Parsed(inv) = parse_at(&["--che", "notaurl"]) else {
            panic!("abbreviated check parse failed");
        };
        assert!(inv.check && inv.url.as_deref() == Some("notaurl"));
        let Outcome::Parsed(inv) = parse_at(&["--au", "notaurl"]) else {
            panic!("abbreviated audio parse failed");
        };
        assert!(inv.audio);
        match parse_at(&["-h", "junk"]) {
            Outcome::Help(text) => assert_eq!(text, help_78()),
            other => panic!("expected help, got {other:?}"),
        }
    }
}
