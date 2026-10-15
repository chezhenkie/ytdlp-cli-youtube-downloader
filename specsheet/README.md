# specsheet - yt-dlp cli (ytdlp-youtube-proj)

## Header

- Title: yt-dlp cli - text-menu download wrapper around yt-dlp.
- Purpose: one source of truth for what the product is and does, written and reviewed per specsheet-design-and-review-manual.md (the design and review manual, held outside this repo). Reading this file and the code should require no other conversation.
- Owner: chezhenkie (all components, interfaces and open questions).
- Stakeholders: chezhenkie (user, engineer, tester, reviewer); consumers of the public repo and the release bundles; yt-dlp and ffmpeg upstreams as read-only dependencies.
- Approvers: chezhenkie approves every gate.
- Status: implemented. This spec documents the shipped v1.4 line (2026-10-13, remote https://github.com/chezhenkie/ytdlp-cli-youtube-downloader), the v1.4.1 rebuild cut, the v1.4.2 release (2026-10-14) and the v1.5.0 definitive-python release (2026-10-14). Approved by the owner on 2026-10-14. Amended 2026-10-14 for the planned port to a native binary (`app-rust/`): sections 3 NFR-1, 4, 5 IF-1/IF-7/IF-8, 2 FR-13, 10 and 13 now describe both implementations. The Python implementation remains shipped, supported and authoritative until cutover; every amendment is additive and changes no requirement of the shipped product. Amended again 2026-10-14 for the download work folder: yt-dlp writes its per-chunk fragment and part files into a work folder inside the output folder, so the user's folder only ever shows finished files (FR-7, FR-8, FR-11); owner decision, shipped in v1.5.0. Restructured 2026-10-15 into the specsheet/ folder: this README is the main document and sections 1-14 are numbered part files.
- Related documents: README.md (published usage), CHANGELOG.md (history with hashes), THIRD-PARTY-NOTICES.md (bundle licensing), LICENSE (MIT), .github/workflows/selftest.yml (CI), RUST-PORT-PLAN.md (phases and gates for the native port).

## Short description

A desktop CLI that wraps standalone yt-dlp: open it, paste a url, press enter, and the download runs with saved defaults. The menu changes those defaults and every change persists immediately to app-python/config/settings.json; menu item 9 collects several urls and downloads them in sequence. The shipped implementation is stdlib-only Python (authoritative until the owner's cutover); a native Rust port of the same specification lives in app-rust/ for the v2.0.0 line. Windows and Linux are fully supported, with no hardcoded paths anywhere.

## Table of contents

Parts are numbered to match their spec section numbers, so citations such as "section 4", "FR-8" or "NFR-1" keep resolving across code, changelog and plan.

| part | describes |
|---|---|
| [01-problem-goals.md](01-problem-goals.md) | problem, goals, non-goals (section 1) |
| [02-functional-requirements.md](02-functional-requirements.md) | FR-1..FR-15, the functional requirements (section 2) |
| [03-non-functional-requirements.md](03-non-functional-requirements.md) | NFR-1..NFR-8, the non-functional requirements (section 3) |
| [04-architecture.md](04-architecture.md) | components and layers, capability map, crate policy, build matrix, port notes (section 4) |
| [05-interfaces.md](05-interfaces.md) | IF-1..IF-8: launchers, argv, settings schema, subprocess contracts, endpoints, CI (section 5) |
| [06-functionality.md](06-functionality.md) | startup journey, user flows, states, failure handling (section 6) |
| [07-implementation.md](07-implementation.md) | implementation rules, observability, process hygiene (section 7) |
| [08-data.md](08-data.md) | settings.json field table and boundary validation (section 8) |
| [09-documentation.md](09-documentation.md) | documentation duties and the related documents (section 9) |
| [10-validation.md](10-validation.md) | requirement-to-verification map, Python and native columns (section 10) |
| [11-stakeholders.md](11-stakeholders.md) | stakeholder roles (section 11) |
| [12-phases-and-gates.md](12-phases-and-gates.md) | phases and gates, Python release gates, native gate 0 (section 12) |
| [13-open-questions.md](13-open-questions.md) | open questions register: OQ-B, OQ-N1, OQ-N2, OQ-N3 (section 13) |
| [14-decision-history.md](14-decision-history.md) | dated decision record, oldest first (section 14) |

