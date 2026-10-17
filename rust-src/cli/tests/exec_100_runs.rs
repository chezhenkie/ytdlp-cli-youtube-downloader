//! Phase-7 download hygiene: 100 consecutive `run_download` runs against
//! the scripted stand-in (G7: "100 consecutive runs leave
//! no lingering processes, temp files, or handles"). Each run must exit
//! cleanly, its child must be gone afterwards, the argv handed over must
//! be identical every time, no work folder may survive and the test
//! process must not accumulate file descriptors.
//!
//! The stand-in is found next to the test binary; when the workspace has
//! not been built (`cargo test -p ytdlp-cli` alone) the test prints SKIP
//! loudly instead of claiming a pass.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use ytdlp_cli::exec;

fn target_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    exe.parent()
        .and_then(|deps| deps.parent())
        .expect("target dir")
        .to_path_buf()
}

fn fake_path() -> PathBuf {
    let name = if cfg!(windows) {
        "fake-ytdlp.exe"
    } else {
        "fake-ytdlp"
    };
    target_dir().join(name)
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        // A recycled pid is not a lingering child: the run loop only
        // returns after try_wait observed the exit, so a bare pid that
        // is alive later must be a different process (the OS recycles
        // pids; CI run 38767929250 flagged one such pid). Require the
        // stand-in's image name in the tasklist match too.
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output();
        match out {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
                text.contains(&pid.to_string()) && text.contains("fake-ytdlp")
            }
            Err(_) => false,
        }
    }
}

fn fd_count() -> Option<usize> {
    for dir in ["/proc/self/fd", "/dev/fd"] {
        if let Ok(entries) = fs::read_dir(dir) {
            return Some(entries.count());
        }
    }
    None
}

#[test]
fn hundred_runs_leave_no_processes_or_temp_files() {
    let fake = fake_path();
    if !fake.exists() {
        eprintln!(
            "SKIP hundred_runs_leave_no_processes_or_temp_files: {} is not built \
             (run cargo build --workspace first)",
            fake.display()
        );
        return;
    }
    let work = std::env::temp_dir().join(format!("ytdlp-100runs-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    let outdir = work.join("out");
    fs::create_dir_all(&outdir).expect("outdir");
    let argv_file = work.join("argv.txt");
    let pid_file = work.join("pid.txt");

    // test binaries are single-test files, so process-wide env is safe
    unsafe {
        std::env::set_var("FAKE_YTDLP_MODE", "ok");
        std::env::set_var("FAKE_YTDLP_SIZE", "32768");
        std::env::set_var("FAKE_YTDLP_NAME", "video.mp4");
        std::env::set_var("FAKE_YTDLP_ARGV", &argv_file);
        std::env::set_var("FAKE_YTDLP_PID_FILE", &pid_file);
    }

    let binary = fake.to_string_lossy().into_owned();
    let opts: Vec<String> = [
        "-f",
        "b",
        "--newline",
        "-P",
        &format!("home:{}", outdir.display()),
        "-P",
        "temp:.ytdlp-tmp",
        "--no-playlist",
        "https://example.invalid/v",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();

    let fds_before = fd_count();
    let mut pids: Vec<u32> = Vec::with_capacity(100);
    for run in 0..100 {
        let mut rt = exec::RunRt::real();
        let (rc, stopped, err, codes) = exec::run_download(&binary, &opts, None, false, &mut rt);
        assert_eq!(
            (rc, stopped, err.as_str(), codes.len()),
            (0, false, "", 0),
            "run {run} did not finish cleanly"
        );
        let pid_text = fs::read_to_string(&pid_file).expect("pid file");
        pids.push(pid_text.trim().parse::<u32>().expect("pid"));
    }

    // every child is gone (a small grace for the kernel reaping)
    std::thread::sleep(Duration::from_millis(300));
    let alive: Vec<u32> = pids.iter().copied().filter(|p| process_alive(*p)).collect();
    assert!(
        alive.is_empty(),
        "pids still alive after their run: {alive:?}"
    );

    // the argv handed over is identical on every run
    let text = fs::read_to_string(&argv_file).expect("argv file");
    let blocks: Vec<&str> = text.split("\n\n").filter(|b| !b.is_empty()).collect();
    assert_eq!(blocks.len(), 100, "one argv record per run");
    assert!(
        blocks.iter().all(|b| *b == blocks[0]),
        "argv drifted between runs"
    );

    // no work folder survives, the finished file is there
    assert!(
        !outdir.join(".ytdlp-tmp").exists(),
        "work folder left behind"
    );
    assert_eq!(
        fs::metadata(outdir.join("video.mp4"))
            .expect("final file")
            .len(),
        32768
    );

    // and the test process did not accumulate descriptors
    if let (Some(before), Some(after)) = (fds_before, fd_count()) {
        assert!(
            after <= before + 4,
            "file descriptors grew: {before} -> {after}"
        );
    }

    fs::remove_dir_all(&work).expect("cleanup");
}
