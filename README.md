Smart menu CLI around [yt-dlp](https://github.com/yt-dlp/yt-dlp). Built in Rust, zippy execution.
Copy youtube link, open app, type '0' and download starts. This project is only the menu, the launchers and the defaults; actual download is performed by yt-dlp, and this is not affiliated with or endorsed by the yt-dlp project. 

The yt-dlp runtime is used exactly as published - nothing in it is patched or rebuilt by this project. Bit for bit.

**Updates.** The updater covers the yt-dlp runtime: menu item 11 runs yt-dlp's own update on demand and menu item 12 toggles the daily update check. The cli never replaces itself; a newer cli comes from this page. The check runs only at the start of the interactive menu, in a real console window.

One-shot URL runs, scripts, CI and piped runs never trigger it: no network contact, nothing written. There is no background process either - the check is a single event when you open the menu yourself. If updates have not happened for a while, start the program normally, or run menu item 11 to update on demand.

**ffmpeg.** The lean bundles ship without it: when none is found on the machine, the CLI offers to fetch a published build - BtbN/FFmpeg-Builds on Windows and Linux, shaka-project/static-ffmpeg-binaries on macOS - and checks it against the provider's published checksum or digest before installing. FFmpeg's own repository is source code only and publishes no binaries, so that is not where a binary can come from.

## Pick your download

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.2.0-windows-lean.zip` | 19.5 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v2.2.0-windows-portable.zip` | 59.5 MB | yt-dlp **and** ffmpeg. Nothing else to install. |
| `ytdlp-cli-v2.2.0-windows-setup.exe` | 48.9 MB | Windows installer, same content as the portable zip: per-user install, no admin prompt, `ytdlp` on your PATH, with an uninstaller. |
| `ytdlp-cli-v2.2.0-linux-lean.tar.gz` | 42.0 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v2.2.0-linux-portable.tar.gz` | 110.1 MB | yt-dlp **and** ffmpeg. Nothing else to install. |
| `ytdlp-cli-v2.2.0-macos-arm64.pkg` | 54.1 MB | macOS installer for Apple silicon (yt-dlp and ffmpeg). Nothing else to install. |
The lean bundles skip ffmpeg, on a machine with no ffmpeg at all it also offers to fetch the published build for the platform, with the checksum checked before it is installed.

## Requirements

None beyond the bundle: no Python, no runtime, no redistributable. The Windows binary is statically linked.

- Windows: `ytdlp.exe` (or the setup.exe below, which installs it for your user and puts it on your PATH)
- Linux: `./ytdlp`
- macOS (Apple silicon): `ytdlp` (the .pkg below installs it and puts it on your PATH)

Every bundle also carries `run-ytdlp.bat` and `run-ytdlp.sh`; they are optional, the chapter below explains when they help (the installers install those files too).

### Installing on Windows

Download `ytdlp-cli-v2.2.0-windows-setup.exe` and double-click it. It needs no administrator rights and installs for your user only. The file is unsigned, so Windows SmartScreen may caution you: click More info, then Run anyway. The installer puts the program in `%LOCALAPPDATA%\Programs\ytdlp-cli`, adds that folder to your PATH and adds a Start Menu entry. Open a new terminal window and type `ytdlp` to start. To remove it later: Windows Settings > Apps, or the Start Menu uninstaller.

### Installing on macOS

Download `ytdlp-cli-v2.2.0-macos-arm64.pkg` and double-click it. The package is not signed, so macOS refuses the first open: open System Settings, then Privacy & Security, click Open Anyway next to the blocked ytdlp-cli message and confirm with your password or Touch ID. That is all the install needs - no terminal. The package puts the program in `/usr/local/ytdlp-cli`, with the portable layout (settings and downloads of extras live inside that folder), and links it onto your PATH as `/usr/local/bin/ytdlp`, so a new Terminal window can just run `ytdlp`. To remove it later: `sudo /usr/local/ytdlp-cli/uninstall-macos.sh` removes the program, the PATH link and the package receipt.

### Installing on Linux

Download a tar.gz from the table and unpack it, or let the script do it: it fetches the latest release, checks the download against the sha256 published in the release notes, unpacks it to `~/.local/share/ytdlp-cli` and links `~/.local/bin/ytdlp`, so `ytdlp` starts it. Nothing runs as root.

```
curl -fsSL https://raw.githubusercontent.com/chezhenkie/ytdlp-cli-youtube-downloader/main/install.sh | sh
```

The portable bundle (with ffmpeg) is the default; `sh -s -- --lean` installs the smaller bundle without ffmpeg instead, `sh -s -- --version v2.2.0` pins a release, and `sh -s -- --uninstall` removes the install and the link.

Linux builds are x86_64 glibc and the macOS build is arm64 (Apple silicon). On any other architecture put `yt-dlp` on PATH or set `yt_dlp_path` in `config/settings.json`. No release publishes a bare cli image on any platform, so menu item 11 updates yt-dlp and a newer cli always comes from this page.

v1.5.0 was the final Python release; its bundles remain on the v1.5.0 release page.

## Using it

Start it and paste a URL, or press 0 to download the link already on your clipboard. Or skip the menu:

```
ytdlp.exe https://youtu.be/xxxx
```

One-shot overrides look like `-f mkv -r 720 --subs`, also `-a`, `--af`, `-o`, `--template`, `--playlist`. Overrides apply to that run only and are never written to your settings.

The menu itself is where the defaults live: type/video mode, container, resolution, audio format, output folder, filename template, subtitles, playlists, and reset. Item 9 takes several URLs and downloads them one after another. Escape stops a running download and removes the partial files.

Output goes to your desktop by default, falling back to your downloads folder. Change it with menu item 5.

## Exe or launcher

Normally you use the exe: `ytdlp.exe` on Windows, `./ytdlp` on Linux and macOS, or plain `ytdlp` once the Windows installer or the macOS package has put it on your PATH. It is the whole program - one static binary, no runtime, and every feature and setting lives inside it.

`run-ytdlp.bat` and `run-ytdlp.sh` are optional thin wrappers around that exe:

| file | what it adds |
|---|---|
| `run-ytdlp.bat` | Windows double-click entry. Runs `ytdlp.exe` from the bundle folder, forwards arguments, and pauses on a nonzero exit so the console does not close before you can read the error. |
| `run-ytdlp.sh` | Posix entry that finds its own folder, so it runs from any directory and under `sh`/`dash` (Linux and macOS). |

They change nothing else: same arguments, same settings, same output. The updater replaces the exe only and never touches them, so deleting them is safe.

## Checksums

```
ace4e1e0405d7339c98c85f5dfb889f288ed3335729b85975fba4ad5be3fda44  ytdlp-cli-v2.2.0-windows-lean.zip
3d542d11b0c5044ab0bd04fb897583bb52df7b12062de9d52ccb5cb1e83270fb  ytdlp-cli-v2.2.0-windows-portable.zip
0d911aaddcc731cbb2fde01824ce3268e5fe549f3854cb2dd9ef3190ba7036b6  ytdlp-cli-v2.2.0-windows-setup.exe
3cdcc3d42c0082a2790bb4fd711bb502cdab416482891dfeb49637329b600cf2  ytdlp-cli-v2.2.0-linux-lean.tar.gz
8ebad27012398df4dad474faf05f6208556e75c598f88cf0856c910140e46477  ytdlp-cli-v2.2.0-linux-portable.tar.gz
3334e3c44721909964c23d304a36f8eb4d9955205d1d854a5490c56b86a7c910  ytdlp-cli-v2.2.0-macos-arm64.pkg
```

No release publishes a bare cli image, so the built-in updater never replaces the cli; a newer cli always comes from this page.

The bundled yt-dlp is the official 2026.08.19 release and matches the SHA-256 sums published with it. The Windows ffmpeg is the gyan.dev 9.0.2 essentials build, the Linux one is the BtbN n9.0.2 build and the macOS one is the shaka-project 8.1.2 static arm64 build; each matched its provider's published checksum or digest, and `THIRD-PARTY-NOTICES.md` in each bundle and installer names the exact archive and its digest.

## Licensing

The menu, the launchers and the defaults are **MIT**. See `LICENSE` in the bundle.

yt-dlp, which does all the downloading, is **Unlicense**, a public domain dedication. It ships its own `THIRD_PARTY_LICENSES.txt` listing every library inside it.

The portable bundles and the installers also contain **ffmpeg**, built `--enable-gpl --enable-version3`, so it is distributed under the GNU General Public License version 2 or later and includes components under GPL version 3 or later. Both license texts ship with it as `GPL-2.0.txt` and `GPL-3.0.txt`, and `THIRD-PARTY-NOTICES.md` points at the corresponding source for each build.

ffmpeg is free software and you may use, study, modify and redistribute it under the terms of the GPL, exactly as its authors intend. Nothing here adds to, limits or restricts the rights the GPL grants you for it.

See [README.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader) for the full behaviour, and [CHANGELOG.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/blob/main/CHANGELOG.md) for the history.
