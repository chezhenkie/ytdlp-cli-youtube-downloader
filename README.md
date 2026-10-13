Text-menu CLI around [yt-dlp](https://github.com/yt-dlp/yt-dlp). Paste a URL, press enter, the download runs with the saved defaults. This project is only the menu, the launchers and the defaults; every actual download is performed by yt-dlp, and this is not affiliated with or endorsed by the yt-dlp project.

## Pick your download

| file | size | contains |
|---|---|---|
| `ytdlp-cli-v1.3-windows-lean.zip` | 17.6 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v1.3-windows-portable.zip` | 109.6 MB | yt-dlp **and** ffmpeg. Nothing else to install. |
| `ytdlp-cli-v1.3-linux-lean.tar.gz` | 2.96 MB | yt-dlp. Install ffmpeg yourself for merge and audio conversion. |
| `ytdlp-cli-v1.3-linux-portable.tar.gz` | 70.7 MB | yt-dlp **and** ffmpeg. Nothing else to install. |

The two portable bundles are the larger downloads and the ones most people want. The lean bundles skip ffmpeg, and the CLI says so and falls back to a single best file rather than failing.

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
db7809d6cd74b4870ef3c9fadd1e68c37c04b6e46b592b9245f2bcc36dfc3edb  ytdlp-cli-v1.3-linux-lean.tar.gz
1cffb6443d13c419b74ecdd18e6da9313740f98f25c94216128d563d737c477b  ytdlp-cli-v1.3-linux-portable.tar.gz
22b7e5d7ea458652090752540b26b62d635e2497999feed7058711c60756cb7f  ytdlp-cli-v1.3-windows-lean.zip
f098e1c7709f2d23b8539a7bb26f2dbf61e6bb58cd84c02e2409b6aed6ed2f25  ytdlp-cli-v1.3-windows-portable.zip
```

The bundled yt-dlp is the official 2026.08.19 release and matches the SHA-256 sums published with it. The Windows ffmpeg tarball is the gyan.dev 8.1.2 full build and the Linux one is the BtbN n8.1.3 build; both matched their providers' published checksums at build time.

## Licensing

The menu, the launchers and the defaults are **MIT**. See `LICENSE` in the bundle.

yt-dlp, which does all the downloading, is **Unlicense**, a public domain dedication. It ships its own `THIRD_PARTY_LICENSES.txt` listing every library inside it.

The portable bundles also contain **ffmpeg**, built `--enable-gpl --enable-version3`, so it is distributed under the GNU General Public License version 2 or later and includes components under GPL version 3 or later. Both license texts ship in those bundles as `GPL-2.0.txt` and `GPL-3.0.txt`, and `THIRD-PARTY-NOTICES.md` points at the corresponding source for each build.

ffmpeg is free software and you may use, study, modify and redistribute it under the terms of the GPL, exactly as its authors intend. Nothing here adds to, limits or restricts the rights the GPL grants you for it.

See [README.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader) for the full behaviour, and [CHANGELOG.md](https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/blob/main/CHANGELOG.md) for the history.
