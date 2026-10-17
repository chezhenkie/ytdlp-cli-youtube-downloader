# 00-overview.md - yt-dlp cli (ytdlp-youtube-proj)

## Header

- Title: yt-dlp cli - text-menu download wrapper around yt-dlp.
- Purpose: one source of truth for what the product is and does, written and reviewed per specsheet-design-and-review-manual.md (the design and review manual, held outside this repo). Reading this file and the code should require no other conversation.
- Owner: chezhenkie (all components, interfaces and open questions).
- Stakeholders: chezhenkie (user, engineer, tester, reviewer); consumers of the public repo and the release bundles; yt-dlp and ffmpeg upstreams as read-only dependencies. Releases are the user's alone; the git history and CHANGELOG carry author identity only as chezhenkie / noreply address.
- Approvers: chezhenkie approves every gate.
- Status: implemented. The product is the native line in `rust-src/` - one self-contained binary per platform - shipped as v2.0.0 through v2.2.0 (2026-10-16 to 2026-10-17): Windows x86_64 and Linux x86_64 from v2.0.0, macOS arm64 from v2.1.0. The earlier v1.x releases were the predecessor line and are history. Cutover to native at v2.0.1 (owner call 2026-10-16). This spec was restructured into specsheet/ on 2026-10-15 and rewritten 2026-10-16 to describe the native product only.
- Reference (the single exception): the stdlib python line in `deprecated-py-src/` is frozen at v1.5.0 - final, no further development, never deleted. It stays in-repo as the reference for the rust.yml differential, parity and session jobs (reference pinned to CPython 3.14.4 exactly) and as the legacy folder. It is not the product; nothing in this spec beyond this note describes it.
- Related documents: README.md (published usage), CHANGELOG.md (history with hashes), THIRD-PARTY-NOTICES.md (bundle licensing), LICENSE (MIT), .github/workflows/rust.yml + bundles.yml (CI).
- Evidence pin: cites in this spec name native source locations (files and functions under `rust-src/`); the dated decision trail lives in git history and CHANGELOG chapters. Cites are written against the code state at the citing commit - re-pin at release time only, when the cited file changes.

## Short description

A desktop CLI that wraps standalone yt-dlp: open it, paste a url, press enter, and the download runs with saved defaults. The menu changes those defaults and every change persists immediately to config/settings.json beside the executable; menu item 9 collects several urls and downloads them in sequence. Windows and Linux are fully supported, with no hardcoded paths anywhere.

## Table of contents

This file is 00-overview.md; the parts are numbered in reading order (01-06) and each part's section range is listed below (section 4 spans parts 02 and 03), so citations such as "section 4", "FR-8" or "NFR-1" keep resolving across code, changelog and plan.

| part | sections | describes |
|---|---|---|
| [01-problem-goals.md](01-problem-goals.md) | 1-3 | problem, goals, non-goals; FR-1..FR-18 the functional requirements; NFR-1..NFR-8 the non-functional requirements |
| [02-architecture.md](02-architecture.md) | 4 | components and layers, capability map, crate policy, language gates, build matrix, data ownership, third-party licensing |
| [03-native-decisions.md](03-native-decisions.md) | 4 | section 4 continuation: the per-phase native decision record - binding decisions, recorded deviations, evidence method |
| [04-interfaces.md](04-interfaces.md) | 5-7 | IF-1..IF-8 launchers, argv, settings schema, subprocess contracts, endpoints, CI; startup journey, user flows, states, failure handling; implementation rules, observability, process hygiene |
| [05-data.md](05-data.md) | 8-10 | settings.json field table and boundary validation; documentation duties; requirement-to-verification map and the conformance map |
| [06-phases-and-gates.md](06-phases-and-gates.md) | 12 | phases and gates, native gate 0, G9 + G10 evidence, the cutover record |
