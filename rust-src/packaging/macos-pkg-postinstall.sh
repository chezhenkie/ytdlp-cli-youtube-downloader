#!/bin/sh
# pkg postinstall for ytdlp-cli: make the installed tree usable by the
# person at the Mac. ytdlp keeps its settings and its updated yt-dlp
# inside its own folder (the app-data-in-tree layout), so a root-owned
# tree would break settings saves and yt-dlp -U. Ownership: the console
# user owns the tree; admin group can replace files as a fallback.
set -eu
tree=/usr/local/ytdlp-cli
chgrp -R admin "$tree" 2>/dev/null || true
chmod -R g+w "$tree" 2>/dev/null || true
owner=$(stat -f %Su /dev/console 2>/dev/null || true)
case "$owner" in
  ""|root|loginwindow|_mbsetupuser) ;;
  *) chown -R "$owner" "$tree" 2>/dev/null || true ;;
esac
exit 0
