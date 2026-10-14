# RUST-PORT-PLAN.md

Companion to `ytdlp-cli-specsheet.md`, which remains the source of truth for product behaviour. This document plans the implementation move from `app-python/` to `app-rust/`.

## 1. Baseline

| fact | value |
|---|---|
| `app-python/src/ytdlp.py` | 119994 bytes, 3200 lines (newline count), 100 top-level defs |
| `selftest()` | L2369-3240, 872 lines, 177 checks |
| non-test code | 2389 lines |
| runtime deps, end user | Python 3, plus ffmpeg in portable bundles only |
| windows lean | already self-contained: `yt-dlp.exe` 17840399 bytes |

Decisions taken for this port:

| decision | value |
|---|---|
| crate policy | minimal vetted set; hand-roll sha256, JSON, RNG, east-asian width |
| linux lean payload | `yt-dlp_linux` (self-contained), not the zipapp |
| python code during port | retained as behavioural oracle; all 177 checks get Rust twins |
| release | `app-rust/`, v2.0.0, parallel product dir (renumbered by owner 2026-10-14; the python line closed at v1.5.0) |
| CLI self-update | release asset + digest check + `<new> --selftest` probe, then rename-to-`.old` and place, behind the existing y/n prompt |

## 2. Toolchain

Local WSL: rustc 1.98.0 (88d9e12ae 2026-08-18), cargo 1.98.0 (797e8a9bc 2026-08-05), binaries under `~/.cargo/bin` (not on the default PATH; call by full path or source `~/.cargo/env`). Targets installed: `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` (plus `x86_64-pc-windows-gnu`, unused). `musl-gcc` is absent - see risk 6.

Crate cache (2026-10-14): 1257 `.crate` files; `xz2` 0.1.7 and `lzma-sys` 0.1.20 are present, as is `lzma-rust2` 0.15.8.

Local environment has no MSVC, no Windows SDK, no `cl.exe`, no `link.exe`. Consequences:

| operation | result |
|---|---|
| `cargo build --target x86_64-pc-windows-msvc` (codegen) | succeeds |
| same, link | fails, `linker 'link.exe' not found` |
| same, `-C linker=rust-lld` | fails, `could not open 'kernel32.lib'` |

The compiler is not the missing piece; the Windows SDK import libraries are. Those are on the CI runner, so the Windows build runs there.

### 2.1 CRT linkage

`x86_64-pc-windows-msvc` links the MSVC runtime dynamically by default, producing a binary requiring `vcruntime140.dll` on the host. That dependency violates the product's premise.

    rustc --print cfg --target x86_64-pc-windows-msvc
    -> no crt-static

    rustc --print cfg -C target-feature=+crt-static --target x86_64-pc-windows-msvc
    -> target_feature="crt-static"

Omitting the flag produces a build that compiles, passes CI, and fails only on hosts lacking the redistributable. Enforced by assertion:

- `.cargo/config.toml` with `[target.x86_64-pc-windows-msvc] rustflags = ["-C", "target-feature=+crt-static"]`, committed at gate 0 (`c34abb6`)
- G1: assert PE import table of the built exe contains no `vcruntime*.dll`, no `msvcp*.dll`
- G10: repeat the assertion against the extracted bundle

### 2.2 CI runner

`windows-latest`, Windows Server 2025, image 20261004.281.1:

| item | version |
|---|---|
| Visual Studio Enterprise 2022 | 17.14.37710.0 |
| `Workload.NativeDesktop` | 17.14.36517.7 |
| `Component.VC.Tools.x86.x64` | 17.14.36510.44 |
| `Component.Windows11SDK.26100` | 17.14.37011.9 |
| Windows SDK | 10.0.26100.0 |
| Rust, Cargo, rustup | 1.99.0, 1.99.0, 1.29.1 |
| clippy, rustfmt | 0.1.99, 1.10.0 |

Toolchain skew: CI 1.99.0, local 1.98.0. Pin with `rust-toolchain.toml`; commit `Cargo.lock`.

### 2.3 Build matrix

| artifact | host | target |
|---|---|---|
| linux | WSL | `x86_64-unknown-linux-gnu` |
| linux static, optional | WSL | `x86_64-unknown-linux-musl` |
| windows | `windows-latest` | `x86_64-pc-windows-msvc`, native link, `+crt-static` |
| macOS | `macos-latest` | CI only; no macOS bundle published |
| local windows feedback | WSL | `cargo check --target x86_64-pc-windows-msvc` |

