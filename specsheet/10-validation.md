## 10. Validation

Two columns: the Python column is live, the native column is the contract the port is held to. A native cell marked "required" is an obligation on the port, not a description of existing tests; a cell counts as met only when its whole check set exists, and the phase-by-phase progress of the native suite is recorded in the decision history (phase 2: the pure layer passes the differential differ and carries rust twins).

| requirement | Python (live) | native (required, not yet present) |
|---|---|---|
| FR-1, FR-2, FR-4, FR-7, FR-8, FR-9 | --selftest sections, 177 checks total, live run 2026-10-14 | Rust twins of the same checks, same expected output |
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

