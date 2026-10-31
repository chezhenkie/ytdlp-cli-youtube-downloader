# 00-overview.md - yt-dlp cli (ytdlp-youtube-proj)

## Header

- Title: yt-dlp cli - text-menu download wrapper around yt-dlp.
- Purpose: one source of truth for what the product is and does, written and reviewed per specsheet-design-and-review-manual.md (the design and review manual, held outside this repo). Reading this file and the code should require no other conversation.
- Stakeholders: consumers of the public repo and the release bundles; yt-dlp and ffmpeg upstreams as read-only dependencies.
- Approvers: every gate is approved before it closes.
- Status: implemented and feature complete at v2.8.0, audited and hardened at v2.8.1. The product is the native line in `rust-src/` - one self-contained binary per platform - shipped as v2.0.0 through v2.9.2: Windows x86_64 and Linux x86_64 from v2.0.0, Linux x86_64 musl (Alpine) from v2.3.0 (FR-19), macOS arm64 from v2.1.0, the srt timecode stripper (FR-20) from v2.4.0, the download info field (FR-21) from v2.5.1, the named downloads folder and the subtitle-first order from v2.7.0, the summary key (FR-22) from v2.8.0, the internal audit hardening (resolution, metadata caps, release profile, the cargo audit job) from v2.8.1, the whole-number progress counters with the steady stage tick from v2.8.2, the two-line hint block from v2.8.3, the single PATH entry in the Windows installer from v2.8.4, the GiB-stable size display with the forward-only progress percent from v2.8.5, the native-only subtitle language matching with kept partial subtitle files from v2.8.6, and the one-bundle-per-platform release with ffmpeg bundled on every platform from v2.9.0, and the subtitle language fallback (retry in the video's own language) from v2.9.1, and the offered-track fallback (an automatic retry walk over the video's own subtitle tracks) from v2.9.2. FR-1..FR-22 are shipped and no feature requirement is open; the remaining work is documentation and diagram maintenance. The earlier v1.x releases were the predecessor line and are history. Cutover to native at v2.0.1. This spec was restructured into specsheet/ and rewritten to describe the native product only.
- Reference (the single exception): the stdlib python line in `deprecated-py-src/` is frozen at v1.5.0 - final, no further development, never deleted. It stays in-repo as the legacy folder; the python-oracle differential layer that consumed it is retired and the packaged selftest is the contract now. It is not the product; nothing in this spec beyond this note describes it.
- Related documents: README.md (published usage), CHANGELOG.md (user-facing change list), THIRD-PARTY-NOTICES.md (bundle licensing), LICENSE (MIT), .github/workflows/rust.yml + bundles.yml (CI).
- Evidence pin: cites in this spec name native source locations (files and functions under `rust-src/`); the dated decision trail lives in git history. Cites are written against the code state at the citing commit - re-pin at release time only, when the cited file changes.

## Short description

A desktop CLI that wraps standalone yt-dlp: open it, paste a url, press enter, and the download runs with saved defaults. The menu changes those defaults and every change persists immediately to config/settings.json beside the executable; menu item 11 collects several urls and downloads them in sequence. Windows and Linux are fully supported, with no hardcoded paths anywhere.

## Table of contents

This file is 00-overview.md; the parts are numbered in reading order (01-06) and each part's section range is listed below (section 4 spans parts 02 and 03), so citations such as "section 4", "FR-8" or "NFR-1" keep resolving across code, changelog and plan.

| part | sections | describes |
|---|---|---|
| [01-problem-goals.md](01-problem-goals.md) | 1-3 | problem, goals, non-goals; FR-1..FR-22 the functional requirements; NFR-1..NFR-8 the non-functional requirements |
| [02-architecture.md](02-architecture.md) | 4 | components and layers, capability map, crate policy, language gates, build matrix, data model, third-party licensing |
| [03-native-decisions.md](03-native-decisions.md) | 4 | section 4 continuation: the per-phase native decision record - binding decisions, recorded deviations, evidence method |
| [04-interfaces.md](04-interfaces.md) | 5-7 | IF-1..IF-7 argv, settings schema, subprocess contracts, endpoints, CI; startup journey, user flows, states, failure handling; implementation rules, observability, process hygiene |
| [05-data.md](05-data.md) | 8-10 | settings.json field table and boundary validation; documentation duties; requirement-to-verification map and the conformance map |
| [06-phases-and-gates.md](06-phases-and-gates.md) | 12 | phases and gates, native gate 0, G9 + G10 evidence, the cutover record |
