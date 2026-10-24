//! The ffmpeg layer: the source pin, the install dance and the offer.
//! The context and the line printer stay in the parent (do_download
//! shares them).

use super::{DownloadCtx, print_out};

use crate::config::Obj;
use crate::net;
use crate::pyfs;
use crate::pytext::py_strip;
use crate::resolve::{self};
use crate::sha256;
use crate::term::{A_AMBER, ascii_safe, c};
use crate::tty::{self, ReadOutcome, Stage};
use crate::update;

/// Where a lean install fetches ffmpeg from. The Linux side is python's
/// BtbN tarball; the Windows side is the BtbN win64 GPL zip per the
/// owner-decided FR-14 amendment (2026-10-16c; python kept gyan.dev);
/// macos was added in the 2026-10-17g amendment: the shaka-project
/// static build - a public repo with contributors and CI-built assets
/// that carry published sha256 digests (owner call: no one-person
/// download sites). `sha_kind` names what `sha_url` carries: "checksums"
/// for the provider's checksum file, "gh-api" for the GitHub release API
/// object whose per-asset `digest` field is read instead. `how` is the
/// payload shape: "zip" / "tar.xz" hold the binary in an archive, "raw"
/// is the published binary itself.
pub struct FfmpegSource {
    pub url: &'static str,
    pub sha_url: &'static str,
    pub binname: &'static str,
    pub how: &'static str,
    pub sha_kind: &'static str,
}

pub fn ffmpeg_source(platform: &str) -> Option<FfmpegSource> {
    match platform {
        // owner-decided 2026-10-16c (FR-14 amendment): the windows offer
        // fetches the BtbN win64 GPL build, digest-first through the
        // provider's checksums.sha256 which is read fresh on every run,
        // so the moving latest tag cannot go stale (gyan.dev measured
        // ~300 KB/s on the owner's line during the OQ-N2 console pass)
        "windows" => Some(FfmpegSource {
            url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-win64-gpl-9.0.zip",
            sha_url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256",
            binname: "ffmpeg.exe",
            how: "zip",
            sha_kind: "checksums",
        }),
        "linux" => Some(FfmpegSource {
            url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz",
            sha_url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256",
            binname: "ffmpeg",
            how: "tar.xz",
            sha_kind: "checksums",
        }),
        // owner-decided 2026-10-17g (FR-14 amendment): macos was
        // instruction-only while the pinned providers shipped no mac
        // build; the shaka-project release is built from source on
        // GitHub's runners (x264/x265/lame -> the GPL line, same as the
        // windows and linux payloads), its tag is pinned so the asset
        // cannot move, and the API digest is checked before anything is
        // installed. arm64 only - that is the runner and the payload.
        "macos" => Some(FfmpegSource {
            url: "https://github.com/shaka-project/static-ffmpeg-binaries/releases/download/n8.1.2-1/ffmpeg-osx-arm64",
            sha_url: "https://api.github.com/repos/shaka-project/static-ffmpeg-binaries/releases/tags/n8.1.2-1",
            binname: "ffmpeg",
            how: "raw",
            sha_kind: "gh-api",
        }),
        _ => None,
    }
}

/// `install_ffmpeg(stage)`: fetch, verify, test and install ffmpeg into
/// bin/ for this platform.
///
/// The order is the gate (FR-14): the published digest is read first and
/// a mismatching download is discarded, and the extracted binary must
/// answer `--version` here before it is moved over whatever is already
/// in bin/. So a truncated or wrong-platform build can never replace a
/// working ffmpeg. Returns the binary path, or None with the reason
/// printed; the temp directory goes in every path, like python's
/// `finally`.
pub fn install_ffmpeg(
    stage: Option<&mut Stage>,
    platform: &str,
    bin_dir: &str,
    is_windows: bool,
    ctx: &mut DownloadCtx,
) -> Option<String> {
    let Some(source) = ffmpeg_source(platform) else {
        print_out(
            &ctx.sink,
            &format!("  no published ffmpeg download for {platform}"),
        );
        print_out(
            &ctx.sink,
            "  install it with your package manager - it is then found on PATH",
        );
        return None;
    };
    let asset = source.url.rsplit('/').next().unwrap_or(source.url);
    let digest = if source.sha_kind == "gh-api" {
        update::expected_sha_api(source.sha_url, asset, ctx.http, &ctx.sink)?
    } else {
        update::expected_sha(source.url, source.sha_url, asset, ctx.http, &ctx.sink)?
    };
    if let Err(e) = std::fs::create_dir_all(bin_dir) {
        print_out(&ctx.sink, &format!("  bin/ is not writable: {e}"));
        return None;
    }
    let Ok(tmpdir) = pyfs::mkdtemp(".ff-", bin_dir) else {
        print_out(&ctx.sink, "  bin/ is not writable: no temp directory");
        return None;
    };
    let blob = resolve::py_path_join(&tmpdir, asset);
    let result = install_ffmpeg_into(
        &source, &digest, &blob, &tmpdir, bin_dir, is_windows, stage, ctx,
    );
    pyfs::rmtree_quiet(&tmpdir);
    result
}