One target per platform. No fallback target: a broken build fails CI rather than producing a different binary.

## 3. Architecture

`app-rust/` as a parallel product dir with its own bundles. One binary per platform. No TUI crate, no terminal framework; the menu remains hand-rolled ANSI over the existing reader.

| capability | python | rust std | choice |
|---|---|---|---|
| raw keys | `msvcrt` | none | FFI: `windows-sys`, `libc` |
| VT enable | `ctypes` kernel32 | none | FFI: `windows-sys` |
| shell folders | `winreg` | none | FFI: `windows-sys` |
| folder dialog | `SHBrowseForFolderW` ctypes | none | FFI: `windows-sys`; posix shell-out unchanged |
| terminal size | `os.get_terminal_size` | none | FFI: `windows-sys` `GetConsoleScreenBufferInfoW`; `libc` `TIOCGWINSZ`; error reads as no room (`block_under_input`) |
| tree kill | `taskkill /F /T` | spawn | unchanged |
| sha256 | `hashlib` | none | hand-rolled, streaming |
| json | `json` | none | hand-rolled, 16-field schema |
| rng | `random` | none | hand-rolled PCG32 |
| east-asian width | `unicodedata` | none | hand-rolled pinned table |
| http + tls | `urllib.request` | none | `ureq` + `rustls` + `webpki-roots` |
| zip | `zipfile` | none | `zip` |
| tar.xz | `tarfile` | none | `xz2` + `tar` |
| charset | fixed map + ascii replace (`ascii_safe`) | per-site (3.1) | hand-rolled 9-pair map + ascii replace; decode policy per site |

### 3.1 Enforced rules

From `python-to-rust-refactor-pitfalls.md`, plus parity rules pinned during the 2026-10-14 plan audit:

- `BufWriter` outside every write loop; explicit `flush()` before drop; `write_all`. Rust `File` is unbuffered where Python's was not.
- Interactive output is locked per frame and never wrapped in a `BufWriter`; flush after every frame and before every blocking read, matching the 16 `sys.stdout.flush()` sites in the Python (G6 asserts it). Bulk paths alone get buffered wrappers: `sha256_file` reads, `http_file` `.part` writes, `extract_member` copies. Rust stdout is a `LineWriter` unconditionally, so a frame that does not end in a newline needs the explicit flush.
- No `format!`, `clone()`, `to_string()`, `Vec::new()`, `HashMap::new()` in hot loops; `with_capacity` where size is known.
- `overflow-checks = true` in the release profile.
- Explicit decode at byte boundaries; no byte-offset slicing assuming ASCII; no `as` casts across widths where range matters.
- Decode per site, not one policy: `run_download` pumps and `http_text` decode UTF-8 with replacement; `probe_version` and the updater subprocess reads use the locale encoding with strict errors (Python `text=True`); settings reads are strict UTF-8 into the settings-unreadable branch.
- Ctrl+C (`\x03`) and EOF exit 0 saving settings; escape stays rc 130; posix raw mode is restored on every exit path, panic included - the Python restores in `finally`, the port uses a Drop guard or panic hook.
- Messages that embed exception text (`probe_version` "error: %s", the checksum-read line, the unreadable-archive line) are shape-gated, not byte-gated: gates pin the text shape and the disk state; any payload deviation is recorded.
- HTTP proxy: the Python's `urlopen` reads `<scheme>_proxy` environment variables (measured: with `http_proxy` set, the proxy is dialed) and, on Windows, falls back to the registry Internet Settings when the environment is empty (ProxyHandler docs); ureq 3.4.2 defaults read neither (`Proxy::try_from_env()` is explicit; `win-system-proxy` is a non-default feature). Decide before G8: wire both, or record the deviation in IF-7.
- `Result` throughout; no bare `unwrap`; every former exception path maps to a tested branch.
- Performance claims measured on `--release` only.

## 4. Phases and gates

Gates are stop conditions. Each produces a number or a diff.

### Phase 0 - Specsheet amendment

Amend `ytdlp-cli-specsheet.md` before any rust:

