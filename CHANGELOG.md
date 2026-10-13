changelog
=========

Newest first. Current level: v1.3.

## v1.3

- version bump to v1.3
Feature complete and user tested: the subtitle language menu is rebuilt end to end and every submenu message now survives the repaint. VERSION reads 1.3 in the menu header, the changelog is chaptered from v1.3, and the repository is prepared for public release (MIT LICENSE, .gitattributes, README credits). Selftest 95 checks. Live defaults moved to 1080p.

- subtitles submenu: every submenu message survives the repaint (0743cab)
The menu clears and re-renders the instant a submenu returns, so a line printed after that submenu's last read is wiped in the same instant. Item 7 got a hold for this earlier; four more places still had it, and all are fixed: pick_list printed "invalid number" and "out of range" (items 1 to 4), item 8 printed "not a number - kept", and item 10 printed "defaults restored", which was never visible at all because it has no read in front of it. hold() is now the single idiom and item 7 uses it too. Selftest pins every call site including that a valid pick does not hold, since the bug bit twice live.

- subtitles submenu: block under the input, typing on its own line (b7a4890)
The ten grey code lines now sit under the input with the caret parked on a dedicated typing row, so the list is on screen while the value is being entered and nothing is printed below it. The first attempt at this moved the caret up but left it at column 0, so typing overwrote the label; giving the value its own line removes the column arithmetic entirely. On enter the typing row is rewritten as the amber verdict and the block is repainted under it. block_under_input() gates the layout on colour, a real console and enough room, because a block wider than the window would wrap and a taller one would scroll the typing row off the top, and counting rows back up would then land on the wrong line; without room the block falls back above the input where it is always correct. Verified cursor-up counts [11, 1, 11] for a 10-line block.

- repo prepared for public release (0743cab, 5abe417, b7a4890)
MIT LICENSE added with the holder westercoder, and a .gitattributes forcing LF everywhere with binaries marked. The attributes matter: core.autocrlf rewrites tracked files to CRLF on checkout, which is the documented cause of the regex-anchor and trailing-space breakage in this codebase and of git reporting the whole tree as modified. README now credits and links yt-dlp (Unlicense) and states that the ffmpeg build in bin/ is 8.1.2 "full build" from gyan.dev, configured --enable-gpl --enable-version3 and therefore GPL-2.0-or-later, so a bundle containing it carries GPL redistribution obligations - the full-versus-essentials question parked for the release step now has a concrete answer. Audited and clean: one author (westercoder <westercoder@local>, non-routable, identifies nobody), no machine paths, no remote, seven tracked files, app-rust/ empty on disk.

- subtitles submenu: the code block is the last thing on screen (5daf8e2)
Item 7 painted the ten grey code lines first and left the prompt as the final line. The order is now the input line, the amber verdict, then the block: nothing under it, no amber below it, no trailing prompt. notice() was only ever used by item 7 and is removed rather than left dead. This entry is superseded by b7a4890, which puts the block under the input so the list is visible while typing; recorded because the check() fix below came out of this work. Separately check() was fixed: it raised TypeError instead of reporting any failure whose value was a tuple, hiding three wrong assertions behind one crash. Selftest 87 checks.

- subtitles menu: the whole language list, 191 codes in ten lines (5abe417)
Item 7 printed a handpicked 32 codes and validated against 74 base codes scraped from the pinned yt-dlp source, so ordinary languages like af, ta, sw or zu still drew the "not in the youtube list" note for no reason. Both come from one set now: the 190 two-letter primary subtags of the IANA BCP 47 language subtag registry (fetched 2026-10-13), which is ISO 639-1 plus the grandfathered aliases youtube still serves (in, iw, ji, mo, sh), plus fil, which has no two-letter form and came from the pinned yt-dlp _SUPPORTED_LANG_CODES. The submenu prints the whole set alphabetically, filled to the 64 char box in ten grey lines. It is still a reference and not a gate: subtitle codes arrive from captionTracks[].languageCode per video and youtube publishes no full list, so a well formed code outside the set (yue, nan, cmn) is kept with a note. Selftest 85 checks.

