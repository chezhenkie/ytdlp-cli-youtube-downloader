changelog
=========

Newest first. Current level: v1.5.0.

## v2.0.0 native port (unreleased)

- specsheet split into the specsheet/ folder (d032d5a)
ytdlp-cli-specsheet.md is split: specsheet/README.md is the main document (header, short project description, table of contents) and sections 1-14 live in numbered part files, so every existing "section N" citation still resolves. The split is byte-verified - the 14 parts reassemble to the pre-split file exactly (67199 bytes, newline count 265, sha256 899b77cd9bbaf701282c957b0485016504dc2c19c48ce55088e79947f6617f26); only the main README's title line, one status sentence, the short description and the table of contents are new text. Root file deleted; RUST-PORT-PLAN.md references repointed. Detail in the spec decision history entry. Doc-only; no requirement changed.

- phase 4: resolution and diagnostics ported, --check wired, G4 parity on three OS (4ebd03d, f9a997c, 407197a, 32d03c1)
platform_name, find_yt_dlp, find_ffmpeg_dir, probe_version, auto_outdir, resolve_outdir and show_check land in a new app-rust/cli/src/resolve.rs together with the pathlib 3.14 lexical model (splitroot/path_render/is_absolute/join/parent) and expandvars - all pinned against probed oracle matrices because the 3.14 parsers are C builtins. Every process-state read is a parameter; the real glue sits in the same module: shutil.which for the four fixed query names (real access(2) via libc on posix, the PATHEXT machinery and curdir candidate on windows), home resolution, the shell-folder registry read via windows-sys FFI (first external dependencies of the workspace, from the spec capability table), a 60 s-timeout subprocess probe, and the first wired main() path: --check prints the seven FR-15 lines with python line endings (CRLF on windows, measured). probe_version memoizes (path, mtime) successes only (audit F4) and auto_outdir keeps an explicit process memo (audit F6); both are named rust tests. Differential: twelve new vector files, 263 cases (the first five drive stdlib entry points: Path, os.path.splitroot/expandvars, shutil.which); run-all reports 40 functions, 665 records, 0 mismatches under the CPython 3.14.4 WSL oracle. 86 rust lib tests (66 before) including the windows expandvars flavor pinned on every OS, the windows which/registry paths and the ffmpeg-9 probe shim (a real executable rejecting --version with a banner, falling back to -version and memoizing). G4: a new CI check-parity job stages each release binary beside app-python's config/bin tree and byte-compares both --check outputs on windows, ubuntu and macos - green in rust run 38728108495, and byte-identical locally in WSL with the real PATH-resolved ffmpeg and the real Downloads folder. Windows differential fixes folded in: pathlib-rendered scratch paths in the records, the $$ collapse, insertion-ordered env maps (os.environ last-assignment-wins). Port notes in specsheet section 4 (spec 7d0fbef). No functional requirement changed; app-python untouched.

- phase 3: state layer ported, settings codec and no-op pin (47a67d9)
load_settings and save_settings land on a new CPython-faithful settings codec (app-rust/cli/src/settings_json.rs): json.loads semantics on read (floats, the NaN/Infinity/-Infinity constants, last-wins duplicate keys, i64 saturating integers, lone-surrogate escapes to U+FFFD), json.dumps(s, indent=2, sort_keys=True) semantics on write (ensure_ascii escaping, json float spellings for non-finite values, repr semantics for finite ones through py_float_repr, one LF tail). Load carries the defaults merge, the unknown-key drop, the legacy audio_only line with python truthiness, and the coercion notes; the shapes where python raises outside its try (non-object JSON) become typed ShapeError branches. Save skip-compares the newline-translated text, creates parents, writes the tmp file and renames over the target; the loader never writes. The differential runs a 36-case corrupted-file matrix (missing, empty, bad json, BOM, invalid UTF-8, every non-dict shape including the "type"-substring short-circuit, unknown keys, legacy truthiness, junk coercion notes, int forms, float quality, unicode, NaN/Infinity, nested values, duplicate keys, bignum-on-unknown-key, CRLF, the tracked file) and a 12-case save matrix (sorted/indent pins, empty object, missing parent, equal skip with the tmp sentinel intact, CRLF skip, overwrite, not_utf8, io_error, the writer pin, duplicate keys, i64 bounds, the tracked file as a byte no-op). run-all: 28 functions, 402 records, 0 mismatches under the CPython 3.14.4 WSL oracle; 66 rust lib tests (36 before) plus the tracked_settings_json_roundtrips_byte_exact integration test - the tracked settings.json renders byte-exact and a no-op save changes nothing, the mechanical meaning of "no-op save leaves git diff empty". The harness json codec gained float parsing for vector inputs; records stay i64-only. Clippy warnings denied clean; windows-msvc and linux-musl cross-checks compile. Port notes recorded in specsheet section 4 (spec 3fb4c82). No functional requirement changed; app-python untouched.

