## 5. Interfaces

IF-1 Launcher contracts. Two sets, one per implementation; both forward arguments unchanged and neither adds or drops one.
  - Python: run-ytdlp.bat resolves %~dp0src\ytdlp.py, forwards %*, pauses on nonzero exit (L1-L5). run-ytdlp.sh resolves its own dir via cd+pwd on $0, prefers python3 then python, forwards "$@" (L1-L5), posix-safe under sh via no bashisms in the arguments it builds.
  - Native: the launcher becomes thin and the contract moves to the executable itself. run-ytdlp.sh resolves its own directory the same $0 way and execs the binary beside it, keeping the dash-safe property; run-ytdlp.bat execs the exe beside it and keeps the nonzero-exit pause. The "pause on nonzero exit" behaviour is preserved because a double-click has no console to show an error in. The project-root resolution that Python gets from `Path(__file__).resolve().parent.parent` is reproduced natively from the executable's own path, so bin/, config/ and downloads/ keep the same meaning and the product stays machine-agnostic (NFR-2).

IF-2 CLI argv. Positional url plus one-shot overrides: -a/--audio, -f/--format (mp4 mkv webm best), -r/--resolution (2160..360 best), --af (mp3 m4a opus wav best), -o/--outdir, --template, --subs, --playlist, and mode flags --menu, --check, --update, --selftest (argparse L3242-L3255). Overrides mutate the in-memory settings only (apply_overrides L3222-L3238). The flag set, the accepted values, the help text and the rejection behaviour are part of the product contract and are identical in both implementations; only the parser is native in the port, and the accepted-value lists stay the single source of truth for both (FR-8).

IF-3 settings.json schema. Serialized defaults, written by the product only (FR-3). Fields in the Data section.

IF-4 yt-dlp subprocess. argv per FR-8 and FR-9 with "--newline" for line-pumpable progress; stdout parsed for "[download]" progress lines, "Writing video subtitles to:" language extraction, "Destination:" suppression; stderr keeps a 6-line error tail (run_download L1252-L1280). Compatibility depends on these shapes; the pinned yt-dlp source is the reference (resources/yt-dlp-master.zip). PARSE-MISS POLICY (owner-approved 2026-10-14, audit F11 softened into detection instead of a json contract): the stdout scraping stays, but a silent degrade becomes a spoken miss. A subtitle pass that exits 0 with no language codes read prints its verdict plus one amber note line ("yt-dlp reported success but no subtitle files were read - wording may have changed, please report"); a subtitle failure whose error line matches no sub_fail_reason pattern prints the not-fetched verdict plus one amber note line ("yt-dlp wording not recognized - please report"). Progress coloring misses nothing verifiable and stays silent. The full uncut error line on any failed download is unchanged. Purpose: yt-dlp upstream rewording its human output surfaces on the user's screen the moment it happens, so the wrapper can be updated to the new wording in the next pass.

IF-5 ffmpeg subprocess. probe_version asks "--version" then "-version" on both stdout and stderr and takes the first rc 0 non-empty line, because ffmpeg 9 rejects the double-dash spelling and answers only "-version" on stderr (probe_version L653-L687; ffmpeg 9 selftest shim L2769-L2797). Called for yt-dlp pre-checks (both binaries) and the freshly downloaded ffmpeg before it replaces anything (FR-14 step 6).

IF-6 Os folder providers. Windows registry Shell Folders (HKCU, expandvars) L506-L520; POSIX XDG user-dirs L527-L538; folder dialogs per FR-6.

