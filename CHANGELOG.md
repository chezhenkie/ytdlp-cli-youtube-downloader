changelog
=========

Newest first. Current level: v1.4.1.

## v1.4.1

- subtitle default: on with en (e0eb55c)
The code default said subtitles false while the shipped settings.json said true, so menu 10 (reset defaults) answered the reset by turning subtitles off - a surprise nobody asked for. DEFAULTS now carries true with langs "en", so the code default and the shipped file agree, a fresh copy seeds subtitles on, and reset keeps them on. The selftest stayed green at 145 checks through the change.

- changelog chapter for v1.3 restored, stale byte pin removed (e0eb55c)
The "## v1.3" heading had been lost when the v1.4 chapter was written on top of it, leaving every v1.3 entry sitting inside the v1.4 region; the chapters are separate again and the current-level line reads v1.4.1. The comment at the CLI-update source block carried a pinned byte count (73630) that no longer matched the file it described (108022 measured on the same day); the comment now describes the one-GET design with no count in it.

- design specsheet added as a repo document (963268b, 8065113)
ytdlp-cli-specsheet.md documents the product end to end: header with status and owners, problem, goals and non-goals, 15 functional and 8 non-functional requirements, architecture, interfaces, user flows, the settings schema as a data table, a verification map from every requirement to its selftest checks, phases and gates, an open-questions register and a decision history. Built from code evidence with every number source-tagged, written and reviewed per the project's design-and-review manual, approved by the owner.

- v1.4.1 published (e0eb55c, 90eed17)
Tag on the changelog tip with four assets rebuilt from the live tree and verified at the destination before upload: all four bundles were extracted on their target platforms first - selftest 145 checks 0 failed from the extracted trees on windows and linux, executable modes at 0755, the yt-dlp zipapp and ffmpeg probed by version, launcher smoke rerun. Sizes at destination: windows lean 17665290, windows portable 57581242, linux lean 2971881, linux portable 72824533; the README asset table and checksum block switched to v1.4.1 and v1.4 stays published below it. The bundles carry the same platform binaries as v1.4, hash-verified inside the build, plus the rebuilt source file.

## v1.4

- random download spinner, the stage line no longer leaves trails
The downloading stage swapped its chevron prefix for one of eight waiting animations (comet, pulse, orbit, scanner, twinkle, eq, fill, bounce), ported from scripts-tools/spinner-demo.py as six-cell frames that can mix colors within one frame, each with its own tick and its own frozen marker; a new animation is picked at random per download run, and it keeps spinning the whole way through - while yt-dlp's progress numbers show the frames keep rotating beside them, and the frozen marker only appears on the finished line. The progress numbers are width-stable too: every number truncates to one decimal (yt-dlp emits two, 198.15MiB -> 198.1MiB) and the speed keeps a zero-padded three digit integer part - 009.3MiB/s at 9 MiB/s, 011.0MiB/s at 11 - so the digit-crossing jumps the user reported are gone and the field sits at exactly the ten characters yt-dlp itself pads its speed to (pinned source common.py format_speed). The same change fixed the messy stage line: the spinner advanced the cursor without erasing it, so a shorter progress value left the tail of the longer one behind and the frozen verdict shared its line with the last progress text. Erase to end of line now runs before every rewrite, a write lock serializes the spinner thread's frames against the final freeze so a stop can no longer interleave them, and the preflight, subtitle and ffmpeg stages keep their amber dots but gained the same erase. The first cut shipped a real crash: dot_stage passed the ANSI string A_AMBER where render_cells expects a numeric palette index, so the preflight stage raised TypeError on every colored run; fixed by passing 214 and pinned by a render check that would have caught it, because the original checks compared frame lists for equality and were blind to the color type. Selftest 141 checks 0 failed; the new ones pin the spinner shapes, the color-run render and the plain freeze helpers.

- fix: linux bundles shipped without executable permissions (e3869b8 follow-up)
The v1.4 linux bundles shipped every file with mode 0o666, so bin/yt-dlp, bin/ffmpeg and run-ytdlp.sh all refused to run with Permission denied on a fresh extract. The cause was the build itself, not the platform: it called os.chmod(path, 0o755) and relied on the filesystem mode, but os.chmod cannot set the executable bit on Windows - python maps it to the read-only flag and drops the rest - so by the time tarfile.add ran there was no mode left to record, and the archive defaulted to 0o666 for every member. v1.3 shipped 0o777 and worked. The build now writes the mode into the tarinfo instead of the filesystem, which is the only thing an extractor reads, and re-verifies the written archive: non_exec_members must be 0 before the artifact is accepted. This is a general rule for any archive built on Windows for another platform - the filesystem mode of the build machine is irrelevant to a target that never sees it. Found only by actually extracting and running the bundle on linux, which is why the release was not verified by hash and smoke test alone: both of those passed on the broken artifact.

