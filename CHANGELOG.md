changelog
=========

All entries 2026-10-12. Version 1.0.

initial - 181d9bf
- git repo for ytdlp-youtube-proj; docs and release zips tracked (103.5MB total)
- src/ytdlp.py: Python 3 stdlib-only wrapper, argv builder, --selftest
- config/settings.json saved defaults; one-shot quick-arg overrides
- run-ytdlp.bat / run-ytdlp.sh launchers, README, .gitignore

fix - 2dc66b4
- menu renderer: row width off by two, string-format precedence crash on the
  header line, blank rows printed as tuples

menu rework - e58348e, 131ddd4, 7e17d59, e7de4fe, 2c9a754, 21c54a9,
              ac58613, d2c20a0, 54a63ec, cca7605
- single input field: paste a URL, enter, download starts; the old paste-url
  menu entry removed
- amber hints above the input field: "enter number to change default
  settings", "paste url, press enter and download will start.", then grey
  "change download location by choosing menu item: 5" and
  "DL location: <full path>"; blue dotted divider only below the block
- header: centered amber title between light-blue dotted lines and blue side
  markers; box corners plain "+" removed
- item 1 "change video/audio": video / video no audio / audio only, where
  "video no audio" maps to -f bv*[height<=N] with no merge
- filename template presets: title, title date, title date time, title id,
  plus free text; shipped default %(title)s
- menu items 1-11 light grey including numbers, values uncoloured
- selection box in submenus: amber "[x]" when chosen (brackets and cross),
  plain "[ ]" when not
- blue centered "press escape to exit submenu" row under item 12, no blank
  row under it
- item 5 value always shows "...\<folder name>"; native folder dialog first
  (SHBrowseForFolder / tkinter / zenity / kdialog), typed path as fallback
- escape exits submenus back to the main menu and keeps the single input
  line at the main prompt; menu repaints onto a cleared screen (VT enabled on
  windows consoles)
- ffmpeg.exe and ffprobe.exe bundled in bin/ next to yt-dlp.exe, so nothing
  depends on PATH for merging or converting
- selftest grew from 25 to 28 checks (video-no-audio branches); as of the
  last run: 28 checks, 0 failed

console window - implemented in 86c9856, reverted same day per user call
- ctypes resize to 90x32 at menu start worked in a classic conhost probe
  (buffer=120x9001 window=120x30 -> buffer=90x9001 window=90x32), but did
  nothing in the user's bat-launched shell, so the code path is not
  dependable across console frontends. removed; no console sizing in the CLI
- selftest back to 28 checks
