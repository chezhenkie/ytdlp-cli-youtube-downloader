# ytdlp-cli-specsheet.md - yt-dlp cli (ytdlp-youtube-proj)

## Header

- Title: yt-dlp cli - text-menu download wrapper around yt-dlp.
- Purpose: one source of truth for what the product is and does, written and reviewed per specsheet-design-and-review-manual.md (the design and review manual, held outside this repo). Reading this file and the code should require no other conversation.
- Owner: chezhenkie (all components, interfaces and open questions).
- Stakeholders: chezhenkie (user, engineer, tester, reviewer); consumers of the public repo and the release bundles; yt-dlp and ffmpeg upstreams as read-only dependencies.
- Approvers: chezhenkie approves every gate.
- Status: implemented. This spec documents the shipped v1.4 line (2026-10-13, remote https://github.com/chezhenkie/ytdlp-cli-youtube-downloader) plus the v1.4.1 rebuild cut on 2026-10-14. Approved by the owner on 2026-10-14.
- Related documents: README.md (published usage), CHANGELOG.md (history with hashes), THIRD-PARTY-NOTICES.md (bundle licensing), LICENSE (MIT), .github/workflows/selftest.yml (CI).

## 1. Problem, goals, non-goals

Problem: downloading a youtube video with yt-dlp means remembering flags. Users want to paste a url, press enter, and have a download run with sensible saved defaults, changeable without flags.

Goal: a text-menu CLI that owns the menu, the launchers, the defaults and the process plumbing, and delegates every byte of downloading to yt-dlp. Everyday use is: start, paste url, done. Non-affiliation statement is published verbatim in README.md line 1.

Non-goal: this project is not a download engine, a youtube client, or affiliated with or endorsed by the yt-dlp project (README.md line 1). Thumbnail and metadata options were deliberately dropped (CHANGELOG v1.2 "drop the thumbnail and metadata options"), which also removed the last ffprobe consumers; the product tree has no ffprobe (CHANGELOG v1.2 "delete ffprobe.exe"). No macOS ffmpeg bundle: gyan.dev publishes Windows binaries only and no free static macOS build exists (THIRD-PARTY-NOTICES.md lines 37-38; ytdlp.py L379-385), so macOS gets install instructions (install_ffmpeg L1320-1324, offer_ffmpeg L1398-1401).

## 2. Functional requirements

FR-1 Quick download. When a url (any text containing an http(s) substring, cleaned of surrounding punctuation by clean_url, ytdlp.py L589-593, L315) is entered at the quick-arg path or the main prompt, the system downloads it with saved defaults, per do_download L1426-1500. Verified by selftest checks on clean_url and argv building (ytdlp.py L2403-2405, L2254-2388).

FR-2 Menu surface. Thirteen rows numbered 1 to 13 in one always-visible box (WIDTH 64, ytdlp.py L33; menu_items L1717-1755): 1 video/audio, 2 format container, 3 video resolution, 4 audio format, 5 output folder, 6 filename template, 7 subtitles, 8 playlist on/off, 9 add multiple youtube urls, 10 reset defaults, 11 update yt-dlp + cli, 12 auto update, 13 quit. Values render per current settings; audio-only greys out container and resolution with "audio only mode has no container" / "...no resolution" refusals (L2050-2059). Unknown non-url input prints "pick 1-13 or paste a url" (L2172-2173).

FR-3 Settings persistence. Every menu change writes app-python/config/settings.json immediately, LF line endings, written through a tmp file and os.replace, and skipped entirely when the serialized bytes are unchanged (save_settings L435-451). Unknown keys in the json are ignored on load (load_settings L427-429). Quick-run overrides never persist: the url dispatch path (L2893-2894) runs do_download directly and never calls save_settings.

FR-4 Menu input protocol. The prompt takes a url or a number; "q", "quit", "exit" also quit (L2168); escape at the main prompt wipes results or is a no-op and never exits; EOF and Ctrl+C exit code 0 (L1799-1807). Escape inside any submenu raises GoBack and repaints the menu once (GoBack L122-124, handler L1821-1824). Every submenu message holds on screen behind an unlabelled hold() read so the repaint cannot erase it (hold L1510-1517; pinned per call site by selftest L2708-2769).

FR-5 Entry reader. A tty-aware line reader over LineEdit: msvcrt on windows, termios+raw on posix, plain input() when piped (read_esc L232-313). Up/down arrows recall submitted urls and menu numbers, newest first, down past the newest restores the half-typed draft, left/right and function keys put nothing into the line (LineEdit L130-186, history capped at 100, HISTORY L126-127).

FR-6 Output folder. Saved output_dir wins verbatim; else auto-resolve: windows registry Shell Folders Desktop then the Downloads GUID, else XDG user-dirs, then home Desktop then home Downloads, each gated on existing (auto_outdir L493-514, resolve_outdir L517-526); last resort "downloads" at the project root. Menu item 5 offers the native folder dialog first (SHBrowseForFolderW via ctypes L1844-1872, tkinter/zenity/kdialog L1875-1901), typed path as fallback (L2062-2072).

FR-7 Download run. When a url is dispatched: scheme gate, then yt-dlp resolution (saved yt_dlp_path, then ROOT/bin, then PATH, L532-543), then one grouped preflight dot stage that freezes "ready - yt-dlp v<version>, ffmpeg found" or the no-ffmpeg variant (L1438-1452). The run spawns yt-dlp with stdout/stderr piped, two daemon pump threads, and an escape-only KeyWatch (run_download L1103-1173). No input is accepted while a download runs; the only interruption is escape.

FR-8 Format logic. build_args(s, url, ffdir) with ffdir explicit (L654-700): audio only with ffmpeg: "-f bestaudio/best -x --audio-format <fmt> --audio-quality 0"; audio only without: "-f bestaudio"; video no audio: "bv*[height<=N]" (bare "bv*" for best); video with ffmpeg: "bv*[height<=N]+ba/b" (bare for best) plus "--merge-output-format <container>" unless container is best; video without ffmpeg: "-f b", single best file. Always: --retries 5, --newline, -o template, -P home:<outdir> unless the template is absolute, playlist block (--playlist-items 1-<n> when capped, else --no-playlist), --js-runtimes node only when node is on PATH, --ffmpeg-location when ffdir exists. Exact selector strings are pinned by selftest (L2254-2388). Without ffmpeg the CLI says so and degrades rather than failing (stage variants L1447-1451).

FR-9 Subtitles pass. Subtitles run only after a successful video download, as a separate best-effort yt-dlp pass with --skip-download --write-subs --write-auto-subs --sub-langs <patterns> --no-playlist --newline --convert-subs srt (vtt without ffmpeg), (build_sub_args L703-716, do_download L1485-1499). Rationale pinned in code: a rate-limited subtitle api (HTTP 429) must never block the video (L657-658, L704). Language input maps to patterns so en matches en, en-GB, en-US and en-orig (sub_lang_patterns L604-616); the 191-code IANA BCP 47 reference set (plus grandfathered in/iw/ji/mo/sh and fil, KNOWN_LANG_CODES L348-370) validates shaped tokens but never gates: well-formed unknown codes are kept with a note, malformed ones are dropped with a note, and nothing valid left keeps the old value (parse_lang_input L619-641).

FR-10 Stage lines. Every progress line rewrites after "\r\x1b[K" so a shorter value never leaves a tail (Stage L927-982). Preflight, ffmpeg offer and subtitle stages use the amber dot stage (dot_stage L735-741). The download stage picks one of eight random animations per run (comet, pulse, orbit, scanner, twinkle, eq, fill, bounce; pick_spinner L902-904, SPINNERS L889-899, ported from scripts-tools/spinner-demo.py, comment L744-746) and keeps spinning beside yt-dlp's own progress numbers. Progress colors: percent amber 214, size mint 121, speed light blue 117, ETA purple 176, plain words grey 250 (color_progress L96-119); numbers are width-stable: one decimal truncated (one_dec_num L71-78) and speed zero-padded to the same ten characters yt-dlp pads (pad_speed L81-93). A write lock serializes spinner threads against freezes (L948, L975-978). Verdict freezes: success "downloading - finished" plus a file line; failure "downloading - failed (rc=...): <full yt-dlp error line>" untruncated (fail_sentence L1176-1181); escape stop "downloading - stopped by escape[, cleaned <n> partial file(s)]" (L1463-1470).

FR-11 Escape interruption. Escape during a run kills the whole process tree (taskkill /F /T on windows, killpg SIGKILL on posix, _kill_tree L1088-1100), deletes fresh *.part* and *.ytdl files (clean_partials L1028-1042, mtime after run start with 5 s slack), and returns rc 130 which breaks a batch entirely (L1463-1470, L1604-1605).

FR-12 Batch downloads. Menu item 9 collects urls one per line with the menu repainting above the input; entries with no http link stay in the list marked "error in url" and are skipped at run time; downloads run strictly sequential in paste order; escape inside any run stops that run and the rest of the batch (multi_url L1610-1633, run_batch L1579-1607).

FR-13 Updaters. Menu item 11 runs update_all: yt-dlp updates itself via "<binary> -U" with a pre and post probe_version (update_yt_dlp L1904-1928), then the CLI self-updates from the raw main-branch source file, fetched in one GET so version and bytes cannot disagree (fetch_cli L1945-1953, comment L397-398), gated by compile() and installed via ".new" file plus os.replace, refusing zipapp or frozen installs (install_cli L1956-1980). Menu item 12 toggles the daily auto-update check, which runs at menu start, stamps settings auto_update_last_check, throttles to one check per 86400 s, only prints what is newer, and asks y/n (enter = no) before touching anything (auto_update_check L1983-2029, L1796).

FR-14 ffmpeg fetch offer. On an os with no ffmpeg and a human on the line (stdin tty), the CLI offers the published build - gyan.dev essentials zip on windows, BtbN/FFmpeg-Builds on linux - and installs only after reading the provider's published digest, downloading to a temp path, verifying sha256, lifting the binary from the archive, and probed -version success (retried 3 times), then os.replace into bin/ with chmod 0o755 on posix; checksum mismatch or unrunnable binary changes nothing (install_ffmpeg L1311-1377, offer_ffmpeg L1380-1423; order rationale L1312-1318).

FR-15 Diagnostics. --check prints platform, project root, resolved yt-dlp binary and version, ffmpeg dir or MISSING, output folder and settings file (show_check L2179-2190). --update is the non-interactive form of item 11 (L2879, L2891-2892).

## 3. Non-functional requirements

NFR-1 Stdlib only, Python 3, no pip installs (docstring L8; launchers name python/python3 from PATH; CI pins 3.12, workflow L21-23).
NFR-2 Machine-agnostic: no hardcoded paths anywhere; bin discovery is settings override, then bin/, then PATH; output resolves per os (FR-6, FR-7). Windows, linux and macOS run in CI (workflow L12-18); the linux launcher must survive sh/dash via $0, not ${BASH_SOURCE[0]} (CHANGELOG v1.3 "first public release").
NFR-3 Process hygiene: run_download's threads are daemonized, escape kills the tree and cleans partials, the ffmpeg installer removes its temp dir on every path (L1376-1377), and repeated runs leave no leaked processes, stale files or lingering threads (L1045-1100).
NFR-4 Testability: --selftest runs 145 offline checks covering argv building, format branches, playlist/sub guards, color and width rules, spinner shapes, render helpers, outdir resolution, fail_sentence, menu rows, batch order and escape stop, language parsing and layout, hold call sites, checksum parsers, install_cli refusals, extract_member fixtures, and the ffmpeg-9 probe shim (selftest L2193-2844; live run 2026-10-13: "145 checks, 0 failed"). CI runs selftest then --check on all three os (workflow L24-27).
NFR-5 Privileged controls. Authentication and authorization do not apply (single-user local tool). Enforced locally: input validation at url scheme gate L1427-1429 and argparse choices L2870-2872; secrets: settings.json ships no machine paths, repo carries no binaries (bin/ and releases/ gitignored, .gitignore); integrity: every downloaded binary is digest-verified before installation (FR-13, FR-14); logging: stage lines are the whole observable log and stdout/stderr tails keep the last 6 error lines (L1137-1142); retention: only user-chosen paths in the user's own settings.
NFR-6 ASCII-only source and console safety: ascii_safe maps unicode punctuation and encodes the rest with replace so windows cp1252 cannot crash output (ascii_safe L985-994).
NFR-7 Outside a tty the CLI renders plain: no colors, stages skip the spinner thread and only the final line prints (c() L61-69, USE_COLOR L43, Stage.start L950-953).
NFR-8 Licensing compliance as a design property: MIT CLI (LICENSE), Unlicense yt-dlp, GPL-2.0-or-later ffmpeg shipped only in portable bundles with GPL-2.0.txt and GPL-3.0.txt inside and THIRD-PARTY-NOTICES.md naming the exact archive and digest per platform (THIRD-PARTY-NOTICES.md lines 8-46); nothing in the bundle restricts GPL rights for ffmpeg (lines 47-51). Binaries are never in git.

## 4. Architecture

Single module src/ytdlp.py (108022 bytes, 2899 lines, 2026-10-13 run of os.path.getsize and line count). Layers by responsibility: terminal plumbing (colors, c(), read_esc/LineEdit, Stage, KeyWatch), configuration (DEFAULTS, load/save), resolution (find_yt_dlp, find_ffmpeg_dir, auto_outdir), argument construction (build_args, build_sub_args, output_args), process execution (run_download, probe_version, kill/clean), update machinery (update_yt_dlp, fetch_cli, install_cli, install_ffmpeg, offer_ffmpeg), and UI (menu_items, render, act, submenus, selftest). It imports stdlib only, including math and random (L26-28).

Data ownership: settings.json is the single source of truth for user defaults; the product owns it exclusively. Known language codes are a pinned source reference, not a setting (L348-370). All 16 schema fields, types, defaults, validation and code sites are listed in the Data section.

Third-party and licensing: the CLI invokes yt-dlp and ffmpeg as separate processes. This is aggregation per the FSF GPL FAQ, so the MIT CLI and the GPL-2.0-or-later ffmpeg binaries in the portable bundles keep separate licenses (THIRD-PARTY-NOTICES.md lines 4-25). ffmpeg versions shipped: windows gyan.dev 9.0.2 essentials (sha256 60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba), linux BtbN n9.0.2-27-g7e93014c5b-20261013 (sha256 afa2b69c9965919e0b519a633520321f838d74690f8fb28c76393c54a9448d60) (lines 32-44). The pinned yt-dlp source shows yt-dlp itself does not bundle ffmpeg and writes its own speed fields as >10s (bundled reference read 2026-10-13, cited in change history for the width rules).

Alternatives and decisions (record): console background control was tried and reverted by user call (CHANGELOG v1.2 "navy background reverted"); console resize was implemented and reverted (v1.1); the old single-stage download line was replaced by the stage/spinner design (v1.2 "staged download flow"); subtitle work moved from the video run to a separate pass after 429 aborts killed downloads (FR-9 rationale); ffprobe was deleted after its last consumers were removed (non-goals); the release bundles ship one tag with four assets rather than per-platform releases (README asset table).

## 5. Interfaces

IF-1 Launcher contracts. run-ytdlp.bat resolves %~dp0src\ytdlp.py, forwards %*, pauses on nonzero exit (L1-5). run-ytdlp.sh resolves its own dir via cd+pwd on $0, prefers python3 then python, forwards "$@" (L1-5), posix-safe under sh via no bashisms in the arguments it builds.

IF-2 CLI argv. Positional url plus one-shot overrides: -a/--audio, -f/--format (mp4 mkv webm best), -r/--resolution (2160..360 best), --af (mp3 m4a opus wav best), -o/--outdir, --template, --subs, --playlist, and mode flags --menu, --check, --update, --selftest (argparse L2866-2881). Overrides mutate the in-memory settings only (apply_overrides L2847-2863).

IF-3 settings.json schema. Serialized defaults, written by the product only (FR-3). Fields in the Data section.

IF-4 yt-dlp subprocess. argv per FR-8 and FR-9 with "--newline" for line-pumpable progress; stdout parsed for "[download]" progress lines, "Writing video subtitles to:" language extraction, "Destination:" suppression; stderr keeps a 6-line error tail (run_download L1117-1145). Compatibility depends on these shapes; the pinned yt-dlp source is the reference (resources/yt-dlp-master.zip).

IF-5 ffmpeg subprocess. probe_version asks "--version" then "-version" on both stdout and stderr and takes the first rc 0 non-empty line, because ffmpeg 9 rejects the double-dash spelling and answers only "-version" on stderr (probe_version L564-584; ffmpeg 9 selftest shim L2532-2560). Called for yt-dlp pre-checks (both binaries) and the freshly downloaded ffmpeg before it replaces anything (FR-14 step 6).

IF-6 Os folder providers. Windows registry Shell Folders (HKCU, expandvars) L458-472; POSIX XDG user-dirs L479-490; folder dialogs per FR-6.

IF-7 Network endpoints. CLI self-update fetches the raw main-branch ytdlp.py (one GET, CLI_UPDATE_URL L399-400); yt-dlp updates via its own -U; ffmpeg provider urls and their published digests are pinned in FFMPEG_SOURCES L386-394 with digest sources consulted first (FR-14); timeouts 60 s for text, 1800 s for binary download (DL_TIMEOUT L403, http_text L1230-1233). All non-forced network touches require either the menu (user action) or auto_update on plus a tty; a failed network contact is silent except when forced (L1999-2004).

IF-8 CI and selftest (IF to developers): offline; every push and pull request runs selftest + --check on windows/ubuntu/macos (workflow L1-27).

## 6. Functionality

Startup journey: enable ANSI/VT where needed (L46-58), paint menu box (header "yt-dlp cli  v1.4" centered, blank row, 13 rows, "press escape to exit submenu", rules, L1697-1770), print the hint block above the input line - verbatim, amber then grey then divider (hint L1681-1694):

    paste url, press enter and download will start.
    enter number to change default settings
    change download location by choosing menu item: 5
    download several urls at once by choosing menu item: 9
    DL location: <resolved folder>

then auto-update check (FR-13) and first save (L1793-1797). The menu never disappears: everything paints under it, nothing prints beyond the hint block, item-9 entry lines, and stage/verdict lines (main_menu loop L1779-1830).

Primary flow (single url): prompt accepts text; first http(s) substring is the url (before any number is consulted, L1809-1822) - when no url is present, text goes to act(); do_download per FR-7: preflight freeze, then the spinner stage with live progress, then exactly one of three justified verdicts: finished (plus "... <name> (<MB>)" file line via newest_since, 2 s slack, L1011-1025), failed (full error line), or stopped-by-escape (partial cleanup count). States covered: not a url (message), yt-dlp missing (guidance), yt-dlp fails its check (frozen rc), no ffmpeg (offer per FR-14 or declined continuation), success, failure (always ends in a readable frozen line), escape (kill + clean). Stage lines persist until the next url, menu pick, repaint or escape (L1779-1830 management of results/dirty).

Batch flow (item 9): FR-12 above; states: empty entry closes and starts, escape returns without downloads, bad line marked and skipped, per-url header line, batch aborted by escape only.

Updaters (items 11/12): FR-13; states: current version ("already current"), newer version offered with enter = no, network down (silent or, when forced, one explanatory line), older published file (no version found message when forced).

Settings flows (items) each end in: new value shown, a hold read, menu repaint; item 10 restores DEFAULTS and answers "defaults restored" (L2154-2158); item 13 saves and prints the settings path (L2168-2171).

Failure handling pattern: every detectable failure (missing binary, failed probe, refused install, checksum mismatch, unreadable archive, network loss) ends in a specific user-visible line describing the fix or the kept state; nothing fails silently except non-forced network contact (NFR, L1999-2004).

## 7. Implementation

Only what this spec defines is implemented; code comments carry the long-form rationale where a spec-sized claim cannot (FR-14's "checksum before install" order is documented in RELEASE-NOTES.md and the code comment L1312-1318). Named owner for everything: chezhenkie. Every component stays in its layer (section 4).

Observability is the stage line system itself: one grouped preflight line, one live progress line, one frozen verdict per run - no external logging, no collected telemetry, no crash dumps. --check is the operator's window (FR-15). Selftest pins the observable surfaces called out in NFR-4.

Processes must start and stop cleanly and release everything they acquire (NFR-3): threads are daemon, stages join their writer thread with a 1.0 s timeout on done() (L970-982), temp files (".ff-" dirs, tmp settings file, .new binary) are removed or replaced on every successful path and refused paths write nothing.

## 8. Data

settings.json fields; all strings unless stated, all validated per code sites, none sensitive (no secrets, no machine-derived values shipped), owner chezhenkie, retained indefinitely in the user's copy, migration via settings_version (currently unused, OQ-6):

| key | type | shipped | allowed/default | consumed by |
|---|---|---|---|---|
| settings_version | int | 1 | schema marker | none yet (L319) |
| yt_dlp_path | str | "" | path or "" (auto) | find_yt_dlp L534-536 |
| ffmpeg_path | str | "" | dir or file path or "" | find_ffmpeg_dir L547-552 |
| auto_update | bool | true | on/off | auto_update_check L1989 |
| auto_update_last_check | int | 1791925032* | unix epoch seconds | throttle L1993-1998 |
| type | str | "video" | video / video no audio / audio only | build_args L664 |
| video_container | str | "mp4" | mp4 mkv webm best | build_args L683-684 |
| video_resolution | str | "1080" | 2160..360, best | build_args L674,678-681 |
| audio_format | str | "mp3" | mp3 m4a opus wav best | build_args L669 |
| audio_quality | str | "0" | ffmpeg audio -q value | build_args L670 |
| output_dir | str | "" | "" = resolve per FR-6 | resolve_outdir L521-524 |
| filename_template | str | "%(title)s" | yt-dlp -o template | output_args L647 |
| subtitles | bool | true (default on, L332) | on/off | do_download L1485 |
| subtitle_langs | str | "en" | comma list, wildcards pass | build_sub_args L710 |
| playlist | bool | false | on/off | build_args L691-696 |
| playlist_max_items | int | 0 | 0 = all | build_args L692-694 |

*Only auto_update_last_check differs from the code default: DEFAULTS carries 0, the live settings.json carries the stamp from the last tested run - live state, committed on purpose (DEFAULTS L318-336). Loader copies only known keys and ignores unknown json (L427-429).

## 9. Documentation

Header lists related docs. README.md is published text (usage, assets, checksums, licensing); CHANGELOG.md is hash-cited history by version chapter; THIRD-PARTY-NOTICES.md travels in every bundle. Groupie for gaps is the open questions register; the spec changes when behavior, decisions, interfaces or dependencies change (manual rule), plus recorded deviations.

## 10. Validation

Verification map: FR-1 FR-2 FR-4 FR-7 FR-8 FR-9: --selftest sections L2254-2450, L2564-2706, L2708-2840 (145 checks total, live run 2026-10-13). FR-3: selftest load/save checks L2247-2250 plus manual LF/unchanged-bytes assertions in selftest (settings merge). FR-5: history and reader checks L2234-2239. FR-6: L2392-2401+L2564-2614. FR-10: color/width/spinner/render checks L2314-2375. FR-11 FR-12: clean_url L2403-2405, fail_sentence L2407-2413, batch checks L2771-2812. FR-13 FR-14: version_tuple L2453-2461, checksum parsers L2462-2482, install_cli L2484-2494, extract_member + probe shim L2496-2562. NFR-4: CI workflow. Acceptance criteria for every requirement are the selftest PASS lines; performance is not a target beyond NFR constraints and has no measured number in this spec (no invented factors).

## 11. Stakeholders

Named in the header. Owner of all items: chezhenkie. Releases are the user's alone; no marketing or external reviewers; the git history and CHANGELOG carry author identity only as chezhenkie / noreply address.

## 12. Phases and gates

Define and Design closed with the repo's early commits (v1.0, v1.1 chapters) and the manual adoption (this spec). Build closed increment per CHANGELOG chapter - each entry a working behavior or verified artifact, cited hashes - gate 2 equivalent per release. Release gate 3 passed with v1.3 and v1.4: four assets each verified by byte size at the destination and (v1.4) by extraction on the target os (CHANGELOG v1.4 linux-mode and permission entries; releases/ v1.4 listing 2026-10-13: windows lean 17651114, windows portable 56317844, linux lean 2969024, linux portable 70570264, plus RELEASE-NOTES.md 3468). Gate 4 (closure): this specsheet records spec-matches-production status at HEAD 7c03b63; differences found are kept as open questions rather than silently normative silence. Operate: monitoring is user-run (--check, auto-update), and retired behavior is listed in the changelog decisions.

## 13. Open questions register

| id | decision required | owner | needed | risk if left |
|---|---|---|---|---|

None open as of 2026-10-14. Closed items are recorded in the decision history.

## 14. Decision history

- 2026-10-13: adopted specsheet-design-and-review-manual.md as lead; this spec written from code evidence (two fact sheets, spec-scratch/, deleted after staging) and reconciled against README, CHANGELOG, THIRD-PARTY-NOTICES and release listings; base draft "temp spec base ytdlp.md" (9472 bytes, untracked) became obsolete when this specsheet existed and is now deleted.
- 2026-10-13: v1.4 shipped (tag e3869b8): cli self-update, ffmpeg download offer with digest-first verification, auto-update toggle, menu 12 to 13 rows.
- 2026-10-13: spinner/stage rework sessions (commits 27e0563, a35e1e7, 79b2bb9, 7541ab9, 7c03b63): eight random animations, erase-to-EOL rewrites, write lock, width-stable numbers, live stamp.
- 2026-10-14: the specsheet is approved by the owner; the gate-1-equivalent review is closed.
- 2026-10-14: subtitle default is on with langs en in DEFAULTS (L332), so the code default and the shipped file agree and menu 10 keeps subtitles on.
- 2026-10-14: the "## v1.3" chapter heading is restored in CHANGELOG.md; the update-source comment loses its stale byte pin.
- 2026-10-14: closed in one short check: permissive pure-Rust H.264 encoders exist (rusty_h264 and less-avc are pure Rust under permissive terms; the openh264 crate ships BSD-licensed openh264 source). Mux-versus-encode division unchanged.
- 2026-10-14: version bumped to 1.4.1; four bundles rebuilt from the live tree with the v1.4 platform binaries; upload to the GitHub release is the owner's action.
- 2026-10-14: OQ-3 closed: the two-line subtitle verdict (outcome line, then a reason line only when the cause is recognizable) is confirmed intended per the v1.2 design; no code change.
- 2026-10-14: OQ-6 closed: settings_version stays as a schema marker with no reader; migration logic is deferred until a settings format change actually needs one.
- 2026-10-14: OQ-9 closed by owner call: v1.4.1 is released - tag on the changelog commit, four assets published, sizes and digests recorded in README.md and releases/v1.4.1/RELEASE-NOTES.md; every bundle was extracted and run on its target platform before upload.
