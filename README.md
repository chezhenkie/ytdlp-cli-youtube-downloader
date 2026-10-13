ytdlp-youtube-proj
==================

Text-menu CLI around standalone yt-dlp, plus reference docs and release zips. Defaults live in config/settings.json, so everyday use is: start, paste URL, done. History in CHANGELOG.md; next-session notes live in the handoff store.

Layout
------
src/ytdlp.py         the CLI - Python 3 stdlib only, no pip installs
run-ytdlp.bat        Windows launcher (menu or quick args)
run-ytdlp.sh         Linux launcher
config/settings.json saved defaults (committed)
ytdlp-files/         yt-dlp release zips + 7-Zip installer (tracked)
ytdlp-docs/          wiki/readme references
bin/                 yt-dlp.exe + ffmpeg.exe + ffprobe.exe (gitignored)
downloads/           output (gitignored)
CHANGELOG.md         commit-grouped history

Usage
-----
run-ytdlp.bat                          menu
run-ytdlp.bat https://youtu.be/xxxx    quick download with saved defaults
run-ytdlp.bat <url> -a                 audio only (mp3)
run-ytdlp.bat <url> -f mkv -r 720      overrides, one run only
run-ytdlp.bat <url> --subs --playlist  subtitles + playlist
python src/ytdlp.py --check            resolved paths and versions
python src/ytdlp.py --selftest         built-in logic tests (no network)
python src/ytdlp.py --update           yt-dlp self-update (same as menu item 12)

Menu options
------------
1  change video/audio   video / video no audio (no audio track) / audio only
2  change container      mp4 / mkv / webm / best (type video)
3  change resolution     max height, 360p..2160p / best (not audio only)
4  change audio format   mp3 / m4a / opus / wav / best (needs ffmpeg)
5  change output folder   default downloads/; native folder dialog first (SHBrowseForFolder / tkinter / zenity / kdialog), typed path as fallback; shows as "...\<folder name>"
6  change filename template 4 standard lines to pick from, or type one
7  change subtitles     on/off + language list
8  change thumbnail     on/off
9  change metadata      on/off (ffmpeg)
10 change playlist      on/off + max items (0 = all)
11 reset defaults       back to the values in config/settings.json
12 update yt-dlp       runs `<yt-dlp> -U` on the resolved binary
13 quit

Shipped filename template is now %(title)s.

Escape backs out of a submenu. The prompt takes either a URL (download starts right away) or a number. Every change writes to config/settings.json immediately, so seeding defaults for the paste-url flow needs no menu.

A download runs below the input line with the menu always on screen, one line per stage, animated prefixes of at most three characters: amber dots for the preflight stages, light-blue chevrons for the download itself. The sub-second yt-dlp and ffmpeg checks share a single grouped line ("ready - yt-dlp v..., ffmpeg found"), then "downloading (escape to stop)" spins until the run ends while its line carries yt-dlp's own progress numbers in the palette ("94.8% of 53.55MiB at 10.59MiB/s ETA 00:00", percent in amber, sizes/speed/ETA in grey, chevron prefix light blue), and the result is a short sentence ("... done - <file> (<size>MB)"). yt-dlp's own console output is otherwise suppressed. No input is accepted while a download runs - only escape, which stops yt-dlp (and any ffmpeg child it spawned) and deletes the fresh partial files, printing ">>> downloading - stopped by escape, cleaned N partial files". The stage lines stay on screen until the next URL, a menu pick, or escape repaints the menu. Subtitles are fetched after the video in a separate best-effort pass, so a rate-limited subtitle API (YouTube answers HTTP 429) can never block the video again: the line reads "subtitles - en-orig, en" or "subtitles - not fetched (video kept)". The language list is turned into patterns ("en" matches en, en-GB, en-US and en-orig), and when node is on PATH it is passed to yt-dlp as a JS runtime, which removes the deprecation warning and restores format coverage.

Notes
-----
First run unpacks yt-dlp from ytdlp-files on Windows and Linux. No macOS zip shipped: put yt-dlp on PATH or set yt_dlp_path in config/settings.json.

Merge, mp3/m4a conversion, srt subtitle conversion and thumbnail/metadata embedding need ffmpeg; without it the CLI says so and falls back to a single best file, so downloads keep working. On this Windows box ffmpeg/ffprobe sit in bin/ next to yt-dlp.exe, PATH-independently; ffmpeg_path in settings points elsewhere if wanted. bin/ is untracked - ~480MB for the full builds here.

Quick-arg overrides are one-shot, they do not change saved defaults.