- cli self-update, ffmpeg download offer, auto-update toggle: v1.4 (393368c)
The README promised two things the code did not do, and both are now real. It claimed an update function and auto updater for both the yt-dlp runtime and the cli, when only yt-dlp was ever updated, and it claimed that on an OS with no ffmpeg the CLI would offer to download from the official FFMPEG github repository, when no download code existed at all. The cli half is a self-update that fetches the published source from the raw main-branch file - one request of 73630 bytes, verified as 73,630 bytes and VERSION 1.3 on 2026-10-13, and neither number is hardcoded: the version is read from the same bytes that would replace the file, so a second request cannot disagree with the first. The new bytes are compiled before they are written and land beside the target first, so a truncated or syntax-broken download can never leave the CLI unbootable. The ffmpeg half downloads the published build for the platform and checks it against the provider's published sha256 before anything is installed, then requires --version to answer on this machine before the binary is moved over whatever is in bin/, so a truncated or wrong-architecture build can never replace a working ffmpeg. Windows comes from gyan.dev and Linux from BtbN; FFmpeg's own repository is source code only and publishes no binaries, so it could not be the source no matter what the README said. macos gets instructions instead of a download, because no free static build is published for it. The auto-update check runs once a day at start, only when stdin is a tty, and only prints what is newer - it asks before it replaces anything. Menu row 12 turns that on or off and row 13 is quit, so the menu grew from 12 rows to 13. There was a bug worth its own note: probe_version asked a binary with the double-dash spelling --version, which yt-dlp accepts and ffmpeg 9 rejects outright, exiting 0xABAFB008 and answering only the single-dash -version, and ffmpeg writes its banner to stderr where yt-dlp writes to stdout. A fresh download therefore failed its own self-check with a meaningless number, and the cause was invisible because probe_version threw stderr away. It now asks both spellings on both streams, and the selftest pins the case with a shim that reproduces ffmpeg 9's exact behaviour, so it cannot regress. The ffmpeg inside the portable bundles changed from the 8.1.2 full build to 9.0.2 essentials, which cut the windows portable from 109,577,908 to 56,317,844 bytes, and THIRD-PARTY-NOTICES.md now names the exact archive and its digest. The lean bundles carry no GPL texts, because there is no ffmpeg in them to license. Selftest went from 95 to 122 checks.

## v1.3

- the release assets and notes for v1.3 (dda9a50)

- first public release: v1.3 (606be03)
The launcher resolved its own folder with the bashism ${BASH_SOURCE[0]}, which is fine when the script is executed but dies with "Bad substitution" under sh, and /bin/sh is dash by default on Ubuntu, so sh run-ytdlp.sh was broken on the most common linux desktop. The POSIX form $0 is identical here because the launcher is executed and never sourced. Verified on Ubuntu with python 3.14.4 in four modes, 95 checks 0 failed each: direct execution, bash, sh, and sh from another working directory; direct execution and bash passed before the change, so only the sh form was ever affected. A selftest matrix was added at the same time, running the offline selftest and --check on windows, ubuntu and macos, and it went green on the first run on all three, which is the first time macos ever executed this code. Linux had been verified by hand before shipping and the windows artifacts by extracting the finished zip. The release is one v1.3 tag with four assets: windows lean 17,646,307 bytes and portable 109,577,908, linux lean 2,957,796 and portable 70,716,175. Linux ships the 3,072,469 byte yt-dlp zipapp because python3 is already mandatory for the menu, so the 40,446,224 byte self-contained build would add weight and buy nothing; ffmpeg for linux is the BtbN n8.1.3 gpl build at 172,328,840 bytes, chosen because gyan.dev publishes Windows binaries only and this keeps the ffmpeg line at 8.1 on both platforms. Both yt-dlp binaries and both ffmpeg tarballs matched their providers' published checksums. Every artifact carries the MIT LICENSE and the third-party notices; the portable ones add GPL-2.0.txt and GPL-3.0.txt, because both ffmpeg builds are configured --enable-gpl --enable-version3 and shipping only one of the two texts would misstate the grant.

