## 13. Open questions register

| id | decision required | owner | needed | risk if left |
|---|---|---|---|---|
| OQ-B | policy for two instances writing settings.json concurrently | chezhenkie | when multi-instance use appears | one side's changes silently dropped |
| OQ-N1 | whether the two implementations share one settings.json path during the overlap window, or whether the native product dir carries its own | chezhenkie | before the first native bundle is cut | a user running both alternately overwrites one side's settings |
| OQ-N2 | console and codepage behaviour of the native build on a real Windows console and under a real pty | chezhenkie | due now: phase 6 landed 2026-10-15, verify on the CI windows artifact (ytdlp-windows-x86_64-msvc); the pty side is covered by the ubuntu+macos session jobs. Phase 7 added the executable side to the same check: a download run with the stand-in, escape mid-download, Ctrl+C mid-download, and the ffmpeg offer's y/n prompt and decline | every Windows probe to date ran with stdout redirected, so VT enable and the cp1252 safety net in ascii_safe are unverified natively |

None open as of 2026-10-15 other than OQ-B, OQ-N1 and OQ-N2. OQ-N3 was closed by owner call on 2026-10-15: the pure-Rust lzma-rust2 decoder replaces xz2/lzma-sys for the tar.xz path (see the decision history). Closed items are recorded in the decision history.