- submenu messages hold the screen instead of vanishing into the repaint (f62a5cb)
The main loop clears the screen and re-renders the menu the moment a submenu returns, so anything printed after that submenu's last read was wiped in the same instant. Caught live: typing yue saved correctly but its "not in the youtube list" notice flashed and disappeared before it could be read. Two probes disagreed on this and the probe was wrong: driving act() in isolation never reaches the repaint that eats the output, which is why the message looked present in a test and absent on screen. This entry's notice() helper was later removed as dead code; the fix itself was kept and generalised in 0743cab.

- subtitles menu: typed language codes are checked against the youtube list (bcee30b)
The item 7 prompt took anything and saved it verbatim, which is how settings.json ended up holding the single character 8: it turned into --sub-langs 8(-.*)?, matched no subtitle track, and the menu still read "on". Input is now split and judged: a bare digit or junk is dropped and named in amber, a well formed code the reference list does not carry is kept and named in amber, case and spacing are cleaned up, and wildcards pass through untouched. If nothing valid is left the old value stands, so a mistyped line can never blank the setting. The reference started as the 74 base codes of the pinned yt-dlp _SUPPORTED_LANG_CODES and was later replaced by the IANA set (5abe417). Selftest 80 checks.

- subtitles menu: grey reference line of language codes (13e4888)
Choosing item 7 and switching subtitles on now prints the common codes: europe first with the baltics in it, then ru zh ko ja id th vi, space separated and filled to the 64 char box. Dark grey 244, the tone the hint block already uses. The prompt changed from "languages (comma list, ...)" to "enter languages separated by commas (enter = keep)". ch is written zh: ch is the country code for Switzerland and no subtitle track uses it, so typing it would match nothing. Selftest 73 checks.

## v1.2

- scripts-tools untracked and ignored (5e7c7d8)
The dev folder holds no product code: spinner-demo.py (the standalone animation picker) and 7-Zip are on disk only now, added to .gitignore and removed with git rm --cached. The files are untouched on disk, the repo drops from nine tracked files to seven, and a fresh clone no longer receives the folder. Note the consequence: the standalone spinner-demo fix is disk-only and does not travel with a clone.

- settings.json stays LF and is only written when it changes (a50d0b5)
Windows default text mode translated every save to CRLF while the tracked file is LF, so git reported the user's settings as modified after every run and no-op runs touched their file. save_settings now opens with newline LF and returns early when the bytes are unchanged. Fresh install subtitle language is en, not en,nl.

- changelog formatting (1347104, 056077a)
Rules reduced to separators between versions, ## on the version numbers, a plain dash on each entry.

- version bump to v1.2 (2f036a8)
VERSION in the menu header, the changelog chaptered by version, every cited hash re-verified against the live history.

- add multiple youtube urls, menu item 9 (aba619f, f5c6b88)
New menu entry. One url per line: paste, enter, paste the next; an empty line is the enter that ends the list and starts the downloads, escape returns to the menu at any point. The list redraws on every paste with the menu on top, so the input line never scrolls away. An entry with no http(s) link stays in the list as "error in url" and is skipped when the downloads run, so one bad paste never stops the others. Downloads run one at a time in paste order, no concurrency, and escape during a run stops that run and the rest of the batch. act() now returns whether the main menu must repaint, which lets the last run's result lines stay on screen after a batch. The row sits directly after playlist on/off; reset defaults, update yt-dlp cli and quit moved down one each. The menu rows moved into menu_items() so they can be asserted in the selftest. Selftest 51 to 64 checks.

- spinner-demo.py is standalone (829ceaa)
The VT enable is inlined, so the demo imports nothing from the project and runs from any folder. It used to load ytdlp.py from its own directory, which broke when the file moved to scripts-tools/ (FileNotFoundError on the sibling path).

- changelog rewritten (5fe7e0b)
Entries newest first, cut from 32865 to about 9300 bytes: no dates, no dead pre-purge hashes, no narrative of who asked for what.

