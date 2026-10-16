## 13. Open questions register

| id | decision required | owner | needed | risk if left |
|---|---|---|---|---|
| OQ-B | policy for two instances writing settings.json concurrently | chezhenkie | when multi-instance use appears | one side's changes silently dropped |
| OQ-N2 | console and codepage behaviour of the native build on a real Windows console and under a real pty | chezhenkie | due now: phase 6 landed 2026-10-15, verify on the CI windows artifact (ytdlp-windows-x86_64-msvc); the pty side is covered by the ubuntu+macos session jobs. Phase 7 added the executable side to the same check: a download run with the stand-in, escape mid-download, Ctrl+C mid-download, and the ffmpeg offer's y/n prompt and decline. Phase 9 adds the phase-8 colors to the same
session: the ffmpeg offer's live download stage text on a real console, item 11's transcript and the startup notice wording | every Windows probe to date ran with stdout redirected, so VT enable and the cp1252 safety net in ascii_safe are unverified natively |

Register stands at OQ-B and OQ-N2. OQ-N1 was closed by owner call on 2026-10-16: python locks at v1.5.0 - final python, no further development, kept as the frozen differential oracle and, past cutover, the in-repo legacy folder (owner call 2026-10-16: never deleted, mentioned in the docs only in passing); the native product is the successor and its product dir carries its own settings.json, no shared path between the two.
OQ-N4 was closed on 2026-10-16 at the v2.0.0 release (G10 evidence in section 12): the ignored live-dance test (`update.rs live_two_image_dance_against_a_published_image`, run manually with `YTDLP_LIVE_IMAGE` pointing at the published `ytdlp-native-linux-x86_64`) drove the real `<new> --selftest` spawn gate against the published image - the placed file is byte-identical to the release asset, `.old` waits for the fresh-start cleanup (`remove_previous_image`), and a dead staged file is refused
with the running image untouched.
OQ-N3 was closed by owner call on 2026-10-15: the pure-Rust lzma-rust2 decoder replaces xz2/lzma-sys for the tar.xz path (revision recorded in the CHANGELOG v2.0.0 phase-8 chapter and git history); closed items keep their verdict line here, the dated trail lives in git history.

