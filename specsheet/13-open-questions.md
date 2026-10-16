## 13. Open questions register

| id | decision required | owner | needed | risk if left |
|---|---|---|---|---|
| OQ-B | policy for two instances writing settings.json concurrently | chezhenkie | when multi-instance use appears | one side's changes silently dropped |
| OQ-N1 | whether the two implementations share one settings.json path during the overlap window, or whether the native product dir carries its own | chezhenkie | before the first native bundle is cut | a user running both alternately overwrites one side's settings |
| OQ-N2 | console and codepage behaviour of the native build on a real Windows console and under a real pty | chezhenkie | due now: phase 6 landed 2026-10-15, verify on the CI windows artifact (ytdlp-windows-x86_64-msvc); the pty side is covered by the ubuntu+macos session jobs. Phase 7 added the executable side to the same check: a download run with the stand-in, escape mid-download, Ctrl+C mid-download, and the ffmpeg offer's y/n prompt and decline. Phase 9 adds the phase-8 colors to the same
session: the ffmpeg offer's live download stage text on a real console, item 11's transcript and the startup notice wording | every Windows probe to date ran with stdout redirected, so VT enable and the cp1252 safety net in ascii_safe are unverified natively |
| OQ-N4 | the `<new> --selftest` self-install gate is wired (install_cli spawns it) but runs only when a real release asset exists, so the live two-image dance (old .old cleanup on a fresh start, the no-op refusals) is unproven; the packaged selftest dances on a scratch side-name pair with a scripted gate by construction (a live-spawned gate would recurse into itself) | chezhenkie | due at the first native tagged release: stage a copy of the published image as `<self>.new`, drive the update
path, confirm the spawn rc gates the dance, the rename dance completes and the next start removes `.old` | the shipped self-update replaces the running image with an unverified one if the gate misfires |

Register stands at OQ-B, OQ-N1, OQ-N2 and - new 2026-10-16 with the phase-9 landing - OQ-N4. OQ-N3 was closed by owner call on 2026-10-15: the pure-Rust lzma-rust2 decoder replaces xz2/lzma-sys for the tar.xz path (revision recorded in the CHANGELOG v2.0.0 phase-8 chapter and git history); closed items keep their verdict line here, the dated trail lives in git history.