/// The body of `install_ffmpeg`, with the temp directory already made.
#[allow(clippy::too_many_arguments)]
fn install_ffmpeg_into(
    source: &FfmpegSource,
    digest: &str,
    blob: &str,
    tmpdir: &str,
    bin_dir: &str,
    is_windows: bool,
    stage: Option<&mut Stage>,
    ctx: &mut DownloadCtx,
) -> Option<String> {
    let Some(stage) = stage else {
        // no stage: the download reports no progress, python's
        // `if stage is not None` guards
        let http = &mut *ctx.http;
        if let Err(e) = http.file(source.url, blob, net::DL_TIMEOUT, None) {
            print_out(&ctx.sink, &format!("  download failed: {}", ascii_safe(&e)));
            return None;
        }
        return verify_and_install(source, digest, blob, tmpdir, bin_dir, is_windows, None, ctx);
    };
    let mut stage = stage;
    {
        let http = &mut *ctx.http;
        let mut report = |text: &str| stage.set_live(Some(text.to_string()));
        if let Err(e) = http.file(source.url, blob, net::DL_TIMEOUT, Some(&mut report)) {
            print_out(&ctx.sink, &format!("  download failed: {}", ascii_safe(&e)));
            return None;
        }
    }
    verify_and_install(
        source,
        digest,
        blob,
        tmpdir,
        bin_dir,
        is_windows,
        Some(&mut stage),
        ctx,
    )
}

/// Everything after the download: verify, unpack, probe, install.
#[allow(clippy::too_many_arguments)]
fn verify_and_install(
    source: &FfmpegSource,
    digest: &str,
    blob: &str,
    tmpdir: &str,
    bin_dir: &str,
    is_windows: bool,
    stage: Option<&mut Stage>,
    ctx: &mut DownloadCtx,
) -> Option<String> {
    if let Some(st) = stage.as_deref() {
        st.set_live(Some("verifying the checksum".to_string()));
    }
    if sha256::sha256_file(blob).ok().as_deref() != Some(digest) {
        print_out(&ctx.sink, "  checksum mismatch - nothing was changed");
        return None;
    }
    if source.how != "raw"
        && let Some(st) = stage.as_deref()
    {
        st.set_live(Some("unpacking".to_string()));
    }
    let fresh = if source.how == "raw" {
        blob.to_string()
    } else {
        match crate::archive::extract_member(blob, source.how, source.binname, tmpdir, &ctx.sink) {
            Some(fresh) => fresh,
            None => {
                print_out(
                    &ctx.sink,
                    &format!("  no {} inside the downloaded archive", source.binname),
                );
                return None;
            }
        }
    };
    // the probe below execs the binary: a fresh download or a fresh
    // extract carries no exec bit on posix (write_member creates it
    // 0644), so the mode is set before the first attempt - the probe
    // would answer "did not run" for a perfectly good build otherwise
    if !is_windows && let Err(e) = pyfs::chmod_755(&fresh) {
        print_out(
            &ctx.sink,
            &format!("  could not set the mode of {fresh}: {e}"),
        );
        return None;
    }
    if let Some(st) = stage.as_deref() {
        st.set_live(Some("testing the binary".to_string()));
    }
    // one exec can fail transiently right after a large download burst,
    // so probe three times before declaring the build unusable. a
    // genuinely incompatible binary (wrong arch, missing glibc) fails
    // all three.
    let (ver, rc) = {
        let probe = &mut *ctx.probe;
        let sleep = &mut *ctx.sleep;
        let mut ver = (String::new(), -1);
        for attempt in 0..3 {
            ver = probe(&fresh);
            if ver.1 == 0 && !ver.0.is_empty() {
                break;
            }
            if attempt < 2 {
                sleep(1.0);
            }
        }
        ver
    };
    if rc != 0 || ver.is_empty() {
        print_out(
            &ctx.sink,
            &format!("  the downloaded ffmpeg did not run on this system (rc={rc})"),
        );
        return None;
    }
    let target = resolve::py_path_join(bin_dir, source.binname);
    if let Err(e) = pyfs::replace(&fresh, &target) {
        print_out(&ctx.sink, &format!("  could not install into bin/: {e}"));
        return None;
    }
    // the rename preserves the mode just set on `fresh`
    Some(target)
}

