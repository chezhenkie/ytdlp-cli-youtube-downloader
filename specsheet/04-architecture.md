## 4. Architecture

Two implementations share one behaviour specification. Both are held to the functional requirements above without exception; the port changes the implementation, not the product.

PYTHON (shipped, authoritative through cutover). Single module src/ytdlp.py (123121 bytes, 3274 lines (newline count), sha256 98818B510A182094E30524CC0218300CDC38FFA08137B22537FF4B416F0728AA at the v1.5.0 release commit). Layers by responsibility: terminal plumbing (colors, c(), read_esc/LineEdit, Stage, KeyWatch), configuration (DEFAULTS, load/save), resolution (find_yt_dlp, find_ffmpeg_dir, auto_outdir, temp_dir, drop_temp_dir), argument construction (build_args, build_sub_args, output_args),
process execution (run_download, probe_version, kill/clean), update machinery (update_yt_dlp, fetch_cli, install_cli, install_ffmpeg, offer_ffmpeg), and UI (menu_items, render, act, submenus, selftest). It imports stdlib only, including math and random (L27-L29).

NATIVE (`app-rust/`, the v2.0.0 line; phases 1-9 landed 2026-10-16). One binary per platform, split into modules along the same seven layers. Single-module is not carried over: the module count is an implementation detail of the port, and the layer boundaries are the constraint that matters (owner decision, F9 stays closed for Python only). Capability-by-capability, because Python's stdlib supplies things Rust's does not:

| capability | Python source | Rust std | native choice |
|---|---|---|---|
| raw keys, echo, kbhit | msvcrt / termios | none | FFI: windows-sys, libc |
| VT enable | ctypes kernel32 | none | FFI: windows-sys |
| shell folders | winreg | none | FFI: windows-sys |
| folder dialog | SHBrowseForFolderW via ctypes | none | FFI: windows-sys; posix shell-out unchanged |
| terminal size | os.get_terminal_size | none | FFI: windows-sys GetConsoleScreenBufferInfoW; libc TIOCGWINSZ; any error reads as no
room (block_under_input) |
| process tree kill | taskkill /F /T | spawn | unchanged |
| sha256 | hashlib | none | hand-rolled, streaming, NIST vectors pinned |
| json | json | none | hand-rolled for the 16-field schema |
| rng | random | none | hand-rolled PCG32 |
| east-asian width | unicodedata | none | hand-rolled pinned table |
| http + tls | urllib.request | none | ureq + rustls + webpki-roots |
| zip | zipfile | none | zip |
| tar.xz | tarfile | none | lzma-rust2 + tar |
| charset safety |
fixed map + ascii replace (ascii_safe) | per-site (see language rules) | hand-rolled 9-pair map + ascii replace; decode policy per site |

Crate policy (owner decision): minimal vetted set. A crate is permitted only where std genuinely lacks the capability and hand-rolling would be a security liability - TLS and HTTP being the clear case. sha256, JSON, the RNG and the east-asian width table are hand-rolled because they are small, auditable and testable offline. No TUI crate and no terminal framework: the menu stays a hand-rolled ANSI box over the existing reader, per the owner's no-special-frontend rule.

Language-specific rules that are gates rather than style, recorded so they are auditable: writers are wrapped in a BufWriter once outside every loop with an explicit flush before drop and write_all, because Rust File is unbuffered where Python's was not - bulk paths only: interactive stdout is locked per frame and flushed after every frame and before every blocking read, matching the 16 flush sites the Python calls explicitly; hot loops allocate no format! / clone / to_string per iteration and
reserve known sizes; the release profile sets overflow-checks = true so a ported arbitrary-precision assumption panics instead of silently wrapping; decoding is explicit at every byte boundary, per site (the download and http pipes decode UTF-8 with replacement; subprocess helper reads use the locale encoding as Python text=True does; settings reads are strict UTF-8 into the settings-unreadable branch), and no byte-offset slicing assumes ASCII; no bare unwrap, every former exception path is a
tested Result branch. Rationale and sources: python-to-rust-refactor-pitfalls.md.

G8 evidence landed (2026-10-15): (1) the non-forced ceiling measured on both sides against the unroutable RFC1918 address 10.255.255.1:9 in WSL - CPython `urlopen(timeout=6)` elapsed 6.01 s and answers URLError, the shipping port's `timeout_global` elapsed 6.01 s and answers `timeout: global`; the port keeps the measurement as the `#[ignore]`d `net.rs::timeout_ceiling_is_six_seconds_against_a_black_holed_host`, not as a CI-hanging assert. (2) The loopback fake proxy is dialed: with HTTP_PROXY
set the production impl sends `CONNECT example.invalid:80 HTTP/1.1` (ureq tunnels plain http transport through CONNECT where urllib sends an absolute-form GET - the dial is the parity, the framing is recorded), and a full tunnelled transfer lands the body bytes on disk. (3) `fake-ytdlp` now speaks `-U`: it answers with yt-dlp's wording, replaces its own image (the same rename-over move a real self-update makes, so a post-update probe is a fresh spawn and not a memo hit - the probe memo keys on
`(path, mtime)`, audit F4), and reports the new version from then on; a whole `--update` round runs offline on both sides with the same transcripts (the pre-check probe, the child's own `Latest version:` / `yt-dlp is up to date (...)` output inherited, the post-check probe reading the new version, `  cli`) and the cli half refused through a closed loopback proxy unfetchable on both - the port prints python's refusal line with the seam's word exclusion (`io: Connection refused (os error 111)`
where python prints `<urlopen error [Errno 111] Connection refused>`), which is the shape the port notes gate. (4) The no-changes guarantees are unit tests in the cli crate: a wrong published digest leaves `.new`, `.new.part` and the image untouched, a truncated archive reports unreadable and writes nothing, an unrunnable build is probed exactly three times with two one-second sleeps and installed nowhere.

