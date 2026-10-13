Text-menu CLI around [yt-dlp](https://github.com/yt-dlp/yt-dlp). Paste a URL, press enter, the download runs with the saved defaults. This project is only the menu, the launchers and the defaults; every actual download is performed by yt-dlp, and this is not affiliated with or endorsed by the yt-dlp project.
Not a single bit is changed in the original yt-dlp runtime. The CLI has an update function and a daily update check for both the yt-dlp runtime and the cli: menu item 11 updates both on demand, and the daily check only prints what is newer and asks before it replaces anything. Menu item 12 turns that check on or off.
For lean download only: on an OS with no ffmpeg installed the CLI offers to fetch the published build - gyan.dev on Windows, BtbN/FFmpeg-Builds on Linux - and checks it against the provider's published checksum before it is installed. FFmpeg's own repository is source code only and publishes no binaries, so that is not where a binary can come from.
## Pick your download

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v1.4-windows-lean.zip` | 17.7 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v1.4-windows-portable.zip` | 56.3 MB | yt-dlp **and** ffmpeg. Nothing else to install. |
| `ytdlp-cli-v1.4-linux-lean.tar.gz` | 2.97 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v1.4-linux-portable.tar.gz` | 70.6 MB | yt-dlp **and** ffmpeg. Nothing else to install. |

The two portable bundles are the larger downloads and the ones most people want. The lean bundles skip ffmpeg, and the CLI says so and falls back to a single best file rather than failing. On a machine with no ffmpeg at all it also offers to fetch the published build, with the checksum checked before it is installed.

## Requirements

Python 3. No pip install, no dependencies: the CLI is stdlib only.

- Windows: `run-ytdlp.bat`, or `python app-python\src\ytdlp.py`
- Linux: `./run-ytdlp.sh`, which also works under `bash` and `sh`

Linux builds are x86_64 glibc. On any other architecture put `yt-dlp` on PATH or set `yt_dlp_path` in `config/settings.json`.

## Using it

Start it and paste a URL. Or skip the menu:

```
app-python\run-ytdlp.bat https://youtu.be/xxxx
```

One-shot overrides look like `-f mkv -r 720 --subs`, also `-a`, `--af`, `-o`, `--template`, `--playlist`. Overrides apply to that run only and are never written to your settings.

The menu itself is where the defaults live: type/video mode, container, resolution, audio format, output folder, filename template, subtitles, playlists, and reset. Item 9 takes several URLs and downloads them one after another. Escape stops a running download and removes the partial files.

Output goes to your desktop by default, falling back to your downloads folder. Change it with menu item 5.

## Checksums

```
ae279f1f11865e348627a437c3cfed03116bdb752d8da858ce0f16de21c5ffc2  ytdlp-cli-v1.4-windows-lean.zip
e13f4013c79ae6138ebecd767862255cde6603fd64776cd070ac5ca5a4fc9ac9  ytdlp-cli-v1.4-windows-portable.zip
7ced2b66bf0434f242d50d1409e749b49a57c6c90aadf6a304ceb5e5fb4db910  ytdlp-cli-v1.4-linux-lean.tar.gz
89280e4713717f5327b26ff376bfe4e4497c366fc307c11036d9c678c35a014a  ytdlp-cli-v1.4-linux-portable.tar.gz
```

The bundled yt-dlp is the official 2026.08.19 release and matches the SHA-256 sums published with it. The Windows ffmpeg is the gyan.dev 9.0.2 essentials build and the Linux one is the BtbN n9.0.2 build; both matched their providers' published checksums, and `THIRD-PARTY-NOTICES.md` in each bundle names the exact archive and its digest.

## Licensing

The menu, the launchers and the defaults are **MIT**. See `LICENSE` in the bundle.

yt-dlp, which does all the downloading, is **Unlicense**, a public domain dedication. It ships its own `THIRD_PARTY_LICENSES.txt` listing every library inside it.

The portable bundles also contain **ffmpeg**, built `--enable-gpl --enable-version3`, so it is distributed under the GNU General Public License version 2 or later and includes components under GPL version 3 or later. Both license texts ship in those bundles as `GPL-2.0.txt` and `GPL-3.0.txt`, and `THIRD-PARTY-NOTICES.md` points at the corresponding source for each build.

ffmpeg is free software and you may use, study, modify and redistribute it under the terms of the GPL, exactly as its authors intend. Nothing here adds to, limits or restricts the rights the GPL grants you for it.

See [README.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader) for the full behaviour, and [CHANGELOG.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/blob/main/CHANGELOG.md) for the history.
