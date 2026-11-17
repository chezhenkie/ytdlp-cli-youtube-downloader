# yt-dlp cli

A menu-driven wrapper around [yt-dlp](https://github.com/yt-dlp/yt-dlp). Copy a YouTube link, start `ytdlp`, press `0`, and the download runs with the saved defaults. The menu holds the container, resolution, audio format, output folder, filename template, subtitle, playlist and update settings; every change is written to `config/settings.json` as it is made.

yt-dlp performs the download. This project adds the menu, the defaults, the progress display, the subtitle pass, the srt stripper and the updater. It runs the official yt-dlp build unmodified and is not affiliated with or endorsed by the yt-dlp project.

The cli is a native Rust binary for Windows x64 and arm64, Linux x64 and arm64 on glibc or musl, and macOS arm64. The Windows, glibc and macOS bundles carry everything: the cli, yt-dlp with its Python runtime, ffmpeg and the srt stripper.

![yt-dlp cli menu on Windows](resources/screenshot%20ytdlp-cli.png)

The four key rows sit above the numbered menu. `s` takes a stripped summary, `a` audio only, `v` video only, `0` video with sound; each one works on the links on the clipboard.

## Bundles

| Platform | File | Contents |
|---|---|---|
| Windows x64 | `ytdlp-cli-<ver>-windows.zip` | cli, yt-dlp with its runtime, ffmpeg, srt stripper |
| Windows x64 installer | `ytdlp-cli-<ver>-windows-setup.exe` | the same payload, per-user install and PATH entry |
| Windows arm64 | `ytdlp-cli-<ver>-windows-arm64.zip` | arm64 cli, yt-dlp and srt stripper, x64 ffmpeg |
| Linux x64 | `ytdlp-cli-<ver>-linux.tar.gz` | cli, yt-dlp with its runtime, ffmpeg, srt stripper |
| Linux arm64 | `ytdlp-cli-<ver>-linux-arm64.tar.gz` | the same payload, arm64 builds |
| Linux x64, musl | `ytdlp-cli-<ver>-linux-musl.tar.gz` | cli and srt stripper; yt-dlp and ffmpeg from apk |
| Linux arm64, musl | `ytdlp-cli-<ver>-linux-arm64-musl.tar.gz` | the same, arm64 builds |
| macOS arm64 | `ytdlp-cli-<ver>-macos-arm64.pkg` | cli, yt-dlp with its runtime, ffmpeg, srt stripper |

`<ver>` is the release tag. The release notes for each tag carry the sizes and sha256 digests, and `install.sh` checks the digest before it unpacks.

## Install

### Windows

Download the setup and run it. It installs for the current user without an administrator prompt, puts the program in `%LOCALAPPDATA%\Programs\ytdlp-cli`, adds that folder to PATH and creates a Start Menu entry. The file is unsigned, so SmartScreen may warn: More info, then Run anyway. Remove it from Windows Settings > Apps, or with the Start Menu uninstaller. The zip needs no install: unpack it and run `ytdlp.exe` from any folder.

On arm64, unpack the arm64 zip and run `ytdlp.exe`; there is no arm64 installer.

### macOS

Download the pkg and open it. It installs to `/usr/local/ytdlp-cli`, links `/usr/local/bin/ytdlp` and needs macOS 15 or newer on Apple silicon. The package is unsigned, so macOS blocks the first open: System Settings > Privacy & Security, Open Anyway, then confirm. Remove it with `sudo /usr/local/ytdlp-cli/uninstall-macos.sh`.

### Linux

Unpack a tar.gz and run `./ytdlp`, or use the installer:

```
curl -fsSL https://raw.githubusercontent.com/chezhenkie/ytdlp-cli-youtube-downloader/main/install.sh | sh
```

It fetches the newest release, checks the digest, unpacks to `~/.local/share/ytdlp-cli` and links `~/.local/bin/ytdlp`. Nothing runs as root. `sh -s -- --version <tag>` pins a release and `sh -s -- --uninstall` removes the tree and the link. On musl systems the script takes the musl bundle; `apk add yt-dlp` provides yt-dlp, ffmpeg and the JS runtime.

## Use

The four direct keys work on the clipboard: `0` video with sound, `s` the stripped summary, `a` audio only, `v` video only. The last three leave the stored settings untouched.

Several links can be taken at once. Commas, standalone `0`s and line breaks separate them, and a `0` inside a link stays part of the link. The keys run the list in order and print `url i of n`; escape stops the current download and the rest of the list.

Menu row 1 asks for the same four modes first and then takes one link per line, or a whole paste; an empty line starts the downloads.

For a single run without the menu:

```
ytdlp https://youtu.be/xxxx
```

One-shot overrides: `-f` container, `-r` height, `-a` audio only, `--af` audio format, `-o` output folder, `--template` filename template, `--subs` and `--playlist`. They apply to that run and are never saved. `ytdlp --check` prints the paths and versions the run would use.

Output goes to `ytdlp-cli-downloads` on the desktop when a desktop folder is known, otherwise in Downloads, otherwise beside the program. Row 5 changes the folder, and a saved folder is used as given. Escape during a download removes the partial files.

### The menu

| Row | Setting | Default |
|---|---|---|
| 1 | download a list | asks for the mode first |
| 2 | container | mp4 |
| 3 | resolution | 1080p |
| 4 | audio format | mp3, quality 0 |
| 5 | output folder | desktop, else Downloads |
| 6 | filename template | `%(title)s` |
| 7 | subtitles | on, `en` |
| 8 | srt stripper | off, text mode |
| 9 | info field | off |
| 10 | playlist | off |
| 11 | reset defaults | |
| 12 | update yt-dlp and the cli | |
| 13 | auto update | on, daily check |
| 14 | quit | |

## The folder after install

```
ytdlp.exe
config/settings.json
bin/yt-dlp.exe
bin/_internal/
bin/ffmpeg.exe
bin/srtstripper.exe
resources/
```

`ytdlp.exe` is the program, and `config/settings.json` holds the menu defaults. `bin/` holds what the program starts: `yt-dlp.exe` with its `_internal` folder, `ffmpeg.exe` and `srtstripper.exe`. `resources/` holds the images shipped with the bundle. Linux and macOS use the same names without `.exe`.

`bin/_internal` is the Python runtime yt-dlp runs on: the interpreter, its standard library, certificates and the modules it imports. yt-dlp is a Python program, so its official build ships that runtime beside the launcher; the installer or archive installs it once, and yt-dlp starts from it directly. It is private to this folder and must stay beside the launcher: nothing is installed system-wide, no background process runs, and removing the folder removes all of it.

## Flow

The interactive process flow is published at [chezhenkie.github.io/ytdlp-cli-youtube-downloader](https://chezhenkie.github.io/ytdlp-cli-youtube-downloader/). The diagram source is in the [ytdlp process flow archify](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/tree/main/ytdlp%20process%20flow%20archify) folder.

## Limits

- The Windows arm64 zip is the one mixed-architecture bundle: it carries the x64 ffmpeg, which Windows 11 runs under emulation.
- The musl bundles carry the cli and the srt stripper only. `apk add yt-dlp` provides the download runtime, and that package follows its own release schedule.
- The macOS pkg is unsigned and supports macOS 15 or newer on Apple silicon. Intel Macs are not covered.
- yt-dlp changes as YouTube changes. A format that yt-dlp cannot extract is outside this wrapper; row 12 updates the bundled yt-dlp.
- Subtitles are requested in the configured language. When YouTube serves another track instead, the pass tries the offered tracks and reports each try; a translated caption request can be refused with HTTP 429.
- Merging containers and converting subtitles to srt need ffmpeg. Every Windows, Linux glibc and macOS bundle carries it.
- The work folder is `.ytdlp-tmp` under the output folder. It is removed when the run ends, so a large download needs the disk space and a writable output folder.

## Checksums

Every release publishes the sizes and sha256 digests of its files in `RELEASE-NOTES.md`; `install.sh` and the cli updater check what they take. No release publishes a bare cli image, so the updater lifts the cli out of the platform bundle.

The bundled yt-dlp is the official onedir build for the platform. The bundled ffmpeg is the pinned shaka-project/static-ffmpeg-binaries build for the platform; `THIRD-PARTY-NOTICES.md` in each bundle names the exact asset and digest. The musl bundles ship neither.

## Files

| File | Purpose |
|---|---|
| `README.md` | This page |
| `install.sh` | Linux installer, served from the main branch |
| `resources/` | Menu capture and process-flow image |
| `specsheet/` | Binding specification, starting at `00-overview.md` |
| `rust-src/` | Rust workspace, the cli and its tests |
| `ytdlp process flow archify/` | Diagram JSON and HTML |
| `docs/` | GitHub Pages entry and the published flow |
| `CHANGELOG.md` | Version history |
| `RELEASE-NOTES.md` | Sizes and digests of the current release |

## Build

From `rust-src/`:

```
cargo build --release -p ytdlp-cli
```

The binary lands at `target/release/ytdlp` (`ytdlp.exe` on Windows), and `rust-toolchain.toml` pins the toolchain. A locally built binary looks for yt-dlp and ffmpeg in `bin/` beside itself, then on PATH.

## Check

From `rust-src/`:

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

CI runs the same checks on Windows, Linux and macOS, with the menu width and printable-text pins, the musl cross-check and `cargo audit` on Linux. The packaged selftest, `ytdlp --selftest`, runs in every bundle and in the bundle build.

## Licensing

The menu and the defaults are MIT; see `LICENSE`.

yt-dlp, which performs the download, is Unlicense, a public-domain dedication, and ships its own `THIRD_PARTY_LICENSES.txt` for the libraries inside it.

The Windows zip, the setup, the Windows arm64 zip, the two Linux glibc tars and the macOS pkg carry ffmpeg under the GNU General Public License version 2 or later, with parts under version 3 or later. `GPL-2.0.txt` and `GPL-3.0.txt` ship with it, and `THIRD-PARTY-NOTICES.md` points at the source for each build.

The musl bundles carry no third-party binaries; yt-dlp and ffmpeg are the system packages.
