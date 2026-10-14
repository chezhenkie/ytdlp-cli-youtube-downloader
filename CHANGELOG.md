changelog
=========

Newest first. Current level: v1.4.2.

## v1.5.0

- v1.5.0 published (d96aec4)
Four bundles rebuilt on the v1.4.2 platform binaries, verified by extraction and selftest on target platforms (177 checks 0 failed): windows lean 17669989, windows portable 57581583, linux lean 2976745, linux portable 70577598. Digests in README.md and releases/v1.5.0/RELEASE-NOTES.md. The definitive python line before the native port; native first release renumbered v2.0.0 by owner call. Carries the change below.

- work folder keeps download churn out of the output folder (e51713b)
yt-dlp writes one file per HLS/DASH fragment, appends it to the .part file and deletes it (pinned source fragment.py _download_fragment/_append_fragment), so the output folder flickered with hundreds of files per download. The run now carries -P temp:<outdir>/.ytdlp-tmp: every intermediate (fragment files, parts, resume markers, pre-merge files) lives in that work folder and the final move is a same-volume rename. Measured for one 10-minute HLS download: 396 fragment create/delete events in the watched folder before, 1 final-file event after. The work folder is removed whenever it ends empty - on escape (after fresh partials are cleaned), on failure and after the subtitle pass; partials with resume value are kept. Selftest 169 -> 177 checks, 0 failed. Specsheet FR-7/FR-8/FR-11/NFR-3 amended first (44ce465).

## v1.4.2

- v1.4.2 published (0b6b1f3)
Four bundles rebuilt on the v1.4.1 platform binaries, verified by extraction and selftest on target platforms (169 checks 0 failed): windows lean 17668963, windows portable 57580557, linux lean 2975671, linux portable 70576586. Digests in README.md and releases/v1.4.2/RELEASE-NOTES.md. Carries the changes below.

- parse misses surfaced on screen (b98f581)
Audit F11 softened per owner call: a subtitle pass exiting 0 with no codes read prints "subtitles - none available" plus an amber note; a failed pass whose error matches no known pattern always prints "yt-dlp wording not recognized - please report". Verdict text moved into sub_success_lines(codes) and sub_fail_lines(err) so selftest pins them offline: 162 -> 169 checks. No download behavior changed.

- auto-update trigger rule pinned in README and specsheet FR-13 (45eec0a, doc only)
The daily check runs only at interactive menu start with a real tty; quick mode, --check, --selftest, CI and piped or detached runs skip it. No code change; also picked up three uncommitted specsheet reconciliation lines (NFR-4 145 -> 162 checks, module figures, verification map).

- audit fixes F1-F8, F10 (788845d specsheet, ee947fc code)
F1/F2: coerce_settings int-parses playlist_max_items and whitelists type/video_container/video_resolution/audio_format/audio_quality against the code's own vocabularies, in memory only. F3: daily update check gets the owner-pinned 6 s timeout. F4: probe_version memoized per binary path+mtime. F6: auto_outdir memoized. F7: ffmpeg directory looked up once. F8: item 12 leaves saving to the one save point in main_menu. F10: menu truncation counts east-asian width so CJK or emoji folder names stay inside the box. F9 stays closed by the pinned single-module decision. Selftest 145 -> 162 checks, 0 failed.

## v1.4.1

- subtitle default: on with en (e0eb55c)
DEFAULTS and the shipped settings.json disagreed on subtitles; both now carry true with langs "en", so reset keeps subtitles on. Selftest stayed at 145 checks.

- v1.3 chapter restored, stale byte pin removed (e0eb55c)
The v1.4 chapter had swallowed the "## v1.3" heading. The CLI-update source comment dropped a byte count that no longer matched; it now describes the one-GET design only.

- design specsheet added (963268b, 8065113)
ytdlp-cli-specsheet.md: status, problem, goals and non-goals, 15 functional and 8 non-functional requirements, architecture, interfaces, user flows, settings schema, verification map, open-questions register, decision history. Built from code evidence, owner approved.

- v1.4.1 published (e0eb55c, 90eed17)
Four bundles extracted and selftested on their target platforms (145 checks 0 failed) before upload. Sizes at destination: windows lean 17665290, windows portable 57581242, linux lean 2971881, linux portable 72824533. Same platform binaries as v1.4 plus rebuilt source.

## v1.4

