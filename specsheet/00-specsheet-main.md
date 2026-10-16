# 00-specsheet-main.md - yt-dlp cli (ytdlp-youtube-proj)

## Header

- Title: yt-dlp cli - text-menu download wrapper around yt-dlp.
- Purpose: one source of truth for what the product is and does, written and reviewed per specsheet-design-and-review-manual.md (the design and review manual, held outside this repo). Reading this file and the code should require no other conversation.
- Owner: chezhenkie (all components, interfaces and open questions).
- Stakeholders: chezhenkie (user, engineer, tester, reviewer); consumers of the public repo and the release bundles; yt-dlp and ffmpeg upstreams as read-only dependencies. Releases are the user's alone; the git history and CHANGELOG carry author identity only as chezhenkie / noreply address.
- Approvers: chezhenkie approves every gate.
- Status: implemented. This spec documents the shipped v1.4 line (2026-10-13, remote https://github.com/chezhenkie/ytdlp-cli-youtube-downloader), the v1.4.1 rebuild cut, the v1.4.2 release (2026-10-14) and the v1.5.0 definitive-python release (2026-10-14). Approved by the owner on 2026-10-14. Amended 2026-10-14 for the port to a native binary (`app-rust/`): sections 3 NFR-1, 4, 5 IF-1/IF-7/IF-8, 2 FR-13, 10 and 13 now describe both implementations.
The Python implementation stays shipped and authoritative as the frozen reference oracle until cutover (owner call 2026-10-16: python is done at v1.5.0 - no further development; the native line is the successor product; app-python stays in the repo as a legacy folder after cutover, never deleted); every amendment is additive and changes no requirement of the shipped product.
Amended again 2026-10-14 for the download work folder: yt-dlp writes its per-chunk fragment and part files into a work folder inside the output folder, so the user's folder only ever shows finished files (FR-7, FR-8, FR-11); owner decision, shipped in v1.5.0. Restructured 2026-10-15 into the specsheet/ folder: this file is the main document and the spec sections live
in numbered part files. Consolidated 2026-10-16: one part file carries a contiguous section range (part number = first section), and the per-entry decision history was retired - decisions are recorded in CHANGELOG chapters and git history.
- Related documents: README.md (published usage), CHANGELOG.md (history with hashes), THIRD-PARTY-NOTICES.md (bundle licensing), LICENSE (MIT), .github/workflows/selftest.yml (CI), RUST-PORT-PLAN.md (phases and gates for the native port).
- Evidence pin: every line cite in this spec (L<N>, e.g. ytdlp.py L1562-L1639) is a location in the recorded code state it was written against - the v1.5.0 tag for the python side, the commit recorded with each port note for the native side - not the working tree. Cite them freely; do not re-verify them before an edit; re-pin only at release time when the cited file changes.

## Short description

A desktop CLI that wraps standalone yt-dlp: open it, paste a url, press enter, and the download runs with saved defaults. The menu changes those defaults and every change persists immediately to app-python/config/settings.json; menu item 9 collects several urls and downloads them in sequence. The shipped implementation is stdlib-only Python, frozen at v1.5.0 (authoritative until the owner's cutover); the native successor for the v2.0.0 line lives in app-rust/. Windows and Linux are fully
supported, with no hardcoded paths anywhere.

## Table of contents

This file is 00-specsheet-main.md; each part carries a contiguous block of spec sections and its number is the first section in the block (01 = sections 1-3, 05 = sections 5-7, 08 = sections 8-10), so citations such as "section 4", "FR-8" or "NFR-1" keep resolving across code, changelog and plan.

| part | sections | describes |
|---|---|---|
| [01-problem-goals.md](01-problem-goals.md) | 1-3 | problem, goals, non-goals; FR-1..FR-15 the functional requirements; NFR-1..NFR-8 the non-functional requirements |
| [04-architecture.md](04-architecture.md) | 4 | components and layers, capability map, crate policy, language gates, build matrix, data ownership, third-party licensing |
| [04-port-notes.md](04-port-notes.md) | 4 | section 4 continuation: the per-phase native port-note audit trail (phases 2-9) |
| [05-interfaces.md](05-interfaces.md) | 5-7 | IF-1..IF-8 launchers, argv, settings schema, subprocess contracts, endpoints, CI; startup journey, user flows, states, failure handling; implementation rules, observability, process hygiene |
| [08-data.md](08-data.md) | 8-10 | settings.json field table and boundary validation; documentation duties; requirement-to-verification map, the two implementation columns and the phase-9 conformance map |
| [12-phases-and-gates.md](12-phases-and-gates.md) | 12 | phases and gates, Python release gates, native gate 0, G9 spec and evidence |
| [13-open-questions.md](13-open-questions.md) | 13 | open questions register: OQ-B, OQ-N2, OQ-N4 |
