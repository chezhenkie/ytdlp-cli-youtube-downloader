"""selftest_compare report diff: name/verdict streams of both reports."""
import sys

def stream(path):
    names = []
    for line in open(path, encoding="utf-8", errors="replace"):
        if line.startswith("PASS ") or line.startswith("FAIL "):
            verdict = line[:4]
            rest = line[5:].rstrip("\n")
            names.append((verdict, rest))
    return names

# The three recorded renames (specsheet 10 conformance map): where python
# patched a module global the port never threads, and the 2026-10-16c
# FR-14 amendment repointed the native windows offer source, the native
# check carries an honest name; the compare maps them instead of failing.
ALIAS = {
    "cli update source is the releases api": "cli update url is raw main branch",
    "install_cli refuses an image that fails its own checks": "install_cli refuses a file that does not parse",
    "windows ffmpeg source is BtbN": "windows ffmpeg source is gyan.dev",
}

native = [(v, ALIAS.get(n, n)) for v, n in stream(sys.argv[1])]
oracle = stream(sys.argv[2])
print(f"native {len(native)} checks, oracle {len(oracle)} checks")
if len(native) != len(oracle):
    print("COUNT MISMATCH")
extra = [n for n in native if n not in oracle]
print("pairs differing:", extra if extra else "none")
for i, (n, o) in enumerate(zip(native, oracle), 1):
    if n != o:
        print(f"line {i}: native={n!r} oracle={o!r}")
