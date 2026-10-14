#!/usr/bin/env python3
"""Dump the PE import table of a windows executable and assert patterns.

The crt-static flag fails silently when lost (RUST-PORT-PLAN.md 2.1), so
the rust CI asserts the built exe's import table contains no
vcruntime*/msvcp* entry, and proves the checker with a deliberately
dynamic-CRT build that must show vcruntime. Prints one "import: <DLL>"
line per imported DLL, then one verdict line.

Exit codes: 0 all assertions hold, 1 an assertion failed, 2 cannot read
or not a parseable PE.
"""
import argparse
import fnmatch
import struct
import sys
from pathlib import Path


def read_pe_imports(path):
    data = Path(path).read_bytes()
    if len(data) < 0x40 or data[:2] != b"MZ":
        raise ValueError("%s: not a PE file (no MZ header)" % path)
    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe_off : pe_off + 4] != b"PE\0\0":
        raise ValueError("%s: no PE signature at 0x%x" % (path, pe_off))
    coff = pe_off + 4
    num_sections = struct.unpack_from("<H", data, coff + 2)[0]
    opt_size = struct.unpack_from("<H", data, coff + 16)[0]
    opt = coff + 20
    magic = struct.unpack_from("<H", data, opt)[0]
    if magic == 0x10B:
        dirs = opt + 96
        num_dirs_off = opt + 92
    elif magic == 0x20B:
        dirs = opt + 112
        num_dirs_off = opt + 108
    else:
        raise ValueError("%s: unknown optional header magic 0x%x" % (path, magic))
    num_dirs = struct.unpack_from("<I", data, num_dirs_off)[0]
    if num_dirs < 2:
        return []
    import_rva, _import_size = struct.unpack_from("<II", data, dirs + 8)
    if import_rva == 0:
        return []
    sections = []
    sec = opt + opt_size
    for i in range(num_sections):
        off = sec + i * 40
        vsize, va, raw_size, raw_ptr = struct.unpack_from("<IIII", data, off + 8)
        sections.append((va, vsize, raw_ptr, raw_size))

    def rva_to_off(rva):
        for va, vsize, raw_ptr, raw_size in sections:
            if va <= rva < va + max(vsize, raw_size):
                return raw_ptr + (rva - va)
        raise ValueError("%s: rva 0x%x outside every section" % (path, rva))

    names = []
    desc = rva_to_off(import_rva)
    while desc + 20 <= len(data):
        fields = struct.unpack_from("<IIIII", data, desc)
        if fields == (0, 0, 0, 0, 0):
            break
        name_rva = fields[3]
        if name_rva == 0:
            desc += 20
            continue
        noff = rva_to_off(name_rva)
        end = data.index(b"\0", noff)
        names.append(data[noff:end].decode("ascii", "replace"))
        desc += 20
    return names


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("file")
    ap.add_argument("--forbid", action="append", default=[], metavar="PATTERN", help="import pattern that must be absent (repeatable)")
    ap.add_argument("--require", action="append", default=[], metavar="PATTERN", help="import pattern that must be present (repeatable)")
    args = ap.parse_args(argv)
    try:
        imports = read_pe_imports(args.file)
    except (OSError, ValueError, struct.error, IndexError) as e:
        print("ERROR: %s" % e)
        return 2
    for name in imports:
        print("import: %s" % name)
    lower = [n.lower() for n in imports]
    failures = []
    for pat in args.forbid:
        hits = sorted({n for n in lower if fnmatch.fnmatchcase(n, pat.lower())})
        if hits:
            failures.append("forbidden import present: %s (pattern %s)" % (", ".join(hits), pat))
    for pat in args.require:
        if not any(fnmatch.fnmatchcase(n, pat.lower()) for n in lower):
            failures.append("required import missing: pattern %s" % pat)
    for f in failures:
        print("FAIL: %s" % f)
    if failures:
        print("VERDICT FAIL: %s (%d imports)" % (args.file, len(imports)))
        return 1
    print("VERDICT PASS: %s (%d imports)" % (args.file, len(imports)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
