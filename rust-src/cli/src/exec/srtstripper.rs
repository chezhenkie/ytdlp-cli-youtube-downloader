//! The srt timecode stripper (FR-20): after a successful subtitle pass
//! the standalone srtstripper binary runs over every subtitle file the
//! pass wrote, and the stripped copy lands next to the subtitle (the
//! tool's own output naming). The binary ships in `bin/` or lives on
//! PATH; a missing binary or a failing run is a report line, never a
//! download failure. The spawn goes through the same `RunRt.spawn` seam
//! as yt-dlp, so tests script whole passes offline.

use super::run::Spawned;
use crate::resolve;
use std::io::BufRead;

/// The srtstripper flag for a menu mode name. The text mode takes no
/// flag; an unknown mode string falls back to it. Stored settings values
/// are canonicalized by `config::srt_mode` before they reach this map.
pub fn mode_flag(mode: &str) -> Option<&'static str> {
    match mode {
        "text" => None,
        "timecode" => Some("--timecode"),
        "numbered" => Some("--numbered"),
        _ => None,
    }
}

fn is_sub_name(name: &str) -> bool {
    let lower = if cfg!(windows) {
        name.to_lowercase()
    } else {
        name.to_string()
    };
    lower.ends_with(".srt")
        || lower.ends_with(".vtt")
        || lower.ends_with(".ass")
        || lower.ends_with(".ssa")
}

/// The subtitle files `folder` holds with an mtime at/after `since`
/// (with the shared slack idiom of `newest_since`): the files a subtitle
/// pass wrote. Path spelling follows `newest_since` (pathlib render).
/// Read order, not mtime order: the report lists them as the directory
/// yields them.
pub fn subs_since(folder: &str, since: f64, slack: f64) -> Vec<String> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut found: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let text = path.to_string_lossy().into_owned();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_sub_name(&name) {
            continue;
        }
        let Some(mtime) = resolve::mtime_seconds(&text) else {
            continue;
        };
        if mtime >= since - slack {
            found.push(resolve::path_render(&text));
        }
    }
    found
}

/// The stripped output path out of the tool's report: the tail of the
/// last `PASS: ... -> <path>` line, empty when the tool said nothing.
fn pass_line_path(stdout: &str) -> String {
    stdout
        .lines()
        .rev()
        .find(|l| l.starts_with("PASS:"))
        .and_then(|line| line.rsplit_once(" -> "))
        .map(|(_, path)| path.trim().to_string())
        .unwrap_or_default()
}

fn read_all(reader: &mut (dyn BufRead + Send)) -> String {
    let mut text = String::new();
    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => text.push_str(&buf),
        }
    }
    text
}

