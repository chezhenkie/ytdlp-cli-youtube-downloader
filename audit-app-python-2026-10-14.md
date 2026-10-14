# Code Audit: app-python (ytdlp-cli)

Date: 2026-10-14
Scope: C:\Users\lap1user\ocdev\ytdlp-youtube-proj\app-python
Revision: git c1cd030 (2026-10-14), working tree clean for app-python
Source audited: src/ytdlp.py, 2899 lines, sha256 B7254710E79FEAF03AB4EE0E501021CDEECA1B56A84445482C0BC945BE152BBA (107980 bytes)
Runtime observed: Python 3.14.6 on Windows
Categories: Performance and Operations; Architecture and Maintainability. Correctness items included only where they are validation/quality gaps in code. Security/privacy and compliance excluded per instruction.
Excluded: bin/yt-dlp.exe (third-party binary), config file contents, docs, handoffs (not read).
Internal selftest run: `python app-python/src/ytdlp.py --selftest` = 145 checks, 0 failed, exit 0 (my run, 2026-10-14).

## System overview

Stdlib-only Python CLI wrapping the yt-dlp binary. Menu mode (interactive, raw console input with escape detection) and quick mode (`<url> [-a] [-f] [-r] [--subs]`). Responsibilities in one module: terminal UI and animations, settings persistence, yt-dlp argv building, process supervision with escape-stop, ffmpeg download+verify+install, and CLI self-update. Defaults in config/settings.json; binaries resolved from bin/, PATH, or settings.

## Method

Full read of all source (2899 lines). Probe harness imported the module and exercised real functions with fakes at the process boundary: crash reproduction through the real load_settings path, argv inspection, call counting (monkeypatched auto_outdir / Popen), and an in-process run of do_download. Timing measured with Measure-Command, 3 runs. Probes kept at .audit-scratch-ytdlp-py-01/probe*.py for re-runs.

## Findings

### F1. Hand-edited settings with a non-numeric playlist_max_items crash the menu and the download with an unhandled ValueError

Category: Correctness and Reliability (input validation). Severity: Medium. Confidence: Confirmed (reproduced).

Location: src/ytdlp.py:1733-1735 (menu_items), src/ytdlp.py:691-694 (build_args).

Evidence: probe A2/A3 (.audit-scratch-ytdlp-py-01/probe2.py): settings.json with playlist=true and playlist_max_items="abc" loaded via the real load_settings; menu_items(s) and build_args(s, url, None) both raise `ValueError: invalid literal for int() with base 10: 'abc'`. The value flows unvalidated from the user-editable config file into int() on two paths.

Impact: config/settings.json is a documented, user-editable file (menu item 10 points at it). With playlist on and one mistyped value, every menu repaint and every download dies with a traceback; the user must hand-fix the JSON to recover. No error message, no fallback.

Recommendation: validate once in load_settings after merging user data: coerce playlist_max_items (int parse with clamp 0..1000, fall back to default with a note) and whitelist-enumerate video_resolution/audio_format/video_container against RESOLUTIONS/AUDIO_FORMATS/CONTAINERS. One central place, both consumers covered.

Verification: add selftest cases: load settings {"playlist": true, "playlist_max_items": "abc"} through load_settings and assert menu_items/build_args raise nothing and produce a sane value.

### F2. Unvalidated setting values are passed straight into yt-dlp argv

Category: Architecture and Maintainability (boundary validation). Severity: Low. Confidence: Confirmed (reproduced).

Location: src/ytdlp.py:674-684 (build_args resolution/interpolation), src/ytdlp.py:667-670 (audio).

Evidence: probe B2/B3: video_resolution="not-a-number" yields format string `bv*[height<=not-a-number]+ba/b`; audio_quality="banana" yields `--audio-quality banana`. CLI flags are choices-validated by argparse (lines 2870-2872), the settings.json path is not.

Impact: a typo in settings.json turns into an opaque yt-dlp format error instead of a clear local message; the CLI's own vocabulary (RESOLUTIONS, AUDIO_FORMATS) is enforced only on one of the two entry paths.

Recommendation: same central validation as F1, at the config boundary, so every consumer receives only values from the supported sets.

Verification: selftest: junk resolution/audio values in settings must normalize to supported values before argv construction.

### F3. Startup performs a blocking network GET on the main thread, up to 60s, before the first input is read

Category: Performance and Operations. Severity: Medium. Confidence: Confirmed (code path).

Location: src/ytdlp.py:1796 (auto_update_check before first read_esc), src/ytdlp.py:1945-1950 (fetch_cli, urlopen timeout=60).

