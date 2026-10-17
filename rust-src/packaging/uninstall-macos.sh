#!/bin/sh
# ytdlp-cli uninstaller for the macOS pkg install.
# Removes the application tree, the PATH symlink and the package receipt.
set -eu
ident=com.github.chezhenkie.ytdlp-cli
if [ "$(id -u)" -ne 0 ]; then
  echo "run with administrative rights: sudo /usr/local/ytdlp-cli/uninstall-macos.sh" >&2
  exit 1
fi
rm -f /usr/local/bin/ytdlp
rm -rf /usr/local/ytdlp-cli
pkgutil --forget "$ident" >/dev/null 2>&1 || true
echo "ytdlp-cli removed"
