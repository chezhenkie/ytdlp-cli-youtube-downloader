# ytdlp cli v2.10.1

The blue notice under the menu box fits the box again: two lines of 52 and 53 columns, no wrap on any terminal width the box itself fits. Nothing else in the product changed; the bundles are rebuilt from the same payload pins as v2.10.0.

## What changed vs v2.10.0

- **The notice fits.** The first line ran to 76 columns in a 66-column frame, so a terminal wrapped it onto an extra line. It now reads `direct downloads 's' 'a' 'v' '0' take several urls` with `from the clipboard, separated by commas or zeroes.` on the second line. The crate test and the packaged check assert that both lines stay inside the box width.
- **No other product change.** Payload pins, arguments, settings, menu rows and the update path are the ones v2.10.0 shipped.

## What is in this release

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.10.1-windows.zip` | 42.2 MB | Windows x86_64: ytdlp.exe, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static x64 build) and the srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.10.1-windows-setup.exe` | 34.4 MB | Windows installer carrying the same x86_64 tree: per-user install, no admin prompt, `ytdlp` on PATH, with an uninstaller. |
| `ytdlp-cli-v2.10.1-windows-arm64.zip` | 45.4 MB | Windows on arm64: the arm64 ytdlp.exe, yt-dlp onedir tree and srtstripper plus the x64 ffmpeg under Windows 11 emulation. Unpack and run. |
| `ytdlp-cli-v2.10.1-linux.tar.gz` | 62.1 MB | Linux x86_64: ytdlp, the yt-dlp onedir tree, ffmpeg (the same pinned build, static x64) and the srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.10.1-linux-arm64.tar.gz` | 60.5 MB | Linux arm64: the arm64 ytdlp, the arm64 yt-dlp onedir tree, the arm64 ffmpeg and the arm64 srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.10.1-linux-musl.tar.gz` | 2.2 MB | Alpine and other musl systems on x86_64: the cli and the static musl srtstripper. Get the download runtime with `apk add yt-dlp`; that package pulls ffmpeg and the JS runtime with it. |
| `ytdlp-cli-v2.10.1-linux-arm64-musl.tar.gz` | 2.0 MB | Alpine and other musl systems on arm64: the arm64 cli and the static musl arm64 srtstripper. Get the download runtime with `apk add yt-dlp`. |
| `ytdlp-cli-v2.10.1-macos-arm64.pkg` | 70.3 MB | macOS installer for Apple silicon: ytdlp, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static arm64 build) and the srtstripper. Nothing else to install. |

The six products with third-party binaries carry GPL-2.0.txt and GPL-3.0.txt inside. THIRD-PARTY-NOTICES.md in every artifact names the exact asset and digest of every bundled binary. The two musl bundles ship no third-party binaries.

## Checksums

```
eaa292322b15de0f73a5b89f97b8f87694bd19a14be720462b65a24648652c49  ytdlp-cli-v2.10.1-linux-arm64-musl.tar.gz
4513fa8b938ce5e4474cb877879d10f7a84703380422f5682e86d0aeb49ce974  ytdlp-cli-v2.10.1-linux-arm64.tar.gz
7d7a3e81e8f7c39e728c368cdfc091d4c30daec189281e0d08c0402fd16a5232  ytdlp-cli-v2.10.1-linux-musl.tar.gz
6610bac2531be4d3c39461addb1c09f4bd9151217e08c8547b7f97eb5bc5026f  ytdlp-cli-v2.10.1-linux.tar.gz
1efe90923e25be29c0470015d9e4f84154a99bba1749a480262ab671b1c19de0  ytdlp-cli-v2.10.1-macos-arm64.pkg
5f2fc30814cb2718015c5df7166c0adce66bb63abc858d545206a39a7439a832  ytdlp-cli-v2.10.1-windows-arm64.zip
1c6047c17b1c7280ab3a411cef20e9c6ab7086b3aec48857966f4fffe25e9e08  ytdlp-cli-v2.10.1-windows-setup.exe
38ca521a5486697ab0dcce513c396a1a90111ed83e0e24da7a72d99da7f1ed3d  ytdlp-cli-v2.10.1-windows.zip
```

No release publishes a bare cli image: the updater lifts the cli out of the platform bundle instead.

## Verified

Built by bundles.yml run 44156328480 on b6ad47c; all seven jobs are green: windows x64 bundle plus setup, windows arm64, linux x64, linux arm64, linux musl, linux arm64 musl and the macOS pkg. Each job ran the packaged selftest 186/0 on its own native runner and asserted its payload digests, tree counts and installer steps. rust.yml run 44156313295 on the same commit is green on windows, ubuntu and macos, including fmt, clippy with warnings denied, the unit tests and the dependency audit.

Local proof from the CI bytes: the windows zip expands with 139 `_internal` files, answers selftest 186/0, `--check` resolves the bundled yt-dlp and the `bin/` payload folder, and the PE fields read FileVersion 2.10.1.0, ProductName `yt-dlp cli`. The linux tar extracts with mode 0755, carries 174 `_internal` entries, answers selftest 186/0, resolves `bin/` for yt-dlp and ffmpeg, and its payloads hash to the pins (ffmpeg 9eac5b2b..., srtstripper 85196f96...). The musl tar extracts to a static-pie binary with `bin/` carrying only srtstripper (0a1a7ae0...), no bundled yt-dlp, and answers selftest 186/0. The arm64 binaries are proven on their own CI runners; they cannot run on an x86_64 host.

The suite at this commit is 224 lib tests (222 passing, 2 ignored) and three integration suites, all green locally.

## Requirements

None beyond the bundle: no Python, no runtime, no redistributable. The Windows binary is statically linked. The musl bundles take their yt-dlp and ffmpeg from the system packages. macOS 15 or newer on Apple silicon, because the bundled ffmpeg sets that floor.

## License

The menu, the launchers and the defaults are MIT. yt-dlp is Unlicense, ffmpeg in the glibc bundles and the installers is GPL-2.0-or-later, and every artifact ships the full texts and THIRD-PARTY-NOTICES.md.
