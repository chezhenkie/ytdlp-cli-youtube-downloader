#!/usr/bin/env python3
"""Binary-level golden-transcript differ for the ytdlp native port (G6).

The two implementations are staged in private trees - the python at
`src/ytdlp.py` with a copied oracle source, the native binary beside an
empty `bin/` and its own `config/settings.json` - and run against the
same settings, the same stdin script and the same terminal size. The
tool compares stdout bytes, exit code, stderr presence and the final
settings.json bytes. Two tokens are projected before comparing, exactly
like the harness's {scratch}:

  - the staged tree path, as `{root}` (the two trees live side by side,
    so resolved output folders differ only by that prefix);
  - the header version `yt-dlp cli  vX.Y.Z` as `v{ver}`: the native
    product version is v2.0.0 by owner call while the ported behaviour
    carries python v1.5.0's transcripts (RUST-PORT-PLAN.md phase 6
    notes).

Modes:

    piped   both processes with a scripted stdin pipe (runs anywhere)
    pty     both processes under a pty with scripted keystroke bursts
            (posix only; the real-console windows check is OQ-N2, owner
            hands-on on the CI artifact)

Oracle: CPython 3.14.x is the pinned oracle interpreter (plan G2).

Usage:
    python session_diff.py piped [--rust-bin B] [--only NAME] [--keep]
    python session_diff.py pty   [--rust-bin B] [--only NAME] [--keep]

Exit codes: 0 pass, 1 a case failed, 2 instrument error.
"""
import argparse
import json
import os
import re
import select
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
APP_RUST = TOOLS_DIR.parent
REPO_ROOT = APP_RUST.parent
PY_SRC = REPO_ROOT / "app-python" / "src" / "ytdlp.py"
ORACLE = (3, 14)
VERSION_RE = re.compile(rb"(yt-dlp cli  v)\d+\.\d+\.\d+")


class InstrumentError(Exception):
    """The instrument itself cannot run; never a verdict."""


def default_rust_bin():
    exe = "ytdlp.exe" if os.name == "nt" else "ytdlp"
    return APP_RUST / "target" / "release" / exe


def require_oracle():
    if sys.version_info[:2] != ORACLE:
        raise InstrumentError(
            "oracle interpreter is CPython %d.%d.x, running %s"
            % (ORACLE[0], ORACLE[1], sys.version.split()[0])
        )


DEFAULT_SETTINGS = {
    "settings_version": 1,
    "yt_dlp_path": "",
    "ffmpeg_path": "",
    "auto_update": False,
    "auto_update_last_check": 0,
    "type": "video",
    "video_container": "mp4",
    "video_resolution": "1080",
    "audio_format": "mp3",
    "audio_quality": "0",
    "output_dir": "",
    "filename_template": "%(title)s",
    "subtitles": True,
    "subtitle_langs": "en",
    "playlist": False,
    "playlist_max_items": 0,
}


def settings_with(**over):
    cfg = dict(DEFAULT_SETTINGS)
    cfg.update(over)
    return cfg


def render_settings(obj):
    return (json.dumps(obj, indent=2, sort_keys=True) + "\n").encode("utf-8")


def stage_tree(kind, work, settings, rust_bin, keep):
    tree = work / kind
    (tree / "bin").mkdir(parents=True, exist_ok=True)
    (tree / "config").mkdir(parents=True, exist_ok=True)
    cfg = dict(settings)
    if isinstance(cfg.get("output_dir"), str) and cfg["output_dir"].startswith("{scratch}"):
        cfg["output_dir"] = cfg["output_dir"].replace("{scratch}", str(tree))
    (tree / "config" / "settings.json").write_bytes(render_settings(cfg))
    if kind == "py":
        (tree / "src").mkdir(parents=True, exist_ok=True)
        shutil.copyfile(PY_SRC, tree / "src" / "ytdlp.py")
        exe = None
    else:
        exe = tree / ("ytdlp.exe" if os.name == "nt" else "ytdlp")
        shutil.copyfile(rust_bin, exe)
        if os.name != "nt":
            exe.chmod(0o755)
    return tree, exe


def py_command(tree):
    args = [sys.executable, str(tree / "src" / "ytdlp.py"), "--menu"]
    env = {"PYTHONIOENCODING": "utf-8"}
    return args, env


def rust_command(tree, exe):
    return [str(exe), "--menu"], {}


def normalize(data, tree):
    data = data.replace(str(tree).encode("utf-8"), b"{root}")
    data = VERSION_RE.sub(rb"\1{ver}", data)
    return data


