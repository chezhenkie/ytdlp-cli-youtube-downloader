#!/usr/bin/env bash
# Runs a packaged srtstripper over one fixture subtitle and asserts the
# tool's own report: the binary starts, prints a PASS line, and writes the
# stripped text next to the subtitle. Every bundle job runs this, so a
# pinned build that cannot start on its platform fails the job.
set -euo pipefail
bin=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
printf '1\n00:00:01,000 --> 00:00:03,000\nHello world.\n\n2\n00:00:04,000 --> 00:00:06,000\nSecond line.\n' > sample.en.srt
out=$("$bin" sample.en.srt)
echo "$out"
case $out in
    PASS:*) ;;
    *) echo "srtstripper printed no PASS line" >&2; exit 1 ;;
esac
txt=$(printf '%s\n' "$out" | sed -n 's/.* -> //p' | tail -n 1)
test -n "$txt" || { echo "srtstripper PASS line carries no output path" >&2; exit 1; }
test -f "$txt" || { echo "srtstripper output missing: $txt" >&2; exit 1; }
grep -qx 'Hello world.' "$txt"
grep -qx 'Second line.' "$txt"
if grep -q -- '-->' "$txt"; then
    echo "timecode survived the strip" >&2
    exit 1
fi
echo "srtstripper smoke passed: $(basename "$txt")"