- third history purge and the first push (67202b0)
A third git filter-repo pass (2.47.0) ran immediately before the repository was first pushed, so nothing needed force-pushing and no published copy was ever rewritten. Two things went: the scripts-tools/7zip-2602-x64.exe blob, a 1657896 byte third-party binary untracked since 5e7c7d8 that still rode in every clone, and the literal string lap1user, present in about 25 old settings.json commits and in the paragraph above describing auto_outdir. A mailmap rewrote the author and committer of all 77 commits from the codename to the GitHub noreply address, so contributions attach to the profile instead of an unroutable local domain. That changed every commit hash, so all 40 hashes cited above were re-cited from a pairing proved by commit message rather than assumed from position; one non-hash token, the 93708417 byte count, was allowlisted by hand. The pack went from 1.64MiB to 345.75KiB and the largest blob is now the 73630 byte CLI source. Verified after the rewrite and before the push: a bundle of the pre-purge history verified complete, no secret pattern in any of the 77 commits, all 9 tracked files ASCII with no CRLF, the tracked tree byte-identical to the previous tip, and selftest 95 checks 0 failed.

- version bump to v1.3 (47ecca3)
Feature complete and user tested: the subtitle language menu is rebuilt end to end and every submenu message now survives the repaint. VERSION reads 1.3 in the menu header, the changelog is chaptered from v1.3, and the repository is prepared for public release (MIT LICENSE, .gitattributes, README credits). Selftest 95 checks. Live defaults moved to 1080p.

- subtitles submenu: every submenu message survives the repaint (354a504)
The menu clears and re-renders the instant a submenu returns, so a line printed after that submenu's last read is wiped in the same instant. Item 7 got a hold for this earlier; four more places still had it, and all are fixed: pick_list printed "invalid number" and "out of range" (items 1 to 4), item 8 printed "not a number - kept", and item 10 printed "defaults restored", which was never visible at all because it has no read in front of it. hold() is now the single idiom and item 7 uses it too. Selftest pins every call site including that a valid pick does not hold, since the bug bit twice live.

- subtitles submenu: block under the input, typing on its own line (1968751)
The ten grey code lines now sit under the input with the caret parked on a dedicated typing row, so the list is on screen while the value is being entered and nothing is printed below it. The first attempt at this moved the caret up but left it at column 0, so typing overwrote the label; giving the value its own line removes the column arithmetic entirely. On enter the typing row is rewritten as the amber verdict and the block is repainted under it. block_under_input() gates the layout on colour, a real console and enough room, because a block wider than the window would wrap and a taller one would scroll the typing row off the top, and counting rows back up would then land on the wrong line; without room the block falls back above the input where it is always correct. Verified cursor-up counts [11, 1, 11] for a 10-line block.

- repo prepared for public release (47ecca3, 354a504, 48397f0, 1968751)
MIT LICENSE added with the holder chezhenkie, and a .gitattributes forcing LF everywhere with binaries marked. The attributes matter: core.autocrlf rewrites tracked files to CRLF on checkout, which is the documented cause of the regex-anchor and trailing-space breakage in this codebase and of git reporting the whole tree as modified. README now credits and links yt-dlp (Unlicense) and states that the ffmpeg build in bin/ is 8.1.2 "full build" from gyan.dev, configured --enable-gpl --enable-version3 and therefore GPL-2.0-or-later, so a bundle containing it carries GPL redistribution obligations - the full-versus-essentials question parked for the release step now has a concrete answer. Audited and clean: no machine paths in the tracked tree, no remote, nine tracked files, app-rust/ empty on disk. The author identity recorded here was rewritten before the first push; see the v1.3 entry on the third history purge.

- subtitles submenu: the code block is the last thing on screen (438031e)
Item 7 painted the ten grey code lines first and left the prompt as the final line. The order is now the input line, the amber verdict, then the block: nothing under it, no amber below it, no trailing prompt. notice() was only ever used by item 7 and is removed rather than left dead. This entry is superseded by 1968751, which puts the block under the input so the list is visible while typing; recorded because the check() fix below came out of this work. Separately check() was fixed: it raised TypeError instead of reporting any failure whose value was a tuple, hiding three wrong assertions behind one crash. Selftest 87 checks.

