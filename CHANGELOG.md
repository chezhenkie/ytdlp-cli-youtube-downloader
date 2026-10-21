# Changelog

## 2.5.2

### Fixes

#### All platforms

* The download info field (menu item 9) now writes the description as a `"<name>.txt"` text file. yt-dlp names its own `.description` suffix; the cli renames the file right after a successful download.

## 2.5.1

### New features

#### All platforms

* Added menu item 9 "download info field" (off by default): turning it on makes every video download also write the description below the video as a `"<name>.description"` text file next to the media. The menu now has 15 items; playlist moved to item 10, multiple urls to item 11, reset to item 12, update to item 13, auto update to item 14, quit to item 15.

## 2.5.0

### Fixes

#### All platforms

* Updated the bundled srtstripper to 1.6.4 and followed its renamed mode flags (`--timecode`, `--numbered`). The menu offers three modes now - text, timecode, numbered - and stored values keep their meaning (alpha reads as text, timecodes as timecode, numbered-only as numbered).
* The stripper's old counter-prefix mode no longer exists; numbered writes a number line per text block.
* On Windows the stripper no longer opens a console window while a download runs.

## 2.4.0

### New features

#### All platforms

* Added the srt timecode stripper (menu item 8): with subtitles enabled, every download writes a timecode-stripped copy of each subtitle file next to it. The menu row shows on/off plus the active mode; turning it on asks for the mode (alpha, timecodes, numbered, numbered-only).
* The bundles now ship the matching srtstripper binary in bin/ on every platform, so the feature works out of the box.

## 2.3.0

### New features

#### Linux (musl)

* Added a bundle for Alpine and other musl x86_64 systems: the cli built for x86_64-unknown-linux-musl, statically linked. It carries no yt-dlp and no ffmpeg (none published for musl); install the download runtime with `apk add yt-dlp`, which pulls ffmpeg and the JS runtime with it.
* The install script detects musl systems and picks the musl bundle automatically; `--lean` and `--portable` are refused there.

## 2.2.0

### New features

#### Windows

* Added a per-user setup.exe installer: installs without an admin prompt, puts `ytdlp` on PATH, adds a Start Menu entry, an optional desktop shortcut and a registered uninstaller.

#### macOS

* Replaced the portable tar bundle with a pkg installer for Apple silicon: installs to /usr/local/ytdlp-cli, puts `ytdlp` on PATH and ships an uninstall script. The pkg is unsigned; the first open is allowed once in System Settings > Privacy & Security (Open Anyway).

## 2.1.1

### Fixes

#### All platforms

* Removed the bare updater images from the release page; the in-app cli self-update now reports "no published cli build for <platform>" (yt-dlp updates are unchanged).
* Fixed the trailing period in the amber hint line.
* Menu item 12's notice now states what auto update off means: "auto update off - no startup check".

## 2.1.0

### New features

#### macOS

* Added macOS support: an arm64 portable bundle for Apple silicon.
* The macOS bundle can download ffmpeg (the shaka-project static build), since macOS ships no ffmpeg.

### Fixes

#### Windows

* Changed the exe's Windows file description to the bare "ytdlp"; Windows surfaces that field as the app name in Explorer and the Task Manager.

## 2.0.4

### New features

#### Windows

* Added the app icon to the exe: Explorer, the taskbar and terminal tabs show it.

## 2.0.3

### Fixes

#### All platforms

* Fixed a hung yt-dlp probe leaving a running yt-dlp process behind: the probe timeout now removes the whole process tree.
* Pressing 0 at the prompt now fires the quick paste at the keypress (no Enter needed); 0 then Enter still works in scripts and pipes.

## 2.0.2

### New features

#### All platforms

* Added the quick-paste row: press 0 to paste the first link from the clipboard and start the download.

## 2.0.1

### New features

#### All platforms

* The Rust build is now the shipped product; the final Python release (v1.5.0) remains on its release page.

### Fixes

#### All platforms

* Fixed the daily update check never firing at startup.
* Fixed a cancelled download (Ctrl+C) keeping the temporary work folder; every run now ends with it removed.
* Changed the Windows ffmpeg download source to the BtbN builds (the previous source was slow).

## 2.0.0

### New features

#### All platforms

* Rewrote the program in Rust: one static binary per platform, no Python and no runtime dependencies. The v1.5.0 bundles remain on the v1.5.0 page.
* Bundles assemble on their own CI runners from digest-checked payloads; every release publishes a checksums file.

## 1.5.0

### Fixes

#### All platforms

* Fixed the output folder flickering with hundreds of fragment files during HLS/DASH downloads: intermediates now live in a hidden work folder and only the final file lands in the output folder.

## 1.4.2

### Fixes

#### All platforms

* Subtitle parse misses are now reported on screen instead of failing silently.

## 1.4.1

### Fixes

#### All platforms

* Subtitles now default to on (English); resetting the menu keeps them on.

## 1.4

### New features

#### All platforms

* Added a download spinner with stable stage lines.
* Added the cli self-update, the ffmpeg download offer and the auto-update toggle.

### Fixes

#### Linux

* Fixed exec permissions in the Linux bundles.

## 1.3

### New features

#### All platforms

* First public release.
* Added a subtitle menu: the full IANA language list with typed-code validation.
* Added GPL license notices to the portable bundles.

### Fixes

#### Linux

* Fixed the launcher to work under sh and dash.

#### All platforms

* Fixed menu repaint issues.

## 1.2

### New features

#### All platforms

* Added batch downloads (menu item 9).
* The default output folder is now the desktop.

### Fixes

#### All platforms

* Fixed settings.json being rewritten on every start; it is now written only when a value changes, with LF line endings.
* Removed the thumbnail and metadata options and the bundled ffprobe.exe (unused).
* Shortened the menu labels.

## 1.1

### New features

#### All platforms

* The menu is now a single input line that accepts a url or a menu number.
* Rebuilt the navigation keys with arrow-key history.

## 1.0

### New features

#### All platforms

* Initial release: the text-menu download wrapper, the selftest harness, release zips and docs.
