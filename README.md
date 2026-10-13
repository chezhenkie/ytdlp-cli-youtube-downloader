ytdlp-youtube-proj
==================

Text-menu CLI around standalone yt-dlp, plus reference docs and release zips. Defaults live in app-python/config/settings.json, so everyday use is: start, paste URL, done. History in CHANGELOG.md; next-session notes live in the handoff store.

Layout
------
app-python/          the portable app - move it anywhere, all paths resolve inside it
  src/ytdlp.py       the CLI - Python 3 stdlib only, no pip installs
  run-ytdlp.bat      Windows launcher (menu or quick args)
  run-ytdlp.sh       Linux launcher
  config/settings.json saved defaults (committed)
  bin/               yt-dlp.exe + ffmpeg.exe (gitignored)
  downloads/         output (gitignored)
resources/           wiki/readme references and the yt-dlp master zip
scripts-tools/       spinner-demo.py, 7-Zip installer
CHANGELOG.md         commit-grouped history

Usage
-----
app-python\run-ytdlp.bat                          menu
app-python\run-ytdlp.bat https://youtu.be/xxxx    quick download with saved defaults
app-python\run-ytdlp.bat <url> -a                 audio only (mp3)
app-python\run-ytdlp.bat <url> -f mkv -r 720      overrides, one run only
app-python\run-ytdlp.bat <url> --subs --playlist  subtitles + playlist
python app-python\src\ytdlp.py --check            resolved paths and versions
python app-python\src\ytdlp.py --selftest         built-in logic tests (no network)
python app-python\src\ytdlp.py --update           yt-dlp self-update (same as menu item 12)

Menu options
------------
1  change video/audio   video / video no audio (no audio track) / audio only
2  change container      mp4 / mkv / webm / best (type video)
3  change resolution     max height, 360p..2160p / best (not audio only)
4  change audio format   mp3 / m4a / opus / wav / best (needs ffmpeg)
5  change output folder   default downloads/; native folder dialog first (SHBrowseForFolder / tkinter / zenity / kdialog), typed path as fallback; shows as "...\<folder name>"
6  change filename template 4 standard lines to pick from, or type one
7  change subtitles     on/off + language list
 8 change playlist      on/off + max items (0 = all)
 9 reset defaults       back to the values in config/settings.json
10 update yt-dlp       runs `<yt-dlp> -U` on the resolved binary
11 quit

Shipped filename template is now %(title)s.

Escape backs out of a submenu. The prompt takes either a URL (download starts right away) or a number. Up and down arrows recall previously submitted entries (urls and menu numbers alike, newest first), the half-typed line comes back as the draft when walking back down, and left/right arrows are swallowed - arrow keys never put characters into the line. Every change writes to config/settings.json immediately, so seeding defaults for the paste-url flow needs no menu.

A download runs below the input line with the menu always on screen, one line per stage, animated prefixes of at most three characters: amber dots for the preflight stages, light-blue chevrons for the download itself. The sub-second yt-dlp and ffmpeg checks share a single grouped line ("ready - yt-dlp v..., ffmpeg found"), then "downloading (escape to stop)" spins until the run ends while its line carries yt-dlp's own progress numbers in the palette ("94.8% of 53.55MiB at 10.59MiB/s ETA 00:00": percent amber, file size mint green, speed light blue, ETA desaturated purple, plain words grey, chevron prefix light blue), and the result is a short sentence ("... done - <file> (<size>MB)"). yt-dlp's own console output is otherwise suppressed. No input is accepted while a download runs - only escape, which stops yt-dlp (and any ffmpeg child it spawned) and deletes the fresh partial files, printing ">>> downloading - stopped by escape, cleaned N partial files". The stage lines stay on screen until the next URL, a menu pick, or escape repaints the menu. Subtitles are fetched after the video in a separate best-effort pass, so a rate-limited subtitle API (YouTube answers HTTP 429) can never block the video again: the line reads "subtitles - en-orig, en", "subtitles - none available", or "subtitles - not fetched (video kept). unable to download subtitles" followed by a second short line with the reason when it is recognizable (for example "subtitle error - the subtitle api answered 429 too many requests"). The language list is turned into patterns ("en" matches en, en-GB, en-US and en-orig), and when node is on PATH it is passed to yt-dlp as a JS runtime, which removes the deprecation warning and restores format coverage.

Notes
-----
yt-dlp.exe and ffmpeg.exe ship in app-python/bin next to the launchers, so bin/ travels with the portable unit. The release zips are deleted and the old first-run unpack path is removed: on a machine without a usable binary in bin/, put yt-dlp on PATH or set yt_dlp_path in config/settings.json. app-python/ is the portable unit: run-ytdlp.bat resolves src/ytdlp.py through %~dp0 and the CLI resolves config/ and bin/ from its own __file__ (ROOT = app-python), so the folder works from any path on any drive.

Merge, mp3/m4a conversion and srt subtitle conversion need ffmpeg; without it the CLI says so and falls back to a single best file, so downloads keep working. ffmpeg sits in bin/ next to yt-dlp.exe, PATH-independently; ffmpeg_path in settings points elsewhere if wanted. bin/ is untracked - ~248MB (yt-dlp.exe 17.01MB + ffmpeg.exe 231.26MB, sizes from Get-ChildItem). ffprobe.exe (231.07MB) lives in scripts-tools/ for dev work, not in the product: the CLI never invokes it (zero references in src/ytdlp.py), and the pinned yt-dlp source (resources/yt-dlp-master.zip, 1128 py files scanned, 15 ffprobe mentions) only reaches it through fallback paths - get_audio_codec parses `ffmpeg -i` stderr when no probe is available (postprocessor/ffmpeg.py:240-243), check_version hard-fails only on missing ffmpeg (ffmpeg.py:223-225), so merge, audio extraction and srt conversion never need it. With the thumbnail and metadata options dropped (fe9181b), no menu item reaches those ffmpeg+ffprobe fallback paths (embedthumbnail.py:176-195 method 3, get_metadata_object) at all - ffprobe.exe in scripts-tools/ is a dev extra with no consumer left in the product.

Quick-arg overrides are one-shot, they do not change saved defaults.

Running the CLI directly (python app-python/src/ytdlp.py) uses the same defaults and resolves config/ and bin/ next to the script, so it works from any working directory and any copy of the folder.