- second history purge: release zips and resources blobs gone (0f7c8ba)
git filter-repo --invert-paths over ytdlp-files/, resources/ and ytdlp-docs/ removed the three deleted release zips (94.23MB) and the seven untracked reference files (10.39MB) from every commit. The pack went from 103.60MiB to 1.64MiB, the tracked file list is unchanged, scripts-tools/7zip-2602-x64.exe was deliberately kept. Every hash named in entries below predates both purges and no longer resolves.

- failed runs print yt-dlp's error line in full (1c32f13, 791397c)
fail_sentence(rc, err) replaces the width-cut sentence, so a failure prints the rc plus yt-dlp's whole error line (a rename blocked by another process keeps the clause naming the holder).

- history purged: the 93MB stray mp4 is gone, all hashes rewritten (791397c)
git filter-repo --invert-paths (2.47.0) dropped one stray test download, "Making Next Gadget ... ESP32-C5 [7GkmNRP8yiQ].mp4" 93708417 bytes, that had been committed and deleted from the working tree but rode in every clone. The repository had never been pushed, so no remote kept a copy. The pack went from 191.1MB of loose objects to 103.60MiB.

- output folder defaults to the OS desktop, falls back to the OS download folder (abc42cc)
auto_outdir() replaces the shipped machine path: windows Shell Folders Desktop, then the Downloads folder GUID {374DE290-123F-4565-9164-39C4925E467B}; unix XDG_DESKTOP_DIR then XDG_DOWNLOAD_DIR; then ~/Desktop and ~/Downloads on both; every candidate gated on isdir. Nothing found resolves to downloads/ under the project root. DEFAULTS ships output_dir "". Selftest 45 to 48 checks.

- menu labels shortened, hint lines swapped (680a5a6, 67753b2)
Labels dropped the "change" prefix: 1 video/audio, 2 format container, 3 video resolution, 4 audio format, 5 output folder, 6 filename template, 7 subtitles, 8 playlist on/off, 9 reset defaults, 10 update yt-dlp cli. The two amber hint lines swapped places, so "paste url, press enter and download will start." sits above "enter number to change default settings".

- resources folder untracked (ac614e5, 7c0cf4c)
resources/ (seven reference files, 10.39MB, including the pinned yt-dlp-master.zip) is gitignored and untracked, with the files kept on disk.

- delete ffprobe.exe (ac614e5, d17dffc)
No menu item reaches the embedthumbnail and get_metadata_object fallbacks that hard-require ffprobe, so the 231.07MB binary left scripts-tools/. Grep over the CLI: zero references.

- drop the thumbnail and metadata options (69180e4, 29c4750)
Removed both settings keys, both build_args flag blocks, both menu rows, both act() handlers and the stale keys from config/settings.json. Playlist became 8, reset defaults 9, update 10, quit 11.

- move ffprobe.exe to scripts-tools (ab95dbe, dc283c5)
ffprobe moved out of the product folder bin/ into scripts-tools/ and was added to .gitignore.

- remove the dead yt-dlp unpack path (4b2dd2e, aa3f04a)
FILES_DIR, PLATFORM_ZIP and extract_yt_dlp are gone with the ytdlp-files/ folder. bin/ ships windows builds only; elsewhere yt-dlp comes from PATH or yt_dlp_path.

- reorg into app-python, resources, scripts-tools (896ec99, f1ae876)
The app is one portable unit: run-ytdlp.bat, run-ytdlp.sh, src/ytdlp.py, config/settings.json and bin/ under app-python/. ROOT derives from __file__, so the folder works from any path on any drive. Reference files moved to resources/, dev tools to scripts-tools/.

- navy background reverted (5af4cb3, 7dd61f2)
"color 1F" in the bat and the OSC 11 sequence in the sh were removed: the shell background shone through wherever characters were, painting solid blocks over the CLI's own cells. Console appearance belongs to the terminal profile, not the launcher.

- navy background in both launchers (4a437f6, 681c878)
run-ytdlp.bat gained "color 1F", run-ytdlp.sh an OSC 11 sequence for #141f3d. Added, then reverted in the next entry.

- subtitle reason line: classify the full error (e5c5b9f, 540e021)
sub_fail_reason(err) classifies the whole error line (429, timeout, network) and the caller prints a second short reason line when it recognizes one.