- subtitles menu: the whole language list, 191 codes in ten lines (48397f0)
Item 7 printed a handpicked 32 codes and validated against 74 base codes scraped from the pinned yt-dlp source, so ordinary languages like af, ta, sw or zu still drew the "not in the youtube list" note for no reason. Both come from one set now: the 190 two-letter primary subtags of the IANA BCP 47 language subtag registry (fetched 2026-10-13), which is ISO 639-1 plus the grandfathered aliases youtube still serves (in, iw, ji, mo, sh), plus fil, which has no two-letter form and came from the pinned yt-dlp _SUPPORTED_LANG_CODES. The submenu prints the whole set alphabetically, filled to the 64 char box in ten grey lines. It is still a reference and not a gate: subtitle codes arrive from captionTracks[].languageCode per video and youtube publishes no full list, so a well formed code outside the set (yue, nan, cmn) is kept with a note. Selftest 85 checks.

- submenu messages hold the screen instead of vanishing into the repaint (e070b86)
The main loop clears the screen and re-renders the menu the moment a submenu returns, so anything printed after that submenu's last read was wiped in the same instant. Caught live: typing yue saved correctly but its "not in the youtube list" notice flashed and disappeared before it could be read. Two probes disagreed on this and the probe was wrong: driving act() in isolation never reaches the repaint that eats the output, which is why the message looked present in a test and absent on screen. This entry's notice() helper was later removed as dead code; the fix itself was kept and generalised in 354a504.

- subtitles menu: typed language codes are checked against the youtube list (6296264)
The item 7 prompt took anything and saved it verbatim, which is how settings.json ended up holding the single character 8: it turned into --sub-langs 8(-.*)?, matched no subtitle track, and the menu still read "on". Input is now split and judged: a bare digit or junk is dropped and named in amber, a well formed code the reference list does not carry is kept and named in amber, case and spacing are cleaned up, and wildcards pass through untouched. If nothing valid is left the old value stands, so a mistyped line can never blank the setting. The reference started as the 74 base codes of the pinned yt-dlp _SUPPORTED_LANG_CODES and was later replaced by the IANA set (48397f0). Selftest 80 checks.

- subtitles menu: grey reference line of language codes (9af4240)
Choosing item 7 and switching subtitles on now prints the common codes: europe first with the baltics in it, then ru zh ko ja id th vi, space separated and filled to the 64 char box. Dark grey 244, the tone the hint block already uses. The prompt changed from "languages (comma list, ...)" to "enter languages separated by commas (enter = keep)". ch is written zh: ch is the country code for Switzerland and no subtitle track uses it, so typing it would match nothing. Selftest 73 checks.

## v1.2

- scripts-tools untracked and ignored (d913cdd)
The dev folder holds no product code: spinner-demo.py (the standalone animation picker) and 7-Zip are on disk only now, added to .gitignore and removed with git rm --cached. The files are untouched on disk, the repo drops from nine tracked files to seven, and a fresh clone no longer receives the folder. Note the consequence: the standalone spinner-demo fix is disk-only and does not travel with a clone.

- settings.json stays LF and is only written when it changes (0f14002)
Windows default text mode translated every save to CRLF while the tracked file is LF, so git reported the user's settings as modified after every run and no-op runs touched their file. save_settings now opens with newline LF and returns early when the bytes are unchanged. Fresh install subtitle language is en, not en,nl.

- changelog formatting (698158d, 1812209)
Rules reduced to separators between versions, ## on the version numbers, a plain dash on each entry.

- version bump to v1.2 (100e66d)
VERSION in the menu header, the changelog chaptered by version, every cited hash re-verified against the live history.

- add multiple youtube urls, menu item 9 (c162d00, 8572c8d)
New menu entry. One url per line: paste, enter, paste the next; an empty line is the enter that ends the list and starts the downloads, escape returns to the menu at any point. The list redraws on every paste with the menu on top, so the input line never scrolls away. An entry with no http(s) link stays in the list as "error in url" and is skipped when the downloads run, so one bad paste never stops the others. Downloads run one at a time in paste order, no concurrency, and escape during a run stops that run and the rest of the batch. act() now returns whether the main menu must repaint, which lets the last run's result lines stay on screen after a batch. The row sits directly after playlist on/off; reset defaults, update yt-dlp cli and quit moved down one each. The menu rows moved into menu_items() so they can be asserted in the selftest. Selftest 51 to 64 checks.