- random download spinner, stable stage line
Downloading stage picks one of eight waiting animations per run (comet, pulse, orbit, scanner, twinkle, eq, fill, bounce), six-cell frames mixing colors, spinning while progress numbers show. Progress numbers truncate to one decimal and pad the speed to three digits (yt-dlp pads to ten chars). Erase-to-end-of-line before every rewrite, a write lock serializes the spinner thread against the freeze. First cut crashed: dot_stage passed an ANSI string where render_cells expects a palette index; fixed and pinned by a render check. Selftest 141 checks 0 failed.

- fix: linux bundles shipped without executable permissions (e3869b8)
v1.4 linux bundles shipped mode 0o666: os.chmod cannot set the exec bit on Windows, so tarfile recorded the default. The build now writes the mode into the tarinfo and re-verifies the archive (non_exec_members must be 0). General rule: filesystem mode on the build machine is irrelevant to the target.

- cli self-update, ffmpeg download offer, auto-update toggle (393368c)
The README promised both and neither existed. Self-update fetches the published raw main-branch source once, reads the version from the same bytes it would install, compiles before writing and lands beside the target first, so a truncated download can never break the CLI. FFmpeg offer downloads the platform build from gyan.dev (windows) or BtbN (linux), verifies the published sha256, then requires -version to answer before replacing anything in bin/. FFmpeg's own repo publishes no binaries; macos gets instructions. Auto-update checks daily at tty start, asks before replacing. Menu grew to 14 rows. Bug noted: probe_version used --version, which ffmpeg rejects with only single-dash -version working, and ffmpeg writes its banner to stderr.

## v1.3

- first public release (dda9a50, 606be03)
Launcher fixed for sh/dash ($0 instead of the bashism ${BASH_SOURCE[0]}); verified on Ubuntu in four modes, 95 checks 0 failed each. Selftest matrix added for windows, ubuntu and macos. One tag, four assets: windows lean 17,646,307 and portable 109,577,908; linux lean 2,957,796 and portable 70,716,175. Linux ships the yt-dlp zipapp instead of the self-contained build; ffmpeg for linux is the BtbN 8.1.3 gpl build. All binaries matched published checksums. Portable bundles carry GPL-2.0.txt and GPL-3.0.txt next to the MIT LICENSE because both ffmpeg builds are --enable-gpl --enable-version3.

- third history purge and first push (67202b0, 60713a7)
A third git filter-repo pass (2.47.0) ran immediately before the first push, so nothing needed force-pushing and no published copy was rewritten. It dropped the scripts-tools/7zip-2602-x64.exe blob, a 1657896 byte third-party binary untracked since 5e7c7d8 that still rode in every clone, and the string lap1user, present in about 25 old settings.json commits. A mailmap rewrote the author and committer of all 77 commits to the GitHub noreply address, which changed every commit hash, so all cited hashes were re-cited from a pairing proved by commit message rather than assumed from position. Pack 1.64MiB -> 345.75KiB. Verified after the rewrite and before the push: pre-purge bundle verified complete, no secret pattern in any of the 77 commits, all tracked files ASCII with no CRLF, tracked tree byte-identical to the previous tip, selftest 95 checks 0 failed.

- version bump to v1.3 (47ecca3)
Feature complete, chaptered changelog, release prep. Selftest 95 checks. Live defaults moved to 1080p.

- subtitles submenu repaint fixes (354a504, e070b86)
The main loop repaints the menu the instant a submenu returns, wiping anything printed after that submenu's last read. hold() became the single idiom at six call sites after "invalid number", "not a number" and "defaults restored" messages vanished live.

- subtitles submenu: block under the input (1968751)
The code list prints under the input with the caret on a dedicated typing row; block_under_input() gates on colour, real console and room, falling back above the input otherwise.

- subtitles menu: full IANA language list (48397f0)
Item 7 shows all 191 codes (190 two-letter IANA BCP 47 primary subtags plus fil) in ten grey lines, instead of a handpicked 32 checked against 74 yt-dlp codes. Reference, not a gate: well formed codes outside the set (yue, nan, cmn) are kept with a note.

- subtitles menu: typed codes validated (6296264)
Input is split and judged: junk dropped, unknown well-formed codes kept with a note, wildcards pass, nothing left means the old value stands - so a mistyped line can never blank the setting (settings.json once held the single character 8).

- subtitles menu: grey reference line (9af4240)
Item 7 prints common codes when subtitles switch on; ch renamed zh.

- repo prepared for public release (47ecca3, 354a504, 48397f0, 1968751)
MIT LICENSE, .gitattributes forcing LF everywhere (core.autocrlf rewrites break regex anchors and trailing-space matching), README credits for yt-dlp and the gyan.dev ffmpeg 8.1.2 gpl build. Audited: no machine paths, no remote, nine tracked files.