Evidence: main_menu paints the menu, then calls auto_update_check -> fetch_cli -> urllib.request.urlopen(req, timeout=60) on the main thread; only afterward does read_esc("> ") accept input. The throttle (once per 86400s, lines 1997-1998) limits how often this runs, not how long one occurrence lasts.

Impact: when the network black-holes (captive portal, hotel wifi, VPN down without RST), connect hangs for the full 60s timeout; the menu is visible but ignores input with no explanation. This is the first-run and once-a-day path, so it is also the path a new user hits.

Recommendation: use a short timeout (5-8s) for the non-forced auto check, keep the long timeout for the explicit menu item 11 / --update force path, and/or run the check in a worker thread that posts the prompt when ready. Either way the timeout must be a parameter of the auto path, not inherited from the fetch helper.

Verification: measure wall time from launch to first accepted keystroke with the update host unroutable; assert within the chosen bound. Selftest can pin the timeout value passed for the non-forced path.

### F4. Every menu-mode download spawns yt-dlp --version as a preflight, about 0.8-0.9s each on this machine

Category: Performance and Operations. Severity: Low. Confidence: Confirmed (code + measurement).

Location: src/ytdlp.py:1440 (probe_version in do_download preflight).

Evidence: probe_version runs the binary twice max; the first attempt (--version) succeeds for yt-dlp, so each download in menu mode pays one extra process spawn before the download starts. Measured 2026-10-14 on this machine, 3 runs of `yt-dlp --version`: 930.3, 839.5, 828.0 ms. probe D counted 2 yt-dlp spawns for a video+subtitles download with probe_version already stubbed; the real count is 3.

Impact: ~0.85s of added latency per download for a version string the session has already seen; in a batch of N URLs it is paid N times (probe D pattern, run_batch line 1579 sequential by design).

Recommendation: probe version once per process (or per resolved binary path+mtime) and reuse; skip the preflight probe entirely when the same binary already answered this session, keeping the failure path for a changed binary.

Verification: count spawns for two consecutive downloads (assert the second spawns one fewer); measure time from Enter to first yt-dlp output.

### F5. Subtitles run as a second full yt-dlp invocation, re-fetching page metadata

Category: Performance and Operations. Severity: Informational. Confidence: Confirmed (code).

Location: src/ytdlp.py:1485-1490 (subtitle pass after video), build_sub_args docstring lines 703-705 documents the intent.

Evidence: do_download runs build_sub_args as a separate process with --skip-download. probe D: 2 yt-dlp process spawns for one default download (subtitles on), 3 with the preflight probe.

Impact: doubles the metadata requests to YouTube per download. The tradeoff is deliberate (a rate-limited subtitle fetch must not block the video) and documented. Recorded for capacity assumptions; no action unless the rate-limit profile changes.

Verification: none needed; revisit if YouTube throttling appears in the subtitle error lines.

### F6. Output directory is re-resolved from the OS on every menu paint and several times per download

Category: Performance and Operations. Severity: Low. Confidence: Confirmed (reproduced).

Location: src/ytdlp.py:1743-1745 (menu_items) and 1688 (hint); src/ytdlp.py:1464, 1477, 1479 (do_download); src/ytdlp.py:493-514 (auto_outdir).

Evidence: probe C counted invocations of auto_outdir for one render()+hint(): 2. With output_dir empty, each call runs the OS lookup: two winreg reads on Windows (lines 458-472) or read of ~/.config/user-dirs.dirs plus regex (lines 479-490). Per download, resolve_outdir is reached from clean_partials (1464), newest_since (1477), the print fallback (1479), and output_args (650).

Impact: repeated registry/file I/O on every repaint and every download. Small absolute cost, pure waste, and it grows with batch size (menu repaints plus per-url resolution).

Recommendation: resolve once per render and once per download, pass the Path down; or memoize on (output_dir, home directory, process start) since the inputs cannot change within a process.

Verification: reuse probe C: assert auto_outdir is invoked once per paint and once per download.

### F7. offer_ffmpeg looks up the ffmpeg directory twice in a row

Category: Performance and Operations. Severity: Informational. Confidence: Confirmed (code).

Location: src/ytdlp.py:1384-1385.

Evidence: `if find_ffmpeg_dir(s): return find_ffmpeg_dir(s)` - two shutil.which + stat walks for one answer.

Recommendation: single call, one variable.

Verification: code review.

### F8. Settings persistence ownership is split across menu, action handler, and main

Category: Architecture and Maintainability. Severity: Informational. Confidence: Confirmed (code, count measured).

