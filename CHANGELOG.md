# Changelog

## 2.11.1

### Fixes

#### All platforms

* A url list counts only the subtitle files its own pass wrote. A later link could start inside the previous item's two-second file window, so it stripped the previous subtitle file and reported the text as ready without fetching its own. The pass now compares the output folder with a snapshot taken before the run.
* `-a` without a url no longer lands in the saved settings. The flag put the transient mode on the object the menu saves, so settings.json gained a key it does not own and the `0` key downloaded audio only for the rest of the session. Command-line overrides now apply only to a run that takes a url.
* Ctrl+C ends a clipboard or batch list instead of starting the next link, and an interrupted batch url prompt ends the menu like every other prompt.

#### Windows arm64

* The zip runs without the Visual C++ redistributable. The arm64 cli was linked against the dynamic CRT, so it imported `VCRUNTIME140.dll` and the api-ms-win-crt set on a machine that carries nothing; the static CRT flag now covers both Windows targets and the bundle proof asserts the import table.
* The bundled yt-dlp updates again. Item 12 and `--update` answered "no published yt-dlp tree for windows/aarch64"; the tree updater now names `yt-dlp_win_arm64.zip` and its `yt-dlp_arm64.exe` member.

#### Linux arm64

* The bundled yt-dlp updates again: the tree updater names `yt-dlp_linux_aarch64.zip` and its launcher instead of answering "no published yt-dlp tree for linux/aarch64".

#### Documentation

* The Pages site serves the 17-node process flow the repository carries instead of the earlier 16-node render.

## 2.11.0

### Changes

#### All platforms

* The README inside every bundle describes the release it belongs to. The download table names the files by pattern (`ytdlp-cli-<ver>-windows.zip`) and points at the release notes for sizes and digests, so the same text is correct in every bundle instead of listing the previous release's files.
* The menu capture and the process-flow image now ship inside every bundle under `resources/`, so both images resolve when the README is read from the installed folder.

## 2.10.1

### Fixes

#### All platforms

* The blue notice under the box fits the box. Its first line ran to 76 columns in a 66-column frame and a terminal wrapped it onto an extra line; the notice now reads `direct downloads 's' 'a' 'v' '0' take several urls` and `from the clipboard, separated by commas or zeroes.`, 52 columns each, and the box width is asserted by the crate test and the packaged check.

## 2.10.0

### Changes

#### All platforms

* Menu item 1 asks for the mode first - `0` video with sound, `a` audio only, `v` video only, `s` the stripped summary - then takes the url list and runs every entry in that mode.
* A url line and the clipboard may carry several links. Commas, standalone `0`s and line breaks separate them; a `0` inside a link stays part of it. The four direct keys download every link on the clipboard in order, and escape stops the rest of the list.
* The box carries a blue notice under it naming the four direct keys and the list rule.
* arm64 bundles. The release adds a Windows arm64 zip, a Linux arm64 tar and a Linux arm64 musl tar. The Windows arm64 zip carries the native arm64 yt-dlp tree and srtstripper plus the x64 ffmpeg, which Windows 11 runs under emulation (the pinned provider publishes no arm64 Windows build). The Linux arm64 bundle carries native arm64 builds of all three payloads; the arm64 musl bundle is the cli plus the musl srtstripper, with yt-dlp and ffmpeg from apk. `install.sh` and the cli self-update pick the arm64 asset on an aarch64 host.

## 2.9.3

### Changes

#### All platforms

* The media mode is picked per run by a key, not stored. `0` downloads video with sound, `a` audio only, `v` video with no sound, and `s` the stripped summary; all four take the link on the clipboard. `settings.json` no longer carries `type`; a `type` left by an older build is ignored on load and dropped on the next save. One-shot `-a` still selects audio only for that run.
* The menu is 14 rows. Row 1 opens the multiple-url list and is plain white, the `video/audio` row is gone, and reset, update, auto update and quit are 11 to 14. Above row 1 sit four key rows: `s` summary in the dark amber, `a` audio only and `v` video only in the brighter amber, and `0` quick paste.
* New icon: six horizontal bands of sky, dark blue at the top to light blue at the bottom, behind the orange download mark.

