## 13. Open questions register

| id | decision required | owner | needed | risk if left |
|---|---|---|---|---|
| OQ-B | policy for two instances writing settings.json concurrently | chezhenkie | when multi-instance use appears | one side's changes silently dropped |
| OQ-N1 | whether the two implementations share one settings.json path during the overlap window, or whether the native product dir carries its own | chezhenkie | before the first native bundle is cut | a user running both alternately overwrites one side's settings |
| OQ-N2 | console and codepage behaviour of the native build on a real Windows console and under a real pty | chezhenkie | due now: phase 6 landed 2026-10-15, verify on the CI windows artifact (ytdlp-windows-x86_64-msvc); the pty side is covered by the ubuntu+macos session jobs. Phase 7 added the executable side to the same check: a download run with the stand-in, escape mid-download, Ctrl+C mid-download, and the ffmpeg offer's y/n prompt and decline | every Windows probe to date ran with stdout redirected, so VT enable and the cp1252 safety net in ascii_safe are unverified natively |
| OQ-N3 | whether xz2, cached locally (xz2 0.1.7 + lzma-sys 0.1.20, 2026-10-14) but never built for the shipping targets, stays in the dependency set or is replaced by a pure-Rust decoder | chezhenkie | before the native tar.xz path ships | the linux ffmpeg fetch depends on a C dependency that has not yet built for x86_64-unknown-linux-gnu or msvc |

None open as of 2026-10-14 other than OQ-B and the three opened by the port amendment (OQ-N1, OQ-N2, OQ-N3). Closed items are recorded in the decision history.

