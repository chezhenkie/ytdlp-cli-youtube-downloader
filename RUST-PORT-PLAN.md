# RUST-PORT-PLAN.md - python to rust port, yt-dlp cli

Companion to ytdlp-cli-specsheet.md. That document stays the single source of truth for what the product does. This one plans how the same product moves from `app-python/` to `app-rust/` without changing a single observable behaviour. Status: plan, owner decisions taken 2026-10-14, no rust code written yet.

Owner decisions behind this plan (2026-10-14):

1. Crate policy: minimal vetted crates. Crates only where std genuinely lacks a capability and hand-rolling it would be a security liability. Hand-roll sha256, JSON, the RNG and the east-asian width table.
2. Linux lean bundle ships `yt-dlp_linux` (self-contained) instead of the zipapp, so a lean install needs nothing installed.
3. Python stays in the repo as the behavioural oracle for the whole migration. All 169 checks get Rust twins. Python is deleted only at cutover.
4. New product dir `app-rust/`, released as v1.5.0. Python stays published and supported until the owner cuts over.
5. Self-update in Rust: release asset + published digest + `<new> --selftest` probe, then rename-to-`.old` and place. Behind the existing y/n prompt.

## 1. What is being ported

| fact | value | source |
|---|---|---|
| python module | 119994 bytes, 3201 lines, 0 CRLF | measured 2026-10-14 over `app-python/src/ytdlp.py` raw bytes |
| top-level defs and classes | 104 | same, `^(def \|class )` |
| selftest() | L2337-3147, 811 lines, 169 checks | file read; specsheet NFR-4 live run 2026-10-14 |
| non-test code | roughly 2390 lines | 3201 minus 811 |
| user-side runtime deps today | Python 3 + ffmpeg (portable only) | README Requirements |
| windows lean | already self-contained: `yt-dlp.exe` 17840399 bytes equals dev `bin/yt-dlp.exe` | zip infolist; file listing |

Dev-toolchain state, measured 2026-10-14:

| fact | value | source |
|---|---|---|
| rust | rustc 1.98.0 (88d9e12ae 2026-08-18), cargo 1.98.0, in WSL Ubuntu | `~/.cargo/bin` |
| installed targets | x86_64-pc-windows-msvc, x86_64-unknown-linux-gnu, x86_64-unknown-linux-musl | `rustup target list --installed` |
| MSVC on this box | absent: no Visual Studio dir, no Windows Kits, `where cl` and `where link.exe` not found | filesystem and `where.exe` |
| `lld-link` | present inside the rustup toolchain | `.../lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld/lld-link` |
| crate cache (WSL) | 1233 crates; rustls 0.23.40, ureq 2.12.1 and 3.4.2, zip 2.4.2 and 7.2.0, sha2 0.10.9, libc 0.2.186/0.2.189, windows-sys up to 0.61.2, encoding_rs 0.8.35, flate2, miniz_oxide present; xz2 and rustls-native-certs absent | `ls ~/.cargo/registry/cache/*/` plus negative grep |

### 1.1 Windows build story, decided

Decision (owner call, 2026-10-14): the shipped Windows target is `x86_64-pc-windows-msvc`, built in GitHub Actions. Standard toolchain, no local Windows toolchain, no cross-compiler, no install step in CI. The user is unaffected either way: the product ships prebuilt binaries, so the toolchain that produced the `.exe` has no user-facing surface. The only people it affects are the CI runner and contributors cloning the repo on Windows.

There is exactly one Windows target in this project. No alternative toolchain, no fallback, no second build. If the msvc build breaks, CI fails and the release stops; it does not silently produce something else.

No Windows toolchain is installed on this box and none will be. The standing environment rule keeps every toolchain other than Node and Python inside WSL, so the Windows binary is built in GitHub Actions and nowhere else.

Measured in WSL on 2026-10-14, to establish that CI is genuinely required rather than assumed:

| probe | result |
|---|---|
| `windows-msvc` codegen | succeeded, 0 errors before link |
| `windows-msvc` link, stock config | fails, `linker 'link.exe' not found` |
| `windows-msvc` link with bundled `rust-lld` | fails, `could not open 'kernel32.lib'` - the Windows SDK import libs are absent from this box, so lld-link cannot substitute |