- phase 2: pure layer ported, differential all green (569dd8e)
26 absent-I/O functions ported into app-rust/cli/src along the spec's layer modules - args (clean_url, sub_lang_patterns, parse_lang_input, js_runtime_args), term (one_dec_num, pad_speed, color_progress, ascii_safe, dwidth, cut_to_width, tail_to_width, render_cells, plain_cells, flat_frames, flat_freeze, the eight spinner animations, pick_spinner), exec (fail_sentence, sub_fail_reason, sub_success_lines, sub_fail_lines), update (version_tuple, parse_sha256, pick_checksum), config (settings value model, 16 defaults, vocabularies, coerce_settings, py_float_repr) and ui (menu_line, short_path, sub_lang_lines, batch_row, pick_rows). One vector file per function; `harness.py run-all` (new, the gate-2 CI command) reports 26 functions, 354 records, 0 mismatches under the CPython 3.14.4 WSL oracle; the harness also gained per-case capture_args (mutated settings compared after coerce_settings) and display-state hooks (node-on-PATH, USE_COLOR, os.name), documented in its docstring. Hand-rolled streaming SHA-256 passes the FIPS 180-4 vectors, CPython-recorded block-boundary digests, and hashes all four v1.5.0 release bundles to the README-pinned digests (dev-only test; skipped when releases/ is absent). East-asian width table generated from the pinned oracle by tools/gen_width_table.py: 122 ranges, 182719 code points, sha256 73ee3105a481eed7e96b61663338a0f21b600c970b35f728e2b38a55384d06d5, identical on 3.14.6 and 3.14.4, re-checked in CI. Spinner table pinned by oracle digest cba9f09dde5804089d343f9eedc1635ca474f6dd93a0c8ce9e3b54d825c089ac; PCG32 pinned against the compiled reference pcg-c-basic (seed 42/54). 36 rust lib tests plus the digest integration test green; clippy warnings denied clean; windows-msvc and linux-musl cross-checks compile. Port notes recorded in specsheet section 4 (i64 saturation, ASCII numeric matching, checksum line splitting, CPython float repr). No functional requirement changed; app-python untouched.

- phase 1: differential harness, workspace skeleton, rust CI, gate 1 (3b3e4f4, b572605)
G0 approved by the owner; phase 1 of RUST-PORT-PLAN.md landed. app-rust/ is a cargo workspace: the cli bin (phase 1 skeleton) plus a harness dispatcher crate that is never shipped. tools/harness.py calls a named function in the python or the rust implementation and byte-compares the outputs in the CPython interchange format (json.dumps ensure_ascii=False, separators=(",", ":"), sort_keys=True) reproduced in harness/src/json.rs against recorded CPython 3.14.6 fixtures; per NFR-4 the instrument is proven on known answers before it judges any rust: selftest green (python vs python, 19 clean_url records, 0 mismatches) and red (one flipped byte detected at record 10). The oracle interpreter is pinned to CPython 3.14.x and enforced by the tool. G1 evidence: 4 rust codec tests green; linux-gnu release 450856 bytes (harness 484176), linux-musl 551672/588680, static-pie without musl-gcc; msvc cross-check compiles with -C target-feature=+crt-static reaching rustc from the repo-root config; CI run 38566072196 green on windows, ubuntu and macos including the PE assertions - the crt-static exe imports KERNEL32, ntdll and api-ms-win-core-synch-l1-2-0 only (3 imports, no vcruntime/msvcp), while a deliberately dynamic-CRT build shows VCRUNTIME140.dll plus five api-ms-win-crt-* entries (9 imports), proving the checker sees what it claims. Windows release sizes: ytdlp.exe 221184, ytdlp-harness.exe 260608. Python selftest workflow stayed green (run 38566072223). tools/pe_imports.py was validated locally against a real msvc-built PE and its red paths.

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
