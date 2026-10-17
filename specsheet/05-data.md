## 8. Data

settings.json fields; all strings unless stated, none sensitive (no secrets, no machine-derived values shipped), owner chezhenkie, retained indefinitely in the user's copy, migration via settings_version (currently unused; OQ-6 closed 2026-10-14: it stays a schema marker until a settings format change needs one).
Boundary validation (audit F1/F2, 2026-10-14): bool fields are safe by truthiness; playlist_max_items, video_resolution, video_container, audio_format and audio_quality reach their consumers raw from the editable file. Fix design: `load_settings` coerces these in memory
only - playlist_max_items int-parsed with fallback 0 (= all), the three vocabulary fields whitelisted against RESOLUTIONS/CONTAINERS/AUDIO_FORMATS (config.rs) with fallback to the default, audio_quality accepted only as a number or "<n>K" per yt-dlp --audio-quality semantics, fallback "0". Coercion never rewrites settings.json (user settings are theirs):

| key | type | shipped | allowed/default | consumed by |
|---|---|---|---|---|
| settings_version | int | 1 | schema marker | loader (config.rs) |
| yt_dlp_path | str | "" | path or "" (auto) | find_yt_dlp (resolve.rs) |
| ffmpeg_path | str | "" | dir or file path or "" | find_ffmpeg_dir (resolve.rs) |
| auto_update | bool | true | on/off | auto_update_check (update.rs) |
| auto_update_last_check | int | 1791925032* | unix epoch seconds | throttle (update.rs) |
| type | str | "video" | video / video no audio / audio only | build_args (args.rs) |
| video_container | str | "mp4" | mp4 mkv webm best | build_args (args.rs) |
| video_resolution | str | "1080" | 2160..360, best | build_args (args.rs) |
| audio_format | str | "mp3" | mp3 m4a opus wav best | build_args (args.rs) |
| audio_quality | str | "0" | ffmpeg audio -q value | build_args (args.rs) |
| output_dir | str | "" | "" = resolve per FR-6 | resolve_outdir (resolve.rs) |
| filename_template | str | "%(title)s" | yt-dlp -o template | output_args (args.rs) |
| subtitles | bool | true | on/off | do_download (exec.rs) |
| subtitle_langs | str | "en" | comma list, wildcards pass | build_sub_args (args.rs) |
| playlist | bool | false | on/off | build_args (args.rs) |
| playlist_max_items | int | 0 | 0 = all | build_args (args.rs) |

*Only auto_update_last_check differs from the code default: DEFAULTS carries 0, the live settings.json carries the stamp from the last tested run - live state, committed on purpose (DEFAULTS, config.rs). The loader copies only known keys and ignores unknown json.

## 9. Documentation

Header lists related docs. README.md is published text (usage, assets, checksums, licensing); CHANGELOG.md is hash-cited history by version chapter; THIRD-PARTY-NOTICES.md travels in every bundle. The spec changes when behavior, decisions, interfaces or dependencies change (manual rule), plus recorded deviations.

## 10. Validation

Every requirement's acceptance criterion is an executable, offline check; the packaged `--selftest` runs the 177-check set and the CI adds the differential layer. The reference bounds the expected values: `rust-src/tools/selftest_compare.py` pairs the two `--selftest` reports line for line (name + verdict; zero mismatches), the harness vectors pin function-level records and the session suites pin binary-level transcripts (NFR-4). Status: FR-5's real-console clause is owner-verified (OQ-N2, closed 2026-10-16c); FR-13's live install dance ran against the published v2.0.0 image (OQ-N4, closed 2026-10-16c); nothing else is pending.