That is the whole reason the Windows build lives in CI: the compiler is not the missing piece, the Windows SDK import libraries are, and those are on the runner.

### 1.2 The CRT flag, which is not optional

An msvc-target Rust binary dynamically links the MSVC runtime by default and needs `vcruntime140.dll` present on the machine. Microsoft states the redistributable "must be installed on the target system along with the app". That is exactly the hidden dependency this product exists to remove, so the flag is mandatory:

    rustc --print cfg --target x86_64-pc-windows-msvc
    (no crt-static)                       -> dynamic CRT

    rustc --print cfg -C target-feature=+crt-static --target x86_64-pc-windows-msvc
    target_feature="crt-static"           -> static CRT

Measured 2026-10-14 on rustc 1.98.0. With it, the user's machine needs nothing: no MSVC, no redistributable, no runtime installer. The exe plus `yt-dlp.exe` is the complete system.

This flag is dangerous precisely because forgetting it fails silently. The binary still builds, CI stays green, and the breakage only appears on a user machine that lacks the redistributable. So it is enforced by assertion, not by memory:

- a committed `.cargo/config.toml` with `[target.x86_64-pc-windows-msvc] rustflags = ["-C", "target-feature=+crt-static"]`, so CI and local builds cannot diverge
- G1 asserts the built exe's PE import table contains no `vcruntime*.dll` and no `msvcp*.dll`
- G10 repeats that assertion on the extracted bundle, not just the build output, so a packaging mistake is caught too

### 1.3 What the CI runner provides

From the runner image (Windows Server 2025, image 20261004.281.1, read 2026-10-14):

| item | version |
|---|---|
| Visual Studio Enterprise 2022 at `C:\Program Files\Microsoft Visual Studio\2022\Enterprise` | 17.14.37710.0 |
| `Microsoft.VisualStudio.Workload.NativeDesktop` | 17.14.36517.7 |
| `Microsoft.VisualStudio.Component.VC.Tools.x86.x64` | 17.14.36510.44 |
| `Microsoft.VisualStudio.Component.Windows11SDK.26100` | 17.14.37011.9 |
| installed Windows SDKs | 10.0.26100.0 |
| Rust, Cargo, rustup | 1.99.0, 1.99.0, 1.29.1 |
| clippy, rustfmt | 0.1.99, 1.10.0 |

A plain `cargo build --release` on that runner uses the default host toolchain `x86_64-pc-windows-msvc` and links natively against the real SDK, including the `kernel32.lib` that `lld-link` could not find locally. No cross-compiler, no local Windows toolchain, no extra install.

Version skew to control: CI has Rust 1.99.0, this WSL has 1.98.0. A committed `Cargo.lock` plus a pinned toolchain in `rust-toolchain.toml` keeps local and CI from diverging silently.

### 1.4 Build matrix, final

| artifact | host | target |
|---|---|---|
| linux bundle | WSL Ubuntu, this machine | `x86_64-unknown-linux-gnu` |
| linux static variant, if wanted | WSL Ubuntu, this machine | `x86_64-unknown-linux-musl` |
| windows bundle | GitHub Actions `windows-latest` | `x86_64-pc-windows-msvc`, host toolchain, native link, `+crt-static` |
| macOS | GitHub Actions `macos-latest` | CI only, as today; no macOS bundle is published |
| fast local Windows feedback | WSL Ubuntu | `cargo check --target x86_64-pc-windows-msvc`, codegen without a linker |

Three targets, one per published platform, no alternatives and no fallbacks. A broken Windows build fails CI rather than quietly shipping something else.

## 2. Target architecture

`app-rust/` is a parallel product dir with its own bundles. One native binary per platform. No TUI crate and no terminal framework: the menu stays a hand-rolled ANSI box on the existing reader, per the owner's no-special-frontend rule.

