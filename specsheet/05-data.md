## 8. Data

settings.json fields; all strings unless stated, none sensitive (no secrets, no machine-derived values shipped), retained indefinitely in the user's copy, migration via settings_version (currently unused; OQ-6 closed: it stays a schema marker until a settings format change needs one).
Boundary validation (audit F1/F2): bool fields are safe by truthiness; playlist_max_items, video_resolution, video_container, audio_format and audio_quality reach their consumers raw from the editable file. Fix design: `load_settings` coerces these in memory
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
| filename_template | str | "%(title)s" | yt-dlp -o template; `.%(ext)s` is appended when the template names no extension (a literal suffix is kept) | output_args (args.rs) |
| subtitles | bool | true | on/off | do_download (exec.rs) |
| subtitle_langs | str | "en" | comma list, wildcards pass | build_sub_args (args.rs) |
| srt_stripper | bool | false | on/off | strip_subtitles (exec.rs) |
| srt_stripper_mode | str | "text" | text timecode numbered | strip_subtitles (exec.rs) |
| info_field | bool | false | on/off | build_args (args.rs) |
| playlist | bool | false | on/off | build_args (args.rs) |
| playlist_max_items | int | 0 | 0 = all | build_args (args.rs) |

*Only auto_update_last_check differs from the code default: DEFAULTS carries 0, the live settings.json carries the stamp from the last tested run - live state, committed on purpose (DEFAULTS, config.rs). The loader copies only known keys and ignores unknown json.

## 9. Documentation

Header lists related docs. README.md is published text (usage, assets, checksums, licensing); CHANGELOG.md is the user-facing change list, one chapter per version; THIRD-PARTY-NOTICES.md travels in every bundle. The spec changes when behavior, decisions, interfaces or dependencies change (manual rule), plus recorded deviations.

## 10. Validation

Every requirement's acceptance criterion is an executable, offline check; the packaged `--selftest` runs the 192-check set (NFR-4). The former python-oracle differential layer (harness vectors, --check byte-parity, session transcripts) is retired: the frozen reference in `deprecated-py-src/` predates the srt stripper feature, so the comparison could not express it, and the native check set is the contract now. Status: FR-5's real-console clause is verified (OQ-N2); FR-13's live install dance ran against the published v2.0.0 image (OQ-N4, closed ); nothing else is pending.

| requirement | verification |
|---|---|
| FR-1, FR-2, FR-4, FR-7, FR-8, FR-9, FR-21, FR-22 | --selftest sections, 192 checks total |
| FR-3 | selftest load/save checks, LF and unchanged-bytes assertions, "coerce never rewrites settings.json"; byte-identical settings.json and coercion notes over a corrupted-input matrix |
| FR-5 | history and reader checks; the real-console pass is (OQ-N2, closed) |
| FR-6 | outdir checks plus the audit memo regression |
| FR-10 | color/width/spinner/render checks, including the erase sequences and the cursor-up block sequence |
| FR-11, FR-12 | clean_url, fail_sentence, batch checks, work-folder cleanup (argv temp path, fresh-partial removal, empty-folder removal); every run end force-removes the work folder (FR-11) |
| FR-13 | version_tuple, release_asset bundle names (windows lean zip, linux lean tar.gz, musl tar.gz, none on macos), checksum parsers, install_cli, the 6 s timeout regression; the digest and `<new> --selftest` gate is tested as such; the bundle install is covered by a windows zip test and a linux tar.gz test (tag lookup, digest gate, extraction, 0755, stage removed); the tree updater is covered by unit tests (release parse, platform asset map, digest mismatch, missing digest, incomplete tree, swap and forced rollback) and a live update moved a pinned 2026.08.19 onedir tree to the then-current published zip with the stage and `.old` trees gone; the live two-image dance ran at the v2.0.0 release; the live cli self-update ran from the published v2.6.1 and v2.6.2 bundles on windows and linux, the installed bytes equal to the published member (OQ-N4, closed) |
| FR-14 | extract_member fixtures, probe shim; `windows ffmpeg source is BtbN` and `macos ffmpeg source is shaka-project` selftest checks |
| FR-15 | the `show_check` line-shape checks (resolve.rs tests) |
| FR-16 | PE VersionInfo fields asserted on the built exe in CI and repeated on the extracted bundle at G10 |
| FR-18 | installers built on their native runners in bundles.yml and proven there: windows silent install + installed-exe PE fields + selftest 192/0 + the installed onedir tree (bin/_internal, at least 100 files, payload --version) + the installed bin/srtstripper.exe + PATH add/remove + clean uninstall; macos `installer -pkg` + selftest 192/0 + the installed tree count + payload digests (yt-dlp, ffmpeg, srtstripper) + tree writability + launcher + the shipped uninstall script (symlink, tree, receipt); G10 repeats the windows install/uninstall on the shipped setup.exe and digest-chains the pkg |
| FR-19 | the linux-musl job in bundles.yml (static-link check, extract, packaged selftest 192/0, the staged srtstripper digest, no bin/yt-dlp in the tar); install.sh musl auto-select (ldd --version plus the /lib/ld-musl-*.so.1 existence check); the v2.3.0 G10 run proved the Alpine userland: official minirootfs chroot, selftest 177/0, apk add yt-dlp, --check resolving the system yt-dlp |
| FR-20 | exec.rs stripper-pass tests (reported output path, missing-binary line, silent when off), srtstripper module tests (mode-flag map, PASS-line parse, spawn-arg shape, failure text, subs_since filtering), selftest menu/default checks; the live download with a real srtstripper is release-acceptance work (G10) |
| FR-22 | the two packaged checks ("summary row sits above the quick row", "summary key answers without a link"), plus the crate tests for do_summary (text path with the forced text mode, sound fallback with the stored settings untouched), ui.rs summary-row bytes and width, menu.rs clipboard summary flow, and the tty key rule (0/s/S on an empty line only) |
| NFR-1 | PE import table free of vcruntime*.dll and msvcp*.dll; asserted on the build output in CI (G1) and repeated on the extracted bundle at release (G10) |
| NFR-4 | rust.yml: fmt, clippy warnings denied, unit tests, musl cross-check, width and printable table pins |

Acceptance criteria for every requirement are the PASS lines of the suite that ships it. Performance is not a target beyond NFR constraints and has no measured number in this spec (no invented factors).

SELFTEST MAP for the packaged `--selftest` (G9 evidence). The report carries 192 checks by name, order and verdict. The former line-for-line compare against the frozen reference (`rust-src/tools/selftest_compare.py` and the `vectors/selftest.json` record) is retired with the python-oracle layer. Historical renames that were recorded for that compare: `cli update source is the releases api` (reference slot: `cli update url is raw main branch`), `install_cli refuses an image that fails its own checks` (reference slot: `install_cli refuses a file that does not parse`), `windows ffmpeg source is BtbN` (reference slot: `windows ffmpeg source is gyan.dev`), `ffmpeg sources cover windows, linux and macos` (reference slot: `ffmpeg sources cover windows and linux`), and `macos ffmpeg source is shaka-project` (reference slot: `no ffmpeg download for macos`). The `{archive-error}` payload of the unreadable-archive line is shape-gated (each archive library words it its own way), and the item-13 audit print goes into the live report uncaptured.
