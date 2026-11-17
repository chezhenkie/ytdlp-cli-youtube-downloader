# ytdlp cli v2.10.0

One mode per list: menu item 1 asks for the mode first, then takes a pasted url list, and the four direct keys run a whole clipboard list in their mode. The box carries a blue notice naming those keys, and the release adds arm64 bundles for Windows, Linux and Linux musl. Every artifact is byte-verified by its own checksum.

## What changed vs v2.9.3

- **Item 1 asks the mode first.** Enter or `0` runs the list as video with sound, `a` audio only, `v` video only, `s` the stripped summary. An unknown answer repeats the question; escape backs out. The list is then collected under that one mode and every entry runs in order, the marked error row and the escape stop included. The mode is a per-run value, so nothing is written to `settings.json`.
- **Links are lists.** A pasted line and the clipboard may carry several links: commas, standalone `0`s and line breaks separate them, while a `0` inside a link stays part of it. The four direct keys (`0`, `a`, `v`, `s`) download every link on the clipboard in paste order, print `url i of n` when there is more than one, and escape stops the rest. Settings are untouched by both the mode and the list.
- **The box carries a blue notice.** Two lines under the box rule name the four direct keys in their own row colours and state the list rule.
- **arm64 bundles.** The release adds a Windows arm64 zip, a Linux arm64 tar and a Linux arm64 musl tar. The Windows arm64 zip carries the native arm64 cli, yt-dlp tree and srtstripper plus the x64 ffmpeg, which Windows 11 runs under emulation because the pinned provider publishes no Windows arm64 build. The Linux arm64 bundle carries native arm64 builds of all three payloads; the arm64 musl bundle is the cli plus the musl srtstripper, with yt-dlp and ffmpeg from `apk`. `install.sh` and the cli self-update pick the arm64 asset on an aarch64 host.
- **Updating.** From v2.9.3 press item 12 or run `ytdlp --update`. The asset names, the digest gate and the quiet selftest gate are unchanged; `windows-arm64.zip` installs by unpacking, since the setup.exe line stays x86_64.

## What is in this release

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.10.0-windows.zip` | 42.2 MB | Windows x86_64: ytdlp.exe, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static x64 build) and the srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.10.0-windows-setup.exe` | 34.4 MB | Windows installer carrying the same x86_64 tree: per-user install, no admin prompt, `ytdlp` on PATH, with an uninstaller. |
| `ytdlp-cli-v2.10.0-windows-arm64.zip` | 45.4 MB | Windows on arm64: the arm64 ytdlp.exe, yt-dlp onedir tree and srtstripper plus the x64 ffmpeg under Windows 11 emulation. Unpack and run. |
| `ytdlp-cli-v2.10.0-linux.tar.gz` | 62.1 MB | Linux x86_64: ytdlp, the yt-dlp onedir tree, ffmpeg (the same pinned build, static x64) and the srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.10.0-linux-arm64.tar.gz` | 60.5 MB | Linux arm64: the arm64 ytdlp, the arm64 yt-dlp onedir tree, the arm64 ffmpeg and the arm64 srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.10.0-linux-musl.tar.gz` | 2.1 MB | Alpine and other musl systems on x86_64: the cli and the static musl srtstripper. Get the download runtime with `apk add yt-dlp`; that package pulls ffmpeg and the JS runtime with it. |
| `ytdlp-cli-v2.10.0-linux-arm64-musl.tar.gz` | 2.0 MB | Alpine and other musl systems on arm64: the arm64 cli and the static musl arm64 srtstripper. Get the download runtime with `apk add yt-dlp`. |
| `ytdlp-cli-v2.10.0-macos-arm64.pkg` | 70.3 MB | macOS installer for Apple silicon: ytdlp, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static arm64 build) and the srtstripper. Nothing else to install. |

The six products with third-party binaries carry GPL-2.0.txt and GPL-3.0.txt inside. THIRD-PARTY-NOTICES.md in every artifact names the exact asset and digest of every bundled binary. The two musl bundles ship no third-party binaries.

## Checksums

```
e9496b110280155a393ca79fbb68be0cb8456ca74bfc314f62ad3fa0cc89d793  ytdlp-cli-v2.10.0-linux-arm64-musl.tar.gz
99ce729be2b82c6887f24792591aa21280f2c2eb1ec43294cc33b3e71015f68c  ytdlp-cli-v2.10.0-linux-arm64.tar.gz
468807cfa66f39b8d66e6514db9fca3c676eabbefbea78425329b9f8beaa9da0  ytdlp-cli-v2.10.0-linux-musl.tar.gz
b2bcda057beaf6a67d7a634c54c66f9821490fa23358f6b50f78af0678a41b43  ytdlp-cli-v2.10.0-linux.tar.gz
41b5da0445881f8d93a442ea1584161635197055f6bb0177b4c92c4a5bb4afcc  ytdlp-cli-v2.10.0-macos-arm64.pkg
c75707413ad55e335e1c6d73602dbb2845f9d624dc064324a2f4d40fcbf059e7  ytdlp-cli-v2.10.0-windows-arm64.zip
6d74ede24c3e58b752e00b4267d98e09098e4c85b28f13030594bea3addf6c18  ytdlp-cli-v2.10.0-windows-setup.exe
a97155edd57715b2dbac58410c438a5f8a8e1ad61aa7954b79ccbdb549652fb0  ytdlp-cli-v2.10.0-windows.zip
```

No release publishes a bare cli image: the updater lifts the cli out of the platform bundle instead.

## Verified

Built by bundles.yml run 44124130833 on cce4b14; all seven jobs are green: windows x64 bundle plus setup, windows arm64, linux x64, linux arm64, linux musl, linux arm64 musl and the macOS pkg. Each job ran the packaged selftest 186/0 on its own native runner and asserted its payload digests, tree counts and installer steps. rust.yml run 44124111081 on the same commit is green on windows, ubuntu and macos, including fmt, clippy with warnings denied, the unit tests and the dependency audit.

Local proof from the CI bytes: the windows zip expands with 139 `_internal` files, answers selftest 186/0, `--check` resolves the bundled yt-dlp and the `bin/` payload folder, the bundled yt-dlp answers 2026.08.19 and ffmpeg n8.1.2, and the PE fields read FileVersion 2.10.0.0, ProductName `yt-dlp cli`. The windows arm64 zip stages the plain `ytdlp-cli-v2.10.0` folder and carries the arm64 launcher, arm64 yt-dlp and arm64 srtstripper with the x64 ffmpeg. The linux tar extracts with mode 0755, carries 174 `_internal` entries, answers selftest 186/0, resolves `bin/` for yt-dlp and ffmpeg, and its payloads hash to the pins (ffmpeg 9eac5b2b..., srtstripper 85196f96...). The musl tar extracts to a static-pie binary with `bin/` carrying only srtstripper (0a1a7ae0...), no bundled yt-dlp, and answers selftest 186/0. The arm64 binaries are proven on their own CI runners; they cannot run on an x86_64 host.

The suite at this commit is 224 lib tests (222 passing, 2 ignored) and three integration suites, all green locally.

## Requirements

None beyond the bundle: no Python, no runtime, no redistributable. The Windows binary is statically linked. The musl bundles take their yt-dlp and ffmpeg from the system packages. macOS 15 or newer on Apple silicon, because the bundled ffmpeg sets that floor.

## License

The menu, the launchers and the defaults are MIT. yt-dlp is Unlicense, ffmpeg in the glibc bundles and the installers is GPL-2.0-or-later, and every artifact ships the full texts and THIRD-PARTY-NOTICES.md.
