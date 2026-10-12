ytdlp-youtube-proj
==================

Text-menu CLI around standalone yt-dlp, plus reference docs and release zips.
Defaults live in config/settings.json, so everyday use is: start, paste URL, done.

Layout
------
src/ytdlp.py         the CLI - Python 3 stdlib only, no pip installs
run-ytdlp.bat        Windows launcher (menu or quick args)
run-ytdlp.sh         Linux launcher
config/settings.json saved defaults (committed)
ytdlp-files/         yt-dlp release zips + 7-Zip installer (tracked)
ytdlp-docs/          wiki/readme references
bin/                 unpacked yt-dlp binary (auto, gitignored)
downloads/           output (gitignored)

Usage
-----
run-ytdlp.bat                          menu
run-ytdlp.bat https://youtu.be/xxxx    quick download with saved defaults
run-ytdlp.bat <url> -a                 audio only (mp3)
run-ytdlp.bat <url> -f mkv -r 720      overrides, one run only
run-ytdlp.bat <url> --subs --playlist  subtitles + playlist
python src/ytdlp.py --check            resolved paths and versions
python src/ytdlp.py --selftest         built-in logic tests (no network)

Menu options
------------
1  Paste URL and download - runs with the values shown below
2  Type              video / audio-only
3  Container         mp4 / mkv / webm / best
4  Resolution        max height, 360p..2160p / best
5  Audio format      mp3 / m4a / opus / wav / best (audio-only)
6  Output folder     default downloads/
7  Filename template default "%(title)s [%(id)s].%(ext)s"
8  Subtitles         on/off + language list
9  Thumbnail         embed (ffmpeg) or sidecar file
10 Embed metadata     on/off (ffmpeg)
11 Playlist          on/off + max items (0 = all)
12 Reset defaults    back to this file's values
13 Quit

Every change writes to config/settings.json immediately. Edit that file to
seed defaults for the paste-URL flow, no menu needed.

Notes
-----
First run unpacks yt-dlp from ytdlp-files on Windows and Linux. No macOS zip
shipped: put yt-dlp on PATH or set yt_dlp_path in config/settings.json.
Merge, mp3/m4a conversion, subtitle convert, thumbnail/metadata embedding need
ffmpeg. If ffmpeg is missing the CLI says so and falls back to a single best
file, so downloads keep working. ffmpeg_path can point at a binary or folder.
Quick-arg overrides are one-shot, they do not change saved defaults.
