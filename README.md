Text-menu CLI around [yt-dlp](https://github.com/yt-dlp/yt-dlp). Paste a URL, press enter, the download runs with the saved defaults. This project is only the menu, the launchers and the defaults; every actual download is performed by yt-dlp, and this is not affiliated with or endorsed by the yt-dlp project.

The yt-dlp runtime is used exactly as published - nothing in it is patched or rebuilt by this project.

**Updates.** The updater covers the yt-dlp runtime: menu item 11 runs yt-dlp's own update on demand and menu item 12 toggles the daily update check. The cli never replaces itself; a newer cli comes from this page. The check runs only at the start of the interactive menu, in a real console window.

One-shot URL runs, scripts, CI and piped runs never trigger it: no network contact, nothing written. There is no background process either - the check is a single event when you open the menu yourself. If updates have not happened for a while, start the program normally, or run menu item 11 to update on demand.

**ffmpeg.** The lean bundles ship without it: when none is found on the machine, the CLI offers to fetch a published build - BtbN/FFmpeg-Builds on Windows and Linux, shaka-project/static-ffmpeg-binaries on macOS - and checks it against the provider's published checksum or digest before installing. FFmpeg's own repository is source code only and publishes no binaries, so that is not where a binary can come from.

## Pick your download

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.1.1-windows-lean.zip` | 19.5 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v2.1.1-windows-portable.zip` | 59.5 MB | yt-dlp **and** ffmpeg. Nothing else to install. |
| `ytdlp-cli-v2.1.1-linux-lean.tar.gz` | 42.0 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v2.1.1-linux-portable.tar.gz` | 110.1 MB | yt-dlp **and** ffmpeg. Nothing else to install. |
| `ytdlp-cli-v2.1.1-macos-arm64-portable.tar.gz` | 54.1 MB | yt-dlp **and** ffmpeg, Apple silicon. Nothing else to install. |
The three portable bundles are the larger downloads and the ones most people want. The lean bundles skip ffmpeg, and the CLI says so and falls back to a single best file rather than failing; on a machine with no ffmpeg at all it also offers to fetch the published build for the platform, with the checksum checked before it is installed.

## Requirements

None beyond the bundle: no Python, no runtime, no redistributable. The Windows binary is statically linked.

- Windows: `ytdlp.exe`
- Linux: `./ytdlp`
- macOS (Apple silicon): `./ytdlp`

The bundle also carries `run-ytdlp.bat` and `run-ytdlp.sh`; they are optional, the chapter below explains when they help.

Linux builds are x86_64 glibc and the macOS bundles are arm64 (Apple silicon). On any other architecture put `yt-dlp` on PATH or set `yt_dlp_path` in `config/settings.json`. The macOS binaries are not signed or notarized; if a browser download attaches a quarantine flag, clear it once with `xattr -d com.apple.quarantine ./ytdlp` (same for files under `bin/`). There is no lean macOS bundle, and no release publishes a bare cli image on any platform, so menu item 11 updates yt-dlp and a newer cli always comes from this page.

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

Normally you use the exe: `ytdlp.exe` on Windows, `./ytdlp` on Linux and macOS. It is the whole program - one static binary, no runtime, no install step, and every feature and setting lives inside it.

`run-ytdlp.bat` and `run-ytdlp.sh` are optional thin wrappers around that exe:

| file | what it adds |
|---|---|
| `run-ytdlp.bat` | Windows double-click entry. Runs `ytdlp.exe` from the bundle folder, forwards arguments, and pauses on a nonzero exit so the console does not close before you can read the error. |
| `run-ytdlp.sh` | Posix entry that finds its own folder, so it runs from any directory and under `sh`/`dash` (Linux and macOS). |

They change nothing else: same arguments, same settings, same output. The updater replaces the exe only and never touches them, so deleting them is safe.

## Checksums

```
61805b2e3513b9c51e8cc35052147a0169bbdd31a69bf185e3fe998b660e9a5c  ytdlp-cli-v2.1.1-windows-lean.zip
380d4b472e9a58f98809fed6a5681f9bac5c339ab5e7920c3c3ec15ac8f4fa29  ytdlp-cli-v2.1.1-windows-portable.zip
f22330ad58b70c9f89df331524f07469176aa8408ef6b5eb7c28ed4c990f1d01  ytdlp-cli-v2.1.1-linux-lean.tar.gz
397b5a587ce7e5e98dee924460f3780b06947f949739beb5347f8c2b6f70fddf  ytdlp-cli-v2.1.1-linux-portable.tar.gz
30c8bf5c89e411c3a3b92ff5eff48481eebd28bf45dca7ca1728689113dee5b5  ytdlp-cli-v2.1.1-macos-arm64-portable.tar.gz
```

No release publishes a bare cli image, so the built-in updater never replaces the cli; a newer cli always comes from this page.

The bundled yt-dlp is the official 2026.08.19 release and matches the SHA-256 sums published with it. The Windows ffmpeg is the gyan.dev 9.0.2 essentials build, the Linux one is the BtbN n9.0.2 build and the macOS one is the shaka-project 8.1.2 static arm64 build; each matched its provider's published checksum or digest, and `THIRD-PARTY-NOTICES.md` in each bundle names the exact archive and its digest.

## Licensing

The menu, the launchers and the defaults are **MIT**. See `LICENSE` in the bundle.

yt-dlp, which does all the downloading, is **Unlicense**, a public domain dedication. It ships its own `THIRD_PARTY_LICENSES.txt` listing every library inside it.

The portable bundles also contain **ffmpeg**, built `--enable-gpl --enable-version3`, so it is distributed under the GNU General Public License version 2 or later and includes components under GPL version 3 or later. Both license texts ship in those bundles as `GPL-2.0.txt` and `GPL-3.0.txt`, and `THIRD-PARTY-NOTICES.md` points at the corresponding source for each build.

ffmpeg is free software and you may use, study, modify and redistribute it under the terms of the GPL, exactly as its authors intend. Nothing here adds to, limits or restricts the rights the GPL grants you for it.

See [README.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader) for the full behaviour, and [CHANGELOG.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/blob/main/CHANGELOG.md) for the history.