| capability | python source | rust std | choice |
|---|---|---|---|
| raw keys, kbhit | `msvcrt` L252, L1172 | none | FFI, `windows-sys` / `libc`. No crossterm |
| ANSI, VT enable | `ctypes` kernel32 L52-57 | none | FFI, `windows-sys` |
| shell folders | `winreg` L510-517 | none | FFI, `windows-sys` RegGetValue |
| folder dialog | `SHBrowseForFolderW` ctypes L1988-2014; tkinter/zenity/kdialog | none | FFI, `windows-sys`; posix shell-out unchanged |
| process tree kill | `taskkill /F /T` L1197 | spawn | unchanged |
| sha256 | `hashlib` L1301-1310 | none | hand-rolled, streaming, NIST vectors pinned |
| json | `json` | none | hand-rolled for the 16-field schema |
| rng | `random` (spinner pick L981-984) | none | hand-rolled PCG32 |
| east-asian width | `unicodedata` L1743-1751 | none | hand-rolled pinned table, ranges from the pitfalls doc audit fix |
| http + tls | `urllib.request` L1336-1365 | none | `ureq` + `rustls` + `webpki-roots`, or `rustls-native-certs` for the OS trust store |
| zip | `zipfile` L1389-1397 | none | `zip` |
| tar.xz | `tarfile` L1398-1408 | none | `xz2` + `tar`, needs a crate fetch, not in cache |
| charset safety | codecs with replace | strict UTF-8 | `encoding_rs`, or hand-rolled cp1252 mapping for `ascii_safe` |

### 2.1 Rules that are gates, not style

Taken from python-to-rust-refactor-pitfalls.md and made into pass/fail conditions:

- `BufWriter` once, outside every write loop; explicit `flush()` before drop; `write_all`, never bare `write`. Rust `File` is unbuffered while Python's was not, so a 1:1 port pays one syscall per line where Python paid one per 128 KiB (pitfalls 1). Hits the pump threads, the settings write and the ffmpeg extract.
- `stdout().lock()` wrapped in a `BufWriter` for the stage and spinner loops, never `println!` per line. Rust stdout is a `LineWriter` unconditionally, Python's was not (pitfalls 1).
- No `format!`, `clone()`, `to_string()`, `Vec::new()` or `HashMap::new()` inside the spinner or pump loops; `with_capacity` where the size is known (pitfalls 2, 3). `s = s + part` is fine, `format!("{}{}", s, part)` is not.
- `overflow-checks = true` in the release profile, so a ported Python arbitrary-precision assumption panics instead of silently wrapping in release (pitfalls 4, 7).
- Explicit decode at every byte boundary; no byte-offset slicing that assumes ASCII; no `as` casts across widths where the range matters (pitfalls 4, 5).
- `Result` everywhere, no bare `unwrap`, every former Python exception path mapped to a tested branch (pitfalls 6).
- Every performance claim measured on a `--release` build, never `cargo run` (pitfalls 7).

## 3. Phases and gates

Each gate is a stop. Nothing in phase N+1 starts before gate N passes, and every gate produces a number or a diff, never an assertion.

### Phase 0 - Specsheet first, no code

Amend `ytdlp-cli-specsheet.md` before any rust is written, per the manual rule. Changes required:

- NFR-1 inverted from "stdlib only, Python 3, no pip installs" to "no system runtime required to run the product", with the build-time supply-chain cost recorded honestly.
- Architecture section rewritten from single-module to the crate map in section 2 above, keeping the layer-by-responsibility split (terminal, config, resolution, argv, execution, update, UI).
- New FR for the native binary; IF-1 launcher contracts replaced (the `.bat` and `.sh` become thin wrappers over the exe and the ELF binary); IF-2 argv surface unchanged; new IF for self-replacement.
- FR-13 amended with the chosen self-update design as an owner decision, dated.
- IF-8 and NFR-4 rewritten over the Rust check suite; verification map rebuilt so every FR maps to Rust tests, in addition to the Python ones that still exist.
- OQ register updated.

**Gate G0:** owner approves the amended specsheet. No rust file exists before this.

### Phase 1 - Skeleton and the differential harness

The harness comes first, because it is what makes every later gate cheap. A tool that takes a function name plus a JSON fixture vector and prints results, callable identically from python and rust, and a differ that asserts byte equality. Then the cargo workspace in `app-rust/`, one `[[bin]]`, release profile with `overflow-checks = true`, and a rust CI workflow: fmt, clippy with `-D warnings`, test, cross-check on all three OS.