/// `offer_ffmpeg(s, kind)`: offer the published build when no ffmpeg is
/// found and a human is on the line, install it behind the answer, and
/// return the resolved ffmpeg dir. Silent, and declining, when nobody
/// can answer.
pub fn offer_ffmpeg(s: &Obj, kind: &str, ctx: &mut DownloadCtx) -> Option<String> {
    let bin_dir = resolve::py_path_join(ctx.root, "bin");
    if let Ok(Some(dir)) = resolve::find_ffmpeg_dir(s, &bin_dir, ctx.is_windows, ctx.which) {
        return Some(dir);
    }
    if !ctx.stdin_tty {
        return None;
    }
    let platform = resolve::host_platform();
    let source = ffmpeg_source(&platform);
    let why = if kind == "audio only" {
        "without it the audio is saved as-is, no mp3/opus/wav conversion"
    } else {
        "without it you get one single best file - no separate audio track, no merge, \
         no mp3, no srt subtitles"
    };
    print_out(&ctx.sink, "");
    print_out(
        &ctx.sink,
        &c(ctx.use_color, A_AMBER, "  ffmpeg is not installed"),
    );
    print_out(&ctx.sink, &format!("  {why}"));
    let Some(source) = source else {
        print_out(
            &ctx.sink,
            &format!(
                "  no published download for {platform} - install it with your package manager instead"
            ),
        );
        return None;
    };
    let archive = if source.how == "zip" {
        "zip"
    } else if source.how == "tar.xz" {
        "tar.xz"
    } else {
        // the macos source publishes the binary itself, no archive
        "single binary"
    };
    print_out(
        &ctx.sink,
        &format!("  download the official {platform} build? ({archive})"),
    );
    print_out(&ctx.sink, &format!("  {}", source.url));
    let raw = match ctx.reader.read_line("  y/n (enter = no): ", None, ctx.out) {
        ReadOutcome::Line(line) => line,
        // python read_esc: escape answers None (decline); EOF and Ctrl+C
        // raise out of the call (recorded deviation: the port continues
        // without ffmpeg)
        ReadOutcome::Escape | ReadOutcome::Eof | ReadOutcome::Interrupted => {
            print_out(&ctx.sink, "  continuing without ffmpeg");
            return None;
        }
    };
    let answer = py_strip(&raw).to_lowercase();
    if answer != "y" && answer != "yes" {
        print_out(&ctx.sink, "  continuing without ffmpeg");
        return None;
    }
    // the install runs behind its own stage line, and python's `finally`
    // verdict prints whether it worked or not
    let mut st = tty::dot_stage("downloading ffmpeg", ctx.use_color, ctx.sink.clone());
    st.start();
    let got = install_ffmpeg(Some(&mut st), &platform, &bin_dir, ctx.is_windows, ctx);
    st.done(Some(&format!(
        "ffmpeg - {}",
        if got.is_some() {
            "installed in bin/"
        } else {
            "not installed"
        }
    )));
    got.as_ref()?;
    // re-find: the install put a binary in bin/, and only a binary the
    // resolver accepts counts (a wrong-platform build would not)
    let dir = resolve::find_ffmpeg_dir(s, &bin_dir, ctx.is_windows, ctx.which)
        .ok()
        .flatten();
    match dir {
        Some(dir) => {
            print_out(&ctx.sink, &format!("  ffmpeg ready: {dir}"));
            Some(dir)
        }
        None => {
            print_out(
                &ctx.sink,
                "  installed, but it is still not found - check ffmpeg_path",
            );
            None
        }
    }
}
