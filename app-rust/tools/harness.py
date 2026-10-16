#!/usr/bin/env python3
"""Differential harness driver and differ for the ytdlp native port.

Plan contract (RUST-PORT-PLAN.md phase 1): a tool that takes a function
name plus a JSON fixture vector and calls it identically in the python
and the rust implementation, plus a differ asserting byte equality.
Acceptance rule (specsheet NFR-4): the instrument is proven against
known answers from the python implementation before it judges any native
code; `selftest` does exactly that, python only.

Vector format (app-rust/vectors/<fn>.json):

    {"fn": "clean_url",
     "cases": [{"args": [...], "note": "optional"}]}

A case calls the python function and the rust twin with the same vector
args. Where the port is deliberately explicit about state the python
original reads implicitly, the case trains the two sides onto the same
state:

    "capture_args": true   the record gains an "args_after" field, so a
                           function that mutates its argument is
                           compared on the mutated state as well
    PY_CALL_HOOKS          js_runtime_args / menu_line / batch_row /
                           short_path take the display state (node on
                           PATH, USE_COLOR, os.name) as a trailing
                           parameter in the rust twin; the hook binds
                           that state on the python side. load_settings /
                           save_settings (phase 3) carry their fixture in
                           the case; each side materializes it in its own
                           scratch dir and the record carries port-domain
                           shape tokens (payloads stay shape-gated).
                           Phase 4 (platform_name, path_render,
                           path_is_absolute, splitroot, expandvars, which,
                           find_yt_dlp, find_ffmpeg_dir, probe_version,
                           auto_outdir, resolve_outdir, show_check) carries
                           its whole world in the case object (fixtures,
                           env, registry answers, which answers, probe
                           outcomes, settings values); each side
                           materializes it in its own scratch dir and the
                           record projects that dir as {scratch}.
                           Phase 5 (output_args, build_args,
                           build_sub_args) carries the settings object,
                           the url, the ffdir answer, the auto-outdir
                           answer and the which map in the case object;
                           the record carries the argv element list, the
                           port-domain shape token or the which call log.
                           Phase 6 (menu_summary, menu_items, header,
                           hint, render, block_under_input, read_esc_piped,
                           line_edit, pick_list, ask_bool, act, main_menu)
                           drives the terminal layer without a terminal:
                           the hooks fake read_esc (a scripted reply
                           queue: a string is a line, null is escape,
                           {"eof": true} / {"ctrl_c": true} are the read
                           failures), bind a StringIO stdin with
                           newline=None, CONFIG_PATH, ROOT, VERSION,
                           USE_COLOR, os.name and a terminal size into a
                           private scratch dir, capture stdout and return
                           records projected as {scratch}: transcript
                           text, prompt lists, settings state and the
                           settings file. The rust twins take the same
                           scripted reader, recording sink and world as
                           parameters. The binary-level sibling is
                           tools/session_diff.py (piped and pty golden
                           transcripts over staged trees).

Interchange format: one line per case carrying exactly the bytes of

    json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)

which the rust dispatcher reproduces byte for byte
(app-rust/harness/src/json.rs). Every record is newline-terminated; an
empty case list means zero bytes. All display output here is forced
ASCII so any console codepage can print it. Case args may carry JSON
floats and the NaN/Infinity constants (phase 3 settings cases), matching
`json.loads`; generated records stay i64-only.

Oracle: CPython 3.14.x is the pinned oracle interpreter (plan G2); the
python capture refuses every other minor version.

Usage:
    python harness.py selftest [--fn F] [--vectors F.json]
    python harness.py run --fn F [--vectors F.json] [--rust-bin B]
    python harness.py run-all [--vectors-dir D] [--rust-bin B]
    python harness.py capture --impl python|rust --fn F --out FILE
        [--vectors F.json] [--rust-bin B]
    python harness.py diff --a FILE --b FILE
    python harness.py probe-rust [--vectors F.json] [--rust-bin B]

`run-all` is the gate-2 differential: every vector file except the
error-path fixture is captured on both sides and byte-compared.

Exit codes: 0 pass, 1 check failed, 2 instrument error.
"""
import argparse
import importlib.util
import io
import json
import lzma
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import types
import urllib.error
import urllib.request
import zipfile
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
APP_RUST = TOOLS_DIR.parent
REPO_ROOT = APP_RUST.parent
PY_IMPL = REPO_ROOT / "app-python" / "src" / "ytdlp.py"
VECTORS_DIR = APP_RUST / "vectors"
ORACLE = (3, 14, 4)
# the dispatcher error-path fixture; it exists to prove the rust side
# refuses unknown names and is never part of the differential.
ERROR_PATH_FIXTURE = "no_such_function.json"


class InstrumentError(Exception):
    """The instrument itself cannot run; never a verdict."""


def require_oracle():
    # The oracle is pinned to the exact patch: argparse's invalid-choice
    # text renders the choices unquoted in 3.14.4 and quoted in 3.14.6
    # (measured 2026-10-15), so "3.14.x" is not one oracle.
    if sys.version_info[:3] != ORACLE:
        raise InstrumentError(
            "oracle interpreter is CPython %d.%d.%d, running %s"
            % (ORACLE[0], ORACLE[1], ORACLE[2], sys.version.split()[0])
        )


def default_vectors(fn):
    return VECTORS_DIR / (fn + ".json")


def default_rust_bin():
    exe = "ytdlp-harness" + (".exe" if os.name == "nt" else "")
    return APP_RUST / "target" / "release" / exe


def normalize_rust_bin(path):
    p = Path(path)
    if os.name == "nt" and p.suffix.lower() != ".exe" and not p.exists():
        p2 = Path(str(p) + ".exe")
        if p2.exists():
            return p2
    return p


def load_vectors(path):
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError as e:
        raise InstrumentError("cannot read vectors %s: %s" % (path, e))
    try:
        doc = json.loads(text)
    except json.JSONDecodeError as e:
        raise InstrumentError("vectors %s: bad json: %s" % (path, e))
    if not isinstance(doc, dict) or not isinstance(doc.get("fn"), str) or not isinstance(doc.get("cases"), list):
        raise InstrumentError('vectors %s: need {"fn": str, "cases": [...]}' % path)
    for i, case in enumerate(doc["cases"]):
        if not isinstance(case, dict) or not isinstance(case.get("args"), list):
            raise InstrumentError("vectors %s: case %d needs an args array" % (path, i))
    return doc


def check_jsonable(value, case_index, depth=0):
    if depth > 64:
        raise InstrumentError("case %d: result nesting too deep" % case_index)
    if value is None or isinstance(value, (str, bool)):
        return
    if isinstance(value, int):
        if not (-(2**63) <= value < 2**63):
            raise InstrumentError("case %d: int outside the i64 range" % case_index)
        return
    if isinstance(value, float):
        raise InstrumentError("case %d: float output is outside the protocol; pin a rendering first" % case_index)
    if isinstance(value, (list, tuple)):
        for item in value:
            check_jsonable(item, case_index, depth + 1)
        return
    if isinstance(value, dict):
        for k, v in value.items():
            if not isinstance(k, str):
                raise InstrumentError("case %d: dict key is not a string" % case_index)
            check_jsonable(v, case_index, depth + 1)
        return
    raise InstrumentError("case %d: unsupported value type %s" % (case_index, type(value).__name__))


def encode_records(records):
    if not records:
        return b""
    return ("\n".join(records) + "\n").encode("utf-8")


def _with_global(module, name, value, fn):
    saved = getattr(module, name)
    setattr(module, name, value)
    try:
        return fn()
    finally:
        setattr(module, name, saved)


def _hook_js_runtime_args(module, args):
    flag = bool(args[0])
    fake = (lambda name: "/fake/node") if flag else (lambda name: None)
    return _with_global(module.shutil, "which", fake, module.js_runtime_args)


def _hook_menu_line(module, args):
    call_args, use_color = args[:-1], bool(args[-1])
    return _with_global(
        module, "USE_COLOR", use_color, lambda: module.menu_line(*call_args)
    )


def _hook_batch_row(module, args):
    call_args, use_color = args[:-1], bool(args[-1])
    return _with_global(
        module, "USE_COLOR", use_color, lambda: module.batch_row(*call_args)
    )


def _hook_short_path(module, args):
    call_args, is_windows = args[:-1], bool(args[-1])
    fake_os = types.SimpleNamespace(name="nt" if is_windows else "posix")
    return _with_global(module, "os", fake_os, lambda: module.short_path(*call_args))


# ----------------------------------------------------- phase 3 state layer
#
# The settings cases carry their fixture in the vector itself. Each side
# materializes it in its own private scratch dir (never a shared path),
# binds CONFIG_PATH and USE_COLOR for the call, and reports a record with
# port-domain shape tokens: python's exception payloads stay shape-gated
# (RUST-PORT-PLAN.md port rules), so the record carries only the token.

# must match dispatch.rs SENTINEL
SENTINEL_BYTES = b"ytdlp-sentinel"


class _bound_settings_globals:
    """CONFIG_PATH and USE_COLOR bound for one hook call; USE_COLOR is
    forced off so note text is plain (colors are phase-6 presentation)."""

    def __init__(self, module, target):
        self.module = module
        self.target = target

    def __enter__(self):
        self.saved_path = self.module.CONFIG_PATH
        self.saved_color = self.module.USE_COLOR
        self.module.CONFIG_PATH = self.target
        self.module.USE_COLOR = False

    def __exit__(self, *exc):
        self.module.CONFIG_PATH = self.saved_path
        self.module.USE_COLOR = self.saved_color
        return False


def _seed_fixture(target, case):
    if "seed_text" in case:
        target.write_bytes(case["seed_text"].encode("utf-8"))
    elif "seed_hex" in case:
        target.write_bytes(bytes.fromhex(case["seed_hex"]))


