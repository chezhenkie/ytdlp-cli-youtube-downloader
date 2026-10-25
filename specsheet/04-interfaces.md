## 5. Interfaces

IF-1 Launcher contracts. The launcher is thin and the contract lives in the executable; it forwards arguments unchanged and adds or drops none. `run-ytdlp.sh` resolves its own directory via cd+pwd on `$0` and execs the binary beside it, keeping the dash-safe property under sh; `run-ytdlp.bat` execs the exe beside it and keeps the nonzero-exit pause (a double-click has no console to show an error in). The project-root resolution is reproduced from the executable's own path, so bin/, config/ and downloads/ keep the same meaning and the product stays machine-agnostic (NFR-2).

IF-2 CLI argv. Positional url plus one-shot overrides: -a/--audio, -f/--format (mp4 mkv webm best), -r/--resolution (2160..360 best), --af (mp3 m4a opus wav best), -o/--outdir, --template, --subs, --playlist, and mode flags --menu, --check, --update, --selftest (cli_args.rs + cli_args/help.rs). Overrides mutate the in-memory settings only. The flag set, the accepted values, the help text and the rejection behaviour are part of the product contract; the parser is argparse-compatible by the phase-7 decision record (section 4), and the accepted-value lists stay the single source of truth (FR-8).

IF-3 settings.json schema. Serialized defaults, written by the product only (FR-3). Fields in the Data section.

IF-4 yt-dlp subprocess. argv per FR-8 and FR-9 with "--newline" for line-pumpable progress; stdout parsed for "[download]" progress lines, "Writing video subtitles to:" language extraction, "Destination:" suppression; stderr keeps a 6-line error tail (exec.rs). Compatibility depends on these shapes; the pinned yt-dlp source is the reference (resources/ytdlp-master.zip). PARSE-MISS POLICY (audit F11 softened into detection instead of a json contract):
the stdout scraping stays, but a silent degrade becomes a spoken miss. A subtitle pass that exits 0 with no language codes read prints its verdict plus one amber note line ("yt-dlp reported success but no subtitle files were read - wording may have changed, please report"); a subtitle failure whose error line matches no sub_fail_reason pattern prints the not-fetched verdict plus one amber note line ("yt-dlp wording not recognized - please report"). Progress coloring misses nothing verifiable and
stays silent. The full uncut error line on any failed download is unchanged. Purpose: yt-dlp upstream rewording its human output surfaces on the user's screen the moment it happens, so the wrapper can be updated to the new wording in the next pass.

IF-5 ffmpeg subprocess. probe_version asks "--version" then "-version" on both stdout and stderr and takes the first rc 0 non-empty line, because ffmpeg 9 rejects the double-dash spelling and answers only "-version" on stderr (resolve.rs; pinned by the ffmpeg-9 selftest shim). Called for yt-dlp pre-checks (both binaries) and the freshly downloaded ffmpeg before it replaces anything (FR-14).

IF-6 Os folder providers. Windows registry Shell Folders (HKCU, expandvars; resolve.rs); posix XDG user-dirs; folder dialogs per FR-6.

IF-7 Network endpoints. CLI self-update: one GitHub releases API GET per FR-13 (newest tag, the platform lean bundle named for that tag, the api sha256 digest gate, the cli member lifted from the archive). yt-dlp updates via its own `-U`. ffmpeg provider urls: windows the BtbN win64 GPL zip with the digest read fresh from the provider's `checksums.sha256` every run, linux the BtbN tar.xz, macos the shaka-project static arm64 binary with the digest read from the pinned release API object's per-asset digest field (FR-14, MACOS SOURCE RULE; exec/ffmpeg.rs `ffmpeg_source`). Timeouts: 60 s for forced text fetches, 6 s for the non-forced daily auto-update check (audit F3), 1800 s for binary download (net.rs). All non-forced network
touches require either the menu (user action) or auto_update on plus a tty; a failed network contact is silent except when forced.
 The 6 s non-forced timeout is load-bearing rather than incidental - a black-holed network would otherwise stall the first keystroke - so it is pinned by a test.
 TLS is provided by rustls with webpki-roots; certificate verification is not made optional. Root certificates are bundled rather than read from the OS trust store, so the binary's behaviour does not vary with the host's certificate configuration.
