## 8. Data

settings.json fields; all strings unless stated, none sensitive (no secrets, no machine-derived values shipped), owner chezhenkie, retained indefinitely in the user's copy, migration via settings_version (currently unused, OQ-6). Boundary validation (audit F1/F2, 2026-10-14): bool fields are safe by truthiness; playlist_max_items, video_resolution, video_container, audio_format and audio_quality reach their consumers raw from the editable file. Fix design: load_settings coerces these in memory only - playlist_max_items int-parsed with fallback 0 (= all), the three vocabulary fields whitelisted against RESOLUTIONS/CONTAINERS/AUDIO_FORMATS with fallback to the default, audio_quality accepted only as a number or "<n>K" per yt-dlp --audio-quality SPEC semantics, fallback "0". Coercion never rewrites settings.json (user settings are theirs):

| key | type | shipped | allowed/default | consumed by |
|---|---|---|---|---|
| settings_version | int | 1 | schema marker | none yet (L320) |
| yt_dlp_path | str | "" | path or "" (auto) | find_yt_dlp L620-L622 |
| ffmpeg_path | str | "" | dir or file path or "" | find_ffmpeg_dir L633-L638 |
| auto_update | bool | true | on/off | auto_update_check L2164 |
| auto_update_last_check | int | 1791925032* | unix epoch seconds | throttle L2168-L2173 |
| type | str | "video" | video / video no audio / audio only | build_args L772 |
| video_container | str | "mp4" | mp4 mkv webm best | build_args L791-L792 |
| video_resolution | str | "1080" | 2160..360, best | build_args L782,702-705 |
| audio_format | str | "mp3" | mp3 m4a opus wav best | build_args L777 |
| audio_quality | str | "0" | ffmpeg audio -q value | build_args L778 |
| output_dir | str | "" | "" = resolve per FR-6 | resolve_outdir L583-L586 |
| filename_template | str | "%(title)s" | yt-dlp -o template | output_args L754 |
| subtitles | bool | true (default on, L333) | on/off | do_download L1623 |
| subtitle_langs | str | "en" | comma list, wildcards pass | build_sub_args L818 |
| playlist | bool | false | on/off | build_args L799-L804 |
| playlist_max_items | int | 0 | 0 = all | build_args L800-L802 |

*Only auto_update_last_check differs from the code default: DEFAULTS carries 0, the live settings.json carries the stamp from the last tested run - live state, committed on purpose (DEFAULTS L319-L337). Loader copies only known keys and ignores unknown json (L473-L475).

## 9. Documentation

Header lists related docs. README.md is published text (usage, assets, checksums, licensing); CHANGELOG.md is hash-cited history by version chapter; THIRD-PARTY-NOTICES.md travels in every bundle. Groupie for gaps is the open questions register; the spec changes when behavior, decisions, interfaces or dependencies change (manual rule), plus recorded deviations.

## 10. Validation

Two columns: the Python column is live, the native column is the contract the port is held to. A native cell marked "required" is an obligation on the port, not a description of existing tests; a cell counts as met only when its whole check set exists, and the phase-by-phase progress of the native suite is recorded in the CHANGELOG chapters (phase 2: the pure layer passes the differential differ and carries rust twins).

| requirement | Python (live) | native (required, not yet present) |
|---|---|---|
| FR-1, FR-2, FR-4, FR-7, FR-8, FR-9 | --selftest sections, 177 checks total, live run 2026-10-14 | Rust twins of the same checks, same expected output; the packaged `--selftest` runs them (phase 9, see the conformance map below) |
| FR-3 | selftest load/save checks, LF and unchanged-bytes assertions, "coerce never rewrites settings.json" | twin, plus byte-identical settings.json and coercion notes over a corrupted-input matrix |
| FR-5 | history and reader checks | twin; console behaviour under a real tty, not only redirected stdout |
| FR-6 | outdir checks plus the audit memo regression | twin |
| FR-10 | color/width/spinner/render checks | twin, including the erase sequences and the cursor-up block sequence |
| FR-11, FR-12 | clean_url, fail_sentence, batch checks, work-folder cleanup (argv temp path, fresh-partial removal, empty-folder removal) | twin, including identical cleaned-partial counts on escape |
| FR-13 | version_tuple, checksum parsers, install_cli, the 6 s timeout regression | twin of the timeout regression; the digest and `<new> --selftest` gate replaces install_cli's compile() gate per FR-13 and is tested as such |
| FR-14 | extract_member fixtures, probe shim | twin |
| FR-15 | --check output | byte-identical `--check` output on all three OS |
| NFR-1 | not applicable (Python 3 on PATH is a stated prerequisite) | PE import table free of vcruntime*.dll and msvcp*.dll, on the build output and again on the extracted bundle |
| NFR-4 | workflow | second workflow: fmt, clippy warnings denied, tests, differential harness |

Acceptance criteria for every requirement are the PASS lines of the suite that ships it, with the differential rule of NFR-4 governing any native twin. Performance is not a target beyond NFR constraints and has no measured number in this spec (no invented factors).

PHASE 9 CONFORMANCE MAP for the packaged `--selftest` (G9 evidence). The native report carries python's 177 checks by name, order and verdict; the honest compare is `app-rust/tools/selftest_compare.py` over the two implementations' `--selftest` reports on the same machine (name + verdict pairs must agree line for line), and the harness vector `vectors/selftest.json` pins the check stream as a record. The two comparator mappings, each a recorded deviation rather than a name change: `cli update url is raw main branch` runs natively as `cli update source is the releases api` (the native FR-13 contract; the raw-main url is python-only), and `install_cli refuses a file that does not parse` runs natively as `install_cli refuses an image that fails its own checks` (the native gate; the rename dance runs on a scratch side-name pair with a scripted gate, because a live-spawned gate would recurse - see OQ-N4 for the live dance). The `{archive-error}` payload of the unreadable-archive line is shape-gated like the phase-8 records (the real stdlib wording differs from the real native wording), and the item-12 audit print goes into the live report on both implementations uncaptured.
