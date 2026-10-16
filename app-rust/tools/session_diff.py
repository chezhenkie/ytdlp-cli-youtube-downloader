#!/usr/bin/env python3
"""Binary-level golden-transcript differ for the ytdlp native port (G6, G7).

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

Phase 7 adds executable cases: a case may stage the scripted yt-dlp
stand-in (`app-rust/fake-ytdlp`) into the tree's `bin/`, pin PATH to that
bin so a runner's real ffmpeg/node cannot leak in, script the stand-in
per case, and compare the argv the wrapper handed over plus a directory
listing. Animated stage rewrites are collapsed to `{frame}` in cases that
declare `project_frames` (frame counts are timing, not behaviour; the
frame content itself is digest-pinned by cli tests). The oracle's listing
drops entries under the work folder before the compare: by the FR-11
amendment (owner call 2026-10-16c) every native run end removes the work
folder, so a work entry surviving a native run is exactly what the case
still catches.

Modes:

    piped   both processes with a scripted stdin pipe (runs anywhere)
    pty     both processes under a pty with scripted keystroke bursts
            (posix only; the real-console windows check is OQ-N2, owner
            hands-on on the CI artifact)

Oracle: CPython 3.14.4 exactly (argparse text changed in a later patch;
see tools/harness.py).

Usage:
    python session_diff.py piped [--rust-bin B] [--fake-bin F] [--only NAME] [--keep]
    python session_diff.py pty   [--rust-bin B] [--fake-bin F] [--only NAME] [--keep]

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
ORACLE = (3, 14, 4)
VERSION_RE = re.compile(rb"(yt-dlp cli  v)\d+\.\d+\.\d+")


class InstrumentError(Exception):
    """The instrument itself cannot run; never a verdict."""


def default_rust_bin():
    exe = "ytdlp.exe" if os.name == "nt" else "ytdlp"
    return APP_RUST / "target" / "release" / exe


def default_fake_bin():
    exe = "fake-ytdlp.exe" if os.name == "nt" else "fake-ytdlp"
    return APP_RUST / "target" / "release" / exe


def require_oracle():
    # exactly 3.14.4, like tools/harness.py: argparse text changed in a
    # later patch (invalid-choice choices quoted, measured 2026-10-15)
    if sys.version_info[:3] != ORACLE:
        raise InstrumentError(
            "oracle interpreter is CPython %d.%d.%d, running %s"
            % (ORACLE[0], ORACLE[1], ORACLE[2], sys.version.split()[0])
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


def stage_tree(kind, work, case, rust_bin, fake_bin):
    tree = work / kind
    # one fresh tree per case: a leftover bin/ or out/ from the previous
    # case would leak into resolution and the listings (measured
    # 2026-10-15: a staged ffmpeg changed a later case's ffmpeg offer)
    if tree.exists():
        shutil.rmtree(tree)
    (tree / "bin").mkdir(parents=True, exist_ok=True)
    (tree / "config").mkdir(parents=True, exist_ok=True)
    cfg = dict(case["settings"])
    if isinstance(cfg.get("output_dir"), str) and cfg["output_dir"].startswith("{scratch}"):
        # Path() collapses the mixed separators into the native form
        cfg["output_dir"] = str(Path(cfg["output_dir"].replace("{scratch}", str(tree))))
    (tree / "config" / "settings.json").write_bytes(render_settings(cfg))
    # scripted stand-ins: one copy per declared name, native exe suffix
    for name in (case.get("bin") or {}):
        suffix = ".exe" if os.name == "nt" else ""
        target = tree / "bin" / (name + suffix)
        shutil.copyfile(fake_bin, target)
        if os.name != "nt":
            target.chmod(0o755)
    if kind == "py":
        (tree / "src").mkdir(parents=True, exist_ok=True)
        shutil.copyfile(PY_SRC, tree / "src" / "ytdlp.py")
        exe = None
    else:
        # staged under the oracle's script name: argparse's help and
        # usage wrap depend on len(prog), so the two fixtures must carry
        # the same program name for a byte comparison (the real launcher
        # names are environment, RUST-PORT-PLAN phase 7 notes)
        exe = tree / "ytdlp.py"
        shutil.copyfile(rust_bin, exe)
        if os.name != "nt":
            exe.chmod(0o755)
    return tree, exe


def case_env(tree, case, kind):
    env = {}
    if kind == "py":
        env["PYTHONIOENCODING"] = "utf-8"
    spec = (case.get("bin") or {}).get("yt-dlp")
    if spec is not None:
        # PATH is pinned to the tree's bin: a CI machine's real ffmpeg or
        # node must not leak into resolution
        env["PATH"] = str(tree / "bin")
        env["FAKE_YTDLP_MODE"] = spec.get("mode", "ok")
        env["FAKE_YTDLP_SUB_MODE"] = spec.get("sub_mode", "sub-ok")
        if "size" in spec:
            env["FAKE_YTDLP_SIZE"] = str(spec["size"])
        if "name" in spec:
            env["FAKE_YTDLP_NAME"] = spec["name"]
        if "sleep" in spec:
            env["FAKE_YTDLP_SLEEP"] = str(spec["sleep"])
        if case.get("argv_file"):
            env["FAKE_YTDLP_ARGV"] = str(tree / case["argv_file"])
    for key, value in (case.get("env") or {}).items():
        env[key] = value.replace("{root}", str(tree))
    return env


def case_args(case):
    return list(case.get("args") or ["--menu"])


def py_command(tree, case):
    return [sys.executable, str(tree / "src" / "ytdlp.py")] + case_args(case)


def rust_command(tree, exe, case):
    return [str(exe)] + case_args(case)


FRAME_RE = re.compile(rb"(?:\r\x1b\[K[^\r\n]*)+(?=\r\x1b\[K)")
# the archive-fault payload is shape-gated (the phase-8 rule): the real
# stdlib wording differs from the real native wording, so the selftest
# session projects the payload like the dispatch arms do
ARCHIVE_ERROR_RE = re.compile(rb"the downloaded archive was unreadable: [^\r\n]*")
PROG_USAGE_RE = re.compile(rb"usage: (?:ytdlp\.py|ytdlp\.exe|ytdlp) ")
PROG_ERROR_RE = re.compile(rb"(?m)^(?:ytdlp\.py|ytdlp\.exe|ytdlp): error:")
# the usage continuation indent is len("usage: ") + len(prog) + 1
USAGE_INDENT_RE = re.compile(rb"(?m)^ {13,16}(\S)")
# a download verdict line starts with the frozen spinner frame, which is
# a random draw per run (python random.choice / the native PCG32)
SPINNER_COLOR_DONE_RE = re.compile(
    rb"\r\x1b\[K(?:\x1b\[[0-9;]+m|[^\x1b\r\n])+\x1b\[0m (?=downloading - )"
)
SPINNER_PIPED_DONE_RE = re.compile(rb"(?m)^\S+ (?=downloading - )")
# the download work folder inside the output folder (exec.rs temp_dir)
TEMP_DIR_NAME = ".ytdlp-tmp"


def normalize(data, tree, project_frames=False):
    # the tree path appears raw in stdout and JSON-escaped inside
    # settings.json, so both spellings project. The python side derives
    # its root through Path.resolve(), which can differ from the
    # mkdtemp spelling (macOS /var -> /private/var, windows canonical
    # case), so the resolved form projects too - longest form first,
    # because the raw path is a suffix of the resolved one on macOS.
    forms = {str(tree)}
    try:
        forms.add(str(Path(str(tree)).resolve()))
    except OSError:
        pass
    for form in sorted(forms, key=len, reverse=True):
        escaped = json.dumps(form).encode("utf-8")[1:-1]
        data = data.replace(escaped, b"{root}")
        data = data.replace(form.encode("utf-8"), b"{root}")
    data = VERSION_RE.sub(rb"\1{ver}", data)
    # the argparse prog is the interpreter's script name on one side and
    # the exe basename on the other
    data = PROG_USAGE_RE.sub(b"usage: {prog} ", data)
    data = PROG_ERROR_RE.sub(b"{prog}: error:", data)
    data = USAGE_INDENT_RE.sub(rb"{ind}\1", data)
    # the three selftest check names that carry an honest native wording
    # (specsheet section 10, conformance map) project to the oracle
    # names, the same rule as the version token above
    data = data.replace(
        b"PASS cli update source is the releases api",
        b"PASS cli update url is raw main branch",
    )
    data = data.replace(
        b"PASS install_cli refuses an image that fails its own checks",
        b"PASS install_cli refuses a file that does not parse",
    )
    data = data.replace(
        b"PASS windows ffmpeg source is BtbN",
        b"PASS windows ffmpeg source is gyan.dev",
    )
    data = ARCHIVE_ERROR_RE.sub(
        b"the downloaded archive was unreadable: {archive-error}", data
    )
    if project_frames:
        # an animated stage rewrites the line until it freezes; collapse
        # every run of rewrites into one token (the frames themselves are
        # pinned by the cli tests, not by a timing-dependent count)
        data = FRAME_RE.sub(b"\r\x1b[K{frame}", data)
        data = SPINNER_COLOR_DONE_RE.sub(b"\r\x1b[K{frame} ", data)
        data = SPINNER_PIPED_DONE_RE.sub(b"{frame} ", data)
    return data


def case_extras(tree, case, project_frames, native=False):
    """The case's extra artifacts: the argv the wrapper handed to the
    stand-in (as a file it appends) and a directory listing."""
    extras = {}
    if case.get("argv_file"):
        path = tree / case["argv_file"]
        raw = path.read_bytes() if path.is_file() else b"<missing>"
        extras["argv_file"] = normalize(raw, tree, project_frames)
    listing_root = case.get("fs_list")
    if listing_root:
        root = tree / listing_root
        names = []
        if root.is_dir():
            for path in sorted(root.rglob("*")):
                names.append(str(path.relative_to(root)).replace(os.sep, "/"))
        if not native:
            # FR-11 amendment (owner call 2026-10-16c): every native run
            # end removes the work folder, so the oracle listing's kept
            # work entries (fail-keeps-partial, subs-429-keeps-vtt) are
            # not part of the native expectation. Only the oracle side is
            # filtered - a work entry surviving a native run still fails.
            names = [
                n
                for n in names
                if n != TEMP_DIR_NAME and not n.startswith(TEMP_DIR_NAME + "/")
            ]
        extras["fs_list"] = ("\n".join(names) + "\n").encode("utf-8")
    return extras


def compare_case(name, py, rs, work, strict_stderr=False):
    py_rc, py_out, py_err, py_cfg, py_extra = py
    rs_rc, rs_out, rs_err, rs_cfg, rs_extra = rs
    problems = []
    if py_rc != rs_rc:
        problems.append("exit code: python %d, native %d" % (py_rc, rs_rc))
    if py_out != rs_out:
        problems.append("stdout differs")
    if strict_stderr:
        if py_err != rs_err:
            problems.append("stderr differs")
    elif (not py_err) != (not rs_err):
        problems.append(
            "stderr presence differs: python %d bytes, native %d bytes"
            % (len(py_err), len(rs_err))
        )
    if py_cfg != rs_cfg:
        problems.append("settings.json differs")
    for key in sorted(set(py_extra) | set(rs_extra)):
        if py_extra.get(key) != rs_extra.get(key):
            problems.append("%s differs" % key)
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
    for key in sorted(set(py_extra) | set(rs_extra)):
        if py_extra.get(key) != rs_extra.get(key):
            print("  python %s: %r" % (key, py_extra.get(key, b"")[:220]))
            print("  native %s: %r" % (key, rs_extra.get(key, b"")[:220]))
    return False


def read_result(tree, case, proc, out, err, native=False):
    cfg = (tree / "config" / "settings.json").read_bytes()
    project_frames = bool(case.get("project_frames"))
    return (
        proc.returncode,
        normalize(out, tree, project_frames),
        normalize(err, tree, project_frames)[:4096],
        normalize(cfg, tree, project_frames),
        case_extras(tree, case, project_frames, native=native),
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
    # ---- phase 7: the download run (offline: the scripted stand-in) ----
    {
        "name": "quick-download-no-ffmpeg",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "args": ["https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "ok", "size": 1572864, "name": "video.mp4"}},
        "argv_file": "argv.txt",
        "fs_list": "out",
        "stdin": b"",
    },
    {
        "name": "quick-download-with-ffmpeg",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "args": ["https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "ok", "size": 1048576}, "ffmpeg": {}},
        "argv_file": "argv.txt",
        "fs_list": "out",
        "stdin": b"",
    },
    {
        "name": "quick-download-fail-keeps-partial",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "args": ["https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "fail"}},
        "fs_list": "out",
        "stdin": b"",
    },
    {
        "name": "quick-download-probe-fail",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "args": ["https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "probe-fail"}},
        "stdin": b"",
    },
    {
        "name": "quick-download-no-binary",
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "args": ["https://example.invalid/v"],
        "env": {"PATH": "{root}/nobin"},
        "stdin": b"",
    },
    {
        "name": "quick-not-a-url",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out"),
        "args": ["notaurl"],
        "stdin": b"",
    },
    {
        "name": "quick-subs-ok",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=True),
        "args": ["https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "ok", "sub_mode": "sub-ok"}},
        "fs_list": "out",
        "stdin": b"",
    },
    {
        "name": "quick-subs-miss-note",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=True),
        "args": ["https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "ok", "sub_mode": "sub-miss"}},
        "stdin": b"",
    },
    {
        "name": "quick-subs-429-keeps-vtt",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=True),
        "args": ["https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "ok", "sub_mode": "sub-429"}},
        "fs_list": "out",
        "stdin": b"",
    },
    {
        "name": "quick-audio-override-argv",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "args": ["-a", "https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "ok"}},
        "argv_file": "argv.txt",
        "stdin": b"",
    },
    {
        "name": "menu-url-download",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "bin": {"yt-dlp": {"mode": "ok"}},
        "argv_file": "argv.txt",
        "stdin": b"https://example.invalid/v\n13\n",
    },
    {
        "name": "batch-two-runs-and-a-bad-row",
        "project_frames": True,
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "bin": {"yt-dlp": {"mode": "ok"}},
        "argv_file": "argv.txt",
        "stdin": b"9\nhttps://example.invalid/1\nnotaurl\nhttps://example.invalid/2\n\n13\n",
    },
    {
        "name": "help-text-78",
        "settings": settings_with(output_dir="{scratch}/out"),
        "args": ["-h"],
        "stdin": b"",
    },
    {
        "name": "help-text-38",
        "settings": settings_with(output_dir="{scratch}/out"),
        "args": ["-h"],
        "env": {"COLUMNS": "40"},
        "stdin": b"",
    },
    {
        "name": "bad-choice-error",
        "settings": settings_with(output_dir="{scratch}/out"),
        "args": ["-f", "junk"],
        "stderr_bytes": True,
        "stdin": b"",
    },
    {
        "name": "url-plus-menu-boots-menu",
        "settings": settings_with(output_dir="{scratch}/out"),
        "args": ["https://example.invalid/v", "--menu"],
        "env": {"PATH": "{root}/nobin"},
        "stdin": b"13\n",
    },
    {
        "name": "check-with-url",
        "settings": settings_with(output_dir="{scratch}/out"),
        "args": ["--check", "https://example.invalid/v"],
        "env": {"PATH": "{root}/nobin"},
        "stdin": b"",
    },
    {
        "name": "selftest-report",
        "settings": settings_with(output_dir="{scratch}/out"),
        "args": ["--selftest"],
        "stdin": b"",
    },
    {
        "name": "terminator-url",
        "settings": settings_with(output_dir="{scratch}/out"),
        "args": ["--", "notaurl"],
        "stdin": b"",
    },
]


def run_piped_case(case, work, rust_bin, fake_bin):
    py_tree, _ = stage_tree("py", work, case, rust_bin, fake_bin)
    rs_tree, rs_exe = stage_tree("rs", work, case, rust_bin, fake_bin)
    env = dict(os.environ)
    env.update(case_env(py_tree, case, "py"))
    py_proc = subprocess.run(
        py_command(py_tree, case),
        input=case["stdin"],
        capture_output=True,
        cwd=py_tree,
        env=env,
        timeout=120,
    )
    env = dict(os.environ)
    env.update(case_env(rs_tree, case, "rs"))
    rs_proc = subprocess.run(
        rust_command(rs_tree, rs_exe, case),
        input=case["stdin"],
        capture_output=True,
        cwd=rs_tree,
        env=env,
        timeout=120,
    )
    py = read_result(py_tree, case, py_proc, py_proc.stdout, py_proc.stderr)
    rs = read_result(
        rs_tree, case, rs_proc, rs_proc.stdout, rs_proc.stderr, native=True
    )
    return compare_case(
        case["name"], py, rs, work, strict_stderr=bool(case.get("stderr_bytes"))
    )


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
    # ---- phase 7: executable download cases ----
    {
        "name": "pty-escape-mid-download",
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "bin": {"yt-dlp": {"mode": "sleep", "sleep": 20}, "ffmpeg": {}},
        "project_frames": True,
        "fs_list": "out",
        "bursts": [
            b"https://example.invalid/v\n",
            {"wait": "downloading (escape", "send": "\x1b"},
            b"13\n",
        ],
    },
    {
        "name": "pty-ctrl-c-mid-download",
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "bin": {"yt-dlp": {"mode": "sleep", "sleep": 20}, "ffmpeg": {}},
        "project_frames": True,
        "fs_list": "out",
        "bursts": [
            b"https://example.invalid/v\n",
            {"wait": "downloading (escape", "send": "\x03"},
            b"13\n",
        ],
    },
    {
        "name": "pty-quick-escape-130",
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "args": ["https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "sleep", "sleep": 20}, "ffmpeg": {}},
        "boot": "checking yt-dlp",
        "project_frames": True,
        "fs_list": "out",
        "bursts": [
            {"wait": "downloading (escape", "send": "\x1b"},
        ],
    },
    {
        "name": "pty-quick-ctrl-c-cancelled",
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "args": ["https://example.invalid/v"],
        "bin": {"yt-dlp": {"mode": "sleep", "sleep": 20}, "ffmpeg": {}},
        "boot": "checking yt-dlp",
        "project_frames": True,
        "fs_list": "out",
        "bursts": [
            {"wait": "downloading (escape", "send": "\x03"},
        ],
    },
    {
        "name": "pty-ffmpeg-offer-decline",
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "bin": {"yt-dlp": {"mode": "ok"}},
        "skip_platform": "darwin",
        "project_frames": True,
        "bursts": [
            b"https://example.invalid/v\n",
            {"wait": "y/n (enter = no)", "send": "n\n"},
            b"13\n",
        ],
    },
    {
        "name": "pty-item9-escape-breaks-batch",
        "settings": settings_with(output_dir="{scratch}/out", subtitles=False),
        "bin": {"yt-dlp": {"mode": "sleep", "sleep": 20}, "ffmpeg": {}},
        "project_frames": True,
        "fs_list": "out",
        "bursts": [
            b"9\n",
            b"https://example.invalid/1\n",
            b"https://example.invalid/2\n",
            b"\n",
            {"wait": "downloading (escape", "send": "\x1b"},
            b"13\n",
        ],
    },
]


def set_winsize(fd, cols, rows):
    import fcntl
    import struct
    import termios

    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def _pty_child_setup():
    """Make the child a session leader with the pty as its controlling
    terminal, so a `\\x03` typed at the terminal generates SIGINT
    (python's cbreak keeps ISIG on). Without this the pty has no
    foreground process group and the byte is delivered to nobody -
    measured 2026-10-15 while wiring the mid-download Ctrl+C case."""
    import fcntl
    import termios

    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


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


def wait_for(master, data, needle, deadline):
    """Read until `needle` shows up in the transcript (CI raced this
    once: a burst written before the child entered raw mode is echoed in
    canonical mode and then discarded by setraw's TCSAFLUSH, and the
    child waits forever)."""
    while needle not in data:
        if time.monotonic() >= deadline:
            raise InstrumentError(
                "never saw %r in the transcript; tail=%r" % (needle, bytes(data[-400:]))
            )
        ready, _, _ = select.select([master], [], [], 0.1)
        if not ready:
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError:
            raise InstrumentError("pty closed before %r appeared" % needle) from None
        if not chunk:
            raise InstrumentError("pty closed before %r appeared" % needle)
        data.extend(chunk)


def run_pty_case(case, work, rust_bin, fake_bin, cols=80, rows=24):
    results = []
    for kind in ("py", "rs"):
        tree, exe = stage_tree(kind, work, case, rust_bin, fake_bin)
        if kind == "py":
            cmd = py_command(tree, case)
        else:
            cmd = rust_command(tree, exe, case)
        master, slave = os.openpty()
        set_winsize(slave, cols, rows)
        err_file = open(work / ("stderr-" + kind + ".txt"), "wb")
        env = dict(os.environ)
        env.update(case_env(tree, case, kind))
        proc = subprocess.Popen(
            cmd,
            stdin=slave,
            stdout=slave,
            stderr=err_file,
            cwd=tree,
            env=env,
            close_fds=True,
            preexec_fn=_pty_child_setup,
        )
        os.close(slave)
        data = bytearray()
        try:
            # wait for the case's boot marker first: the child must be in
            # read_esc (raw mode) before a burst can survive (menu cases
            # boot to the "> " prompt; quick-mode cases to the preflight
            # stage line)
            boot = case.get("boot", "> ").encode()
            wait_for(master, data, boot, time.monotonic() + 20.0)
            for burst in case["bursts"]:
                if isinstance(burst, dict):
                    # wait for a transcript marker (e.g. the download
                    # stage line) before sending
                    wait_for(
                        master, data, burst["wait"].encode(), time.monotonic() + 20.0
                    )
                    time.sleep(0.15)
                    os.write(master, burst["send"].encode())
                    continue
                deadline = time.monotonic() + 8.0
                drain(master, data, 0.25, deadline)
                # the child prints its prompt before it switches the
                # terminal to raw; give that switch a margin
                time.sleep(0.15)
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
        project_frames = bool(case.get("project_frames"))
        results.append(
            (
                rc,
                normalize(bytes(data), tree, project_frames),
                normalize(stderr_bytes, tree, project_frames)[:4096],
                normalize(cfg, tree, project_frames),
                case_extras(tree, case, project_frames, native=(kind == "rs")),
            )
        )
    return compare_case(
        case["name"],
        results[0],
        results[1],
        work,
        strict_stderr=bool(case.get("stderr_bytes")),
    )


# ------------------------------------------------------------------ main


def main(argv=None):
    parser = argparse.ArgumentParser(prog="session_diff.py", description=__doc__.splitlines()[0])
    parser.add_argument("mode", choices=["piped", "pty"])
    parser.add_argument("--rust-bin")
    parser.add_argument("--fake-bin")
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
        fake_bin = Path(args.fake_bin) if args.fake_bin else default_fake_bin()
        if not fake_bin.exists():
            raise InstrumentError(
                "fake yt-dlp not found: %s (cargo build --release -p fake-ytdlp) "
                % fake_bin
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
        skipped = [c for c in cases if sys.platform.startswith(c.get("skip_platform", "\0"))]
        cases = [c for c in cases if c not in skipped]
        for case in skipped:
            print("SKIP %s (platform %s)" % (case["name"], sys.platform))

        work = Path(tempfile.mkdtemp(prefix="ytdlp-session-"))
        passed = 0
        failed = 0
        try:
            for case in cases:
                ok = (
                    run_piped_case(case, work, rust_bin, fake_bin)
                    if args.mode == "piped"
                    else run_pty_case(case, work, rust_bin, fake_bin)
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