- NFR-1 from "stdlib only, Python 3" to "no system runtime"
- architecture section: single module to crate map
- new FR for the native binary
- IF-1 launcher contracts replaced
- FR-13 amended with the self-update design
- IF-8, NFR-4 rebuilt over the Rust suite; verification map extended
- OQ register updated

**G0:** amendment approved. No rust file before this.

### Phase 1 - Skeleton and harness

Harness first: a tool taking a function name plus JSON fixture vector, callable identically from python and rust, with a differ asserting byte equality; the oracle interpreter version is pinned explicitly (3.14.x, see G2). Then the cargo workspace, one `[[bin]]`, release profile with `overflow-checks = true`, rust CI (fmt, clippy `-D warnings`, test, cross-check).

**G1:** release build succeeds for `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` (local) and `x86_64-pc-windows-msvc` (CI). Harness proven green against a python-only function before it judges any rust. CI green on windows, ubuntu, macos. PE import table of the built exe dumped and asserted free of `vcruntime*.dll` and `msvcp*.dll` (2.1). Sizes and import tables recorded.

### Phase 2 - Pure layer

No I/O, no terminal: `one_dec_num`, `pad_speed`, `color_progress`, `dwidth`, `cut_to_width`, `tail_to_width`, `short_path`, `menu_line`, `version_tuple`, `parse_sha256`, `pick_checksum`, `sub_fail_reason`, `sub_success_lines`, `sub_fail_lines`, `fail_sentence`, `clean_url`, `sub_lang_patterns`, `parse_lang_input`, `coerce_settings`, `js_runtime_args`, `flat_frames`, `render_cells`, `plain_cells`, `pick_spinner`, `sub_lang_lines`, `batch_row`, `pick_list` row shapes.

**G2:** every pure check has a Rust twin; differ reports zero mismatches over a corpus including CJK and emoji folder names, unknown language codes, malformed audio-quality strings, the six boundary values (the F1/F2 junk set in `selftest`). The east-asian width table is generated from one pinned oracle - CPython 3.14.x (`unicodedata` 16.0.0; dev 3.14.6 and WSL 3.14.4 measured). CI's selftest workflow pins python 3.12 (`unicodedata` 15.0.0), so the differential workflow pins its own interpreter to the oracle version and does not reuse the 3.12 job. Hand-rolled sha256 matches the FIPS 180-4 example vectors in every run; the README-pinned digests are checked additionally when the `releases/` artifacts exist locally (dev-only; `releases/` is gitignored and absent in CI).

### Phase 3 - State

`load_settings`, `save_settings`, `DEFAULTS`, tmp-plus-atomic-rename, LF, skip-when-unchanged, single save point, coercion notes.

**G3:** over a matrix of corrupted settings files, rust and python produce byte-identical `settings.json` and byte-identical coercion notes. No-op save leaves `git diff` empty.

### Phase 4 - Resolution and diagnostics

`find_yt_dlp`, `find_ffmpeg_dir`, `probe_version` with path+mtime memo, `auto_outdir` memo, `resolve_outdir`, `platform_name`, `show_check`.

**G4:** `--check` output byte-identical between implementations on each OS (its content is OS-specific by design). The ffmpeg-9 probe shim passes. Audit F4 and F6 memo regressions are named tests.

### Phase 5 - Argument construction

`output_args`, `build_args`, `build_sub_args` over the full settings x ffmpeg-present matrix.

**G5:** argv vectors compared element-wise across the entire matrix; zero differences, counts reported.

### Phase 6 - Terminal layer

`enable_vt_output`, `c`, `USE_COLOR`, `read_esc`, `LineEdit`, history, `_esc_more`, `_csi_final`, `block_under_input`, `Stage`, eight spinners, `ascii_safe`, `KeyWatch`, `hold`, `menu_items`, `render`, `hint`, `header`, submenus, `act`.

**G6:** golden transcripts compared byte-wise. Piped and non-tty first. Then a real tty: linux under a pty harness, windows in a real console, comparing erase sequences and column widths. Cursor-up sequence `[11,1,11]` pinned. Write-lock serialisation under spinner-versus-freeze contention measured. The interactive flush points of 3.1 are asserted so a frame cannot sit in a buffer.

### Phase 7 - Execution

`run_download` with two pump threads, `_kill_tree`, `clean_partials`, `newest_since`, `do_download`, `run_batch`, `multi_url`, three verdicts, escape rc 130 breaking a batch.

