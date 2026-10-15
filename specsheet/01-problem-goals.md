## 1. Problem, goals, non-goals

Problem: downloading a youtube video with yt-dlp means remembering flags. Users want to paste a url, press enter, and have a download run with sensible saved defaults, changeable without flags.

Goal: a text-menu CLI that owns the menu, the launchers, the defaults and the process plumbing, and delegates every byte of downloading to yt-dlp. Everyday use is: start, paste url, done. Non-affiliation statement is published verbatim in README.md line 1.

Non-goal: this project is not a download engine, a youtube client, or affiliated with or endorsed by the yt-dlp project (README.md line 1). Thumbnail and metadata options were deliberately dropped (CHANGELOG v1.2 "drop the thumbnail and metadata options"), which also removed the last ffprobe consumers; the product tree has no ffprobe (CHANGELOG v1.2 "delete ffprobe.exe"). No macOS ffmpeg bundle: gyan.dev publishes Windows binaries only and no free static macOS build exists (THIRD-PARTY-NOTICES.md lines 37-38; ytdlp.py L380-L386), so macOS gets install instructions (install_ffmpeg L1455-L1459, offer_ffmpeg L1534-L1537).