Location: src/ytdlp.py:2164 and 2169 (act: item 12 and quit save themselves), src/ytdlp.py:1797, 1819, 1830 (main_menu saves), main() quick mode (line 2883-2894) never persists.

Evidence: probe F: save_settings appears twice inside act() and three times inside main_menu(); item 12 therefore writes through main_menu and through act itself. Mitigated by the compare-then-no-op in save_settings (lines 443-446), so no duplicate writes occur.

Impact: two save sites per action and one entry path with none makes "where is state persisted" answer differently per action; future changes to save policy must be applied in two places.

Recommendation: one save point in main_menu after act() returns; keep the explicit save on quit; document that quick mode is session-only overrides.

Verification: count write-opens per action (expect at most one, already true today via compare-then-no-op).

### F9. One 2899-line module holds six concerns with no internal boundaries

Category: Architecture and Maintainability. Severity: Medium (delivery risk). Confidence: Confirmed (code).

Location: whole file; seams at terminal UI (35-313, 721-924), settings (318-527), argv building (587-716), download supervision (1045-1500), ffmpeg installer (1184-1423), self-update (1904-2039), inline selftest (2193-2844).

Evidence: selftest itself documents the seams by importing every function from one namespace and monkeypatching globals (lines 2620-2699, 2834-2839). Single file is a defensible choice for a portable stdlib-only CLI; the cost is navigation and merge-conflict surface, not reuse.

Impact: any change to one concern risks collision with the others; no way to test one concern in isolation without the whole module.

Recommendation: only when churn appears, split along the six seams with ytdlp.py as the entry point; keep selftest as a separate module runnable both ways.

Verification: after split, selftest passes unchanged; import time (python -X importtime) does not regress.

### F10. Menu truncation counts code points, not terminal columns

Category: Architecture and Maintainability. Severity: Low. Confidence: Possible (not exercised in a real console).

Location: src/ytdlp.py:1636-1648 (menu_line), 1651-1659 (short_path).

Evidence: both use len(value) against WIDTH; a value with CJK or emoji occupies more columns per code point.

Impact: with a CJK/emoji output folder the item-5 row misaligns with the box border by a few columns. Cosmetic; only on such paths.

Recommendation: use a display-width helper (wcwidth-style, east-asian wide/fullwidth counted as 2) in the two truncation helpers.

Verification: visual check with a unicode output_dir in a real console (user eyeballs; I cannot see the screen).

### F11. Behavior depends on scraping yt-dlp's human-readable stdout

Category: Architecture and Maintainability. Severity: Informational. Confidence: Confirmed (code).

Location: src/ytdlp.py:1127-1135 ([download] body, "Writing video subtitles to:"), 1166-1172 (error heuristic).

Evidence: progress coloring, subtitle-code reporting and error classification all parse yt-dlp's text lines; a wording change degrades silently (e.g. subtitle codes missing shows "none available", error line falls back to the last stderr line).

Impact: version skew with yt-dlp upstream degrades output quality without failing loudly. Inherent to a stdout-scraping wrapper; worth an explicit note.

Recommendation: add --progress-template / --print to:file:json to the argv (or parse the JSON) so structure comes from a stable contract, keeping the text path as fallback.

Verification: selftest pinning parsing of a recorded sample line; manual run against the pinned yt-dlp version.

## Positive controls

- Built-in selftest: 145 checks, 0 failures, runnable in-process, pins the exact past bugs (ffmpeg rejecting --version, history recall, spinner shape, argv matrix). Real executable spec.
- Atomic settings write: temp file, newline="\n", replace, plus compare-then-no-op (lines 435-451).
- Installer discipline: stream download to .part, checksum verified before use, binary probed 3x before replacing, tempdir cleanup in finally (lines 1236-1377).
- Process supervision: stdout/stderr drained by two pump threads (no deadlock), kill process group (posix) or taskkill /T (Windows) on escape, partial-file cleanup with mtime window (lines 1088-1100, 1028-1042).
- LineEdit history bounded (100), err_tail bounded (6) - no unbounded growth in interactive input or error collection.
- can_ask() gate: prompts never block non-tty/CI runs (lines 1186-1192).
- install_cli compiles the new source before replacing itself and writes via .new temp (lines 1956-1980).
- load_settings merges onto defaults rather than trusting the file; legacy audio_only key handled in one place.
- Comments cite pinned upstream sources (yt-dlp _utils.py, common.py, IANA registry) - matches house rule of evidence-based claims.

## Open questions