- item 7 layout and check() fix (438031e)
Code block became the last thing on screen (superseded by 1968751). check() raised TypeError on tuple assertion values, hiding real failures.

## v1.2

- scripts-tools untracked and ignored (d913cdd)
spinner-demo.py and 7-Zip removed from tracking; repo drops to seven tracked files, and the standalone spinner fix is disk-only.

- settings.json: LF and write-only-on-change (0f14002)
Windows text mode translated saves to CRLF, so git showed the file modified after every run. save_settings now writes LF and returns early on unchanged bytes. Fresh subtitle language is en.

- multiple youtube urls, menu item 9 (c162d00, 8572c8d)
One url per line, empty line starts the batch, escape backs out. Bad urls stay as "error in url" and are skipped. Downloads run one at a time in paste order; escape stops the run and the batch. Menu rows moved into menu_items() for selftest pinning. Selftest 51 -> 64 checks.

- changelog rewritten, chaptered (662965f, 698158d, 100e66d, 1812209)
Newest first, no dates or dead pre-purge hashes, 32865 -> ~9300 bytes. Dashes only between versions, version items start with a dash. Rules finalized at v1.2.

- second history purge (9fe9037)
filter-repo removed release zips and reference blobs: pack 103.60MiB -> 1.64MiB.

- first history purge (eba9a2b)
One stray committed-then-deleted 93708417 byte mp4 removed from history; pack 191.1MB loose -> 103.60MiB.

- output folder defaults to desktop (40d7784)
auto_outdir(): windows Shell Folders Desktop then Downloads GUID, unix XDG desktop then download dirs, ~/Desktop and ~/Downloads fallbacks, else downloads/ in the project. Selftest 45 -> 48 checks.

- menu labels shortened (065c4df, 553018c)
Labels dropped the "change" prefix; the two amber hint lines swapped order.

- housekeeping (d65a43d, 7e8a886, 54241ee, 519577c, 1c235ef, 8b8e65a, bf9c367, 80f5cb2, 9f952eb, 11b3378, 0c3a...)
resources/ untracked, ffprobe.exe deleted (no code path requires it), thumbnail and metadata options dropped entirely (d702da7, 519577c), dead yt-dlp unpack path removed, ffprobe moved to scripts-tools, reorg into app-python/, resources/, scripts-tools/ with ROOT derived from __file__.

- navy background added then reverted (11355bf, ce72ba6, 624d961, eab5957)
Launchers gained a navy background via color 1F on windows and OSC 11 on linux. Reverted the same day: white terminals do not exist and the artifacts were not worth it. Recorded because the decision history is the point.

- spinner-demo.py is standalone (80c3fb3)
VT enable inlined so the demo runs without importing ytdlp.py.

- nav keys rebuilt (f5b6f67, d9b1b5d)
The 0xe0 extended-key byte garbled windows input; read_esc eats it, LineEdit rebuilt with echo, backspace, arrow-key history capped at 100, and posix escape/CSI separation.

- download flow and subtitles hardened
Staged download flow (18ab83d, 1e9463e, 0c41002, e52c789, 0803a99): one line per stage, animated prefixes, escape-driven kill of the process tree, partial cleanup. Subtitle 429 fix (9e6a39c, b6a30f0): a separate best-effort pass after the video, lang patterns expand en to en-GB/en-orig, node passed as JS runtime. Failures print yt-dlp's full error line (0b3edcc, eba9a2b) and the subtitle failure splits into two short lines (640de3c, fd3eede) with reason classification over the full error (0df05f2, a451285). Colored progress numbers (e933a5a). Update option added (2de5c44).

## v1.1

- menu rework, nine commits (d07db27, d2845e1, 12e352b, f75f8a4, 9c58c47, 651c324, df9b6bf, 038068c, 1f2e256)
Single input line: a url downloads, anything else is a menu number. Centered amber title, grey frame, escape backs out of submenus, VT processing enabled on windows. Three-way type, four filename presets, native folder picker with typed fallback. Selection box amber "[x]", plain "[ ]" when unselected (cde50c2). (4f573ad, e23c726: console resize attempted and reverted; the CLI never sizes the console.)

- docs: changelog, readme and handoff refresh (98a6730)

- release v1.1 (5c670ec)
Version bump plus changelog and readme.

## v1.0

- initial scaffold and renderer fix (e88e27e, b4c2975)
CLI skeleton, selftest harness, release zips, docs. Renderer fixed: menu rows two chars off, a header string-format crash, blank rows printing as tuples.