**Gate G1:** `cargo build --release` succeeds for `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` (WSL, this machine) and `x86_64-pc-windows-msvc` (CI runner). The harness runs green against a python-only function before it is allowed to judge any rust code, proving the instrument before trusting it. Rust CI green on windows, ubuntu and macos. The built Windows exe's PE import table is dumped and asserted to contain no `vcruntime*.dll` and no `msvcp*.dll`, per section 1.2. Sizes and import tables recorded.

### Phase 2 - Pure layer, leaf first

No I/O, no terminal: `one_dec_num`, `pad_speed`, `color_progress`, `dwidth`, `cut_to_width`, `tail_to_width`, `short_path`, `menu_line`, `version_tuple`, `parse_sha256`, `pick_checksum`, `sub_fail_reason`, `sub_success_lines`, `sub_fail_lines`, `fail_sentence`, `clean_url`, `sub_lang_patterns`, `parse_lang_input`, `coerce_settings`, `js_runtime_args`, `flat_frames`, `plain_cells`, `pick_spinner`, `batch_row`, `pick_list` row shapes.

**Gate G2:** every pure check has a Rust twin; the differ reports zero mismatches over a fixture corpus that includes CJK and emoji folder names, unknown language codes, malformed audio-quality strings, and the six boundary values. Hand-rolled sha256 matches the NIST vectors and the digests already pinned in README.md.

### Phase 3 - State

`load_settings`, `save_settings`, `DEFAULTS`, tmp-file plus atomic rename, LF endings, the skip-when-bytes-unchanged rule, the single save point, the coercion note lines.

**Gate G3:** over a matrix of hand-corrupted settings files, rust and python produce byte-identical `settings.json` and byte-identical coercion notes. A no-op save leaves `git diff` empty.

### Phase 4 - Resolution and diagnostics

`find_yt_dlp`, `find_ffmpeg_dir`, `probe_version` with its path+mtime memo, `auto_outdir` memo, `resolve_outdir`, `platform_name`, `show_check`.

**Gate G4:** `--check` output byte-identical on all three OS. The ffmpeg-9 probe shim, both spellings on both streams, passes in rust exactly as the python shim pins it. The memo regressions from audit F4 and F6 are named tests, not incidental.

### Phase 5 - Argument construction

`output_args`, `build_args`, `build_sub_args` across the full settings x ffmpeg-present matrix. This is the largest combinatorial surface in the product and must not drift by one flag.

**Gate G5:** generated argv vectors from rust and python compared element by element over the entire matrix, zero differences, counts reported.

### Phase 6 - Terminal layer

`enable_vt_output`, `c`, `USE_COLOR`, `read_esc`, `LineEdit`, history, `_esc_more`, `_csi_final`, `block_under_input`, `Stage`, the eight spinners, `ascii_safe`, `KeyWatch`, `hold`, `menu_items`, `render`, `hint`, `header`, every submenu, `act`.

**Gate G6:** golden transcripts, compared byte for byte. First piped and non-tty, against transcripts produced by the python implementation. Then a real tty: linux under a pty harness, windows in a real console, comparing erase sequences and column widths. The ten-line block cursor-up sequence `[11,1,11]` is pinned identically, and the write-lock serialisation under spinner-versus-freeze contention is measured.

### Phase 7 - Execution

`run_download` with its two pump threads, `_kill_tree` via taskkill `/F /T` and killpg on a new session, `clean_partials`, `newest_since`, `do_download`, `run_batch`, `multi_url`, the three verdicts, escape rc 130 breaking a batch.

**Gate G7:** a real download through both implementations with stage and verdict lines diffed. Escape mid-run yields the same cleaned-partial count. 100 consecutive runs leave zero lingering processes, zero stale temp files, zero leaked handles.

### Phase 8 - Network and updaters

`http_text`, `http_file` with its `.part` staging, the 6 s and 60 s timeout pins, `sha256_file` streaming, `expected_sha`, `extract_member` for zip and tar.xz, `install_ffmpeg`, `offer_ffmpeg`, `update_yt_dlp`, `fetch_cli`, `install_cli`, `auto_update_check` with the tty-only menu-start trigger rule.