- subtitle failure line split into two short lines (f62207c, b12506c)
The frozen subtitle stage line carries a short sentence only; the yt-dlp error text moved to its own line so the stage line stops truncating mid-word.

- arrow keys: no garble, up/down recall previous entries (e4e8f9e, c49e877)
On windows getwch returns an extended key as 0xe0 plus a scan code, and the 0xe0 byte was falling into the buffer and being echoed as a cp1252 glyph. read_esc now eats the 0x00 and 0xe0 pairs. The reader was rebuilt on LineEdit: echo, backspace, and history capped at 100 entries with the half-typed line restored as the draft when walking down past the newest. Left/right arrows are consumed without effect. On posix ESC [ A and ESC [ B are separated from a lone escape by _esc_more and _csi_final.

- colored progress numbers, four numbers four colors (bb5a81c)
color_progress() colors the [download] body: percent amber 214, file size mint 121, speed light blue 117, ETA purple 176, plain words grey 250. The stage line carries yt-dlp's own numbers instead of raw console output.

- subtitle 429 fix, progress line, lang patterns (8a67691, 5c62d14)
Subtitles left the video run: YouTube answers the subtitle API with HTTP 429 and that error aborted the run before any video byte landed. build_sub_args runs a separate best-effort pass after the video, and a subtitle failure can no longer cost the download. sub_lang_patterns turns "en,nl" into "en(-.*)?,nl(-.*)?" so en, en-GB, en-US and en-orig all match. node on PATH is passed as a JS runtime, which removes the deprecation warning and restores format coverage.

- staged download flow, spinner demo (c3c4751, 8cdeef1, efdc930, b65124e, f28a2f8)
One line per stage with animated prefixes capped at three characters: amber dots for preflight, light blue chevrons for the download. The yt-dlp and ffmpeg checks share one grouped line. Stage lines freeze when the stage ends and are never cleared by the CLI. The menu stays on screen, the input line sits under it, results sit under the input, and no input is accepted while a download runs except escape: run_download polls a KeyWatch, kills the process tree (taskkill /F /T on windows, killpg with start_new_session on posix) and clean_partials deletes the fresh *.part* and *.ytdl leftovers. newest_since() picks the finished file for the result line. Five 4-char animations in the demo, and it runs headless by printing the frames when stdout is not a tty.

- update option added (0b2ea3a)
Menu item 10 and --update run `<resolved yt-dlp> -U` on whatever find_yt_dlp picked (bin/, PATH, yt_dlp_path) and print the version before and after.

------------------------------------------------------------------

## v1.1

- console window resize, implemented and reverted (91b01cc, 97537cc)
A ctypes resize to 90x32 at menu start worked in a probe and did nothing in the bat-launched shell, so it was reverted. The CLI never sizes the console.

- selection color (3a06803)
The submenu selection box is amber "[x]" when chosen, plain "[ ]" when not.

- docs: changelog, readme and handoff refresh (12ae506)
Docs pass in the menu rework era; the handoff store note came with it.

- menu rework, nine commits (5f27ed6, 6034cc0, 67879ed, 66c4080, c58588e, f73d898, 06c76e2, 61850f9, a9c48bb)
The single input line replaced the paste-url menu entry: a url starts the download, any other input is a menu number. The header became a centered amber title between light blue dotted lines, the frame plain dark grey. Items are light grey, values uncoloured, the hint block amber over grey detail lines with the blue divider below. Escape backs out of a submenu, and VT processing is enabled at menu start on windows consoles. Type became three-way (video / video no audio / audio only), the filename template gained four presets plus free text, and the output folder gained a native picker (SHBrowseForFolder, tkinter, zenity or kdialog) with a typed path as fallback.

- release v1.1 (ea0ccb9)
Version bump plus changelog and readme.

------------------------------------------------------------------

## v1.0

- initial scaffold and renderer fix (2398b76, 8c78a94)
The CLI skeleton, the selftest harness, the release zips and the docs went in as a git repo. Followed by a renderer fix: menu rows were two chars off, the header line crashed on a string-format precedence bug, and blank rows printed as tuples.