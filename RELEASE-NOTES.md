# ytdlp cli v2.11.0

The documentation inside every bundle now belongs to the release it ships in: the README names the files by version pattern and sends sizes and digests to the release notes, and the two images it links travel in `resources/`. No program behaviour changed against v2.10.1.

## What changed vs v2.10.1

- **The bundled README is release-agnostic.** Its download table names `ytdlp-cli-<ver>-windows.zip` and its siblings instead of one release's file names and sizes, and points at the release's RELEASE-NOTES.md for sizes and sha256 digests. The same text is now correct in every bundle; before, each release shipped the previous release's table.
- **The images ship with the README.** `resources/` with the menu capture and the process-flow image is part of every bundle and the macOS pkg, so both links resolve when the README is read from the installed folder instead of the repository.
- **No program change.** Arguments, settings, menu rows, payloads and the update path are the ones v2.10.1 shipped.

## What is in this release

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.11.0-windows.zip` | 42.3 MB | Windows x86_64: ytdlp.exe, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static x64 build), the srtstripper and the README images. Nothing else to install. |
| `ytdlp-cli-v2.11.0-windows-setup.exe` | 34.6 MB | Windows installer carrying the same x86_64 tree: per-user install, no admin prompt, `ytdlp` on PATH, with an uninstaller. |
| `ytdlp-cli-v2.11.0-windows-arm64.zip` | 45.5 MB | Windows on arm64: the arm64 ytdlp.exe, yt-dlp onedir tree and srtstripper plus the x64 ffmpeg under Windows 11 emulation. Unpack and run. |
| `ytdlp-cli-v2.11.0-linux.tar.gz` | 62.3 MB | Linux x86_64: ytdlp, the yt-dlp onedir tree, ffmpeg (the same pinned build, static x64), the srtstripper and the README images. Nothing else to install. |
| `ytdlp-cli-v2.11.0-linux-arm64.tar.gz` | 60.7 MB | Linux arm64: the arm64 ytdlp, the arm64 yt-dlp onedir tree, the arm64 ffmpeg, the arm64 srtstripper and the README images. Nothing else to install. |
| `ytdlp-cli-v2.11.0-linux-musl.tar.gz` | 2.3 MB | Alpine and other musl systems on x86_64: the cli, the static musl srtstripper and the README images. Get the download runtime with `apk add yt-dlp`; that package pulls ffmpeg and the JS runtime with it. |
| `ytdlp-cli-v2.11.0-linux-arm64-musl.tar.gz` | 2.2 MB | Alpine and other musl systems on arm64: the arm64 cli, the static musl arm64 srtstripper and the README images. Get the download runtime with `apk add yt-dlp`. |
| `ytdlp-cli-v2.11.0-macos-arm64.pkg` | 70.5 MB | macOS installer for Apple silicon: ytdlp, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static arm64 build), the srtstripper and the README images. Nothing else to install. |

The six products with third-party binaries carry GPL-2.0.txt and GPL-3.0.txt inside. THIRD-PARTY-NOTICES.md in every artifact names the exact asset and digest of every bundled binary. The two musl bundles ship no third-party binaries.

## Checksums

```
0fdb1439421c781451705f92a8ed10a4aa24cc1a6b01320798dc9ff139561837  ytdlp-cli-v2.11.0-linux-arm64-musl.tar.gz
84d692d0209435bef37b6d8fa1b19b6b858ded9cdb10acf3471cd22b67521527  ytdlp-cli-v2.11.0-linux-arm64.tar.gz
46885f0d39c4ee1550fc8b39a49a971db362e281a3bee5033df44d9d5277d812  ytdlp-cli-v2.11.0-linux-musl.tar.gz
03d2fd87c3ee7209564ed2dcf3cb7edfd1a3700cc08539bbea57283b9096947b  ytdlp-cli-v2.11.0-linux.tar.gz
3e09eaa8c839b958a5703854e115273a729749de02dc9e77a0472d0503be562a  ytdlp-cli-v2.11.0-macos-arm64.pkg
735c957352551fec5dcf62cbfda13ed61896b2437ae7c014bbe70359fc928b0a  ytdlp-cli-v2.11.0-windows-arm64.zip
8285b75bceb51ff4ba2b775857024b716909459188cfa0875c38cdf4e1bf905c  ytdlp-cli-v2.11.0-windows-setup.exe
c7d2942076ef462ea4f95ce4c41447eac70b3e60ec874131d764495099a22b96  ytdlp-cli-v2.11.0-windows.zip
```

No release publishes a bare cli image: the updater lifts the cli out of the platform bundle instead.

## Verified

Built by bundles.yml run 44166617703 on d79b35e; all seven jobs are green: windows x64 bundle plus setup, windows arm64, linux x64, linux arm64, linux musl, linux arm64 musl and the macOS pkg. Each job ran the packaged selftest 186/0 on its own native runner and asserted its payload digests, tree counts and installer steps. rust.yml run 44165943207 on the same commit is green on windows, ubuntu and macos, including fmt, clippy with warnings denied, the unit tests and the dependency audit.

Local proof from the CI bytes: the windows zip expands with 139 `_internal` files, `resources/` carrying both images, answers selftest 186/0, `--check` resolves the bundled yt-dlp, and the PE fields read FileVersion 2.11.0.0, ProductName `yt-dlp cli`. Its README.md and `resources/screenshot ytdlp-cli.png` hash to the repository files (49187a4b..., b3b76452...). The linux tar extracts with mode 0755, carries 174 `_internal` entries and `resources/`, answers selftest 186/0, and its payloads hash to the pins (ffmpeg 9eac5b2b..., srtstripper 85196f96...). The musl tar is static-pie, carries `bin/srtstripper` only plus `resources/`, and answers selftest 186/0. The arm64 binaries are proven on their own CI runners; they cannot run on an x86_64 host.

The suite at this commit is 224 lib tests (222 passing, 2 ignored) and three integration suites, all green locally.

## Requirements

None beyond the bundle: no Python, no runtime, no redistributable. The Windows binary is statically linked. The musl bundles take their yt-dlp and ffmpeg from the system packages. macOS 15 or newer on Apple silicon, because the bundled ffmpeg sets that floor.

## License

The menu, the launchers and the defaults are MIT. yt-dlp is Unlicense, ffmpeg in the glibc bundles and the installers is GPL-2.0-or-later, and every artifact ships the full texts and THIRD-PARTY-NOTICES.md.
