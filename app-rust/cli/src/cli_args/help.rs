//! The help and usage renderer: python's argparse HelpFormatter port.
//! Reads the option table from the parent module.

use super::{OptionDef, OPTIONS, DESCRIPTION};
use crate::tty;

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