| requirement | verification |
|---|---|
| FR-1, FR-2, FR-4, FR-7, FR-8, FR-9 | --selftest sections, 177 checks total; report parity against the reference stream (phase 9, conformance map below) |
| FR-3 | selftest load/save checks, LF and unchanged-bytes assertions, "coerce never rewrites settings.json"; byte-identical settings.json and coercion notes over a corrupted-input matrix |
| FR-5 | history and reader checks; the real-console pass is owner-verified (OQ-N2); the pty side is pinned by the ubuntu+macos session jobs |
| FR-6 | outdir checks plus the audit memo regression |
| FR-10 | color/width/spinner/render checks, including the erase sequences and the cursor-up block sequence |
| FR-11, FR-12 | clean_url, fail_sentence, batch checks, work-folder cleanup (argv temp path, fresh-partial removal, empty-folder removal); every run end force-removes the work folder (FR-11; session fs listings drop the reference's kept work entries from the expectation, a surviving work entry still fails) |
| FR-13 | version_tuple, checksum parsers, install_cli, the 6 s timeout regression; the digest and `<new> --selftest` gate is tested as such; the live two-image dance ran at the v2.0.0 release (OQ-N4, closed) |
| FR-14 | extract_member fixtures, probe shim; `windows ffmpeg source is BtbN` and `macos ffmpeg source is shaka-project` (FR-14; the third and fifth recorded comparator mappings) |
| FR-15 | --check output byte-identical to the reference on all three OS |
| FR-16 | PE VersionInfo fields asserted on the built exe in CI and repeated on the extracted bundle at G10 |
| FR-18 | installers built on their native runners in bundles.yml and proven there: windows silent install + installed-exe PE fields + selftest 177/0 + PATH add/remove + clean uninstall; macos `installer -pkg` + selftest 177/0 + payload digests + tree writability + launcher + the shipped uninstall script (symlink, tree, receipt); G10 repeats the windows install/uninstall on the shipped setup.exe and digest-chains the pkg |
| FR-19 | the linux-musl job in bundles.yml (static-link check, extract, packaged selftest 177/0, no bin/yt-dlp in the tar); install.sh musl auto-select (ldd --version plus the /lib/ld-musl-*.so.1 existence check); G10 runs the published musl tar and repeats the Alpine userland run - official minirootfs chroot: selftest 177/0, apk add yt-dlp, --check resolving the system yt-dlp |
| NFR-1 | PE import table free of vcruntime*.dll and msvcp*.dll; asserted on the build output in CI (G1) and repeated on the extracted bundle at release (G10) |
| NFR-4 | rust.yml: fmt, clippy warnings denied, tests, differential harness, session suites |

Acceptance criteria for every requirement are the PASS lines of the suite that ships it, with the differential rule of NFR-4 governing every check. Performance is not a target beyond NFR constraints and has no measured number in this spec (no invented factors).

CONFORMANCE MAP for the packaged `--selftest` (G9 evidence). The report carries 177 checks by name, order and verdict; the honest compare is `rust-src/tools/selftest_compare.py` over the two `--selftest` reports on the same machine (name + verdict pairs must agree line for line), and the harness vector `vectors/selftest.json` pins the check stream as a record. Five slots carry honest native names where the reference's slot differs; each is a recorded rename in selftest_compare.py rather than a name change: `cli update source is the releases api` (reference slot: `cli update url is raw main branch` - the releases-api source is the FR-13 contract), `install_cli refuses an image that fails its own checks` (reference slot: `install_cli refuses a file that does not parse` - the native gate; the rename dance runs on a scratch side-name pair with a scripted gate, because a live-spawned gate would recurse - the live dance ran at the v2.0.0 release), `windows ffmpeg source is BtbN` (reference slot: `windows ffmpeg source is gyan.dev`; FR-14), `ffmpeg sources cover windows, linux and macos` (reference slot: `ffmpeg sources cover windows and linux`; FR-14 macos amendment 2026-10-17g), and `macos ffmpeg source is shaka-project` (reference slot: `no ffmpeg download for macos`; FR-14 macos amendment). The `{archive-error}` payload of the unreadable-archive line is shape-gated (each archive library words it its own way), and the item-12 audit print goes into the live report on both sides uncaptured.
