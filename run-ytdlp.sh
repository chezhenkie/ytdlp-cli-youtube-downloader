#!/usr/bin/env bash
set -u
# navy background (matches the bat's color 1F) for xterm-family terminals;
# a terminal that ignores OSC 11 simply keeps its own background
printf '\033]11;#141f3d\007'
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PY="$(command -v python3 || command -v python)"
"$PY" "$DIR/src/ytdlp.py" "$@"
