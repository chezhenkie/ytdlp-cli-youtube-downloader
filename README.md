Smart menu CLI around [yt-dlp](https://github.com/yt-dlp/yt-dlp). Built in Rust, zippy execution.
Copy youtube link, open app, type '0' and download starts. This project is only the menu, the launchers and the defaults; actual download is performed by yt-dlp, and this is not affiliated with or endorsed by the yt-dlp project.

The yt-dlp runtime is used exactly as published - nothing in it is patched or rebuilt by this project. Bit for bit.

**Updates.** Menu item 13 updates both halves: the bundled yt-dlp payload from the official yt-dlp releases (or yt-dlp's own update for a yt-dlp you point it at), then the cli from this project's newest release - the platform lean bundle is digest-verified, the cli is lifted out of it and installed after the new build passes its own checks. Menu item 14 toggles the daily update check, which only tells you a newer cli exists. macOS has no cli self-update (the mac line ships a pkg): item 13 points at this page. The check runs only at the start of the interactive menu, in a real console window.

One-shot URL runs, scripts, CI and piped runs never trigger it: no network contact, nothing written. There is no background process either - the check is a single event when you open the menu yourself. If updates have not happened for a while, start the program normally, or run menu item 13 to update on demand.

**ffmpeg.** The lean bundles ship without it: when none is found on the machine, the CLI offers to fetch a published build - BtbN/FFmpeg-Builds on Windows and Linux, shaka-project/static-ffmpeg-binaries on macOS - and checks it against the provider's published checksum or digest before installing. FFmpeg's own repository is source code only and publishes no binaries, so that is not where a binary can come from.

![ytdlp-cli main menu](resources/screenshot%20ytdlp-cli.png)

## Pick your download

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.8.1-windows-lean.zip` | 20.1 MB | yt-dlp and the srtstripper. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v2.8.1-windows-portable.zip` | 60.1 MB | yt-dlp, the srtstripper **and** ffmpeg. Nothing else to install. |
| `ytdlp-cli-v2.8.1-windows-setup.exe` | 46.2 MB | Windows installer, same content as the portable zip: per-user install, no admin prompt, `ytdlp` on your PATH, with an uninstaller. |
| `ytdlp-cli-v2.8.1-linux-lean.tar.gz` | 42.5 MB | yt-dlp and the srtstripper. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v2.8.1-linux-portable.tar.gz` | 110.6 MB | yt-dlp, the srtstripper **and** ffmpeg. Nothing else to install. |
| `ytdlp-cli-v2.8.1-linux-musl.tar.gz` | 2.1 MB | Alpine and other musl systems: the cli and the srtstripper. Get the runtime with `apk add yt-dlp`; that package pulls ffmpeg and the JS runtime with it. |
| `ytdlp-cli-v2.8.1-macos-arm64.pkg` | 70.3 MB | macOS installer for Apple silicon (yt-dlp, ffmpeg and the srtstripper). Nothing else to install. |
The lean bundles skip ffmpeg, on a machine with no ffmpeg at all it also offers to fetch the published build for the platform, with the checksum checked before it is installed.

![ytdlp-cli process flow](resources/archify%20flow%20ytdlp-cli.png)

The live, interactive flow is published at [chezhenkie.github.io/ytdlp-cli-youtube-downloader](https://chezhenkie.github.io/ytdlp-cli-youtube-downloader/); the diagram source is in the [ytdlp process flow archify](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/tree/main/ytdlp%20process%20flow%20archify) folder.

## Requirements

None beyond the bundle, the Windows binary is statically linked. The musl bundle carries no download runtime: its yt-dlp is the system's own (see the Alpine bullet below).

- Windows: `ytdlp.exe` (or the setup.exe below, which installs it for your user and puts it on your PATH)
- Linux: `./ytdlp`
- Alpine and other musl Linux: `./ytdlp` plus the system runtime - `apk add yt-dlp` (that package pulls ffmpeg and the JS runtime with it); no musl yt-dlp or ffmpeg builds are published, so the musl bundle ships the cli and the srtstripper only
- macOS (Apple silicon): `ytdlp` (the .pkg below installs it and puts it on your PATH)

Every bundle also carries `run-ytdlp.bat` and `run-ytdlp.sh`; they are optional, the chapter below explains when they help (the installers install those files too).

### Installing on Windows

Download `ytdlp-cli-v2.8.1-windows-setup.exe` and double-click it. It needs no administrator rights and installs for your user only. The file is unsigned, so Windows SmartScreen may caution you: click More info, then Run anyway. The installer puts the program in `%LOCALAPPDATA%\Programs\ytdlp-cli`, adds that folder to your PATH and adds a Start Menu entry. Open a new terminal window and type `ytdlp` to start. To remove it later: Windows Settings > Apps, or the Start Menu uninstaller.

### Installing on macOS

Download `ytdlp-cli-v2.8.1-macos-arm64.pkg` and double-click it. The package is not signed, so macOS refuses the first open: open System Settings, then Privacy & Security, click Open Anyway next to the blocked ytdlp-cli message and confirm with your password or Touch ID. That is all the install needs - no terminal. The package puts the program in `/usr/local/ytdlp-cli`, with the portable layout (settings and downloads of extras live inside that folder), and links it onto your PATH as `/usr/local/bin/ytdlp`, so a new Terminal window can just run `ytdlp`. To remove it later: `sudo /usr/local/ytdlp-cli/uninstall-macos.sh` removes the program, the PATH link and the package receipt.

### Installing on Linux

Download a tar.gz from the table and unpack it, or let the script do it: it fetches the latest release, checks the download against the sha256 published in the release notes, unpacks it to `~/.local/share/ytdlp-cli` and links `~/.local/bin/ytdlp`, so `ytdlp` starts it. Nothing runs as root.

```
curl -fsSL https://raw.githubusercontent.com/chezhenkie/ytdlp-cli-youtube-downloader/main/install.sh | sh
```

The portable bundle (with ffmpeg) is the default; `sh -s -- --lean` installs the smaller bundle without ffmpeg instead, `sh -s -- --version v2.8.1` pins a release, and `sh -s -- --uninstall` removes the install and the link.

On Alpine and other musl systems the script installs the musl bundle automatically - the cli and the srtstripper; get the download runtime with `apk add yt-dlp`, which pulls ffmpeg and the JS runtime with it. `--lean` and `--portable` name the glibc bundles and are refused on musl.

Linux builds are x86_64: glibc bundles plus a musl bundle for Alpine (cli plus the srtstripper, the download runtime comes from apk), and the macOS build is arm64 (Apple silicon). On any other architecture put `yt-dlp` on PATH or set `yt_dlp_path` in `config/settings.json`. Menu item 13 updates the cli from the platform bundle; macOS takes its cli from this page.

v1.5.0 was the final Python release; its bundles remain on the v1.5.0 release page.

## Using it

Start it and paste a URL, or press 0 to download the link already on your clipboard. Press `s` instead to grab only a summary: the subtitles are downloaded and stripped to plain text, and the sound is downloaded only when no subtitle text can be produced. Or skip the menu:

```
ytdlp.exe https://youtu.be/xxxx
```

One-shot overrides look like `-f mkv -r 720 --subs`, also `-a`, `--af`, `-o`, `--template`, `--playlist`. Overrides apply to that run only and are never written to your settings.

The menu itself is where the defaults live: type/video mode, container, resolution, audio format, output folder, filename template, subtitles, the srt timecode stripper, playlists, and reset. Item 10 takes several URLs and downloads them one after another. Every bundle ships the stripper in `bin/`, so with subtitles on and item 8 turned on, a stripped copy lands next to each downloaded subtitle. Escape stops a running download and removes the partial files.

Output goes to a `ytdlp-cli-downloads` folder on your desktop by default, falling back to the same folder inside your downloads folder. Change it with menu item 5.

## Exe or launcher

Normally you use the exe: `ytdlp.exe` on Windows, `./ytdlp` on Linux and macOS, or plain `ytdlp` once the Windows installer or the macOS package has put it on your PATH. It is the whole program - one static binary, no runtime, and every feature and setting lives inside it.

`run-ytdlp.bat` and `run-ytdlp.sh` are optional thin wrappers around that exe:

| file | what it adds |
|---|---|
| `run-ytdlp.bat` | Windows double-click entry. Runs `ytdlp.exe` from the bundle folder, forwards arguments, and pauses on a nonzero exit so the console does not close before you can read the error. |
| `run-ytdlp.sh` | Posix entry that finds its own folder, so it runs from any directory and under `sh`/`dash` (Linux and macOS). |

They change nothing else: same arguments, same settings, same output. The yt-dlp update never touches the launchers, so deleting them is safe.

## Checksums

```
c0708713697d40ba0facea9e240c0d6b08452580d7c3816d8bd5371c2c39c46c  ytdlp-cli-v2.8.1-windows-lean.zip
c7fb678c8da5ad935d6b284cc028f78a4912920058011f78b1aaf052ca118216  ytdlp-cli-v2.8.1-windows-portable.zip
4d69d222dd03615f7fcd8f938ebefc160c3043266d39d109e6dfa6cbd177df41  ytdlp-cli-v2.8.1-windows-setup.exe
ecf3abde0c7ee80df8041cfc0ca95d1e5500990e9650d3e2607609cfcb08663a  ytdlp-cli-v2.8.1-linux-lean.tar.gz
644e7f53a1b8f8a3bb713badf7f3b5ff0af3281fb003aaad2b2ff9e552fa2692  ytdlp-cli-v2.8.1-linux-portable.tar.gz
40bebc1a86bf3bd340eba51dd31e510675d35f6766da295b5822d3ae6fafa8ca  ytdlp-cli-v2.8.1-linux-musl.tar.gz
4237a5e70bd65bbd80dd91135b30c330dba39643b5ca95b34f1c5cb6932e7155  ytdlp-cli-v2.8.1-macos-arm64.pkg
```

No release publishes a bare cli image: menu item 13 updates the cli from the platform lean bundle.

The bundled yt-dlp is the official 2026.08.19 release and matches the SHA-256 sums published with it. The Windows ffmpeg is the gyan.dev 9.0.2 essentials build, the Linux one is the BtbN n9.0.2 build and the macOS one is the shaka-project 8.1.2 static arm64 build; each matched its provider's published checksum or digest, and `THIRD-PARTY-NOTICES.md` in each bundle and installer names the exact archive and its digest. The linux-musl bundle ships none of the three: its yt-dlp and ffmpeg are the system's own packages.

## Licensing

The menu, the launchers and the defaults are **MIT**. See `LICENSE` in the bundle.

yt-dlp, which does all the downloading, is **Unlicense**, a public domain dedication. It ships its own `THIRD_PARTY_LICENSES.txt` listing every library inside it.

The portable bundles and the installers also contain **ffmpeg**, built `--enable-gpl --enable-version3`, so it is distributed under the GNU General Public License version 2 or later and includes components under GPL version 3 or later. Both license texts ship with it as `GPL-2.0.txt` and `GPL-3.0.txt`, and `THIRD-PARTY-NOTICES.md` points at the corresponding source for each build.

The `linux-musl` bundle contains no third-party binaries: its yt-dlp and ffmpeg are the system's own Alpine packages, under the licenses named above.

ffmpeg is free software and you may use, study, modify and redistribute it under the terms of the GPL, exactly as its authors intend. Nothing here adds to, limits or restricts the rights the GPL grants you for it.

See [README.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader) for the full behaviour, and [CHANGELOG.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/blob/main/CHANGELOG.md) for the history.