PROXY: the native HTTP layer resolves the environment first (HTTPS_PROXY/https_proxy, HTTP_PROXY/http_proxy, ALL_PROXY/all_proxy, with NO_PROXY honoured) and, on Windows when no such variable is set, the HKCU Internet Settings fallback (ProxyEnable/ProxyServer) through ureq's win-system-proxy feature (winreg 0.55, a windows-only target dependency); posix has no registry step.
 The environment path is pinned offline: a loopback fake proxy must be dialed by the built binary in a test.

IF-8 CI and selftest (IF to developers): offline; every push and pull request runs fmt, clippy with warnings denied, unit tests, the musl cross-check and the pinned width/printable table checks on windows/ubuntu/macos, plus a dependency audit (cargo audit) on ubuntu (.github/workflows/rust.yml); bundles.yml (workflow_dispatch) assembles the five release bundles (windows/linux lean+portable each plus the linux-musl bundle - FR-19; that job also proves it: static link, extract, packaged selftest 192/0, no bundled yt-dlp, the staged srtstripper digest); every bundle's bin/ carries the yt-dlp onedir tree (launcher plus `_internal`, staged from the pinned zips) and the platform's srtstripper build, pinned to the v1.6.4 asset digests (FR-20), alongside the ffmpeg payload pins; the job re-opens the published windows zips and linux tars and asserts the tree (bin/yt-dlp, at least 100 `_internal` entries, a running payload, lean without ffmpeg and portable with it), extracts the linux lean tar and runs its packaged selftest, and requires the macos pkg payload list and the installed tree to carry at least 100 `_internal` entries; the job also builds the windows setup.exe and the macos-arm64 pkg (FR-18; both installers are installed and run on their own runners in the job) plus RELEASE-NOTES.md. No workflow contacts the network in a test (NFR-4).

## 6. Functionality

Startup journey: enable ANSI/VT where needed (tty.rs), paint the menu box (header "yt-dlp cli v<version>" centered, the FR-17 quick-paste row, 15 rows, "press escape to exit submenu", rules), print the hint block above the input line - verbatim, grey then divider:

 change download location by choosing menu item: 5
 DL location: <resolved folder>

then auto-update check (FR-13) and first save. The menu never disappears: everything paints under it, nothing prints beyond the hint block, item-11 entry lines, and stage/verdict lines (`main_menu`, menu.rs).

Primary flow (single url): prompt accepts text; first http(s) substring is the url (before any number is consulted) - when no url is present, text goes to `act()`; on a real tty a bare 0 goes there at the keypress with no enter (FR-17's quick-key read; piped and scripted reads keep the 0-then-enter line path); do_download per FR-7: preflight freeze, then the spinner stage with live progress, then exactly one of three justified verdicts: finished (plus "... <name> (<MB>)" file line via `newest_since`, 2 s slack), failed (full error line), or stopped-by-escape (partial cleanup count). States covered: not a url (message),
yt-dlp missing (guidance), yt-dlp fails its check (frozen rc), no ffmpeg (offer per FR-14 or declined continuation), success, failure (always ends in a readable frozen line), escape (kill + clean). Stage lines persist until the next url, menu pick, repaint or escape (the results/dirty management in `main_menu`).

Batch flow (item 11): FR-12 above; states: empty entry closes and starts, escape returns without downloads, bad line marked and skipped, per-url header line, batch aborted by escape only.

Updaters (items 13/14): FR-13; states: current version ("already current"), newer version offered with enter = no, network down (silent or, when forced, one explanatory line), older published file (no version found message when forced).

Settings flows (items) each end in: new value shown, a hold read, menu repaint; item 12 restores DEFAULTS and answers "defaults restored"; item 15 saves and prints the settings path (the quit path).

Failure handling pattern: every detectable failure (missing binary, failed probe, refused install, checksum mismatch, unreadable archive, network loss) ends in a specific user-visible line describing the fix or the kept state; nothing fails silently except non-forced network contact (IF-7, FR-13).

## 7. Implementation

Only what this spec defines is implemented; code comments carry the long-form rationale where a spec-sized claim cannot (FR-14's "checksum before install" order is documented in RELEASE-NOTES.md and the exec/ffmpeg.rs comment). Every component stays in its layer (section 4).

Observability is the stage line system itself: one grouped preflight line, one live progress line, one frozen verdict per run - no external logging, no collected telemetry, no crash dumps. --check is the operator's window (FR-15). Selftest pins the observable surfaces called out in NFR-4.

Processes must start and stop cleanly and release everything they acquire (NFR-3): threads are daemon, stages join their writer thread within one tick on done() (tty.rs), temp files (".ff-" dirs, tmp settings file,.new binary) are removed or replaced on every successful path and refused paths write nothing.