- spinner-demo.py is standalone (80c3fb3)
The VT enable is inlined, so the demo imports nothing from the project and runs from any folder. It used to load ytdlp.py from its own directory, which broke when the file moved to scripts-tools/ (FileNotFoundError on the sibling path).

- changelog rewritten (662965f)
Entries newest first, cut from 32865 to about 9300 bytes: no dates, no dead pre-purge hashes, no narrative of who asked for what.

- second history purge: release zips and resources blobs gone (9fe9037)
git filter-repo --invert-paths over ytdlp-files/, resources/ and ytdlp-docs/ removed the three deleted release zips (94.23MB) and the seven untracked reference files (10.39MB) from every commit. The pack went from 103.60MiB to 1.64MiB, the tracked file list is unchanged, scripts-tools/7zip-2602-x64.exe was kept at this point and removed by the third purge. The hashes cited below were re-cited against the rewritten history and all resolve.

- failed runs print yt-dlp's error line in full (0b3edcc, eba9a2b)
fail_sentence(rc, err) replaces the width-cut sentence, so a failure prints the rc plus yt-dlp's whole error line (a rename blocked by another process keeps the clause naming the holder).

- history purged: the 93MB stray mp4 is gone, all hashes rewritten (eba9a2b)
git filter-repo --invert-paths (2.47.0) dropped one stray test download, "Making Next Gadget ... ESP32-C5 [7GkmNRP8yiQ].mp4" 93708417 bytes, that had been committed and deleted from the working tree but rode in every clone. The repository had never been pushed, so no remote kept a copy. The pack went from 191.1MB of loose objects to 103.60MiB.

- output folder defaults to the OS desktop, falls back to the OS download folder (40d7784)
auto_outdir() replaces the shipped machine path: windows Shell Folders Desktop, then the Downloads folder GUID {374DE290-123F-4565-9164-39C4925E467B}; unix XDG_DESKTOP_DIR then XDG_DOWNLOAD_DIR; then ~/Desktop and ~/Downloads on both; every candidate gated on isdir. Nothing found resolves to downloads/ under the project root. DEFAULTS ships output_dir "". Selftest 45 to 48 checks.

- menu labels shortened, hint lines swapped (065c4df, 553018c)
Labels dropped the "change" prefix: 1 video/audio, 2 format container, 3 video resolution, 4 audio format, 5 output folder, 6 filename template, 7 subtitles, 8 playlist on/off, 9 reset defaults, 10 update yt-dlp cli. The two amber hint lines swapped places, so "paste url, press enter and download will start." sits above "enter number to change default settings".

- resources folder untracked (d65a43d, 7e8a886)
resources/ (seven reference files, 10.39MB, including the pinned yt-dlp-master.zip) is gitignored and untracked, with the files kept on disk.

- delete ffprobe.exe (d65a43d, 54241ee)
No menu item reaches the embedthumbnail and get_metadata_object fallbacks that hard-require ffprobe, so the 231.07MB binary left scripts-tools/. Grep over the CLI: zero references.

- drop the thumbnail and metadata options (d702da7, 519577c)
Removed both settings keys, both build_args flag blocks, both menu rows, both act() handlers and the stale keys from config/settings.json. Playlist became 8, reset defaults 9, update 10, quit 11.

- move ffprobe.exe to scripts-tools (1c235ef, 8b8e65a)
ffprobe moved out of the product folder bin/ into scripts-tools/ and was added to .gitignore.

- remove the dead yt-dlp unpack path (11b3378, 9f952eb)
FILES_DIR, PLATFORM_ZIP and extract_yt_dlp are gone with the ytdlp-files/ folder. bin/ ships windows builds only; elsewhere yt-dlp comes from PATH or yt_dlp_path.

- reorg into app-python, resources, scripts-tools (bf9c367, 80f5cb2)
The app is one portable unit: run-ytdlp.bat, run-ytdlp.sh, src/ytdlp.py, config/settings.json and bin/ under app-python/. ROOT derives from __file__, so the folder works from any path on any drive. Reference files moved to resources/, dev tools to scripts-tools/.

- navy background reverted (624d961, eab5957)
"color 1F" in the bat and the OSC 11 sequence in the sh were removed: the shell background shone through wherever characters were, painting solid blocks over the CLI's own cells. Console appearance belongs to the terminal profile, not the launcher.

