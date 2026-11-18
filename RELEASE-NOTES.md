# ytdlp cli v2.11.1

A fix release. The Windows arm64 zip now starts on a machine with nothing installed, both arm64 bundles can update their bundled yt-dlp again, and a url list counts only the subtitle files its own link wrote.

## What changed vs v2.11.0

- **The Windows arm64 zip runs without the Visual C++ redistributable.** The arm64 cli was linked against the dynamic CRT and imported `VCRUNTIME140.dll` plus the api-ms-win-crt set, so a clean Windows 11 arm64 machine would refuse to start it. Both Windows targets now take the static CRT flag, and the arm64 bundle proof reads the import table and fails on a vcruntime or msvcp entry.
- **Both arm64 bundles update the bundled yt-dlp.** Item 12 and `--update` answered "no published yt-dlp tree for windows/aarch64" or "for linux/aarch64", because the tree updater knew only the x64 assets. It now names `yt-dlp_win_arm64.zip` with its `yt-dlp_arm64.exe` member, and `yt-dlp_linux_aarch64.zip` with `yt-dlp_linux_aarch64`.
- **A url list counts only its own subtitles.** A later link could start inside the previous item's two-second mtime window; the pass then stripped the earlier subtitle file and reported the text as ready without fetching its own. The pass now compares the output folder against a snapshot taken before it runs.
- **`-a` without a url no longer lands in settings.json.** The flag put the transient audio mode on the object the menu saves, so the file gained a key it does not own and the `0` key downloaded audio only for that session. Command-line overrides now apply only to a run that takes a url.
- **Ctrl+C ends a list.** A cancelled item no longer starts the next link in a clipboard or batch list, and an interrupted batch url prompt ends the menu like every other prompt.
- **Documentation and gates.** The Pages site serves the 17-node process flow instead of the earlier 16-node render; every bundle proof asserts both `resources/` images; the release digest test hashes the local artifacts by name at any depth, and the README guard rejects any release pin. The specsheet loses the stale `type` settings row, and its crate versions, notice width and live stamp are current.

The menu surface, the settings schema (18 keys) and the argument set are unchanged; only the override placement above changed behaviour.

