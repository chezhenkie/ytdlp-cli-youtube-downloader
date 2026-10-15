## 7. Implementation

Only what this spec defines is implemented; code comments carry the long-form rationale where a spec-sized claim cannot (FR-14's "checksum before install" order is documented in RELEASE-NOTES.md and the code comment L1447-L1453). Named owner for everything: chezhenkie. Every component stays in its layer (section 4).

Observability is the stage line system itself: one grouped preflight line, one live progress line, one frozen verdict per run - no external logging, no collected telemetry, no crash dumps. --check is the operator's window (FR-15). Selftest pins the observable surfaces called out in NFR-4.

Processes must start and stop cleanly and release everything they acquire (NFR-3): threads are daemon, stages join their writer thread with a 1.0 s timeout on done() (L1078-L1090), temp files (".ff-" dirs, tmp settings file, .new binary) are removed or replaced on every successful path and refused paths write nothing.