**G7:** real download through both implementations, stage and verdict lines diffed. Escape mid-run yields identical cleaned-partial counts. Ctrl+C mid-run kills the tree, exits 0 and restores the terminal. 100 consecutive runs leave no lingering processes, temp files, or handles.

### Phase 8 - Network and updaters

`http_text`, `http_file` with `.part` staging, 6 s and 60 s timeout pins, `sha256_file`, `expected_sha`, `extract_member` for zip and tar.xz, `install_ffmpeg`, `offer_ffmpeg`, `update_yt_dlp`, `fetch_cli`, `install_cli`, `auto_update_check`.

**G8:** digest mismatch, truncated archive, and unrunnable binary each change nothing on disk. The 6.00 s unroutable-host measurement reproduced. The proxy environment path is exercised offline (a fake proxy must be dialed) or the IF-7 deviation is recorded. `-U` on a downloaded `yt-dlp_linux` confirmed to self-replace: per pinned `yt_dlp/update.py`, a frozen onefile on linux x86_64 maps to variant `linux_exe`, `_FILE_SUFFIXES` maps that to the `_linux` asset, and `_NON_UPDATEABLE_REASONS` marks every `_FILE_SUFFIXES` key updatable.

### Phase 9 - Parity

All 177 checks as Rust tests, harness across the full fixture set, FR-by-FR read against the implementation.

**G9:** 177/177 rust, 177/177 python, zero harness diffs, conformance table written into the specsheet verification map.

### Phase 10 - Bundles and v2.0.0

Four assets. Linux lean ships `yt-dlp_linux` at 40446224 bytes instead of the 3072469-byte zipapp (both are release 2026.08.19 asset sizes; they sum to 43518693). The bundle replaces one entry with the other, so the shipped size lands below that sum - G10 records the measured figure. Portable adds ffmpeg plus GPL texts. Windows unchanged in shape. Launchers become thin, `run-ytdlp.sh` keeping its `$0`-based dash-safe contract.

**G10:** every bundle extracted and run from its destination on its target platform; selftest and `--check` from the extracted tree; digests in README and RELEASE-NOTES verified at the destination via the api; tag on the release commit; `gh release` with four assets. PE import table of the extracted Windows exe re-asserted free of `vcruntime*.dll` and `msvcp*.dll`.

### Phase 11 - Cutover

Python stays published until cutover. Then `app-python/` is retired, README drops the Python requirement, CI drops the python job, specsheet records the retirement.

## 5. Risks

1. **CRT flag fails silently if lost.** Builds clean, CI green, breaks only on hosts without the redistributable. Mitigated by committed `.cargo/config.toml` plus G1 and G10 assertions. Highest-value check in the plan: nothing else catches it.
2. **`xz2` unproven on shipped targets.** The crates are cached locally (`xz2` 0.1.7, `lzma-sys` 0.1.20, 2026-10-14), so no fetch is needed; what stays unproven is the build for the shipping targets. A probe compiled `lzma-sys` and extracted a tar.xz member byte-identically, but under a Windows toolchain this project does not use; never built for `x86_64-unknown-linux-gnu` or msvc. G1 covers it.
3. **Self-replacement cannot be literal.** A running Windows exe cannot be overwritten; a running ELF gives `ETXTBSY`. Rename-to-`.old` then place works on both. `compile()` has no analogue, so the gate is digest verification plus a `--selftest` run before anything is touched.
4. **NFR-1 inverts.** The shipped artifact stops needing a system runtime; the pipeline gains a Rust toolchain, crate review, and a CI-side Windows build. Trade belongs in the specsheet.
5. **Audit regressions are the regression suite.** Boundary coercion, 6 s timeout, both memos, single save point, column-width truncation, parse-miss notes. Port as named tests in each layer, not at the end.
6. **C build dependencies.** `ring` and `lzma-sys` need a C compiler: the CI runner's VS 2022 toolchain on Windows, `gcc`/`clang` locally (both present). `musl-gcc` is absent, so a musl build with C dependencies needs `musl-tools` first and the musl leg stays optional until then. Both unproven on shipping configurations until G1.
7. **Console and codepage behaviour unverified on all targets.** Every Windows probe ran with stdout redirected, so VT enable and the cp1252 safety net in `ascii_safe` have never run against a real console. G6 covers it; no toolchain claim holds until then.