/// One srtstripper run over one file through the spawn seam. Ok carries
/// the output path (empty when the tool printed no PASS line); Err
/// carries "srtstripper failed (rc=N): <first output line>".
pub fn run_one(
    binary: &str,
    input: &str,
    mode: &str,
    spawn: &mut dyn FnMut(&str, &[String]) -> std::io::Result<Spawned>,
) -> Result<String, String> {
    let mut args: Vec<String> = vec![input.to_string()];
    if let Some(flag) = mode_flag(mode) {
        args.push(flag.to_string());
    }
    let spawned = match spawn(binary, &args) {
        Ok(spawned) => spawned,
        Err(e) => return Err(format!("srtstripper failed: could not start: {e}")),
    };
    let Spawned {
        mut stdout,
        mut stderr,
        mut child,
    } = spawned;
    let stdout = read_all(stdout.as_mut());
    let stderr = read_all(stderr.as_mut());
    let rc = child.wait();
    if rc != 0 {
        let first = stderr
            .lines()
            .chain(stdout.lines())
            .map(|l| l.trim())
            .find(|l| !l.is_empty())
            .unwrap_or("no output");
        return Err(format!("srtstripper failed (rc={rc}): {first}"));
    }
    Ok(pass_line_path(&stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OkChild(i64);

    impl super::super::run::ChildHandle for OkChild {
        fn poll(&mut self) -> Option<i64> {
            Some(self.0)
        }
        fn wait(&mut self) -> i64 {
            self.0
        }
        fn pid(&self) -> u32 {
            7
        }
    }

    fn scripted_spawn(
        stdout: &'static str,
        stderr: &'static str,
        rc: i64,
    ) -> impl FnMut(&str, &[String]) -> std::io::Result<Spawned> {
        move |binary: &str, _args: &[String]| {
            assert_eq!(binary, "srtstripper");
            Ok(Spawned {
                stdout: Box::new(std::io::Cursor::new(stdout.as_bytes().to_vec())),
                stderr: Box::new(std::io::Cursor::new(stderr.as_bytes().to_vec())),
                child: Box::new(OkChild(rc)),
            })
        }
    }

    #[test]
    fn mode_flag_maps_the_menu_modes() {
        assert_eq!(mode_flag("text"), None);
        assert_eq!(mode_flag("timecode"), Some("--timecode"));
        assert_eq!(mode_flag("numbered"), Some("--numbered"));
        assert_eq!(
            mode_flag("junk"),
            None,
            "unknown mode falls back to the text mode"
        );
    }

    #[test]
    fn run_one_reads_the_pass_line() {
        let mut spawn = scripted_spawn(
            "PASS: mode=default, wrote 5 lines from 9 input lines -> /out/video.en - alpha.txt\n",
            "",
            0,
        );
        let got = run_one("srtstripper", "/in/video.en.srt", "text", &mut spawn).unwrap();
        assert_eq!(got, "/out/video.en - alpha.txt");
    }

    #[test]
    fn run_one_passes_the_mode_flag() {
        let seen = std::rc::Rc::<std::cell::RefCell<Vec<Vec<String>>>>::default();
        let seen2 = std::rc::Rc::clone(&seen);
        let mut spawn = move |_binary: &str, args: &[String]| -> std::io::Result<Spawned> {
            seen2.borrow_mut().push(args.to_vec());
            Ok(Spawned {
                stdout: Box::new(std::io::Cursor::new(Vec::new())),
                stderr: Box::new(std::io::Cursor::new(Vec::new())),
                child: Box::new(OkChild(0)),
            })
        };
        assert_eq!(
            run_one("srtstripper", "x.srt", "timecode", &mut spawn).unwrap(),
            ""
        );
        let want: Vec<Vec<String>> = vec![vec!["x.srt".to_string(), "--timecode".to_string()]];
        assert_eq!(*seen.borrow(), want);
        assert_eq!(
            run_one("srtstripper", "x.srt", "text", &mut spawn).unwrap(),
            ""
        );
        assert_eq!(seen.borrow().last().unwrap(), &vec!["x.srt".to_string()]);
    }

    #[test]
    fn run_one_reports_the_failure() {
        let mut spawn = scripted_spawn("", "FAIL: input not found: x.srt\n", 2);
        let got = run_one("srtstripper", "x.srt", "text", &mut spawn);
        assert_eq!(
            got,
            Err("srtstripper failed (rc=2): FAIL: input not found: x.srt".to_string())
        );
    }

    #[test]
    fn run_one_reports_a_start_failure() {
        let mut spawn = |_binary: &str, _args: &[String]| -> std::io::Result<Spawned> {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no exe"))
        };
        let got = run_one("srtstripper", "x.srt", "text", &mut spawn);
        assert!(
            got.unwrap_err()
                .starts_with("srtstripper failed: could not start")
        );
    }

    #[test]
    fn subs_since_keeps_only_fresh_subtitle_files() {
        let dir = scratch_dir("subs-since");
        let old = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        write_file(&dir, "video.en.srt", b"1\n");
        write_file(&dir, "video.de.vtt", b"2\n");
        write_file(&dir, "notes.txt", b"3\n");
        write_file(&dir, "video.fr.srt", b"4\n");
        let got = subs_since(&dir, old, 2.0);
        let names: Vec<String> = got
            .iter()
            .map(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect();
        assert!(names.contains(&"video.en.srt".to_string()));
        assert!(names.contains(&"video.de.vtt".to_string()));
        assert!(names.contains(&"video.fr.srt".to_string()));
        assert!(!names.contains(&"notes.txt".to_string()));
    }

    fn scratch_dir(tag: &str) -> String {
        let dir = std::env::temp_dir().join(format!("srt-strip-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    fn write_file(dir: &str, name: &str, body: &[u8]) {
        std::fs::write(std::path::Path::new(dir).join(name), body).unwrap();
    }
}
