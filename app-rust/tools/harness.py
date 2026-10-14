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
                           shape tokens (payloads stay shape-gated)

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
import os
import shutil
import subprocess
import sys
import tempfile
import types
from contextlib import redirect_stdout
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
APP_RUST = TOOLS_DIR.parent
REPO_ROOT = APP_RUST.parent
PY_IMPL = REPO_ROOT / "app-python" / "src" / "ytdlp.py"
VECTORS_DIR = APP_RUST / "vectors"
ORACLE = (3, 14)
# the dispatcher error-path fixture; it exists to prove the rust side
# refuses unknown names and is never part of the differential.
ERROR_PATH_FIXTURE = "no_such_function.json"


class InstrumentError(Exception):
    """The instrument itself cannot run; never a verdict."""


def require_oracle():
    if sys.version_info[:2] != ORACLE:
        raise InstrumentError(
            "oracle interpreter is CPython %d.%d.x, running %s"
            % (ORACLE[0], ORACLE[1], sys.version.split()[0])
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
    func = getattr(module, fn, None)
    if func is None or not callable(func):
        raise InstrumentError("python implementation has no callable %r" % fn)
    records = []
    hook = PY_CALL_HOOKS.get(fn)
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
