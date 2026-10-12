#!/usr/bin/env bash
set -u
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PY="$(command -v python3 || command -v python)"
"$PY" "$DIR/src/ytdlp.py" "$@"