- navy background in both launchers (11355bf, ce72ba6)
run-ytdlp.bat gained "color 1F", run-ytdlp.sh an OSC 11 sequence for #141f3d. Added, then reverted in the next entry.

- subtitle reason line: classify the full error (0df05f2, a451285)
sub_fail_reason(err) classifies the whole error line (429, timeout, network) and the caller prints a second short reason line when it recognizes one.

- subtitle failure line split into two short lines (640de3c, fd3eede)
The frozen subtitle stage line carries a short sentence only; the yt-dlp error text moved to its own line so the stage line stops truncating mid-word.

- arrow keys: no garble, up/down recall previous entries (f5b6f67, d9b1b5d)
On windows getwch returns an extended key as 0xe0 plus a scan code, and the 0xe0 byte was falling into the buffer and being echoed as a cp1252 glyph. read_esc now eats the 0x00 and 0xe0 pairs. The reader was rebuilt on LineEdit: echo, backspace, and history capped at 100 entries with the half-typed line restored as the draft when walking down past the newest. Left/right arrows are consumed without effect. On posix ESC [ A and ESC [ B are separated from a lone escape by _esc_more and _csi_final.

- colored progress numbers, four numbers four colors (e933a5a)
color_progress() colors the [download] body: percent amber 214, file size mint 121, speed light blue 117, ETA purple 176, plain words grey 250. The stage line carries yt-dlp's own numbers instead of raw console output.

- subtitle 429 fix, progress line, lang patterns (9e6a39c, b6a30f0)
Subtitles left the video run: YouTube answers the subtitle API with HTTP 429 and that error aborted the run before any video byte landed. build_sub_args runs a separate best-effort pass after the video, and a subtitle failure can no longer cost the download. sub_lang_patterns turns "en,nl" into "en(-.*)?,nl(-.*)?" so en, en-GB, en-US and en-orig all match. node on PATH is passed as a JS runtime, which removes the deprecation warning and restores format coverage.

- staged download flow, spinner demo (18ab83d, 1e9463e, 0c41002, e52c789, 0803a99)
One line per stage with animated prefixes capped at three characters: amber dots for preflight, light blue chevrons for the download. The yt-dlp and ffmpeg checks share one grouped line. Stage lines freeze when the stage ends and are never cleared by the CLI. The menu stays on screen, the input line sits under it, results sit under the input, and no input is accepted while a download runs except escape: run_download polls a KeyWatch, kills the process tree (taskkill /F /T on windows, killpg with start_new_session on posix) and clean_partials deletes the fresh *.part* and *.ytdl leftovers. newest_since() picks the finished file for the result line. Five 4-char animations in the demo, and it runs headless by printing the frames when stdout is not a tty.

- update option added (2de5c44)
Menu item 10 and --update run `<resolved yt-dlp> -U` on whatever find_yt_dlp picked (bin/, PATH, yt_dlp_path) and print the version before and after.

------------------------------------------------------------------

## v1.1

- console window resize, implemented and reverted (4f573ad, e23c726)
A ctypes resize to 90x32 at menu start worked in a probe and did nothing in the bat-launched shell, so it was reverted. The CLI never sizes the console.

- selection color (cde50c2)
The submenu selection box is amber "[x]" when chosen, plain "[ ]" when not.

- docs: changelog, readme and handoff refresh (98a6730)
Docs pass in the menu rework era; the handoff store note came with it.

- menu rework, nine commits (d07db27, d2845e1, 12e352b, f75f8a4, 9c58c47, 651c324, df9b6bf, 038068c, 1f2e256)
The single input line replaced the paste-url menu entry: a url starts the download, any other input is a menu number. The header became a centered amber title between light blue dotted lines, the frame plain dark grey. Items are light grey, values uncoloured, the hint block amber over grey detail lines with the blue divider below. Escape backs out of a submenu, and VT processing is enabled at menu start on windows consoles. Type became three-way (video / video no audio / audio only), the filename template gained four presets plus free text, and the output folder gained a native picker (SHBrowseForFolder, tkinter, zenity or kdialog) with a typed path as fallback.

- release v1.1 (5c670ec)
Version bump plus changelog and readme.

------------------------------------------------------------------

## v1.0

- initial scaffold and renderer fix (e88e27e, b4c2975)
The CLI skeleton, the selftest harness, the release zips and the docs went in as a git repo. Followed by a renderer fix: menu rows were two chars off, the header line crashed on a string-format precedence bug, and blank rows printed as tuples.