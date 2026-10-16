#!/bin/sh
# thin native launcher (IF-1): resolves its own directory and execs the binary
# beside it. $0-based and posix-only, so bash, dash and sh all work.
set -u
DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$DIR/ytdlp" "$@"