def _hook_load_settings(module, args):
    case = args[0]
    work = Path(tempfile.mkdtemp(prefix="ytdlp-harness-load-"))
    try:
        target = work / case["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        _seed_fixture(target, case)
        with _bound_settings_globals(module, target):
            side = io.StringIO()
            try:
                with redirect_stdout(side):
                    settings = module.load_settings()
                outcome = "ok"
            except TypeError:
                settings, outcome = None, "type_error"
            except AttributeError:
                settings, outcome = None, "attribute_error"
            except Exception as e:
                raise InstrumentError(
                    "load_settings raised %s: %s" % (type(e).__name__, e)
                )
        notes = []
        unreadable = False
        for line in side.getvalue().splitlines():
            if line.startswith("  note: "):
                notes.append(line[len("  note: "):])
            elif line.startswith("settings unreadable (") and line.endswith(
                "), using defaults"
            ):
                unreadable = True
            else:
                raise InstrumentError(
                    "load_settings printed an unrecognized line: %r" % line
                )
        if unreadable:
            outcome = "unreadable"
        settings_file = None
        if settings is not None:
            # the exact call save_settings makes for the file content
            settings_file = json.dumps(settings, indent=2, sort_keys=True) + "\n"
        return {
            "load": outcome,
            "settings_file": settings_file,
            "notes": notes,
            "file_after_hex": target.read_bytes().hex() if target.is_file() else None,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_save_settings(module, args):
    case = args[0]
    work = Path(tempfile.mkdtemp(prefix="ytdlp-harness-save-"))
    try:
        target = work / case["path"]
        if case.get("parents", True):
            target.parent.mkdir(parents=True, exist_ok=True)
        if case.get("block_parent"):
            target.parent.write_bytes(b"blocked")
        _seed_fixture(target, case)
        sentinel = bool(case.get("sentinel"))
        if sentinel:
            target.with_suffix(".tmp").write_bytes(SENTINEL_BYTES)
        with _bound_settings_globals(module, target):
            side = io.StringIO()
            try:
                with redirect_stdout(side):
                    module.save_settings(case["settings"])
                outcome = "ok"
            except UnicodeDecodeError:
                outcome = "not_utf8"
            except OSError:
                outcome = "io_error"
            except Exception as e:
                raise InstrumentError(
                    "save_settings raised %s: %s" % (type(e).__name__, e)
                )
        if side.getvalue():
            raise InstrumentError(
                "save_settings printed %d chars" % len(side.getvalue())
            )
        skipped = None
        if sentinel:
            tmp = target.with_suffix(".tmp")
            skipped = tmp.is_file() and tmp.read_bytes() == SENTINEL_BYTES
        file_text = None
        file_hex = None
        if target.is_file():
            raw = target.read_bytes()
            file_hex = raw.hex()
            try:
                file_text = raw.decode("utf-8")
            except UnicodeDecodeError:
                file_text = None
        return {
            "save": outcome,
            "file": file_text,
            "file_after_hex": file_hex,
            "skipped": skipped,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


# Ports of the terminal/display state the python originals read from the
# module or the environment; the rust twins take it as a parameter, so
# the hook binds it on the python side from the vector's trailing args.
# load_settings / save_settings take the case object and bind CONFIG_PATH
# themselves (each side gets its own scratch dir).
PY_CALL_HOOKS = {
    "js_runtime_args": _hook_js_runtime_args,
    "menu_line": _hook_menu_line,
    "batch_row": _hook_batch_row,
    "short_path": _hook_short_path,
    "load_settings": _hook_load_settings,
    "save_settings": _hook_save_settings,
}


# ----------------------------------------------------- phase 4 resolve
#
# Phase-4 cases carry their world (fixtures, env, registry answers, which
# answers, probe outcomes, settings values) in the case object. Each side
# materializes it in its own scratch dir and the record projects that dir
# as {scratch}, so the records compare byte-equal across two different
# filesystems.


class _bound_world:
    """Binds module attributes and the process environment for one
    phase-4 hook call; everything is restored on exit."""

    def __init__(self, module, attrs=None, env=None):
        self.module = module
        self.attrs = attrs or {}
        self.env = env
        self.saved_attrs = {}
        self.saved_env = None

    def __enter__(self):
        for name, value in self.attrs.items():
            self.saved_attrs[name] = getattr(self.module, name)
            setattr(self.module, name, value)
        if self.env is not None:
            self.saved_env = dict(os.environ)
            os.environ.clear()
            os.environ.update(self.env)
        return self

    def __exit__(self, *exc):
        for name, value in self.saved_attrs.items():
            setattr(self.module, name, value)
        if self.saved_env is not None:
            os.environ.clear()
            os.environ.update(self.saved_env)
        return False


def _case_scratch(case, prefix):
    work = Path(tempfile.mkdtemp(prefix="ytdlp-harness-%s-" % prefix))
    for item in case.get("files") or []:
        target = work / item["path"]
        if item.get("dir"):
            target.mkdir(parents=True, exist_ok=True)
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(b"x")
        target.chmod(0o755 if item.get("exec") else 0o644)
    return work


def _resolve_scratch(text, work):
    return text.replace("{scratch}", str(work))


def _project(work, text):
    return text.replace(str(work), "{scratch}")


def _project_path(work, value):
    return _project(work, str(value)) if value is not None else None


def _fake_os(module, name):
    return types.SimpleNamespace(name=name, path=module.os.path)


def _hook_platform_name(module, args):
    os_name, sys_platform = args
    fake_sys = types.SimpleNamespace(platform=sys_platform)
    with _bound_world(
        module, attrs={"os": _fake_os(module, os_name), "sys": fake_sys}
    ):
        return module.platform_name()


def _hook_path_render(module, args):
    return str(module.Path(args[0]))


def _hook_path_is_absolute(module, args):
    return module.Path(args[0]).is_absolute()


def _hook_splitroot(module, args):
    return list(module.os.path.splitroot(args[0]))


def _hook_expandvars(module, args):
    case = args[0]
    with _bound_world(module, env=dict(case.get("env") or {})):
        return module.os.path.expandvars(case["text"])


def _hook_which(module, args):
    case = args[0]
    work = _case_scratch(case, "which")
    try:
        cmd = _resolve_scratch(case["cmd"], work)
        entries = [_resolve_scratch(e, work) for e in case["entries"]]
        env = {}
        if case.get("pathext") is not None:
            env["PATHEXT"] = case["pathext"]
        with _bound_world(module, env=env):
            found = module.shutil.which(cmd, path=os.pathsep.join(entries))
        return _project_path(work, found)
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _which_fake(module, work, answer_map, calls):
    def fake(name):
        calls.append(name)
        rel = (answer_map or {}).get(name)
        return _resolve_scratch(rel, work) if rel else None

    return fake


def _hook_find_yt_dlp(module, args):
    case = args[0]
    work = _case_scratch(case, "ytdlp")
    try:
        calls = []
        fake_sys = types.SimpleNamespace(platform=case["sys_platform"])
        with _bound_world(
            module,
            attrs={
                "BIN_DIR": work / "bin",
                "os": _fake_os(module, case["os_name"]),
                "sys": fake_sys,
            },
        ):
            with _bound_world(
                module.shutil,
                attrs={"which": _which_fake(module, work, case.get("which"), calls)},
            ):
                try:
                    found = module.find_yt_dlp(case.get("saved") or {})
                except TypeError:
                    return {"found": None, "shape": "type_error", "which_calls": calls}
        return {
            "found": _project_path(work, found),
            "shape": None,
            "which_calls": calls,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_find_ffmpeg_dir(module, args):
    case = args[0]
    work = _case_scratch(case, "ffmpeg")
    try:
        calls = []
        with _bound_world(
            module,
            attrs={
                "BIN_DIR": work / "bin",
                "os": _fake_os(module, case["os_name"]),
            },
        ):
            with _bound_world(
                module.shutil,
                attrs={"which": _which_fake(module, work, case.get("which"), calls)},
            ):
                try:
                    found = module.find_ffmpeg_dir(case.get("saved") or {})
                except TypeError:
                    return {"found": None, "shape": "type_error", "which_calls": calls}
        return {
            "found": _project_path(work, found),
            "shape": None,
            "which_calls": calls,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_probe_version(module, args):
    case = args[0]
    calls = case["calls"]
    queue = list(case["spawns"])
    spawn_log = []
    results = []

    def fake_run(cmd, **kwargs):
        spawn_log.append(cmd[1])
        item = queue.pop(0)
        if "raise" in item:
            raise OSError(item["raise"])
        return types.SimpleNamespace(
            stdout=item.get("stdout", ""),
            stderr=item.get("stderr", ""),
            returncode=item["rc"],
        )

    module._VERSION_CACHE.clear()
    with _bound_world(module.subprocess, attrs={"run": fake_run}):
        for call in calls:
            mtime = call.get("mtime")

            def fake_getmtime(path, _value=mtime):
                if _value is None:
                    raise OSError("fake mtime missing")
                return _value

            with _bound_world(module.os.path, attrs={"getmtime": fake_getmtime}):
                line, rc = module.probe_version(module.Path(call["binary"]))
            results.append([line, rc])
    return {"calls": results, "spawn_log": spawn_log, "spawns_used": len(spawn_log)}


def _win_shell_fake(module, work, win_map, real_expandvars):
    def fake(key):
        value = (win_map or {}).get(key)
        if not value:
            return None
        return real_expandvars(_resolve_scratch(value, work))

    return fake


def _bind_home_env(case, work):
    env = {k: _resolve_scratch(v, work) for k, v in (case.get("env") or {}).items()}
    home = work / "home"
    env["HOME"] = str(home)
    env["USERPROFILE"] = str(home)
    return env, home


def _seed_user_dirs(work, case, home):
    text = case.get("user_dirs")
    hex_text = case.get("user_dirs_hex")
    if text is None and hex_text is None:
        return
    target = home / ".config" / "user-dirs.dirs"
    target.parent.mkdir(parents=True, exist_ok=True)
    if hex_text is not None:
        target.write_bytes(bytes.fromhex(hex_text))
    else:
        target.write_bytes(text.encode("utf-8"))


def _hook_auto_outdir(module, args):
    case = args[0]
    work = _case_scratch(case, "outdir")
    try:
        env, home = _bind_home_env(case, work)
        _seed_user_dirs(work, case, home)
        lookups = {"n": 0}
        real_lookup = module._auto_outdir_lookup

        def counting_lookup():
            lookups["n"] += 1
            return real_lookup()

        module._outdir_cache.clear()
        with _bound_world(
            module,
            attrs={
                "os": _fake_os(module, case["os_name"]),
                "_auto_outdir_lookup": counting_lookup,
                "_win_shell_folder": _win_shell_fake(
                    module, work, case.get("win"), module.os.path.expandvars
                ),
            },
            env=env,
        ):
            results = []
            n_calls = case.get("calls", 2)
            try:
                for _ in range(n_calls):
                    value = module.auto_outdir()
                    results.append(_project_path(work, value))
            except UnicodeDecodeError:
                return {"error": "not_utf8", "lookups": lookups["n"], "results": None}
        return {"error": None, "lookups": lookups["n"], "results": results}
    finally:
        module._outdir_cache.clear()
        shutil.rmtree(work, ignore_errors=True)


def _hook_resolve_outdir(module, args):
    case = args[0]
    work = _case_scratch(case, "resolve")
    try:
        saved = {}
        if "output_dir" in case:
            saved["output_dir"] = case["output_dir"]
        auto = case.get("auto")
        if auto:
            auto_value = _resolve_scratch(auto, work)
        else:
            auto_value = None
        with _bound_world(
            module,
            attrs={
                "ROOT": work / "root",
                "auto_outdir": (lambda v=auto_value: v),
            },
        ):
            try:
                out = module.resolve_outdir(saved)
            except AttributeError:
                return {"outdir": None, "shape": "attribute_error"}
        return {"outdir": _project_path(work, out), "shape": None}
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_show_check(module, args):
    case = args[0]
    work = _case_scratch(case, "check")
    try:
        root = work / "root"
        bin_dir = root / "bin"
        config = work / "config" / "settings.json"
        if case.get("config_text") is not None:
            config.parent.mkdir(parents=True, exist_ok=True)
            config.write_bytes(case["config_text"].encode("utf-8"))
        env, home = _bind_home_env(case, work)
        _seed_user_dirs(work, case, home)
        calls = {"which": [], "probe": []}
        fake_sys = types.SimpleNamespace(platform=case["sys_platform"])
        module._outdir_cache.clear()

        def fake_probe(binary):
            calls["probe"].append(_project(work, str(binary)))
            probe = case.get("probe") or {}
            return probe.get("version", "no version output"), probe.get("rc", -1)

        with _bound_world(
            module,
            attrs={
                "ROOT": root,
                "BIN_DIR": bin_dir,
                "CONFIG_PATH": config,
                "os": _fake_os(module, case["os_name"]),
                "sys": fake_sys,
                "probe_version": fake_probe,
                "_win_shell_folder": _win_shell_fake(
                    module, work, case.get("win"), module.os.path.expandvars
                ),
            },
            env=env,
        ):
            with _bound_world(
                module.shutil,
                attrs={"which": _which_fake(module, work, case.get("which"), calls["which"])},
            ):
                side = io.StringIO()
                try:
                    with redirect_stdout(side):
                        module.show_check(case.get("saved") or {})
                except (TypeError, AttributeError) as e:
                    raise InstrumentError(
                        "show_check raised %s: %s" % (type(e).__name__, e)
                    ) from None
        return {
            "stdout": _project(work, side.getvalue()),
            "probe_calls": calls["probe"],
            "which_calls": calls["which"],
        }
    finally:
        module._outdir_cache.clear()
        shutil.rmtree(work, ignore_errors=True)


def _shape_token(exc):
    if isinstance(exc, TypeError):
        return "type_error"
    if isinstance(exc, AttributeError):
        return "attribute_error"
    if isinstance(exc, ValueError):
        return "value_error"
    raise InstrumentError("unexpected exception shape %s" % type(exc).__name__)


def _hook_output_args(module, args):
    case = args[0]
    work = _case_scratch(case, "outargs")
    try:
        auto = _resolve_scratch(case["auto"], work) if case.get("auto") else None
        with _bound_world(
            module,
            attrs={"ROOT": work / "root", "auto_outdir": (lambda v=auto: v)},
        ):
            try:
                argv = module.output_args(case["s"])
            except (TypeError, AttributeError, ValueError) as e:
                return {"args": None, "shape": _shape_token(e)}
        return {"args": [_project(work, a) for a in argv], "shape": None}
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_build_args(module, args):
    case = args[0]
    work = _case_scratch(case, "buildargs")
    try:
        calls = []
        ffdir = case.get("ffdir")
        ffdir = Path(_resolve_scratch(ffdir, work)) if ffdir else None
        auto = _resolve_scratch(case["auto"], work) if case.get("auto") else None
        with _bound_world(
            module,
            attrs={"ROOT": work / "root", "auto_outdir": (lambda v=auto: v)},
        ):
            with _bound_world(
                module.shutil,
                attrs={"which": _which_fake(module, work, case.get("which"), calls)},
            ):
                try:
                    argv = module.build_args(case["s"], case["url"], ffdir)
                except (TypeError, AttributeError, ValueError) as e:
                    return {"args": None, "shape": _shape_token(e), "which_calls": calls}
        return {
            "args": [_project(work, a) for a in argv],
            "shape": None,
            "which_calls": calls,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_build_sub_args(module, args):
    case = args[0]
    work = _case_scratch(case, "subargs")
    try:
        calls = []
        ffdir = case.get("ffdir")
        ffdir = Path(_resolve_scratch(ffdir, work)) if ffdir else None
        auto = _resolve_scratch(case["auto"], work) if case.get("auto") else None
        with _bound_world(
            module,
            attrs={"ROOT": work / "root", "auto_outdir": (lambda v=auto: v)},
        ):
            with _bound_world(
                module.shutil,
                attrs={"which": _which_fake(module, work, case.get("which"), calls)},
            ):
                try:
                    argv = module.build_sub_args(case["s"], case["url"], ffdir)
                except (TypeError, AttributeError, ValueError) as e:
                    return {"args": None, "shape": _shape_token(e), "which_calls": calls}
        return {
            "args": [_project(work, a) for a in argv],
            "shape": None,
            "which_calls": calls,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


PY_CALL_HOOKS.update(
    {
        "platform_name": _hook_platform_name,
        "path_render": _hook_path_render,
        "path_is_absolute": _hook_path_is_absolute,
        "splitroot": _hook_splitroot,
        "expandvars": _hook_expandvars,
        "which": _hook_which,
        "find_yt_dlp": _hook_find_yt_dlp,
        "find_ffmpeg_dir": _hook_find_ffmpeg_dir,
        "probe_version": _hook_probe_version,
        "auto_outdir": _hook_auto_outdir,
        "resolve_outdir": _hook_resolve_outdir,
        "show_check": _hook_show_check,
        "output_args": _hook_output_args,
        "build_args": _hook_build_args,
        "build_sub_args": _hook_build_sub_args,
    }
)


# --------------------------------------------------- phase 6 terminal layer
#
# The menu cases drive the terminal layer without a terminal, exactly
# like the python selftest: the hooks fake read_esc (or bind a StringIO
# stdin built with newline=None so python's universal-newline rules
# apply), capture stdout into a StringIO, bind CONFIG_PATH/ROOT/VERSION/
# auto_outdir/os.name/sys.stdin into a private scratch dir, and return
# records projected as {scratch}. The scripted-reply vocabulary matches
# dispatch.rs: a string is a submitted line, null is escape,
# {"eof": true} and {"ctrl_c": true} are the read failures. The fake
# read_esc writes nothing (prompts are recorded separately), so the
# captured stdout is exactly the menu layer's own output - the rust
# ScriptedReader behaves the same.


class _scripted_reader:
    def __init__(self, replies):
        self.replies = list(replies)
        self.prompts = []
        self.belows = []

    def __call__(self, prompt="", below=None):
        self.prompts.append(prompt)
        self.belows.append(list(below) if below is not None else None)
        if not self.replies:
            return ""
        item = self.replies.pop(0)
        if item is None:
            return None
        if isinstance(item, dict):
            if item.get("eof"):
                raise EOFError("EOF when reading a line")
            if item.get("ctrl_c"):
                raise KeyboardInterrupt()
            if item.get("esc"):
                return None
        return item


class _fake_stdin:
    def __init__(self, isatty):
        self._isatty = bool(isatty)

    def isatty(self):
        return self._isatty


class _bound_menu:
    """Binds the terminal-layer globals for one menu call: ROOT,
    CONFIG_PATH, VERSION, USE_COLOR, auto_outdir, os.name / the fake
    os.get_terminal_size and sys.stdin."""

    def __init__(self, module, work, case):
        self.module = module
        self.work = work
        self.case = case
        self.saved = {}

    def __enter__(self):
        module = self.module
        case = self.case
        self.saved = {
            name: getattr(module, name)
            for name in ("ROOT", "CONFIG_PATH", "VERSION", "USE_COLOR", "auto_outdir", "os")
        }
        self.saved["sys.stdin"] = module.sys.stdin
        auto = _resolve_scratch(case["auto"], self.work) if case.get("auto") else None
        real_os = module.os
        fake_os = types.SimpleNamespace(
            name="nt" if case.get("is_windows") else "posix", path=real_os.path
        )
        terminal = case.get("terminal")
        if terminal:
            fake_os.get_terminal_size = (
                lambda _t=tuple(terminal): real_os.terminal_size(_t)
            )
        else:

            def _no_size():
                raise OSError(25, "Inappropriate ioctl for device")

            fake_os.get_terminal_size = _no_size
        module.ROOT = self.work
        module.CONFIG_PATH = self.work / "config" / "settings.json"
        if "version" in case:
            module.VERSION = case["version"]
        module.USE_COLOR = bool(case.get("use_color"))
        module.auto_outdir = lambda v=auto: v
        module.os = fake_os
        module.sys.stdin = _fake_stdin(bool(case.get("stdin_tty")))
        return self

    def __exit__(self, *exc):
        module = self.module
        for name, value in self.saved.items():
            if name == "sys.stdin":
                module.sys.stdin = value
            else:
                setattr(module, name, value)
        return False


def _project_value(work, value):
    if isinstance(value, str):
        return _project(work, value)
    if isinstance(value, list):
        return [_project_value(work, item) for item in value]
    if isinstance(value, dict):
        return {key: _project_value(work, item) for key, item in value.items()}
    return value


def _seed_settings_file(module, settings):
    target = module.CONFIG_PATH
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes((json.dumps(settings, indent=2, sort_keys=True) + "\n").encode("utf-8"))
    return target


# v2.0.2 FR-17: the native menu paints the quick-paste row where the
# oracle paints its blank box row; both project to one token so the
# differ compares the row's position, not its bytes (the native row
# bytes are pinned by the crate unit tests).
QUICK_ROW_TOKEN = "{menu-quick-row}"
QUICK_ROW_TEXT = "  0  press zero '0' to paste link and start direct download"


def _project_quick_row(text):
    pad = " " * (64 - len(QUICK_ROW_TEXT))
    m = "\x1b[38;5;244m|\x1b[0m"
    text = text.replace(
        m + "\x1b[38;5;208m" + QUICK_ROW_TEXT + "\x1b[0m" + pad + m, QUICK_ROW_TOKEN
    )
    text = text.replace(m + " " * 64 + m, QUICK_ROW_TOKEN)
    text = text.replace("|" + QUICK_ROW_TEXT + pad + "|", QUICK_ROW_TOKEN)
    text = text.replace("|" + " " * 64 + "|", QUICK_ROW_TOKEN)
    return text


def _hook_menu_summary(module, args):
    try:
        value = module.menu_summary(args[0])
    except (TypeError, AttributeError, ValueError) as e:
        return {"value": None, "shape": _shape_token(e)}
    return {"value": str(value), "shape": None}


def _hook_menu_items(module, args):
    case = args[0]
    work = _case_scratch(case, "menu-items")
    try:
        with _bound_menu(module, work, case):
            try:
                rows = module.menu_items(case["s"])
            except (TypeError, AttributeError, ValueError) as e:
                return {"rows": None, "shape": _shape_token(e)}
        return {
            "rows": [
                [idx, label, _project(work, str(value)), bool(grey)]
                for idx, label, value, grey in rows
            ],
            "shape": None,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_header(module, args):
    version, use_color = args
    side = io.StringIO()

    def call():
        return _with_global(module, "USE_COLOR", bool(use_color), module.header)

    with redirect_stdout(side):
        _with_global(module, "VERSION", version, call)
    return {"stdout": side.getvalue()}


def _hook_hint(module, args):
    case = args[0]
    work = _case_scratch(case, "hint")
    try:
        with _bound_menu(module, work, case):
            side = io.StringIO()
            with redirect_stdout(side):
                module.hint(case["s"])
        return {"stdout": _project(work, side.getvalue())}
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_selftest(module, args):
    """Python `selftest()` in a clean scratch world: a defaults-only
    settings.json at CONFIG_PATH, the CLI self path a fresh copy of this
    module, and the report filtered to the check stream (the port prints
    nothing between the checks either way)."""
    work = _case_scratch({}, "selftest")
    try:
        config_dir = work / "config"
        config_dir.mkdir()
        config_path = config_dir / "settings.json"
        config_path.write_text(
            module.json.dumps(module.DEFAULTS, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        script = work / "ytdlp.py"
        shutil.copyfile(PY_IMPL, script)
        saved = {}
        for name, value in (
            ("CONFIG_PATH", config_path),
            ("CLI_SELF_PATH", script),
            ("ROOT", work),
        ):
            saved[name] = getattr(module, name)
            setattr(module, name, value)
        try:
            side = io.StringIO()
            with redirect_stdout(side):
                rc = module.selftest()
        finally:
            for name, value in saved.items():
                setattr(module, name, value)
        lines = [
            line
            for line in side.getvalue().splitlines()
            if line.startswith(("PASS ", "FAIL ", "selftest: ", "--"))
        ]
        return {"rc": int(rc), "stdout": "\n".join(lines)}
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_render(module, args):
    case = args[0]
    work = _case_scratch(case, "render")
    try:
        with _bound_menu(module, work, case):
            side = io.StringIO()
            try:
                with redirect_stdout(side):
                    module.render(case["s"])
            except (TypeError, AttributeError, ValueError) as e:
                return {"stdout": None, "shape": _shape_token(e)}
        return {"stdout": _project_quick_row(_project(work, side.getvalue())), "shape": None}
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_block_under_input(module, args):
    block, use_color, stdin_tty, terminal = args
    real_os = module.os
    saved_size = real_os.get_terminal_size
    saved_stdin = module.sys.stdin

    def fake_size():
        if terminal is None:
            raise OSError(25, "Inappropriate ioctl for device")
        return real_os.terminal_size((int(terminal[0]), int(terminal[1])))

    module.sys.stdin = _fake_stdin(stdin_tty)
    real_os.get_terminal_size = fake_size
    try:
        result = _with_global(
            module, "USE_COLOR", bool(use_color), lambda: module.block_under_input(block)
        )
    finally:
        module.sys.stdin = saved_stdin
        real_os.get_terminal_size = saved_size
    return {"result": bool(result)}


def _hook_read_esc_piped(module, args):
    prompt, below, stdin_text = args
    fake_in = io.StringIO(stdin_text, newline=None)
    side = io.StringIO()
    saved_stdin = module.sys.stdin
    module.sys.stdin = fake_in
    try:
        with redirect_stdout(side):
            try:
                line = module.read_esc(prompt or "", below=below)
                eof = False
            except EOFError:
                line, eof = None, True
    finally:
        module.sys.stdin = saved_stdin
    return {"stdout": side.getvalue(), "line": line, "eof": eof}


def _hook_line_edit(module, args):
    case = args[0]
    prompt = case.get("prompt", "> ")
    saved_history = list(module.HISTORY)
    module.HISTORY.clear()
    module.HISTORY.extend(case.get("history") or [])
    side = io.StringIO()
    snapshots = []
    try:
        with redirect_stdout(side):
            ed = module.LineEdit(prompt)
            for op in case["ops"]:
                name = op["op"]
                if name == "add":
                    ed.add(op["ch"])
                elif name == "back":
                    ed.back()
                elif name == "recall":
                    ed.recall(bool(op.get("older", True)))
                elif name == "remember":
                    ed.remember()
                elif name != "text":
                    raise InstrumentError("unknown line_edit op %r" % name)
                snapshots.append(ed.text())
            history_after = list(module.HISTORY)
    finally:
        module.HISTORY.clear()
        module.HISTORY.extend(saved_history)
    return {
        "stdout": side.getvalue(),
        "snapshots": snapshots,
        "history_after": history_after,
    }


def _menu_reader_case(module, case, work, kind):
    """Shared prologue for pick_list / ask_bool / act: scripted reader,
    bound world, seeded settings file. Returns (reader, side, cleanup
    callable)."""
    reader = _scripted_reader(case.get("replies") or [])
    side = io.StringIO()
    bound = _bound_menu(module, work, case)
    bound.__enter__()
    saved_esc = module.read_esc
    module.read_esc = reader
    return reader, side, bound, saved_esc


def _menu_case_epilogue(module, bound, saved_esc):
    module.read_esc = saved_esc
    bound.__exit__(None, None, None)


def _hook_pick_list(module, args):
    case = args[0]
    work = _case_scratch(case, "pick")
    try:
        reader, side, bound, saved_esc = _menu_reader_case(module, case, work, "pick")
        current = case.get("current")
        try:
            with redirect_stdout(side):
                try:
                    result = module.pick_list(case["title"], case["options"], current)
                    outcome = "kept" if result is current else "picked:%s" % (result,)
                except module.GoBack:
                    outcome = "goback"
                except EOFError:
                    outcome = "eof"
                except KeyboardInterrupt:
                    outcome = "interrupted"
        finally:
            _menu_case_epilogue(module, bound, saved_esc)
        return {
            "stdout": side.getvalue(),
            "outcome": outcome,
            "prompts": [_project(work, prompt) for prompt in reader.prompts],
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_ask_bool(module, args):
    case = args[0]
    work = _case_scratch(case, "ask")
    try:
        reader, side, bound, saved_esc = _menu_reader_case(module, case, work, "ask")
        current = case.get("current")
        try:
            with redirect_stdout(side):
                try:
                    result = module.ask_bool(case["label"], current)
                    if result is True:
                        outcome = "true"
                    elif result is False:
                        outcome = "false"
                    else:
                        outcome = "other"
                except module.GoBack:
                    outcome = "goback"
                except EOFError:
                    outcome = "eof"
                except KeyboardInterrupt:
                    outcome = "interrupted"
        finally:
            _menu_case_epilogue(module, bound, saved_esc)
        return {
            "stdout": side.getvalue(),
            "outcome": outcome,
            "prompts": [_project(work, prompt) for prompt in reader.prompts],
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_act(module, args):
    case = args[0]
    work = _case_scratch(case, "menu")
    try:
        reader, side, bound, saved_esc = _menu_reader_case(module, case, work, "menu")
        settings = json.loads(json.dumps(case["settings"]))
        target = _seed_settings_file(module, settings)
        try:
            with redirect_stdout(side):
                try:
                    result = module.act(case["choice"], settings)
                    outcome = "repaint_true" if result else "repaint_false"
                except module.GoBack:
                    outcome = "goback"
                except SystemExit:
                    outcome = "exit"
                except EOFError:
                    outcome = "eof"
                except KeyboardInterrupt:
                    outcome = "interrupted"
        finally:
            _menu_case_epilogue(module, bound, saved_esc)
        file_text = None
        if target.is_file():
            file_text = _project(work, target.read_text(encoding="utf-8"))
        return {
            "stdout": _project_quick_row(_project(work, side.getvalue())),
            "outcome": outcome,
            "exit": None,
            "settings_after": _project_value(work, settings),
            "settings_file": file_text,
            "prompts": [_project(work, prompt) for prompt in reader.prompts],
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_main_menu(module, args):
    case = args[0]
    work = _case_scratch(case, "menu")
    try:
        reader = _scripted_reader(case.get("replies") or [])
        side = io.StringIO()
        with _bound_menu(module, work, case):
            settings = json.loads(json.dumps(case["settings"]))
            target = _seed_settings_file(module, settings)
            saved_esc = module.read_esc
            module.read_esc = reader
            # the startup check is out of scope for this arm (phase 8);
            # can_ask false keeps the oracle off the network
            saved_ask = module.can_ask
            module.can_ask = lambda: False
            try:
                with redirect_stdout(side):
                    try:
                        module.main_menu(settings)
                        outcome, code = "return", 0
                    except SystemExit as exc:
                        outcome = "return"
                        code = exc.code if isinstance(exc.code, int) else 0
                    except EOFError:
                        outcome, code = "eof", None
                    except KeyboardInterrupt:
                        outcome, code = "interrupted", None
            finally:
                module.read_esc = saved_esc
                module.can_ask = saved_ask
            file_text = None
            if target.is_file():
                file_text = _project(work, target.read_text(encoding="utf-8"))
        return {
            "stdout": _project_quick_row(_project(work, side.getvalue())),
            "outcome": outcome,
            "exit": code,
            "settings_after": _project_value(work, settings),
            "settings_file": file_text,
            "prompts": [_project(work, prompt) for prompt in reader.prompts],
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


# --------------------------------------------------- phase 7 execution
#
# The run_download cases script the whole process surface exactly like
# the rust `RunRt` seam: a fake Popen (scripted stdout/stderr lines and
# poll returns), a fake KeyWatch (scripted escape hits), a recording
# `_kill_tree` and a fake time shim whose sleep raises KeyboardInterrupt
# once `interrupt_after` sleeps have completed (the rust side checks its
# flag after the same number of frame sleeps). The cli_args hook runs the
# real argparse parser over the schema main() builds - the same schema,
# and cmd_selftest proves the replica against the real CLI - capturing
# the three outcome classes the rust `cli_args::parse` returns.


def _age_fixture_files(work, case):
    """Backdate the case's fixture files (mtime = now - age_secs)."""
    for item in case.get("files") or []:
        if item.get("dir") or "age_secs" not in item:
            continue
        target = work / item["path"]
        when = time.time() - float(item["age_secs"])
        os.utime(str(target), (when, when))


def _hook_newest_since(module, args):
    case = args[0]
    work = _case_scratch(case, "dlfs")
    try:
        _age_fixture_files(work, case)
        folder = Path(_resolve_scratch(case["folder"], work))
        since = time.time() + float(case["since_offset"])
        result = module.newest_since(folder, since, float(case["slack"]))
        return _project(work, str(result)) if result is not None else None
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_clean_partials(module, args):
    case = args[0]
    work = _case_scratch(case, "dlfs")
    try:
        _age_fixture_files(work, case)
        folder = Path(_resolve_scratch(case["folder"], work))
        since = time.time() + float(case["since_offset"])
        removed = module.clean_partials(folder, since, float(case["slack"]))
        return sorted(removed)
    finally:
        shutil.rmtree(work, ignore_errors=True)


class _fake_proc:
    def __init__(self, case):
        self.stdout = [line.encode("utf-8") + b"\n" for line in case.get("stdout") or []]
        self.stderr = [line.encode("utf-8") + b"\n" for line in case.get("stderr") or []]
        self.polls = list(case.get("polls") or [])
        self.pid = 4242
        self.returncode = 0

    def poll(self):
        if self.polls:
            rc = self.polls.pop(0)
        else:
            rc = 0
        if rc is not None:
            self.returncode = rc
        return rc

    def wait(self):
        return 0


class _fake_keywatch:
    def __init__(self, case):
        self.hits = list(case.get("escapes") or [])
        self.restored = False

    def escape_hit(self):
        return self.hits.pop(0) if self.hits else False

    def restore(self):
        self.restored = True


class _fake_time:
    def __init__(self, interrupt_after):
        self.interrupt_after = interrupt_after
        self.sleeps = 0

    def sleep(self, _secs):
        self.sleeps += 1
        if self.interrupt_after is not None and self.sleeps > self.interrupt_after:
            raise KeyboardInterrupt()

    def time(self):
        return 0.0


class _bound_exec_globals:
    """subprocess / KeyWatch / _kill_tree / time / USE_COLOR bound for one
    scripted run_download call."""

    def __init__(self, module, case, proc, watch, shim, kills):
        self.module = module
        self.case = case
        self.proc = proc
        self.watch = watch
        self.shim = shim
        self.kills = kills

    def __enter__(self):
        m = self.module
        names = ("subprocess", "KeyWatch", "_kill_tree", "time", "USE_COLOR")
        self.saved = {name: getattr(m, name) for name in names}
        m.subprocess = types.SimpleNamespace(
            PIPE=subprocess.PIPE,
            DEVNULL=subprocess.DEVNULL,
            Popen=lambda argv, **kw: self.proc,
        )
        m.KeyWatch = lambda: self.watch
        m._kill_tree = lambda p: self.kills.__setitem__(0, self.kills[0] + 1)
        m.time = self.shim
        m.USE_COLOR = bool(self.case.get("use_color"))
        return self

    def __exit__(self, *exc):
        for name, value in self.saved.items():
            setattr(self.module, name, value)
        return False


def _hook_run_download(module, args):
    case = args[0]
    kills = [0]
    proc = _fake_proc(case)
    watch = _fake_keywatch(case)
    shim = _fake_time(case.get("interrupt_after"))
    stage = None
    if case.get("stage"):
        stage = module.Stage("downloading (escape to stop)", [], 0.0, [(".", 214)])
    with _bound_exec_globals(module, case, proc, watch, shim, kills):
        rc, stopped, err, codes = module.run_download("yt-dlp", [], stage)
    return {
        "rc": rc,
        "stopped": stopped,
        "err": err,
        "sub_codes": list(codes),
        "live": stage.live if stage is not None else None,
        "kill_calls": kills[0],
    }


def _cli_parser(module, prog, width_hint=None):
    """main()'s parser rebuilt for the arm; the container/resolution/audio
    value lists come from the module so they cannot drift."""
    if width_hint is not None:
        os.environ["COLUMNS"] = str(width_hint + 2)
    else:
        os.environ.pop("COLUMNS", None)
    p = argparse.ArgumentParser(description="yt-dlp CLI wrapper", prog=prog)
    p.add_argument("url", nargs="?", help="download this URL with saved defaults")
    p.add_argument("-a", "--audio", action="store_true", help="audio only")
    p.add_argument("-f", "--format", choices=module.CONTAINERS, help="container")
    p.add_argument("-r", "--resolution", choices=module.RESOLUTIONS, help="max height")
    p.add_argument("--af", choices=module.AUDIO_FORMATS, help="audio format")
    p.add_argument("-o", "--outdir", help="output folder")
    p.add_argument("--template", help="output filename template")
    p.add_argument("--subs", action="store_true", help="subtitles")
    p.add_argument("--playlist", action="store_true", help="playlist mode")
    p.add_argument("--menu", action="store_true", help="force menu even with URL")
    p.add_argument("--check", action="store_true", help="show resolved paths/versions")
    p.add_argument("--update", action="store_true", help="update the yt-dlp binary in place")
    p.add_argument("--selftest", action="store_true", help="run built-in logic tests")
    return p


def _hook_cli_args(module, args):
    case = args[0]
    parser = _cli_parser(module, case["prog"], width_hint=int(case["width"]))
    out, err = io.StringIO(), io.StringIO()
    try:
        with redirect_stdout(out), redirect_stderr(err):
            ns = parser.parse_args(case["argv"])
    except SystemExit as exc:
        if exc.code == 0:
            return {"kind": "help", "exit": 0, "text": out.getvalue()}
        return {"kind": "error", "exit": 2, "text": err.getvalue()}
    finally:
        os.environ.pop("COLUMNS", None)
    return {
        "kind": "parsed",
        "url": ns.url,
        "audio": bool(ns.audio),
        "format": ns.format,
        "resolution": ns.resolution,
        "af": ns.af,
        "outdir": ns.outdir,
        "template": ns.template,
        "subs": bool(ns.subs),
        "playlist": bool(ns.playlist),
        "menu": bool(ns.menu),
        "check": bool(ns.check),
        "update": bool(ns.update),
        "selftest": bool(ns.selftest),
    }


def _selftest_cli_replica():
    """Instrument self-check (NFR-4 spirit): the cli_args hook's parser
    replica must match the real CLI before the arm judges anything."""
    spec = importlib.util.spec_from_file_location("ytdlp_oracle", PY_IMPL)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    problems = []
    env = dict(os.environ)
    env["PYTHONIOENCODING"] = "utf-8"
    env["COLUMNS"] = "80"
    env["LINES"] = "24"
    proc = subprocess.run(
        [sys.executable, str(PY_IMPL), "-h"], capture_output=True, env=env, timeout=60
    )
    real_help = proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")
    parser = _cli_parser(module, "ytdlp.py", width_hint=78)
    replica_help = parser.format_help()
    os.environ.pop("COLUMNS", None)
    if real_help != replica_help:
        problems.append("help text differs from the real CLI")
    env2 = dict(os.environ)
    env2["PYTHONIOENCODING"] = "utf-8"
    proc2 = subprocess.run(
        [sys.executable, str(PY_IMPL), "-f", "junk"],
        capture_output=True,
        env=env2,
        timeout=60,
        input=b"",
    )
    real_err = proc2.stderr.decode("utf-8", "replace").replace("\r\n", "\n")
    out, err = io.StringIO(), io.StringIO()
    try:
        with redirect_stdout(out), redirect_stderr(err):
            _cli_parser(module, "ytdlp.py", width_hint=78).parse_args(["-f", "junk"])
    except SystemExit:
        pass
    os.environ.pop("COLUMNS", None)
    if err.getvalue() != real_err:
        problems.append("error text differs from the real CLI")
    return problems


PY_CALL_HOOKS.update(
    {
        "menu_summary": _hook_menu_summary,
        "menu_items": _hook_menu_items,
        "header": _hook_header,
        "hint": _hook_hint,
        "render": _hook_render,
        "block_under_input": _hook_block_under_input,
        "read_esc_piped": _hook_read_esc_piped,
        "line_edit": _hook_line_edit,
        "pick_list": _hook_pick_list,
        "ask_bool": _hook_ask_bool,
        "act": _hook_act,
        "main_menu": _hook_main_menu,
        "selftest": _hook_selftest,
        "newest_since": _hook_newest_since,
        "clean_partials": _hook_clean_partials,
        "run_download": _hook_run_download,
        "cli_args": _hook_cli_args,
    }
)


# ------------------------------------ phase 8: network and updaters
#
# The network never dials: `urlopen` is replaced by a scripted response, so
# http_text / http_file / expected_sha / install_ffmpeg / the updater arms
# run offline on both sides. Lines that interpolate an exception's text are
# pinned because the *fake* raises with the same text the seam returns; the
# real transports word their failures differently, which the port notes
# record as shape-gated.
#
# Archive fixtures are built by each side from a member list rather than
# carried as bytes in the vector: the case stays readable and both sides
# prove they read the same archive shape.

import lzma  # noqa: E402
import re  # noqa: E402
import stat  # noqa: E402
import tarfile  # noqa: E402
import zipfile  # noqa: E402

FALLBACK_PROBE = ("", -1)


def _file_hex(path):
    try:
        return path.read_bytes().hex()
    except (OSError, ValueError):
        return None


def _mode_of(path):
    """The file mode on posix, null elsewhere: python's os.stat and rust's
    PermissionsExt report different things on windows, so the record does
    not ask there."""
    if os.name == "nt":
        return None
    try:
        return oct(stat.S_IMODE(path.stat().st_mode))
    except OSError:
        return None


def _listing(folder):
    """`[{"name", "dir", "hex", "mode"}]` sorted, so a case can assert what a
    run left on disk."""
    out = []
    if not folder.is_dir():
        return out
    for item in sorted(folder.iterdir(), key=lambda p: p.name):
        out.append(
            {
                "name": item.name,
                "dir": item.is_dir(),
                "hex": None if item.is_dir() else _file_hex(item),
                "mode": None if item.is_dir() else _mode_of(item),
            }
        )
    return out


def _project_rand(text):
    """`tempfile.mkdtemp` and the port's mkdtemp both hide a suffix behind
    the same `.ff-` prefix - a random one and a clock-derived one - so the
    record pins the shape, not the name."""
    return re.sub(r"\.ff-[^/\\]*", ".ff-<rand>", text)


def _project_install(text, work):
    return _project_rand(_project(work, text))


# the archive-fault payload is exception text, so it is shape-gated like
# every other one: the line shape is the pin, the wording is each
# library's own
UNREADABLE = "  the downloaded archive was unreadable: "
UNREADABLE_TOKEN = UNREADABLE + "{archive-error}\n"


def _project_product(text, module):
    """The two lines the native port spells differently by design: the
    product version and the update source (FR-13). Both become tokens, so
    the arm pins the line shapes and the spec pins the values."""
    return text.replace(
        "this is v%s" % module.VERSION, "this is v{version}"
    ).replace(module.CLI_UPDATE_URL, "{update-url}")


def _project_unreadable(text):
    # the whole line is replaced: the payload is each library's own wording
    return re.sub(re.escape(UNREADABLE) + r"[^\n]*", UNREADABLE_TOKEN.rstrip("\n"), text)


def _body_bytes(case):
    """The body a case carries: `chunks` (hex strings) or `text`."""
    if case.get("chunks") is not None:
        return b"".join(bytes.fromhex(chunk) for chunk in case["chunks"])
    return (case.get("text") or "").encode("utf-8")


class _fake_response:
    """What urlopen hands back: read(n), geturl(), headers, and a context
    manager - the shape http_text and http_file use."""

    def __init__(self, case, url):
        self._body = _body_bytes(case)
        self._pos = 0
        self._reads = 0
        self._final = case.get("final_url") or url
        self._error = case.get("error")
        self._error_after = case.get("error_after_reads")
        if "content_length" in case:
            length = case["content_length"]
            self.headers = {} if length is None else {"Content-Length": str(length)}
        else:
            self.headers = {"Content-Length": str(len(self._body))}

    def read(self, size=-1):
        self._reads += 1
        if self._error and (self._error_after is None or self._reads > self._error_after):
            raise OSError(self._error)
        if size is None or size < 0:
            chunk = self._body[self._pos :]
        else:
            chunk = self._body[self._pos : self._pos + size]
        self._pos += len(chunk)
        return chunk

    def geturl(self):
        return self._final

    def close(self):
        pass

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


def _fake_urlopen(kind, case, log):
    def urlopen(req, timeout=None, **kwargs):
        url = getattr(req, "full_url", req)
        headers = {k.lower(): v for k, v in (getattr(req, "headers", None) or {}).items()}
        log.append(
            {
                "kind": kind,
                "url": url,
                "timeout": timeout,
                "user_agent": headers.get("user-agent"),
            }
        )
        if case.get("open_error"):
            raise OSError(case["open_error"])
        return _fake_response(case, url)

    return urlopen


def _bound_urlopen(module, kind, case, log):
    return _bound_world(module.urllib.request, attrs={"urlopen": _fake_urlopen(kind, case, log)})


class _stage_shim:
    """What http_file writes its progress into: python sets `stage.live`
    and the arm records the last text, exactly like the run_download arm
    records its stage."""

    def __init__(self):
        self.live = None


def _hook_http_text(module, args):
    case = args[0]
    log = []
    with _bound_urlopen(module, "text", case, log):
        try:
            text = module.http_text(case["url"], int(case.get("timeout", 60)))
            out = {"ok": True, "text": text, "error": None}
        except Exception as exc:
            out = {"ok": False, "text": None, "error": str(exc)}
    out["calls"] = log
    return out


def _hook_http_file(module, args):
    case = args[0]
    work = _case_scratch(case, "httpfile")
    try:
        dest = Path(_resolve_scratch(case["dest"], work))
        dest.parent.mkdir(parents=True, exist_ok=True)
        stage = _stage_shim() if case.get("stage") else None
        log = []
        with _bound_urlopen(module, "file", case, log):
            try:
                got, final, total = module.http_file(
                    case["url"], dest, stage, int(case.get("timeout", module.DL_TIMEOUT))
                )
                error = None
            except Exception as exc:
                got = final = total = None
                error = str(exc)
        part = dest.with_name(dest.name + ".part")
        return {
            "ok": error is None,
            "error": error,
            "got": got,
            "final": final,
            "total": total,
            "live": None if stage is None else stage.live,
            "dest": _project(work, str(dest)),
            "dest_hex": _file_hex(dest),
            "part_exists": part.exists(),
            "part_hex": _file_hex(part),
            "calls": log,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_sha256_file(module, args):
    case = args[0]
    work = _case_scratch(case, "sha")
    try:
        path = Path(_resolve_scratch(case["path"], work))
        path.parent.mkdir(parents=True, exist_ok=True)
        if case.get("missing"):
            return {"ok": False, "digest": None}
        path.write_bytes(_body_bytes(case))
        return {"ok": True, "digest": module.sha256_file(path)}
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _hook_expected_sha(module, args):
    case = args[0]
    side = io.StringIO()
    log = []
    with _bound_urlopen(module, "text", case, log), redirect_stdout(side):
        digest = module.expected_sha(case["url"], case["sha_url"], case["asset"])
    return {"digest": digest, "stdout": side.getvalue(), "calls": log}


def _build_archive(work, spec):
    """Materialise one archive from the case's member list. Each side
    builds its own bytes from the same shape; the record never carries the
    archive itself."""
    path = Path(_resolve_scratch(spec["path"], work))
    path.parent.mkdir(parents=True, exist_ok=True)
    members = [
        (member[0], (member[1] if len(member) > 1 else "").encode("utf-8"))
        for member in spec["members"]
    ]
    if spec.get("how") == "zip":
        with zipfile.ZipFile(str(path), "w", zipfile.ZIP_DEFLATED) as zf:
            for name, body in members:
                zf.writestr(name, body)
    else:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w") as tf:
            for name, body in members:
                info = tarfile.TarInfo(name)
                info.size = len(body)
                tf.addfile(info, io.BytesIO(body))
        with lzma.open(str(path), "wb") as fh:
            fh.write(raw.getvalue())
    if spec.get("truncate"):
        data = path.read_bytes()
        path.write_bytes(data[: max(1, int(len(data) * float(spec["truncate"])))])
    return str(path)


def _hook_extract_member(module, args):
    case = args[0]
    work = _case_scratch(case, "archive")
    try:
        blob = _build_archive(work, case["archive"])
        outdir = Path(_resolve_scratch(case["outdir"], work))
        outdir.mkdir(parents=True, exist_ok=True)
        side = io.StringIO()
        with redirect_stdout(side):
            result = module.extract_member(blob, case["how"], case["binname"], outdir)
        return {
            "result": _project_install(str(result), work) if result is not None else None,
            "stdout": _project_unreadable(side.getvalue()),
            "listing": _listing(outdir),
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


class _fake_probe:
    """`probe_version(script)`: one (version, rc) per call, and the paths
    each call saw, in order."""

    def __init__(self, queue, log):
        self.queue = list(queue)
        self.log = log

    def __call__(self, path):
        self.log.append(str(path))
        if not self.queue:
            return FALLBACK_PROBE
        return tuple(self.queue.pop(0))


def _hook_install_ffmpeg(module, args):
    case = args[0]
    work = _case_scratch(case, "ffmpeg")
    try:
        bin_dir = Path(_resolve_scratch(case.get("bin_dir", "{scratch}/bin"), work))
        bin_dir.mkdir(parents=True, exist_ok=True)
        blob = _build_archive(work, case["archive"])
        digest = module.sha256_file(blob)
        asset = Path(blob).name
        log = []
        side = io.StringIO()
        probe_log = []
        probe = _fake_probe(case.get("probes") or [], probe_log)
        sleeps = []
        saved = {
            "platform_name": module.platform_name,
            "BIN_DIR": module.BIN_DIR,
            "http_file": module.http_file,
            "http_text": module.http_text,
            "probe_version": module.probe_version,
        }
        module.platform_name = lambda: case["platform"]
        module.BIN_DIR = bin_dir
        module.probe_version = probe

        src = module.FFMPEG_SOURCES.get(case["platform"])
        src_url = src[0] if src else None
        src_sha = src[1] if src else None

        def _tok(url):
            # the offer source urls are per-implementation (the native
            # FR-14 amendment points windows at BtbN); project them to
            # tokens like the update-source token in the phase-8 records
            if url == src_url:
                return "{ffmpeg-url}"
            if url == src_sha:
                return "{ffmpeg-sha-url}"
            return url

        def fake_http_text(url, timeout=60):
            log.append({"kind": "text", "url": _tok(url), "timeout": timeout, "user_agent": None})
            if case.get("sha_error"):
                raise OSError(case["sha_error"])
            return "%s  %s\n" % (digest, asset)

        def fake_http_file(url, dest, stage=None, timeout=module.DL_TIMEOUT):
            log.append({"kind": "file", "url": _tok(url), "timeout": timeout, "user_agent": None})
            if stage is not None:
                stage.live = "downloading ffmpeg - 0.0 / 0.0 MB"
            if case.get("download_error"):
                raise OSError(case["download_error"])
            shutil.copyfile(blob, str(dest))
            size = os.path.getsize(blob)
            return (size, url, size)

        module.http_text = fake_http_text
        module.http_file = fake_http_file
        try:
            with _bound_world(
                module.time, attrs={"sleep": lambda secs: sleeps.append(secs)}
            ), redirect_stdout(side):
                result = module.install_ffmpeg(_stage_shim() if case.get("stage") else None)
        finally:
            for name, value in saved.items():
                setattr(module, name, value)
        return {
            "result": _project_install(str(result), work) if result is not None else None,
            "stdout": _project_unreadable(_project_rand(side.getvalue())),
            "probes": [_project_install(path, work) for path in probe_log],
            # the protocol carries no bare floats, so python's float()
            # spelling is the value
            "sleeps": [repr(secs) for secs in sleeps],
            "listing": _listing(bin_dir),
            "calls": log,
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _scripted_call(case, calls, argv):
    calls.append(list(argv))
    if case.get("call_interrupt"):
        raise KeyboardInterrupt()
    queue = list(case.get("call_rcs") or [])
    return queue.pop(0) if queue else 0


def _hook_update_yt_dlp(module, args):
    case = args[0]
    work = _case_scratch(case, "upd")
    try:
        settings = json.loads(json.dumps(case.get("settings") or {}))
        target = None
        if case.get("binary"):
            target = Path(_resolve_scratch(case["binary"], work))
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"yt-dlp")
        probe_log = []
        probe = _fake_probe(case.get("probes") or [], probe_log)
        calls = []
        side = io.StringIO()
        saved = {
            "find_yt_dlp": module.find_yt_dlp,
            "probe_version": module.probe_version,
        }
        module.find_yt_dlp = lambda s, *a, **k: target
        module.probe_version = probe
        try:
            with _bound_world(
                module.subprocess, attrs={"call": lambda argv, **kw: _scripted_call(case, calls, argv)}
            ), redirect_stdout(side):
                result = module.update_yt_dlp(settings)
        finally:
            for name, value in saved.items():
                setattr(module, name, value)
        return {
            "result": result,
            "stdout": _project(work, side.getvalue()),
            "probes": [_project_install(path, work) for path in probe_log],
            "calls": [
                [_project(work, part) for part in argv] for argv in calls
            ],
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


class _updater_clock:
    """`time.time()` answers the case's stamp, so the 86400 s throttle is
    testable and no wall clock enters the record."""

    def __init__(self, case):
        self.now = float(case.get("now", 0.0))

    def time(self):
        return self.now

    def sleep(self, _secs):
        pass


def _hook_auto_update_check(module, args):
    return _hook_updater(module, args, mode="check")


def _hook_update_all(module, args):
    return _hook_updater(module, args, mode="all")


def _hook_updater(module, args, mode):
    """The updater arms. They stop before the install on both sides: the
    gates, the notice, the prompt and the stamps are python's, and the
    native install path past the prompt is covered by the cli crate's own
    tests (the native contract is a recorded deviation)."""
    case = args[0]
    work = _case_scratch(case, "upd")
    try:
        settings = json.loads(json.dumps(case.get("settings") or {}))
        reader = _scripted_reader(case.get("replies") or [])
        fetches = []
        installs = []
        probe_log = []
        side = io.StringIO()
        target = Path(_resolve_scratch(case.get("self_path", "{scratch}/ytdlp"), work))
        target.parent.mkdir(parents=True, exist_ok=True)
        if not case.get("self_missing"):
            target.write_bytes(b"the running image")

        def fake_fetch(url=None, timeout=60):
            fetches.append({"timeout": timeout})
            if case.get("fetch_error"):
                raise OSError(case["fetch_error"])
            return (case.get("version"), case.get("raw") or "")

        def fake_install(raw, target_path=None):
            installs.append({"raw": raw, "target": _project(work, str(target_path or target))})
            if case.get("install_error"):
                return (False, case["install_error"])
            return (True, str(target_path or target))

        saved = {
            "fetch_cli": module.fetch_cli,
            "install_cli": module.install_cli,
            "read_esc": module.read_esc,
            "can_ask": module.can_ask,
            "time": module.time,
        }
        if mode == "all":
            saved["find_yt_dlp"] = module.find_yt_dlp
            saved["probe_version"] = module.probe_version
            calls = []
            module.find_yt_dlp = lambda s, *a, **k: (
                Path(_resolve_scratch(case["binary"], work)) if case.get("binary") else None
            )
            module.probe_version = _fake_probe(case.get("probes") or [], probe_log)
        module.fetch_cli = fake_fetch
        module.install_cli = fake_install
        module.read_esc = reader
        module.can_ask = lambda: bool(case.get("can_ask", True))
        module.time = _updater_clock(case)
        try:
            with redirect_stdout(side):
                try:
                    if mode == "all":
                        with _bound_world(
                            module.subprocess,
                            attrs={
                                "call": lambda argv, **kw: _scripted_call(case, calls, argv)
                            },
                        ):
                            result = module.update_all(settings)
                    else:
                        result = module.auto_update_check(settings, bool(case.get("force")))
                    error = None
                except EOFError:
                    result, error = None, "eof"
                except KeyboardInterrupt:
                    result, error = None, "interrupted"
        finally:
            for name, value in saved.items():
                setattr(module, name, value)
        return {
            "result": result,
            "error": error,
            "stdout": _project(work, _project_product(side.getvalue(), module)),
            "fetches": fetches,
            "installs": installs,
            "probes": [_project_install(path, work) for path in probe_log],
            "prompts": [_project(work, prompt) for prompt in reader.prompts],
            "settings_after": _project_value(work, settings),
        }
    finally:
        shutil.rmtree(work, ignore_errors=True)


PY_CALL_HOOKS.update(
    {
        "http_text": _hook_http_text,
        "http_file": _hook_http_file,
        "sha256_file": _hook_sha256_file,
        "expected_sha": _hook_expected_sha,
        "extract_member": _hook_extract_member,
        "install_ffmpeg": _hook_install_ffmpeg,
        "update_yt_dlp": _hook_update_yt_dlp,
        "auto_update_check": _hook_auto_update_check,
        "update_all": _hook_update_all,
    }
)


def capture_python(fn, vectors_path):
    require_oracle()
    doc = load_vectors(vectors_path)
    if doc["fn"] != fn:
        raise InstrumentError("%s is for %s, requested %s" % (vectors_path, doc["fn"], fn))
    if fn.startswith("_"):
        raise InstrumentError("private names are outside the harness surface: %r" % fn)
    spec = importlib.util.spec_from_file_location("ytdlp_oracle", PY_IMPL)
    if spec is None or spec.loader is None:
        raise InstrumentError("cannot load %s" % PY_IMPL)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    hook = PY_CALL_HOOKS.get(fn)
    if hook is None:
        func = getattr(module, fn, None)
        if func is None or not callable(func):
            raise InstrumentError("python implementation has no callable %r" % fn)
    else:
        # Some phase-4 names (path_render, splitroot, expandvars, which)
        # drive stdlib entry points instead of a module-level function.
        func = None
    records = []
    for i, case in enumerate(doc["cases"]):
        side = io.StringIO()
        try:
            with redirect_stdout(side):
                if hook is not None:
                    value = hook(module, case["args"])
                else:
                    value = func(*case["args"])
                if case.get("capture_args"):
                    value = {"args_after": case["args"], "result": value}
        except InstrumentError:
            raise
        except Exception as e:
            raise InstrumentError("case %d raised %s: %s" % (i, type(e).__name__, e))
        if side.getvalue():
            raise InstrumentError(
                "case %d printed %d chars; the value protocol does not carry side output"
                % (i, len(side.getvalue()))
            )
        check_jsonable(value, i)
        records.append(json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True))
    return encode_records(records)


def capture_rust(fn, vectors_path, rust_bin):
    bin_path = normalize_rust_bin(rust_bin)
    if not bin_path.exists():
        raise InstrumentError(
            "rust harness binary not found: %s (build with: cargo build --release -p ytdlp-harness)" % bin_path
        )
    proc = subprocess.run([str(bin_path), fn, "--vectors", str(vectors_path)], capture_output=True)
    if proc.returncode != 0:
        err = proc.stderr.decode("utf-8", "replace").strip().splitlines()
        first = err[0] if err else "(no stderr)"
        raise InstrumentError("rust harness exited %d: %s" % (proc.returncode, first))
    return proc.stdout


def count_records(data):
    if not data:
        return 0
    return data.count(b"\n")


def excerpt(data):
    if isinstance(data, bytes):
        data = data.decode("utf-8", "replace")
    text = ascii(data)
    if len(text) > 120:
        text = text[:117] + "..."
    return text


def diff_bytes(expected, actual):
    """Return (equal, report). The report is ASCII-only and capped."""
    if expected == actual:
        return True, "0 mismatches over %d records" % count_records(expected)
    exp = expected.split(b"\n")
    act = actual.split(b"\n")
    if exp and exp[-1] == b"":
        exp.pop()
    if act and act[-1] == b"":
        act.pop()
    total = max(len(exp), len(act))
    diffs = 0
    first = None
    for i in range(total):
        e = exp[i] if i < len(exp) else None
        a = act[i] if i < len(act) else None
        if e != a:
            diffs += 1
            if first is None:
                first = i
    if first is None:
        return False, "records match but framing differs (trailing newline)"
    lines = [
        "%d of %d records differ; first at record %d" % (diffs, total, first + 1),
        "  expected: " + excerpt(exp[first]) if first < len(exp) else "  expected: (missing)",
        "  actual:   " + excerpt(act[first]) if first < len(act) else "  actual:   (missing)",
    ]
    return False, "\n".join(lines)


def cmd_selftest(args):
    fn = args.fn
    vectors = Path(args.vectors) if args.vectors else default_vectors(fn)
    doc = load_vectors(vectors)
    if doc["fn"] != fn:
        raise InstrumentError("%s is for %s, requested %s" % (vectors, doc["fn"], fn))
    print("instrument: oracle CPython %s, fn %s, %d cases, vectors %s" % (sys.version.split()[0], fn, len(doc["cases"]), vectors))
    checks = []

    first = capture_python(fn, vectors)
    second = capture_python(fn, vectors)
    equal, report = diff_bytes(first, second)
    checks.append(("differ green (python vs python, known equal)", equal, report))

    mutated = bytearray(second)
    if mutated:
        mutated[len(mutated) // 2] ^= 0x01
    else:
        mutated.extend(b"X")
    equal, report = diff_bytes(first, bytes(mutated))
    checks.append(("differ red (one flipped byte, known unequal)", not equal, report))

    got = count_records(first)
    checks.append(("record count matches vector cases", got == len(doc["cases"]), "%d records, %d cases" % (got, len(doc["cases"]))))

    if fn == "cli_args":
        problems = _selftest_cli_replica()
        checks.append(
            (
                "cli_args parser replica matches the real CLI",
                not problems,
                "ok" if not problems else "; ".join(problems),
            )
        )

    failed = 0
    for name, ok, detail in checks:
        print("%s %s: %s" % ("PASS" if ok else "FAIL", name, detail))
        if not ok:
            failed += 1
    print("SELFTEST %s (%d checks, %d failed)" % ("FAIL" if failed else "PASS", len(checks), failed))
    return 1 if failed else 0


def cmd_run(args):
    vectors = Path(args.vectors) if args.vectors else default_vectors(args.fn)
    expected = capture_python(args.fn, vectors)
    rust_bin = Path(args.rust_bin) if args.rust_bin else default_rust_bin()
    actual = capture_rust(args.fn, vectors, rust_bin)
    equal, report = diff_bytes(expected, actual)
    if equal:
        print("PASS %s: %d records, %s" % (args.fn, count_records(expected), report))
        return 0
    print("FAIL %s: %s" % (args.fn, report))
    return 1


def cmd_run_all(args):
    vectors_dir = Path(args.vectors_dir) if args.vectors_dir else VECTORS_DIR
    files = sorted(p for p in vectors_dir.glob("*.json") if p.name != ERROR_PATH_FIXTURE)
    if not files:
        raise InstrumentError("no vector files in %s" % vectors_dir)
    rust_bin = Path(args.rust_bin) if args.rust_bin else default_rust_bin()
    passed = 0
    failed = 0
    total_records = 0
    for path in files:
        doc = load_vectors(path)
        fn = doc["fn"]
        if path.stem != fn:
            raise InstrumentError(
                "vector %s declares fn %r; file name and fn must agree" % (path, fn)
            )
        expected = capture_python(fn, path)
        actual = capture_rust(fn, path, rust_bin)
        equal, report = diff_bytes(expected, actual)
        total_records += count_records(expected)
        print("%s %s: %s" % ("PASS" if equal else "FAIL", fn, report))
        if equal:
            passed += 1
        else:
            failed += 1
    print(
        "RUN-ALL %s (%d functions, %d passed, %d failed, %d records)"
        % ("FAIL" if failed else "PASS", len(files), passed, failed, total_records)
    )
    return 1 if failed else 0


def cmd_capture(args):
    vectors = Path(args.vectors) if args.vectors else default_vectors(args.fn)
    if args.impl == "python":
        data = capture_python(args.fn, vectors)
    else:
        rust_bin = Path(args.rust_bin) if args.rust_bin else default_rust_bin()
        data = capture_rust(args.fn, vectors, rust_bin)
    Path(args.out).write_bytes(data)
    print("captured %d records from %s into %s" % (count_records(data), args.impl, args.out))
    return 0


def cmd_diff(args):
    try:
        a = Path(args.a).read_bytes()
        b = Path(args.b).read_bytes()
    except OSError as e:
        raise InstrumentError("cannot read diff input: %s" % e)
    equal, report = diff_bytes(a, b)
    print("%s diff: %s" % ("PASS" if equal else "FAIL", report))
    return 0 if equal else 1


def cmd_probe_rust(args):
    vectors = Path(args.vectors) if args.vectors else VECTORS_DIR / "no_such_function.json"
    doc = load_vectors(vectors)
    rust_bin = Path(args.rust_bin) if args.rust_bin else default_rust_bin()
    bin_path = normalize_rust_bin(rust_bin)
    if not bin_path.exists():
        raise InstrumentError(
            "rust harness binary not found: %s (build with: cargo build --release -p ytdlp-harness)" % bin_path
        )
    proc = subprocess.run([str(bin_path), doc["fn"], "--vectors", str(vectors)], capture_output=True)
    first = proc.stderr.decode("utf-8", "replace").strip().splitlines()
    first = first[0] if first else ""
    ok = proc.returncode == 2 and b"unknown function" in proc.stderr and proc.stdout == b""
    detail = "exit %d, stderr %s" % (proc.returncode, excerpt(first))
    print("%s probe-rust: %s" % ("PASS" if ok else "FAIL", detail))
    return 0 if ok else 1


def main(argv=None):
    parser = argparse.ArgumentParser(prog="harness.py", description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("selftest", help="prove the instrument on known answers (python only)")
    p.add_argument("--fn", default="clean_url")
    p.add_argument("--vectors")
    p.set_defaults(func=cmd_selftest)

    p = sub.add_parser("run", help="python vs rust differential over one vector file")
    p.add_argument("--fn", required=True)
    p.add_argument("--vectors")
    p.add_argument("--rust-bin")
    p.set_defaults(func=cmd_run)

    p = sub.add_parser("run-all", help="differential over every vector file (gate 2)")
    p.add_argument("--vectors-dir")
    p.add_argument("--rust-bin")
    p.set_defaults(func=cmd_run_all)

    p = sub.add_parser("capture", help="write one implementation's output bytes")
    p.add_argument("--impl", choices=["python", "rust"], required=True)
    p.add_argument("--fn", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--vectors")
    p.add_argument("--rust-bin")
    p.set_defaults(func=cmd_capture)

    p = sub.add_parser("diff", help="byte-compare two captures")
    p.add_argument("--a", required=True)
    p.add_argument("--b", required=True)
    p.set_defaults(func=cmd_diff)

    p = sub.add_parser("probe-rust", help="assert the dispatcher refuses unknown functions")
    p.add_argument("--vectors")
    p.add_argument("--rust-bin")
    p.set_defaults(func=cmd_probe_rust)

    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except InstrumentError as e:
        print("ERROR: %s" % e, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
