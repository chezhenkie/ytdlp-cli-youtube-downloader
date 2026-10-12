changelog
=========

All entries 2026-10-12. Current released level: v1.1.

initial scaffold and renderer fix (181d9bf, 2dc66b4)
----------------------------------------------------
The project started as a git repo with the CLI skeleton, the selftest harness, the release zips and the docs tracked. One renderer fix followed: menu rows were two chars off, the header line crashed on a string-format precedence bug, and blank rows printed as tuples.

menu rework, nine commits (e58348e through 54a63ec)
--------------------------------------------------
The single input line replaced the old paste-url menu entry: typing a URL and pressing enter starts the download right away, and every other input is a menu number. The header became a centered amber title between light-blue dotted lines with blue side markers and no plus corners, while the menu frame went plain dark grey. Items 1 to 11 are light grey including their numbers, values are uncoloured, and the hints above the input field are amber ("enter number to change default settings", "paste url, press enter and download will start.") over two grey detail lines ("change download location by choosing menu item: 5" and "DL location: <full path>") with the blue dotted divider only below the block. Escape backs out of a submenu, keeps the single input line at the main prompt, and the menu repaints onto a cleared screen once; on windows consoles VT processing is enabled at menu start for the ANSI colors.

Type became three-way (video / video no audio / audio only), the filename template menu gained four presets (title, title date, title date time, title id) plus free text on top of the shipped default %(title)s, ffmpeg.exe and ffprobe.exe were bundled into bin/ next to yt-dlp.exe so nothing needs PATH for merging or converting, and the output folder gained a native picker first (SHBrowseForFolder via ctypes, tkinter, zenity or kdialog depending on platform) with a typed path as fallback; item 5 then always shows "...\<folder name>" instead of a raw path cut.

selection color (cca7605)
-------------------------
The selection box in the submenus is now amber "[x]" - brackets and cross in the same amber - when an option is chosen, and it stays plain "[ ]" when not. The unused red constant was removed.

console window resize, implemented and reverted (86c9856, 9ebe9d0)
----------------------------------------------------------------
A ctypes resize to 90x32 at menu start was added and proven in a hidden classic conhost probe (screen buffer 120x9001 with window 120x30 changed to buffer 90x9001 with window 90x32, dwMaximumWindowSize 120x86 to 90x86), but it did nothing in the bat-launched shell, so by the user's call it is gone: the CLI no longer sizes the console in any way, and the selftest is back to 28 checks, 0 failed.

verified state at v1.1
----------------------
HEAD 9ebe9d0, tree clean, selftest 28 checks 0 failed. yt-dlp 2026.08.19 runs from bin/yt-dlp.exe, ffmpeg and ffprobe resolve to bin/, and the downloads folder is empty. v1.1 is the level the user confirmed working end to end (menu, selection, folder dialog with a deep long path, hint block, amber selection box) in a bat-launched shell.
