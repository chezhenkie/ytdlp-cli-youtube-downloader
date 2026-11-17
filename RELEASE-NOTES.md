# ytdlp cli v2.9.3 (native)

One mode per key: `0` downloads video with sound, `a` audio only, `v` video only, `s` the stripped summary. The media type leaves `settings.json`, the menu drops to 14 rows, and the icon is new. Every artifact is byte-verified by its own checksum.

## What changed vs v2.9.2

- **The mode is picked per run.** `0` and a pasted url download video with sound. `a` downloads the best audio track, converted to the configured audio format when ffmpeg is present. `v` downloads the best video track without sound. `s` runs the summary grab: subtitles stripped to plain text first, sound only when no text can be produced. All four read the clipboard link; a clipboard with no link prints one amber line and holds.
- **Settings no longer store a type.** `type` is gone from `settings.json`; a value left by an older build is ignored on load and dropped on the next save. The one-shot flag `-a` still selects audio only for that run and is never written.
- **The menu is 14 rows.** Row 1 opens the multiple-url list in plain white, the `video/audio` row is gone, and reset, update, auto update and quit are 11 to 14. Above row 1 sit four key rows: `s` summary in the dark amber, `a` audio only and `v` video only in the brighter amber, `0` quick paste.
- **New icon.** Six horizontal bands of sky, dark blue at the top to light blue at the bottom, behind the orange download mark.
- **Updating.** From v2.9.2 press item 13 or run `ytdlp --update`. The new build shows the updater at item 12; the asset names and the digest gate are unchanged.

## What is in this release

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.9.3-windows.zip` | 42.1 MB | ytdlp.exe, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static x64 build) and the srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.9.3-windows-setup.exe` | 34.4 MB | Windows installer carrying the same tree: per-user install, no admin prompt, `ytdlp` on PATH, with an uninstaller. |
| `ytdlp-cli-v2.9.3-linux.tar.gz` | 62.1 MB | ytdlp, the yt-dlp onedir tree, ffmpeg (the same pinned build, static x64) and the srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.9.3-linux-musl.tar.gz` | 2.1 MB | Alpine and other musl systems: the cli and the static musl srtstripper. Get the download runtime with `apk add yt-dlp`; that package pulls ffmpeg and the JS runtime with it. |
| `ytdlp-cli-v2.9.3-macos-arm64.pkg` | 70.3 MB | macOS installer for Apple silicon: ytdlp, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static arm64 build) and the srtstripper. Nothing else to install. |

The four products with third-party binaries carry GPL-2.0.txt and GPL-3.0.txt inside. THIRD-PARTY-NOTICES.md in every artifact names the exact asset and digest of every bundled binary.

## Checksums

```
6d9ecd5f975c3ee80b7d2afa2d621b8c6ac150ff6a30c8011081a07c77062ef9  ytdlp-cli-v2.9.3-linux.tar.gz
6156730958c6f0c6ed0209faf7c05ed1254338c002aa29c5e7a97a2cf52dbde6  ytdlp-cli-v2.9.3-linux-musl.tar.gz
0fb86aaa8818a5ba4de39ac0f495c3232b069a03702f3b770145bdc6dea7c6c8  ytdlp-cli-v2.9.3-macos-arm64.pkg
9a2855c111541e79a4ae9a4eba588d22c18961f36a911e09b0e5f794ce27e86d  ytdlp-cli-v2.9.3-windows.zip
22517634ff565da928a05ce23f99ba6a2832f36a4a87bc1c8c05a2c80f9f1e64  ytdlp-cli-v2.9.3-windows-setup.exe
```

No release publishes a bare cli image: the updater lifts the cli out of the platform bundle instead.

## Verified

Built by bundles.yml run 44029934967 on b3ce8e3, all four jobs green. The payload pins are the v2.9.0 set (yt-dlp 2026.08.19 onedir zips, shaka-project n8.1.2-1 ffmpeg for windows, linux and macos, srtstripper v1.6.4, GPL texts from rust-src/packaging). The run proved the packaged bytes: the windows zip carries 139 `_internal` files and runs the bundled yt-dlp 2026.08.19 and ffmpeg n8.1.2; the installer installs twice with one PATH entry after each install and none after uninstall, answers selftest 184/0 and passes the PE fields; the linux tar carries 174 `_internal` entries and answers selftest 184/0 with the payloads running; the musl binary is static-pie, carries no yt-dlp and answers selftest 184/0; the pkg installs, answers selftest 184/0, checks the payload digests and uninstalls through the shipped script.

The suite at this commit is 218 lib tests (216 passing, 2 ignored) and three integration suites, all green locally; the windows and macOS test jobs on the same commit are green as well. The ubuntu job stopped at its last step because the ubuntu-26.04 runner image no longer offers the pinned Python 3.14.4 for setup-python; fmt, clippy with warnings denied and the tests had already passed there. The workflow now asks for Python 3.14, and rust.yml run 44033121850 on the documentation commit is green on all three platforms. The update path was proven live from v2.9.2: item 13 fetched the windows bundle, the digest matched the release API, and the restarted build shows the 14-row menu with the updater at 12.

## Requirements

None beyond the bundle: no Python, no runtime, no redistributable. The Windows binary is statically linked. The musl bundle takes its yt-dlp and ffmpeg from the system packages.

## License

The menu, the launchers and the defaults are MIT. yt-dlp is Unlicense, ffmpeg in the glibc bundles and the installers is GPL-2.0-or-later, and every artifact ships the full texts and THIRD-PARTY-NOTICES.md.