## 2.9.2

### Fixes

#### All platforms

* Subtitle fallback walks the offered tracks. A selected language that YouTube refuses no longer ends with no subtitles while the video offers a servable track: after a pass writes nothing, one extraction reads the video language and the native and automatic track keys, and the cli retries the native track in the video language, the video language itself, the native tracks in the requested language, the other native tracks, and finally the code the pass already tried, at most four tries. Generated translations are not tried (they answer 429 by design).
* The walk is automatic and prints one notice per try: `subtitle fallback - video language <code>` or `subtitle fallback - offered track <code>`. No question is asked, and escape during a try still ends the run with rc 130.

## 2.9.1

### Fixes

#### All platforms

* Subtitle language fallback. A configured language that writes no subtitle file is retried once in the video's own language (`%(language)s`, primary subtag), so a video whose requested track is a refused YouTube translation still gets the track the service actually serves. The report prints `subtitle fallback - video language <code>`, and the verdict names the codes that landed. The fallback is skipped when the video language was already requested by name.
* Subtitle verdicts follow the files on disk. A pass that printed `Writing video subtitles to:` and then failed with HTTP 429 no longer reports the code as landed; the empty file is gone, so the pass reports `not fetched` and can fall back. A pass that keeps files after a nonzero exit still reports and strips them.

## 2.9.0

### Changes

#### All platforms

* One bundle per platform, ffmpeg included. The windows zip, the setup.exe and the linux glibc tar now carry yt-dlp, ffmpeg and the srtstripper, so merge, audio conversion and srt subtitles work with nothing else installed. The lean and portable variants are gone; the release drops from seven products to five.
* The on-demand ffmpeg download is removed. The cli no longer offers to fetch ffmpeg when it is missing, and no longer writes into `bin/` on a download run: it uses the bundled binary, a system ffmpeg on PATH, or the single-file fallback with one frozen preflight line.
* The bundled ffmpeg is the shaka-project/static-ffmpeg-binaries n8.1.2-1 build on every platform, digest-checked at build time against the release API's per-asset sha256. The dated BtbN pin and the gyan.dev windows pin are gone.
* The cli self-update reads the platform bundle under its new name (`-windows.zip`, `-linux.tar.gz`); the musl asset name is unchanged. A build older than v2.9.0 still asks for the retired lean asset, so v2.8.6 and older reinstall from the release page once and update through item 13 from v2.9.0 on.
* `install.sh` takes `--version` and `--uninstall`; `--portable` and `--lean` are accepted as ignored with one line on stderr.

## 2.8.6

### Fixes

#### All platforms

* Subtitle language matching selects native tracks only. A plain code like `en` matches `en`, `en-GB` and `en-US`, and no longer matches translated keys (`en-de`, `en-de-DE`) whose requests answer HTTP 429 and fail the whole subtitle pass.
* A subtitle pass that exits nonzero after writing files keeps and strips those files. The verdict lists the languages that landed and adds the classified error line, instead of reporting `subtitles - not fetched` and dropping usable subtitles.

## 2.8.5

### Fixes

#### All platforms

* Sizes at the GiB edge print in one unit: from 0.95 GiB up the stage line shows GiB with one decimal (`1.0GiB`, `1.1GiB`), so a total estimate that moves around the boundary can no longer alternate between `1.0GiB` and a four-digit MiB count.
* The progress percent keeps a high-water mark on the stage line. A revised total estimate that lowers the source percentage no longer moves the painted counter back.

## 2.8.4

### Fixes

#### Windows

* The setup.exe keeps a single PATH entry. The install-once guard compared the literal `{app}` text, so every installer run appended the directory again; the entry is now written from setup code against the expanded directory, with quotes, trailing slashes and case normalized on both sides. A reinstall keeps one entry, earlier duplicates collapse to one, and uninstall removes every copy.

## 2.8.3

### Fixes

#### All platforms