## What is in this release

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.11.1-windows.zip` | 42.3 MB | Windows x86_64: ytdlp.exe, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static x64 build), the srtstripper and the README images. Nothing else to install. |
| `ytdlp-cli-v2.11.1-windows-setup.exe` | 34.5 MB | Windows installer carrying the same x86_64 tree: per-user install, no admin prompt, `ytdlp` on PATH, with an uninstaller. |
| `ytdlp-cli-v2.11.1-windows-arm64.zip` | 45.5 MB | Windows on arm64: the arm64 ytdlp.exe, the arm64 yt-dlp onedir tree, the arm64 srtstripper and the x64 ffmpeg under Windows 11 emulation. Unpack and run; both exe files are statically linked. |
| `ytdlp-cli-v2.11.1-linux.tar.gz` | 62.2 MB | Linux x86_64: ytdlp, the yt-dlp onedir tree, ffmpeg (the same pinned build, static x64), the srtstripper and the README images. Nothing else to install. |
| `ytdlp-cli-v2.11.1-linux-arm64.tar.gz` | 60.6 MB | Linux arm64: the arm64 ytdlp, the arm64 yt-dlp onedir tree, the arm64 ffmpeg, the arm64 srtstripper and the README images. Nothing else to install. |
| `ytdlp-cli-v2.11.1-linux-musl.tar.gz` | 2.2 MB | Alpine and other musl systems on x86_64: the cli, the static musl srtstripper and the README images. Get the download runtime with `apk add yt-dlp`; that package pulls ffmpeg and the JS runtime with it. |
| `ytdlp-cli-v2.11.1-linux-arm64-musl.tar.gz` | 2.1 MB | Alpine and other musl systems on arm64: the arm64 cli, the static musl arm64 srtstripper and the README images. Get the download runtime with `apk add yt-dlp`. |
| `ytdlp-cli-v2.11.1-macos-arm64.pkg` | 70.4 MB | macOS installer for Apple silicon: ytdlp, the yt-dlp onedir tree, ffmpeg (shaka-project 8.1.2 static arm64 build), the srtstripper and the README images. Nothing else to install. |

The six products with third-party binaries carry GPL-2.0.txt and GPL-3.0.txt inside. THIRD-PARTY-NOTICES.md in every artifact names the exact asset and digest of every bundled binary. The two musl bundles ship no third-party binaries.

## Checksums

```
3093285d673dcb203e9a7b874acb5b67d57cf53e46e8eaa7c9eeedd72e58d009  ytdlp-cli-v2.11.1-linux-arm64-musl.tar.gz
fb3a2000a8bbff0106d01af8323b7e29e4ee877a2d8b76c256fc8ab8902744cd  ytdlp-cli-v2.11.1-linux-arm64.tar.gz
4fbe5a6830a904d05bda1c2c20eb4ba2dac31bb9259f9af7bd3d7203b1571d44  ytdlp-cli-v2.11.1-linux-musl.tar.gz
25120de9079fd43463ad381310b598432c85b1ea110f16f7525ab691c99ac33f  ytdlp-cli-v2.11.1-linux.tar.gz
b6dc1bd902dbfc5714d82560377ddfa80b0b602edb174d12d0e71847a211b02b  ytdlp-cli-v2.11.1-macos-arm64.pkg
b562f86291636dfbf24802006f2df1558085928f8919ab7abc63bde9a428f2d1  ytdlp-cli-v2.11.1-windows-arm64.zip
1802541b6a680ff30337db70f04665a568a7dc03eaf62df8945fe7d1d41cbd6b  ytdlp-cli-v2.11.1-windows-setup.exe
7a384f8dce66828ab29c17dc24006a9cf003987390045d86c7fe82a3073f0294  ytdlp-cli-v2.11.1-windows.zip
```

No release publishes a bare cli image: the updater lifts the cli out of the platform bundle instead.

## Verified

Built by bundles.yml run 44210163886 on 75f1de8; all seven jobs are green: windows x64 bundle plus setup, windows arm64, linux x64, linux arm64, linux musl, linux arm64 musl and the macOS pkg. Each job ran the packaged selftest 186/0 on its own native runner, asserted its payload digests, tree counts and `resources/` images, and the windows arm64 job asserted the extracted exe imports no vcruntime or msvcp entry. rust.yml run 44210155597 on the same commit is green on windows, ubuntu and macos, including fmt, clippy with warnings denied, the unit tests, the PE import assertions and the dependency audit.

Local proof from the CI bytes: the windows zip expands with 139 `_internal` files and both `resources/` images, answers selftest 186/0, and reads FileVersion 2.11.1.0; the windows arm64 zip carries 135 `_internal` files and both images, reads FileVersion 2.11.1.0, and its import table lists only system DLLs. The linux tar carries 147 `_internal` files and both images, answers selftest 186/0, and its payloads hash to the pins (ffmpeg 9eac5b2b..., srtstripper 85196f96...). The linux arm64 tar carries 145 `_internal` files and both images, with ffmpeg 6e7b1d7d... and srtstripper 25b2ca6e...; its aarch64 binaries run on the arm runner only. The musl tar is static-pie, carries `bin/srtstripper` (0a1a7ae0...) and both images, and answers selftest 186/0; the arm64 musl tar is a static aarch64 binary with srtstripper 9218bb78..., proven by its CI job. The macOS pkg installed, answered selftest 186/0, passed the payload digest and arm64-slice checks and uninstalled through the shipped script on the macOS runner.

The bundles carry the README and the menu capture of the build commit: README.md 01a25d50... and `resources/screenshot ytdlp-cli.png` 187fcf27... equal the repository files.

The suite at this commit is 225 lib tests (223 passing, 2 ignored) and three integration suites, all green locally.

## Requirements

None beyond the bundle: no Python, no runtime, no redistributable. Both Windows binaries are statically linked. The musl bundles take their yt-dlp and ffmpeg from the system packages. macOS 15 or newer on Apple silicon, because the bundled ffmpeg sets that floor.

## License

The menu, the launchers and the defaults are MIT. yt-dlp is Unlicense, and ffmpeg in the glibc bundles and the installers is GPL-2.0-or-later. The six artifacts that carry ffmpeg ship GPL-2.0.txt and GPL-3.0.txt; every artifact ships THIRD-PARTY-NOTICES.md.