The python selftest monkeypatches module globals (`read_esc`, `USE_COLOR`, `multi_url`, `save_settings`, `fetch_cli`, `subprocess.run`, `os.get_terminal_size`); the port keeps its injection discipline instead - every subcall takes its seams (`Io` out+reader, `World` flags, closures) from a `SelftestWorld` built by the selftest, and recordings follow the same shapes the crate tests and the phase-6 harness arms established (scripted `Reader` queues, recording `Out` sink, scripted
fetch/time/selftest-gate closures). Where python patched a call hook that the port never threaded, the check targets an honest native equivalent and is recorded as a named deviation in the conformance map (section 10): `act 9 is multi url` drives `act("9")` for real and asserts the multi-url screen bytes plus the non-repaint outcome instead of patching `multi_url`; `item 12 leaves the save to main_menu` asserts the settings file bytes are untouched by the `act("12")` round (the port's type
system already forbids a save inside `act`); `cli update url is raw main branch` becomes the releases-api source pin (`RELEASES_API` is the native FR-13 contract); the two `install_cli` write checks stage the dance on a scratch side-name pair (`<copy>` plus `<copy>.new`, the staged copy carrying the running image's bytes) because python's own checks also never rename the running image - its dance wrote into `<self>.new` and replaced a side name - and the `<new> --selftest` gate is scripted 0/2
at the dance, since a live-spawned gate would recurse (a shipped `--selftest` run genuinely installing its copy would spawn a nested selftest whose own dance spawns again); the gate's REAL spawn path is native-only (FR-13) and is pinned by the crate's named unit test plus one live two-image dance once a real tagged release exists (OQ-N4).

Runtime liveness rules for the packaged run: the probe-memo check and the probe-shim check spawn real child processes like python's do (a `.bat` shim on windows, `.sh` on posix, mirroring python's exactly, rc 2880417800 rejection spelling included; the memo probe drives a second, guaranteed-answer shim because python's `sys.executable --version` has no native twin), scratch fixtures live in fresh `mkdtemp` dirs that are removed at the end of every path, the settings file is only read (bytes
compared before and after; nothing written), the history/LineEdit checks run against a fresh `History` so process state is untouched, and the whole report prints through the production `Out` so windows' newline translation behaves like python's `print` everywhere.

Build matrix. One target per published platform, no fallback target: a broken build fails CI rather than producing a different binary. Linux builds in WSL; Windows builds in GitHub Actions because the Windows SDK import libraries are absent from the dev environment and `lld-link` cannot substitute for them - the compiler is not the missing piece, the SDK is; macOS is CI-only with no published bundle. The Windows build uses host target x86_64-pc-windows-msvc with the NFR-1 crt-static flag.

Data ownership: settings.json is the single source of truth for user defaults; the product owns it exclusively. Known language codes are a pinned source reference, not a setting (L349-L371). All 16 schema fields, types, defaults, validation and code sites are listed in the Data section.

Third-party and licensing: the CLI invokes yt-dlp and ffmpeg as separate processes. This is aggregation per the FSF GPL FAQ, so the MIT CLI and the GPL-2.0-or-later ffmpeg binaries in the portable bundles keep separate licenses (THIRD-PARTY-NOTICES.md lines 4-25). ffmpeg versions shipped: windows gyan.dev 9.0.2 essentials (sha256 60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba), linux BtbN n9.0.2-27-g7e93014c5b-20261013 (sha256
afa2b69c9965919e0b519a633520321f838d74690f8fb28c76393c54a9448d60) (lines 32-44). The pinned yt-dlp source shows yt-dlp itself does not bundle ffmpeg and writes its own speed fields as >10s (bundled reference read 2026-10-13, cited in change history for the width rules).

Alternatives and decisions (record): console background control was tried and reverted by user call (CHANGELOG v1.2 "navy background reverted"); console resize was implemented and reverted (v1.1); the old single-stage download line was replaced by the stage/spinner design (v1.2 "staged download flow"); subtitle work moved from the video run to a separate pass after 429 aborts killed downloads (FR-9 rationale); ffprobe was deleted after its last consumers were removed (non-goals); the release
bundles ship one tag rather than per-platform releases: four bundles plus, from v2.0.0, the two bare-binary updater assets FR-13 defines (README asset table for the shipped python lines).

Native port notes: the per-phase decision trail (phases 2-9) lives in 04-port-notes.md, which is still section 4 for every citation; this file keeps the spec itself.