* The menu hint block carries the download-folder pointer and the resolved folder only. The paste prompt, the "enter number" line and the summary-order line are gone, so the block is two grey lines instead of five.

## 2.8.2

### Fixes

#### All platforms

* The spinner beside the download line keeps its own pace again: animation frames run on the stage tick instead of once per `[download]` chunk, so a fast transfer no longer spins the animation faster.
* The progress numbers repaint after a whole percent step, no more than four times a second, so chunk arrival cannot make the counters jump.
* Progress numbers print whole: the percentage, the downloaded size and the speed drop their decimals, and so do the ffmpeg byte counter and the saved-file size. The line reads `45% of 198MiB at 2MiB/s ETA 01:12`.

## 2.8.1

### Fixes

#### All platforms

* The shipped binaries are about a quarter smaller with unchanged startup timing: thin link-time optimization, one codegen unit, symbols stripped, overflow checks kept.
* A metadata response that would exceed 8 MiB is refused with `response exceeds the 8 MiB cap` instead of being buffered without a limit; a declared longer length is refused before anything is read.
* An unset HOME resolves the home from the passwd entry instead of leaving the output-folder candidates relative to the working directory and failing `--check`.
* `--selftest` removes the info-field section's fixture folder like every other one, so a selftest run leaves no temp folder behind.

## 2.8.0

### New features

#### All platforms

* A new `s` key row above the `0` row starts a summary grab of the clipboard link. It downloads the subtitles first and runs them through the srt stripper in text mode, so the result is plain text; only when no subtitle text can be produced does it download the sound. The menu hint block states that order, and the key also answers when typed. The stored type, playlist and stripper settings are left as they are.

## 2.7.0

### New features

#### All platforms

* Default and fallback downloads land in a `ytdlp-cli-downloads` folder inside the resolved base (Desktop, Downloads, or the program folder), so the base stays free of media files. A saved output folder is used unchanged.
* Subtitles download before the video. The subtitle pass runs first and the video starts after it; a subtitle failure is still reported and never blocks the media pass. Escape during the subtitle pass stops the download.

### Fixes

#### All platforms

* The grey hint line about downloading several urls at once is gone. Menu item 11 still takes the batch.

## 2.6.2

### Fixes

#### Windows and Linux

* The pre-install selftest gate no longer dumps its 190-line report into the update screen. The gate still runs on the staged build; only the outcome prints.

## 2.6.1

### Fixes

#### Windows and Linux

* Menu item 13 updates the cli itself. It reads the newest release, verifies the platform lean bundle against the published digest, lifts the cli out of the archive, and installs it after the new build passes `--selftest`. The musl build takes the musl bundle. macOS keeps the release page path and now says so.

#### All platforms

* The ui shows versions only. yt-dlp numbers its releases by date, so the update report prints the state (`yt-dlp updated`, `yt-dlp is current`), `--check` prints `yt-dlp check : ok`, and the preflight prints `yt-dlp, ytdlp-cli, ffmpeg are all present` when everything resolves. yt-dlp's own `-U` output is suppressed so no upstream date reaches the screen.

## 2.6.0

### New features

#### Windows, Linux and macOS

* The bundled yt-dlp payload is now the official onedir build: the launcher and its `_internal` runtime tree ship in `bin/` instead of a onefile exe that unpacked itself into the temp folder on every launch (thousands of writes per run). Downloads start without that unpack step.
* Menu item 13 updates the bundled payload as a tree: it reads the newest yt-dlp release, verifies the platform zip against the published digest, extracts it beside the launcher and swaps the binary and `_internal` in, with rollback if a step fails. An existing onefile install moves to the tree on its first update. A user-set `yt_dlp_path` or a yt-dlp found on PATH keeps `yt-dlp -U`.

## 2.5.3

### Fixes

#### All platforms

* Downloads keep their media extension in every mode. A video without audio has no merge step, so yt-dlp kept the bare output template and the file finished without a suffix; the cli now appends `"%(ext)s"` to a template that names no extension. Stored templates and the menu presets are unchanged.

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
