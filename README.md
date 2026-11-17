Smart menu CLI around [yt-dlp](https://github.com/yt-dlp/yt-dlp). Built in Rust, zippy execution.
Copy youtube link, open app, type '0' and download starts. This project is only the menu and the defaults; actual download is performed by yt-dlp, and this is not affiliated with or endorsed by the yt-dlp project.

The yt-dlp runtime is used exactly as published - nothing in it is patched or rebuilt by this project. Bit for bit.

**Updates.** Menu item 12 updates both halves: the bundled yt-dlp payload from the official yt-dlp releases (or yt-dlp's own update for a yt-dlp you point it at), then the cli from this project's newest release - the platform bundle is digest-verified, the cli is lifted out of it and installed after the new build passes its own checks. Menu item 13 toggles the daily update check, which only tells you a newer cli exists. macOS has no cli self-update (the mac line ships a pkg): item 12 points at this page. The check runs only at the start of the interactive menu, in a real console window.

One-shot URL runs, scripts, CI and piped runs never trigger it: no network contact, nothing written. There is no background process either - the check is a single event when you open the menu yourself. If updates have not happened for a while, start the program normally, or run menu item 12 to update on demand.

**ffmpeg.** The Windows zip, the setup.exe, the Linux glibc tar and the macOS pkg carry ffmpeg in `bin/` - shaka-project/static-ffmpeg-binaries n8.1.2-1, digest-checked at build time - so merge, audio conversion and srt subtitles work with nothing else installed. The cli looks in `bin/` first and then on PATH; a system ffmpeg you install yourself is used too. The Alpine/musl bundle carries no third-party binaries: `apk add yt-dlp` provides yt-dlp and ffmpeg there.

![ytdlp-cli main menu](resources/screenshot%20ytdlp-cli.png)

## Pick your download

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.9.3-windows.zip` | 42.1 MB | yt-dlp, ffmpeg and the srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.9.3-windows-setup.exe` | 34.4 MB | Windows installer, same content as the zip: per-user install, no admin prompt, `ytdlp` on your PATH, with an uninstaller. |
| `ytdlp-cli-v2.9.3-linux.tar.gz` | 62.1 MB | yt-dlp, ffmpeg and the srtstripper. Nothing else to install. |
| `ytdlp-cli-v2.9.3-linux-musl.tar.gz` | 2.1 MB | Alpine and other musl systems: the cli and the srtstripper. Get the runtime with `apk add yt-dlp`; that package pulls ffmpeg and the JS runtime with it. |
| `ytdlp-cli-v2.9.3-macos-arm64.pkg` | 70.3 MB | macOS installer for Apple silicon (yt-dlp, ffmpeg and the srtstripper). Nothing else to install. |
Every glibc bundle and the macOS pkg carries the full runtime: yt-dlp, ffmpeg and the srtstripper. The musl bundle carries the cli and the srtstripper, and takes yt-dlp and ffmpeg from apk.

**The folder after install.** `ytdlp.exe` is the program; `config/settings.json` holds the menu defaults. `bin/` holds what it starts: `yt-dlp.exe` with its `_internal` folder, `ffmpeg.exe` and `srtstripper.exe`. `_internal` is yt-dlp's own Python runtime (interpreter, libraries, modules) and must stay next to the launcher; the bundled copy is the official onedir build, unpacked from the yt-dlp release zip unchanged.

![ytdlp-cli process flow](resources/archify%20flow%20ytdlp-cli.png)

The live, interactive flow is published at [chezhenkie.github.io/ytdlp-cli-youtube-downloader](https://chezhenkie.github.io/ytdlp-cli-youtube-downloader/); the diagram source is in the [ytdlp process flow archify](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/tree/main/ytdlp%20process%20flow%20archify) folder.

## Requirements

None beyond the bundle, the Windows binary is statically linked. The musl bundle carries no download runtime: its yt-dlp is the system's own (see the Alpine bullet below).

- Windows: `ytdlp.exe` (or the setup.exe below, which installs it for your user and puts it on your PATH)
- Linux: `./ytdlp`
- Alpine and other musl Linux: `./ytdlp` plus the system runtime - `apk add yt-dlp` (that package pulls ffmpeg and the JS runtime with it); no musl yt-dlp or ffmpeg builds are published, so the musl bundle ships the cli and the srtstripper only
- macOS 15+ (Apple silicon): `ytdlp` (the .pkg below installs it and puts it on your PATH; the bundled ffmpeg sets the floor)

### Installing on Windows

Download `ytdlp-cli-v2.9.3-windows-setup.exe` and double-click it. It needs no administrator rights and installs for your user only. The file is unsigned, so Windows SmartScreen may caution you: click More info, then Run anyway. The installer puts the program in `%LOCALAPPDATA%\Programs\ytdlp-cli`, adds that folder to your PATH and adds a Start Menu entry. Open a new terminal window and type `ytdlp` to start. To remove it later: Windows Settings > Apps, or the Start Menu uninstaller.

### Installing on macOS

Download `ytdlp-cli-v2.9.3-macos-arm64.pkg` and double-click it. The package is not signed, so macOS refuses the first open: open System Settings, then Privacy & Security, click Open Anyway next to the blocked ytdlp-cli message and confirm with your password or Touch ID. That is all the install needs - no terminal. The package puts the program in `/usr/local/ytdlp-cli`, with the portable layout (settings and downloads of extras live inside that folder), and links it onto your PATH as `/usr/local/bin/ytdlp`, so a new Terminal window can just run `ytdlp`. To remove it later: `sudo /usr/local/ytdlp-cli/uninstall-macos.sh` removes the program, the PATH link and the package receipt.

### Installing on Linux

Download a tar.gz from the table and unpack it, or let the script do it: it fetches the latest release, checks the download against the sha256 published in the release notes, unpacks it to `~/.local/share/ytdlp-cli` and links `~/.local/bin/ytdlp`, so `ytdlp` starts it. Nothing runs as root.

```
curl -fsSL https://raw.githubusercontent.com/chezhenkie/ytdlp-cli-youtube-downloader/main/install.sh | sh
```

`sh -s -- --version v2.9.3` pins a release, and `sh -s -- --uninstall` removes the install and the link. `--portable` and `--lean` are accepted as ignored: one bundle per platform now, ffmpeg included.

On Alpine and other musl systems the script installs the musl bundle automatically - the cli and the srtstripper; get the download runtime with `apk add yt-dlp`, which pulls ffmpeg and the JS runtime with it.

Linux builds are x86_64: glibc bundles plus a musl bundle for Alpine (cli plus the srtstripper, the download runtime comes from apk), and the macOS build is arm64 (Apple silicon). On any other architecture put `yt-dlp` on PATH or set `yt_dlp_path` in `config/settings.json`. Menu item 12 updates the cli from the platform bundle; macOS takes its cli from this page.

v1.5.0 was the final Python release; its bundles remain on the v1.5.0 release page.

## Using it

Start it and paste a URL, or press 0 to download the link already on your clipboard as video with sound. Press `s` instead to grab only a summary: the subtitles are downloaded and stripped to plain text, and the sound is downloaded only when no subtitle text can be produced. Press `a` for audio only or `v` for video only on the same clipboard link; neither changes your settings. Menu item 1 asks for the same four modes first, then collects the list. Every one of those paths takes several links at once: a pasted line or the clipboard may separate them with commas, standalone `0`s or line breaks, and the links run in the order they appear. Or skip the menu:

```
ytdlp.exe https://youtu.be/xxxx
```

One-shot overrides look like `-f mkv -r 720 --subs`, also `-a`, `--af`, `-o`, `--template`, `--playlist`. Overrides apply to that run only and are never written to your settings.

The menu itself is where the defaults live: container, resolution, audio format, output folder, filename template, subtitles, the srt timecode stripper, playlists, and reset. Item 1 takes several URLs and downloads them one after another. Every bundle ships the stripper in `bin/`, so with subtitles on and item 8 turned on, a stripped copy lands next to each downloaded subtitle. Escape stops a running download and removes the partial files.

Output goes to a `ytdlp-cli-downloads` folder on your desktop by default, falling back to the same folder inside your downloads folder. Change it with menu item 5.

## Checksums

```
6d9ecd5f975c3ee80b7d2afa2d621b8c6ac150ff6a30c8011081a07c77062ef9  ytdlp-cli-v2.9.3-linux.tar.gz
6156730958c6f0c6ed0209faf7c05ed1254338c002aa29c5e7a97a2cf52dbde6  ytdlp-cli-v2.9.3-linux-musl.tar.gz
0fb86aaa8818a5ba4de39ac0f495c3232b069a03702f3b770145bdc6dea7c6c8  ytdlp-cli-v2.9.3-macos-arm64.pkg
9a2855c111541e79a4ae9a4eba588d22c18961f36a911e09b0e5f794ce27e86d  ytdlp-cli-v2.9.3-windows.zip
22517634ff565da928a05ce23f99ba6a2832f36a4a87bc1c8c05a2c80f9f1e64  ytdlp-cli-v2.9.3-windows-setup.exe
```

No release publishes a bare cli image: menu item 12 updates the cli from the platform bundle.

The bundled yt-dlp is the official 2026.08.19 release and matches the SHA-256 sums published with it. The bundled ffmpeg is the shaka-project/static-ffmpeg-binaries n8.1.2-1 build for each platform, matched to the release's published per-asset digest, and `THIRD-PARTY-NOTICES.md` in each bundle and installer names the exact asset and its digest. The linux-musl bundle ships none of the three: its yt-dlp and ffmpeg are the system's own packages.

## Licensing

The menu and the defaults are **MIT**. See `LICENSE` in the bundle.

yt-dlp, which does all the downloading, is **Unlicense**, a public domain dedication. It ships its own `THIRD_PARTY_LICENSES.txt` listing every library inside it.

The Windows zip, the setup.exe, the Linux glibc tar and the macOS pkg also contain **ffmpeg**, built `--enable-gpl --enable-version3`, so it is distributed under the GNU General Public License version 2 or later and includes components under GPL version 3 or later. Both license texts ship with it as `GPL-2.0.txt` and `GPL-3.0.txt`, and `THIRD-PARTY-NOTICES.md` points at the corresponding source for each build.

The `linux-musl` bundle contains no third-party binaries: its yt-dlp and ffmpeg are the system's own Alpine packages, under the licenses named above.

ffmpeg is free software and you may use, study, modify and redistribute it under the terms of the GPL, exactly as its authors intend. Nothing here adds to, limits or restricts the rights the GPL grants you for it.

See [README.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader) for the full behaviour, and [CHANGELOG.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/blob/main/CHANGELOG.md) for the history.
