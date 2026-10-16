Text-menu CLI around [yt-dlp](https://github.com/yt-dlp/yt-dlp). Paste a URL, press enter, the download runs with the saved defaults. This project is only the menu, the launchers and the defaults; every actual download is performed by yt-dlp, and this is not affiliated with or endorsed by the yt-dlp project.
Not a single bit is changed in the original yt-dlp runtime. The CLI has an update function and a daily update check for both the yt-dlp runtime and the cli: menu item 11 updates both on demand, and the daily check only prints what is newer and asks before it replaces anything. Menu item 12 turns that check on or off. Note when the daily check runs: only at the start of the interactive menu, and only in a real console window. Running the program with a URL as argument (one-shot download), or in a
script, CI or any piped run, never triggers it - no network contact, nothing written. There is no background process; the check is a single event when you open the menu yourself. If updates have not happened for a while, start the program normally, or run menu item 11 to update on demand.
For lean download only: on an OS with no ffmpeg installed the CLI offers to fetch the published build - gyan.dev on Windows, BtbN/FFmpeg-Builds on Linux - and checks it against the provider's published checksum before it is installed. FFmpeg's own repository is source code only and publishes no binaries, so that is not where a binary can come from.
## Pick your download

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v2.0.0-windows-lean.zip` | 19.5 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v2.0.0-windows-portable.zip` | 59.5 MB | yt-dlp **and** ffmpeg. Nothing else to install. |
| `ytdlp-cli-v2.0.0-linux-lean.tar.gz` | 42.0 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v2.0.0-linux-portable.tar.gz` | 110.1 MB | yt-dlp **and** ffmpeg. Nothing else to install. |
The two portable bundles are the larger downloads and the ones most people want. The lean bundles skip ffmpeg, and the CLI says so and falls back to a single best file rather than failing. On a machine with no ffmpeg at all it also offers to fetch the published build, with the checksum checked before it is installed.

## Requirements

None beyond the bundle: no Python, no runtime, no redistributable. The Windows binary is statically linked.

- Windows: `run-ytdlp.bat`, or `ytdlp.exe`
- Linux: `./run-ytdlp.sh`, which also works under `sh` and `dash`, or `./ytdlp`

Linux builds are x86_64 glibc. On any other architecture put `yt-dlp` on PATH or set `yt_dlp_path` in `config/settings.json`.

v1.5.0 was the final Python release; its bundles remain on the v1.5.0 release page.

## Using it

Start it and paste a URL. Or skip the menu:

```
run-ytdlp.bat https://youtu.be/xxxx
```

One-shot overrides look like `-f mkv -r 720 --subs`, also `-a`, `--af`, `-o`, `--template`, `--playlist`. Overrides apply to that run only and are never written to your settings.

The menu itself is where the defaults live: type/video mode, container, resolution, audio format, output folder, filename template, subtitles, playlists, and reset. Item 9 takes several URLs and downloads them one after another. Escape stops a running download and removes the partial files.

Output goes to your desktop by default, falling back to your downloads folder. Change it with menu item 5.

## Checksums

```
86726f1a91907c0aa4850bc7abc0e3c846f02531726eb4f1c3cb49bcd312e0e9  ytdlp-cli-v2.0.0-windows-lean.zip
03c9e19b4cce29bc07f9b73ac77aba1cadf4125d3b4d0301da2eb0da06e64c42  ytdlp-cli-v2.0.0-windows-portable.zip
6af6d371e2c28acd4899e1328a4c690fa10f8dc67492a6d34514514ee164627d  ytdlp-cli-v2.0.0-linux-lean.tar.gz
8379c17cc5399ea35d4ddfe0c399ee64ad26c633a25d506da690d8b5a3b91f15  ytdlp-cli-v2.0.0-linux-portable.tar.gz
6d81b69998a87b0c445a3bb109e2bb1d20ed11bed6325a23297328a25481e28d  ytdlp-native-windows-x86_64.exe
5d3433352aa220486408bc8bdc904b64b81b00a4998c8aa93f278c0ae41a5d17  ytdlp-native-linux-x86_64
```

The last two are the updater images the built-in updater fetches; their digests are published so every update is verified against them.

The bundled yt-dlp is the official 2026.08.19 release and matches the SHA-256 sums published with it. The Windows ffmpeg is the gyan.dev 9.0.2 essentials build and the Linux one is the BtbN n9.0.2 build; both matched their providers' published checksums, and `THIRD-PARTY-NOTICES.md` in each bundle names the exact archive and its digest.

## Licensing

The menu, the launchers and the defaults are **MIT**. See `LICENSE` in the bundle.

yt-dlp, which does all the downloading, is **Unlicense**, a public domain dedication. It ships its own `THIRD_PARTY_LICENSES.txt` listing every library inside it.

The portable bundles also contain **ffmpeg**, built `--enable-gpl --enable-version3`, so it is distributed under the GNU General Public License version 2 or later and includes components under GPL version 3 or later. Both license texts ship in those bundles as `GPL-2.0.txt` and `GPL-3.0.txt`, and `THIRD-PARTY-NOTICES.md` points at the corresponding source for each build.

ffmpeg is free software and you may use, study, modify and redistribute it under the terms of the GPL, exactly as its authors intend. Nothing here adds to, limits or restricts the rights the GPL grants you for it.

See [README.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader) for the full behaviour, and [CHANGELOG.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/blob/main/CHANGELOG.md) for the history.