def compare_case(name, py, rs, work):
    py_rc, py_out, py_err, py_cfg = py
    rs_rc, rs_out, rs_err, rs_cfg = rs
    problems = []
    if py_rc != rs_rc:
        problems.append("exit code: python %d, native %d" % (py_rc, rs_rc))
    if py_out != rs_out:
        problems.append("stdout differs")
    if (not py_err) != (not rs_err):
        problems.append(
            "stderr presence differs: python %d bytes, native %d bytes"
            % (len(py_err), len(rs_err))
        )
    if py_cfg != rs_cfg:
        problems.append("settings.json differs")
    if not problems:
        print("PASS %s" % name)
        return True
    print("FAIL %s: %s" % (name, "; ".join(problems)))
    if py_out != rs_out:
        first = next(
            (
                i
                for i, (a, b) in enumerate(zip(py_out.split(b"\n"), rs_out.split(b"\n")))
                if a != b
            ),
            0,
        )
        exp = py_out.split(b"\n")[first][:110]
        act = rs_out.split(b"\n")[first][:110]
        print("  first stdout difference at line %d" % (first + 1))
        print("  python: %r" % exp)
        print("  native: %r" % act)
    if py_cfg != rs_cfg:
        print("  python settings: %r" % py_cfg[:220])
        print("  native settings: %r" % rs_cfg[:220])
    return False


def read_result(tree, proc, out, err):
    cfg = (tree / "config" / "settings.json").read_bytes()
    return (
        proc.returncode,
        normalize(out, tree),
        normalize(err, tree)[:4096],
        normalize(cfg, tree),
    )


# ---------------------------------------------------------------- piped

PIPED_CASES = [
    {
        "name": "quit",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"13\n",
    },
    {
        "name": "invalid-and-empty-choices",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"99\n\n14\n13\n",
    },
    {
        "name": "submenu-round-trip",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"7\nn\n13\n",
    },
    {
        "name": "langs-with-bad-code",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"7\n\nen,nl,8\n\n13\n",
    },
    {
        "name": "audio-only-switch",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"1\n3\n13\n",
    },
    {
        "name": "folder-typed",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"5\nrelfolder\n13\n",
    },
    {
        "name": "template-preset-and-custom",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"6\n2\n6\n%(title)s %(id)s custom\n13\n",
    },
    {
        "name": "playlist-flows",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"8\ny\n3\n8\ny\nabc\n\n13\n",
    },
    {
        "name": "reset-defaults",
        "settings": settings_with(output_dir="{scratch}/out", playlist=True, playlist_max_items=9),
        "stdin": b"10\n\n13\n",
    },
    {
        "name": "auto-update-silent-when-piped",
        "settings": settings_with(
            output_dir="{scratch}/out", auto_update=True, auto_update_last_check=0
        ),
        "stdin": b"13\n",
    },
    {
        "name": "corrupt-settings-coerced",
        "settings": settings_with(
            output_dir="{scratch}/out",
            video_container="junk",
            playlist_max_items="abc",
            audio_quality=9,
        ),
        "stdin": b"13\n",
    },
    {
        "name": "eof-mid-submenu",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"7\n",
    },
    {
        "name": "multi-url-empty-list",
        "settings": settings_with(output_dir="{scratch}/out"),
        "stdin": b"9\n\n13\n",
    },
]


def run_piped_case(case, work, rust_bin):
    py_tree, _ = stage_tree("py", work, case["settings"], rust_bin, False)
    rs_tree, rs_exe = stage_tree("rs", work, case["settings"], rust_bin, False)
    py_args, py_env = py_command(py_tree)
    rs_args, rs_env = rust_command(rs_tree, rs_exe)
    env = dict(os.environ)
    env.update(py_env)
    py_proc = subprocess.run(
        py_args, input=case["stdin"], capture_output=True, cwd=py_tree, env=env, timeout=120
    )
    env = dict(os.environ)
    env.update(rs_env)
    rs_proc = subprocess.run(
        rs_args, input=case["stdin"], capture_output=True, cwd=rs_tree, env=env, timeout=120
    )
    py = read_result(py_tree, py_proc, py_proc.stdout, py_proc.stderr)
    rs = read_result(rs_tree, rs_proc, rs_proc.stdout, rs_proc.stderr)
    return compare_case(case["name"], py, rs, work)


# ------------------------------------------------------------------ pty

PTY_CASES = [
    {
        "name": "pty-boot-and-quit",
        "settings": settings_with(output_dir="{scratch}/out"),
        "bursts": [b"13\n"],
    },
    {
        "name": "pty-escape-at-main-is-a-noop",
        "settings": settings_with(output_dir="{scratch}/out"),
        "bursts": [b"\x1b", b"13\n"],
    },
    {
        "name": "pty-escape-in-submenu-repaints",
        "settings": settings_with(output_dir="{scratch}/out"),
        "bursts": [b"7\n", b"\x1b", b"13\n"],
    },
    {
        "name": "pty-history-recall-and-backspace",
        "settings": settings_with(output_dir="{scratch}/out"),
        "bursts": [b"12\n", b"\n", b"\x1b[A\x1b[A", b"\x7f\x7f", b"13\n"],
    },
    {
        "name": "pty-item7-block-under-input",
        "settings": settings_with(output_dir="{scratch}/out"),
        "bursts": [b"7\n", b"\n", b"en,nl\n", b"13\n"],
    },
    {
        "name": "pty-item7-verdict-repaint",
        "settings": settings_with(output_dir="{scratch}/out"),
        "bursts": [b"7\n", b"\n", b"en,8\n", b"\n", b"13\n"],
    },
    {
        "name": "pty-invalid-pick-holds",
        "settings": settings_with(output_dir="{scratch}/out"),
        "bursts": [b"4\n", b"abc\n", b"\n", b"13\n"],
    },
    {
        "name": "pty-ctrl-c-at-main-exits-zero",
        "settings": settings_with(output_dir="{scratch}/out"),
        "bursts": [b"\x03"],
    },
]


