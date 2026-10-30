#!/bin/sh
# ytdlp-cli installer for Linux x86_64 - no system package, no root.
#
#   curl -fsSL https://raw.githubusercontent.com/chezhenkie/ytdlp-cli-youtube-downloader/main/install.sh | sh
#
# Options (when piped, pass them after sh -s --):
#   --version V   install release V (like v2.3.0) instead of the latest
#   --uninstall   remove the tree and the ytdlp link
#
# On musl systems (Alpine) the musl bundle is installed automatically: the
# cli only - yt-dlp and ffmpeg come from the system (apk add yt-dlp; the
# Alpine package pulls ffmpeg and the JS runtime with it).
#
# It downloads the bundle from the GitHub release, verifies its sha256 against
# the Checksums block of that release's RELEASE-NOTES.md, unpacks it to
# ~/.local/share/ytdlp-cli and links ~/.local/bin/ytdlp. An existing
# config/settings.json survives a re-install.

set -eu

repo=chezhenkie/ytdlp-cli-youtube-downloader
github=https://github.com/$repo
version=latest
uninstall=no

usage() {
	cat <<'EOF'
usage: install.sh [--version vX.Y.Z] [--uninstall]

  --version V  install release V instead of the latest one
  --uninstall  remove ~/.local/share/ytdlp-cli and the ~/.local/bin/ytdlp link

On musl (Alpine) the cli-only musl bundle is installed and yt-dlp comes from
apk. The glibc bundle carries yt-dlp, ffmpeg and the srtstripper.
EOF
}

die() { echo "install.sh: $*" >&2; exit 1; }

while [ $# -gt 0 ]; do
	case $1 in
		--portable|--lean)
			# one bundle per platform now; the flavour flags are gone
			echo "install.sh: $1 is ignored - the bundle always includes ffmpeg" >&2
			;;
		--version)
			[ $# -ge 2 ] || die "--version needs a value like v2.3.0"
			version=$2
			shift
			;;
		--uninstall) uninstall=yes ;;
		-h|--help) usage; exit 0 ;;
		*) die "unknown option: $1 (try --help)" ;;
	esac
	shift
done

target=$HOME/.local/share/ytdlp-cli
link=$HOME/.local/bin/ytdlp

if [ "$uninstall" = yes ]; then
	rm -rf "$target"
	if [ -L "$link" ]; then
		case $(readlink "$link") in
			"$target"/*) rm -f "$link"; echo "removed $link" ;;
		esac
	fi
	echo "removed $target (saved settings included)"
	exit 0
fi

[ "$(uname -s)" = Linux ] \
	|| die "Linux only - on macOS download the .pkg from $github/releases/latest (see README)"
case $(uname -m) in
	x86_64|amd64) ;;
	*) die "no bundle for $(uname -m) - releases carry Linux x86_64 builds only" ;;
esac
suffix=linux
if { command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; } \
	|| ls /lib/ld-musl-*.so.1 >/dev/null 2>&1; then
	suffix=linux-musl
fi
command -v curl >/dev/null 2>&1 || die "curl is required"
command -v tar >/dev/null 2>&1 || die "tar is required"
command -v awk >/dev/null 2>&1 || die "awk is required"
if command -v sha256sum >/dev/null 2>&1; then
	sha_tool=sha256sum
elif command -v shasum >/dev/null 2>&1; then
	sha_tool=shasum
else
	die "sha256sum or shasum is required to verify the download"
fi

case $version in
	latest|v*) ;;
	*) version=v$version ;;
esac

if [ "$version" = latest ]; then
	base=$github/releases/latest/download
else
	base=$github/releases/download/$version
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/ytdlp-cli.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
trap 'rm -rf "$tmp"; exit 1' HUP INT TERM

echo "fetching the release notes ($version)"
curl -fsSL "$base/RELEASE-NOTES.md" -o "$tmp/notes.md" \
	|| die "cannot fetch RELEASE-NOTES.md - no network, or no such release?"

row=$(grep -E "^[0-9a-f]{64}[[:space:]]+ytdlp-cli-v[0-9.]+-$suffix\.tar\.gz$" "$tmp/notes.md") \
	|| die "no $suffix bundle in the checksums of $version"
[ "$(printf '%s\n' "$row" | awk 'END {print NR}')" = 1 ] \
	|| die "the checksum block is ambiguous - refusing to guess"

digest=$(printf '%s\n' "$row" | awk '{print $1}')
asset=$(printf '%s\n' "$row" | awk '{print $2}')

echo "downloading $asset"
curl -fL --retry 3 -o "$tmp/$asset" "$base/$asset" || die "download failed"

echo "verifying sha256"
if [ "$sha_tool" = sha256sum ]; then
	actual=$(sha256sum "$tmp/$asset" | awk '{print $1}')
else
	actual=$(shasum -a 256 "$tmp/$asset" | awk '{print $1}')
fi
[ "$actual" = "$digest" ] \
	|| die "sha256 mismatch for $asset (expected $digest, got $actual) - nothing installed"

mkdir "$tmp/x"
tar -xzf "$tmp/$asset" -C "$tmp/x" || die "cannot unpack $asset"

payload=$tmp/x
set -- "$tmp"/x/*
if [ $# -eq 1 ] && [ -d "$1" ]; then
	payload=$1
fi
[ -f "$payload/ytdlp" ] || die "the archive does not look like a ytdlp-cli bundle"

mkdir -p "$HOME/.local/share" "$HOME/.local/bin"
new=$HOME/.local/share/.ytdlp-cli.new.$$
rm -rf "$new"
mv "$payload" "$new"
if [ -d "$target/config" ]; then
	mkdir -p "$new/config"
	cp -Rp "$target/config/." "$new/config/"
fi
rm -rf "$target"
mv "$new" "$target"

ln -sf "$target/ytdlp" "$link"

echo "installed $asset"
echo "  tree: $target"
echo "  link: $link"
if [ "$suffix" = linux-musl ] && ! command -v yt-dlp >/dev/null 2>&1; then
	echo "note: yt-dlp is not on PATH yet; on Alpine install it with:"
	echo "  apk add yt-dlp"
	echo "  (the Alpine package pulls ffmpeg and the JS runtime with it)"
fi
case :$PATH: in
	*:$HOME/.local/bin:*) ;;
	*) echo "note: $HOME/.local/bin is not on PATH yet; add this to your shell rc:"
	   echo '  export PATH="$HOME/.local/bin:$PATH"' ;;
esac
echo "run: ytdlp"