**Gate G8:** the digest-mismatch path changes nothing on disk; a truncated archive changes nothing; an unrunnable binary changes nothing. The 6.00 s unroutable-host measurement reproduced for rust. `-U` on a downloaded `yt-dlp_linux` confirmed to self-replace, which the pinned `yt_dlp/update.py` predicts: a frozen onefile on linux x86_64 maps to variant `linux_exe`, `_FILE_SUFFIXES` maps that to the `_linux` asset, and `_NON_UPDATEABLE_REASONS` marks every `_FILE_SUFFIXES` key updatable.

### Phase 9 - Full parity

All 169 checks present as Rust tests, the differential harness over the whole fixture set, and an FR-by-FR read of the specsheet against the rust implementation.

**Gate G9:** 169/169 rust, 169/169 python, zero harness diffs, and a written conformance table in the specsheet verification map.

### Phase 10 - Bundles and v1.5.0

Four assets. Linux lean ships `yt-dlp_linux` at 40446224 bytes instead of the 3072469-byte zipapp, so the lean bundle grows to roughly 43 MB and needs nothing installed; portable adds ffmpeg and the GPL texts. Windows keeps its shape: native exe plus `yt-dlp.exe` at 17840399. Launchers become thin, keeping the `$0`-based dash-safe contract of `run-ytdlp.sh`.

**Gate G10:** every bundle extracted and run from its destination on its target platform, selftest and `--check` from the extracted tree, digests in README and RELEASE-NOTES verified at the destination through the api, tag on the release commit, `gh release` with four assets. The extracted Windows exe's PE import table is re-asserted to be free of `vcruntime*.dll` and `msvcp*.dll` (section 1.2), so a packaging mistake cannot ship a binary that needs a redistributable.

### Phase 11 - Cutover

Python stays published and supported until the owner cuts over. Then `app-python/` is retired, the README Requirements section drops the Python line, CI drops the python job, and the specsheet records the retirement.

## 4. Risks

1. **The CRT flag fails silently if lost.** Without `+crt-static` the Windows binary builds fine, CI stays green, and it breaks only on a user machine with no redistributable. Mitigated by a committed `.cargo/config.toml` plus the G1 and G10 import-table assertions (section 1.2). This is the single highest-value check in the plan because nothing else would catch it.
2. **`xz2` needs a crate fetch and is unproven on the shipped targets.** Not in the WSL cache, so tar.xz support for the linux ffmpeg fetch requires network access from WSL at first build. A probe compiled `lzma-sys` and extracted a tar.xz member byte-identically on 2026-10-14, but under a Windows toolchain that this project does not use; it has never been built for `x86_64-unknown-linux-gnu` or for msvc, which is what actually ships. G1 covers it.
3. **Self-replacement differs structurally and cannot be literal.** Python replaces a `.py`; a running Windows exe cannot be overwritten and a running ELF binary gives `ETXTBSY`. Rename-to-`.old` then place works on both. `compile()` has no analogue, so the stronger gate applies: verify the published digest, then run `<new> --selftest` once before anything is touched. This is the one place where "same as python" is deliberately not literal, and it is recorded as an owner decision.
4. **NFR-1 inverts.** The shipped artifact stops needing a system runtime, which is the point. The dev and release pipeline gains a Rust toolchain, crate supply-chain review, and a Windows build that happens in CI rather than locally. That trade belongs in the specsheet, not in a chat message.
5. **The audit regressions are the regression suite.** Boundary coercion, the 6 s timeout, both memos, the single save point, column-width truncation and the parse-miss notes are pinned inside the 169 checks. They port as named tests in every layer, not at the end.
6. **`ring` and `lzma-sys` need a C compiler at build time.** Satisfied out of the box on the CI runner by the preinstalled VS 2022 toolchain, and locally by the Linux host toolchain. Both C dependencies are unproven on the exact configurations that ship until G1 runs.
7. **Console and codepage behaviour is unverified on every target.** Every Windows probe run so far had stdout redirected, so the VT-enable path and the cp1252 safety net behind `ascii_safe` have never been exercised against a real console. G6 covers this for the real product, but no toolchain claim can be made until then.