IF-7 Network endpoints. CLI self-update fetches the raw main-branch ytdlp.py (one GET, CLI_UPDATE_URL L400-L401); yt-dlp updates via its own -U; ffmpeg provider urls and their published digests are pinned in FFMPEG_SOURCES L387-L395 with digest sources consulted first (FR-14); timeouts: 60 s for forced text fetches, 6 s for the non-forced daily auto-update check (owner-pinned 2026-10-14, audit F3), 1800 s for binary download (DL_TIMEOUT L404, http_text L1365-L1368). All non-forced network touches require either the menu (user action) or auto_update on plus a tty; a failed network contact is silent except when forced (L2174-L2179).

  Native implementation: the first endpoint is replaced per FR-13 (release tag, asset, published digest) and every other endpoint, its timeout and its silence rule are unchanged.
  The 6 s non-forced timeout is load-bearing rather than incidental - a black-holed network would otherwise stall the first keystroke - so it is pinned by a test in both suites.
  TLS is provided by rustls with webpki-roots; certificate verification is not made optional. Root certificates are bundled rather than read from the OS trust store, so the binary's behaviour does not vary with the host's certificate configuration.
  PROXY (decided 2026-10-15, the plan's "wire both or record the deviation" call): the native HTTP layer resolves proxies exactly where urlopen does - the environment first (HTTPS_PROXY/https_proxy, HTTP_PROXY/http_proxy, ALL_PROXY/all_proxy, with NO_PROXY honoured) and, on Windows when no such variable is set, the HKCU Internet Settings fallback (ProxyEnable/ProxyServer) through ureq's win-system-proxy feature (winreg 0.55, a windows-only target dependency); posix has no registry step, matching getproxies().
  The environment path is pinned offline: a loopback fake proxy must be dialed by the built binary in a test.

IF-8 CI and selftest (IF to developers): offline; every push and pull request runs selftest + --check on windows/ubuntu/macos (workflow L1-L28). The native port adds a second workflow covering fmt, clippy with warnings denied, unit tests and a cross-implementation differential check on the same three OS; neither workflow contacts the network in a test (NFR-4).

## 6. Functionality

Startup journey: enable ANSI/VT where needed (L47-L59), paint menu box (header "yt-dlp cli  v1.5.0" centered, blank row, 13 rows, "press escape to exit submenu", rules, L1872-L1945), print the hint block above the input line - verbatim, amber then grey then divider (hint L1856-L1869):

    paste url, press enter and download will start.
    enter number to change default settings
    change download location by choosing menu item: 5
    download several urls at once by choosing menu item: 9
    DL location: <resolved folder>

then auto-update check (FR-13) and first save (L1968-L1972). The menu never disappears: everything paints under it, nothing prints beyond the hint block, item-9 entry lines, and stage/verdict lines (main_menu loop L1954-L2005).

Primary flow (single url): prompt accepts text; first http(s) substring is the url (before any number is consulted, L1984-L1997) - when no url is present, text goes to act(); do_download per FR-7: preflight freeze, then the spinner stage with live progress, then exactly one of three justified verdicts: finished (plus "... <name> (<MB>)" file line via newest_since, 2 s slack, L1146-L1160), failed (full error line), or stopped-by-escape (partial cleanup count). States covered: not a url (message), yt-dlp missing (guidance), yt-dlp fails its check (frozen rc), no ffmpeg (offer per FR-14 or declined continuation), success, failure (always ends in a readable frozen line), escape (kill + clean). Stage lines persist until the next url, menu pick, repaint or escape (L1954-L2005 management of results/dirty).

Batch flow (item 9): FR-12 above; states: empty entry closes and starts, escape returns without downloads, bad line marked and skipped, per-url header line, batch aborted by escape only.

Updaters (items 11/12): FR-13; states: current version ("already current"), newer version offered with enter = no, network down (silent or, when forced, one explanatory line), older published file (no version found message when forced).

Settings flows (items) each end in: new value shown, a hold read, menu repaint; item 10 restores DEFAULTS and answers "defaults restored" (L2329-L2333); item 13 saves and prints the settings path (L2344-L2347).

Failure handling pattern: every detectable failure (missing binary, failed probe, refused install, checksum mismatch, unreadable archive, network loss) ends in a specific user-visible line describing the fix or the kept state; nothing fails silently except non-forced network contact (NFR, L2174-L2179).

## 7. Implementation

Only what this spec defines is implemented; code comments carry the long-form rationale where a spec-sized claim cannot (FR-14's "checksum before install" order is documented in RELEASE-NOTES.md and the code comment L1447-L1453). Named owner for everything: chezhenkie. Every component stays in its layer (section 4).

Observability is the stage line system itself: one grouped preflight line, one live progress line, one frozen verdict per run - no external logging, no collected telemetry, no crash dumps. --check is the operator's window (FR-15). Selftest pins the observable surfaces called out in NFR-4.

Processes must start and stop cleanly and release everything they acquire (NFR-3): threads are daemon, stages join their writer thread with a 1.0 s timeout on done() (L1078-L1090), temp files (".ff-" dirs, tmp settings file, .new binary) are removed or replaced on every successful path and refused paths write nothing.