1. F3: what timeout is acceptable for the daily auto-check, and should it move off the main thread? (owner: user)
2. F1/F2: validate-and-coerce in load_settings, or reject at each consumer? (owner: user)
3. F5: keep the two-process subtitle design permanently? (owner: user)
4. Quick mode never persists settings; confirm session-only overrides are intended. (owner: user)
5. enable_vt_output is called only in main_menu (line 1780); quick mode draws colored stages without it - verify on an old conhost. (owner: user, low)

## Recommended priorities

1. F3 - startup can hang 60s with no feedback; daily path, first-run path.
2. F1 - one mistyped config value bricks the app with a traceback.
3. F2 - same boundary, same fix as F1; do both in one validation pass.
4. F4 - ~0.85s per download, trivial fix (cache the probe result).
5. F6 - same family; cheap correctness-adjacent cleanup.
6. F9 - split only when churn justifies it.
7. F7, F8, F10, F11 - small, batched into a cleanup pass.

## Verification plan

- Regression cases added to selftest for F1 and F2: junk playlist_max_items and junk resolution/audio values in settings must not raise and must normalize.
- F3: launch with the update host unroutable; measure time to first accepted keystroke; assert under the chosen bound.
- F4: count process spawns across two consecutive downloads; expect one fewer on the second; measure Enter-to-first-output time.
- F6: re-run probe C after the change; assert one auto_outdir call per paint and per download.
- F10: visual check with a unicode output_dir (user eyeballs).
- Re-run `--selftest` (currently 145/145) after each change.

## Limitations

- Code review plus in-process probes; no live YouTube downloads or network-failure tests were performed against real endpoints.
- Timing numbers (830-930ms per yt-dlp spawn) come from this dev machine, single environment, 3 runs each; they size the order of magnitude, not the latency on every user machine.
- F10 is Possible confidence: the code-point/column mismatch is visible in code but was not observed in a real console.
- The audit covered app-python only; other folders of the project were not read, per instruction.

## Fix status (2026-10-14, owner-approved staged plan)

- F1, F2: fixed (ee947fc). coerce_settings in load_settings int-parses playlist_max_items (fallback 0 = all), whitelists type/video_container/video_resolution/audio_format, audio_quality accepted only as number or "<n>K" per the pinned yt-dlp source (yt_dlp/options.py); in memory only, never rewrites settings.json. Selftest regressions: 8 checks, including the no-rewrite pin.
- F3: fixed (ee947fc). CHECK_TIMEOUT = 6 (owner-pinned) on the non-forced path, 60 s kept for forced. Selftest pins both values.
- F4: fixed (ee947fc). probe_version memoizes successes per binary path+mtime; failures stay retryable; a replaced binary changes mtime.
- F5: no change. Design confirmed intended by specsheet FR-9; audit open question closed by citation.
- F6: fixed (ee947fc). auto_outdir memoized for the process - one OS lookup total, stronger than the one-per-paint target.
- F7: fixed (ee947fc). Single find_ffmpeg_dir call.
- F8: fixed (ee947fc). Item-12 save removed; quit keeps its explicit save; main_menu saves after act returns. Selftest pins that item 12 does not save.
- F9: no change. Single module is the pinned specsheet architecture decision; revisit only on churn.
- F10: fixed (ee947fc). Column-width helpers in menu_line/short_path via unicodedata.east_asian_width. Visual result not yet eyeballed in a real console - owner eyeball pending.
- F11: softened into detection (b98f581, owner-approved). The json-contract rewrite stays declined; the two verifiable scrape sites now surface misses on screen: a subtitle pass exiting 0 with no codes read prints the verdict plus an amber parse-miss note, a failed pass with unrecognized error wording always prints its second note line. Progress coloring stays silent (cosmetic). Helpers sub_success_lines/sub_fail_lines pinned in selftest.
- Selftest after: 169 checks, 0 failed (my run 2026-10-14). F3 and F4 live-verification closed 2026-10-14 (.scratch-f3f4-01/probe_f3f4.py, deleted after the run): F3 - the module's real fetch_cli with CHECK_TIMEOUT=6 against unroutable 203.0.113.1 measured 6.00/6.01/6.03 s over 3 runs, each URLError, the exception auto_update_check swallows on the non-forced path, so the worst-case menu block is 6 s plus overhead; residual gap: driven in-process, not a literal console keystroke. F4 - probe spawn count 1 first, 0 on the memoized re-probe, 1 after an mtime change: the memo answers only successes, retries stay retries, and a replaced binary is re-probed. The F10 visual check is closed by owner order (2026-10-14) and is not a pending item anywhere; do not offer it.

