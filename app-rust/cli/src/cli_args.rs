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

mod help;

pub use help::{cli_width, format_help, format_usage, wrap_text};

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