def set_winsize(fd, cols, rows):
    import fcntl
    import struct
    import termios

    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def drain(master, data, quiet, deadline):
    """Read until `quiet` seconds of silence or the deadline."""
    last = time.monotonic()
    while time.monotonic() < deadline:
        ready, _, _ = select.select([master], [], [], quiet)
        now = time.monotonic()
        if not ready:
            if now - last >= quiet:
                return
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError:
            return
        if not chunk:
            return
        data.extend(chunk)
        last = time.monotonic()


def run_pty_case(case, work, rust_bin, cols=80, rows=24):
    results = []
    for kind, extra_env in (("py", {"PYTHONIOENCODING": "utf-8"}), ("rs", {})):
        tree, exe = stage_tree(kind, work, case["settings"], rust_bin, False)
        if kind == "py":
            cmd = [sys.executable, str(tree / "src" / "ytdlp.py"), "--menu"]
        else:
            cmd = [str(exe), "--menu"]
        master, slave = os.openpty()
        set_winsize(slave, cols, rows)
        err_file = open(work / ("stderr-" + kind + ".txt"), "wb")
        env = dict(os.environ)
        env.update(extra_env)
        proc = subprocess.Popen(
            cmd,
            stdin=slave,
            stdout=slave,
            stderr=err_file,
            cwd=tree,
            env=env,
            close_fds=True,
        )
        os.close(slave)
        data = bytearray()
        try:
            for burst in case["bursts"]:
                deadline = time.monotonic() + 8.0
                drain(master, data, 0.25, deadline)
                os.write(master, burst)
            deadline = time.monotonic() + 20.0
            while proc.poll() is None and time.monotonic() < deadline:
                drain(master, data, 0.25, deadline)
            if proc.poll() is None:
                proc.kill()
                raise InstrumentError("case %s (%s) did not exit" % (case["name"], kind))
            drain(master, data, 0.05, time.monotonic() + 1.0)
            rc = proc.wait(timeout=10)
        finally:
            err_file.close()
            os.close(master)
        stderr_bytes = (work / ("stderr-" + kind + ".txt")).read_bytes()
        cfg = (tree / "config" / "settings.json").read_bytes()
        results.append(
            (
                rc,
                normalize(bytes(data), tree),
                normalize(stderr_bytes, tree)[:4096],
                normalize(cfg, tree),
            )
        )
    return compare_case(case["name"], results[0], results[1], work)


# ------------------------------------------------------------------ main


def main(argv=None):
    parser = argparse.ArgumentParser(prog="session_diff.py", description=__doc__.splitlines()[0])
    parser.add_argument("mode", choices=["piped", "pty"])
    parser.add_argument("--rust-bin")
    parser.add_argument("--only", help="run one case by name")
    parser.add_argument("--keep", action="store_true", help="keep the staged trees")
    args = parser.parse_args(argv)

    try:
        require_oracle()
        rust_bin = Path(args.rust_bin) if args.rust_bin else default_rust_bin()
        if not rust_bin.exists():
            raise InstrumentError(
                "rust binary not found: %s (cargo build --release -p ytdlp-cli)" % rust_bin
            )
        if args.mode == "pty" and os.name == "nt":
            raise InstrumentError(
                "pty mode is posix; the windows console check is owner hands-on (OQ-N2)"
            )
        cases = PIPED_CASES if args.mode == "piped" else PTY_CASES
        if args.only:
            cases = [c for c in cases if c["name"] == args.only]
            if not cases:
                raise InstrumentError("no case named %r" % args.only)

        work = Path(tempfile.mkdtemp(prefix="ytdlp-session-"))
        passed = 0
        failed = 0
        try:
            for case in cases:
                ok = (
                    run_piped_case(case, work, rust_bin)
                    if args.mode == "piped"
                    else run_pty_case(case, work, rust_bin)
                )
                if ok:
                    passed += 1
                else:
                    failed += 1
        finally:
            if args.keep:
                print("trees kept in %s" % work)
            else:
                shutil.rmtree(work, ignore_errors=True)
        print(
            "SESSION-DIFF %s (%s: %d cases, %d passed, %d failed)"
            % ("FAIL" if failed else "PASS", args.mode, len(cases), passed, failed)
        )
        return 1 if failed else 0
    except InstrumentError as e:
        print("ERROR: %s" % e, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
