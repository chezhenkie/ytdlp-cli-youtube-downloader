//! Update machinery (specsheet section 4, layer 6; FR-13): version
//! comparison, the two checksum-file parsers, the published-digest
//! reader, the native self-updater and the two updater halves.
//!
//! Phase 2 brought the pure half, so the updater's decisions were
//! testable without a network; phase 8 adds the networked half. Every
//! network touch goes through the `net::HttpRt` seam and every child
//! process through an injected closure, so the whole family runs
//! offline in tests and in the differential harness.

use crate::config::Value as Json;
use crate::config::{self, Obj, Value};
use crate::net::{self, HttpRt};
use crate::pyfs;
use crate::resolve;
use crate::settings_json;
use crate::sha256;
use crate::term::ascii_safe;
use crate::tty::{self, Out, ReadOutcome, Reader, SharedSink};

/// The GitHub releases API the native self-update reads: one answer
/// carries the newest tag and a sha256 digest per asset, so the version
/// and the digest that binds the bytes cannot come from two requests
/// that disagree.
pub const RELEASES_API: &str =
    "https://api.github.com/repos/chezhenkie/ytdlp-cli-youtube-downloader/releases/latest";
/// The human-readable release page, the line the notice prints.
pub const RELEASES_PAGE: &str =
    "https://github.com/chezhenkie/ytdlp-cli-youtube-downloader/releases";

/// An input that ended the updater the way python's `read_esc` raises:
/// EOF or Ctrl+C inside an updater prompt. The menu maps it to the same
/// outcome it maps for its own reads (phase-6 rule).
#[derive(Debug, PartialEq)]
pub enum UpdErr {
    Eof,
    Interrupted,
}

/// Everything the updater family reads from its world (python reads
/// module globals, the filesystem, the console and the clock).
pub struct UpdateCtx<'a> {
    /// `platform_name()`.
    pub platform: &'a str,
    /// `target_arch` for the release asset name.
    pub arch: &'a str,
    /// `CLI_SELF_PATH`: the running image's path.
    pub self_path: &'a str,
    pub root: &'a str,
    pub is_windows: bool,
    /// `can_ask()`: python's `sys.stdin.isatty()`.
    pub stdin_tty: bool,
    pub use_color: bool,
    pub sink: SharedSink,
    pub http: &'a mut dyn HttpRt,
    /// `probe_version(binary)` with its memo.
    pub probe: &'a mut dyn FnMut(&str) -> (String, i64),
    /// `<binary> --selftest` to completion, the rc.
    pub selftest: &'a mut dyn FnMut(&str) -> i64,
    /// `subprocess.call([binary, "-U"])`: spawn with the stdio
    /// inherited, wait, answer the rc.
    pub call: &'a mut dyn FnMut(&str, &[String]) -> i64,
    /// `time.time()`.
    pub now: &'a mut dyn FnMut() -> f64,
    /// `which(cmd)` on PATH.
    pub which: &'a mut dyn FnMut(&str) -> Option<String>,
    pub out: &'a mut dyn Out,
    pub reader: &'a mut dyn Reader,
}

/// A version string as a comparable list. Non-numeric pieces count as 0
/// rather than raising, because the published string is not under our
/// control.
///
/// PORT NOTE (recorded for the spec change history): python splits on
/// `[.\-+]` and converts a piece with `p.isdigit()` - a unicode test -
/// and unbounded `int()`. The port takes ASCII digits only and saturates a
/// piece beyond i64 at i64::MAX (i64::MIN is unreachable: pieces have no
/// sign). Both differences need digits outside `[0-9]`, which the
/// published version strings and the harness protocol cannot carry, so
/// the differential corpus stays exact.
pub fn version_tuple(v: Option<&str>) -> Vec<i64> {
    let mut parts = Vec::new();
    for piece in v.unwrap_or("").split(['.', '-', '+']) {
        if piece.is_empty() {
            continue;
        }
        parts.push(decimal_piece(piece));
    }
    if parts.is_empty() { vec![0] } else { parts }
}

fn decimal_piece(piece: &str) -> i64 {
    if !piece.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    piece.parse::<i64>().unwrap_or(i64::MAX)
}

/// The digest out of a single-asset checksum file. gyan.dev publishes a
/// bare 64-char hash, a github checksums.sha256 publishes
/// `<hash>  <file>`, so take the first field that is a digest rather
/// than assuming a shape.
pub fn parse_sha256(text: &str) -> Option<String> {
    for field in text.split_whitespace() {
        let cand = field.trim_matches('*').to_lowercase();
        if is_digest(&cand) {
            return Some(cand);
        }
    }
    None
}

/// The digest for one asset out of a multi-asset checksums file, where
/// the digest is the line that names that file.
pub fn pick_checksum(text: &str, filename: &str) -> Option<String> {
    for line in text.split('\n') {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1].trim_matches('*') == filename {
            let cand = parts[0].to_lowercase();
            if is_digest(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

/// 64 lowercase hex characters (`[0-9a-f]{64}` with the input lowered).
fn is_digest(cand: &str) -> bool {
    cand.len() == 64 && cand.bytes().all(|b| b.is_ascii_hexdigit())
}

// -------------------------------------------------- the published digest

/// `expected_sha(url, sha_url, asset_name)`: the published digest for
/// the asset, read before anything is downloaded. Returns None with the
/// reason printed, because a download with nothing to check it against
/// is not a download this CLI should perform. Python's first parameter
/// is unused and kept for the same call shape.
pub fn expected_sha(
    _url: &str,
    sha_url: &str,
    asset_name: &str,
    http: &mut dyn HttpRt,
    sink: &SharedSink,
) -> Option<String> {
    let text = match http.text(sha_url, net::FORCED_TIMEOUT) {
        Ok(text) => text,
        Err(e) => {
            tty::emit_line(
                sink,
                &format!("  could not read the published checksum: {e}"),
            );
            return None;
        }
    };
    let digest = pick_checksum(&text, asset_name).or_else(|| parse_sha256(&text));
    if digest.is_none() {
        tty::emit_line(sink, "  the published checksum did not name this build");
    }
    digest
}

/// `expected_sha_api(api_url, asset_name)`: the published digest for the
/// asset, read from the GitHub release API object instead of a checksum
/// file - the shaka-project macos ffmpeg source publishes its sha256 as
/// the per-asset `digest` field, and the pinned release tag means this
/// read guards the same artifact the checksum-file read does. Same
/// contract as `expected_sha`: None with the reason printed.
pub fn expected_sha_api(
    api_url: &str,
    asset_name: &str,
    http: &mut dyn HttpRt,
    sink: &SharedSink,
) -> Option<String> {
    let text = match http.text(api_url, net::FORCED_TIMEOUT) {
        Ok(text) => text,
        Err(e) => {
            tty::emit_line(
                sink,
                &format!("  could not read the published checksum: {e}"),
            );
            return None;
        }
    };
    let digest = settings_json::parse(&text).ok().and_then(|root| {
        let Json::Obj(fields) = root else { return None };
        let Some(Json::Arr(assets)) = config::get(&fields, "assets") else {
            return None;
        };
        for item in assets {
            let Json::Obj(entry) = item else { continue };
            let Some(Json::Str(name)) = config::get(entry, "name") else {
                continue;
            };
            if name != asset_name {
                continue;
            }
            if let Some(Json::Str(text)) = config::get(entry, "digest") {
                return Some(text.strip_prefix("sha256:").unwrap_or(text).to_lowercase());
            }
            return None;
        }
        None
    });
    if digest.is_none() {
        tty::emit_line(sink, "  the published checksum did not name this build");
    }
    digest
}

// ------------------------------------------------------- the native updater

/// What one GET of the releases API yields for this platform.
#[derive(Debug, Default, PartialEq)]
pub struct Release {
    /// The newest tag with a leading `v` removed: the native fetch
    /// compares against the product version, which carries no `v`
    /// (python read a `VERSION = "..."` line instead).
    pub version: Option<String>,
    /// The bare-binary asset for the running platform.
    pub asset: Option<String>,
    pub asset_url: Option<String>,
    /// The API's `digest` field with its `sha256:` prefix removed.
    pub digest: Option<String>,
}

/// The bare-binary release asset for a platform and architecture
/// (FR-13; the names the release publishes alongside the bundles).
/// macOS deliberately has none (owner call 2026-10-17h): the mac line
/// is the portable bundle only, so there is no cli self-update image.
pub fn release_asset(platform: &str, arch: &str) -> Option<&'static str> {
    match (platform, arch) {
        ("windows", "x86_64") => Some("ytdlp-native-windows-x86_64.exe"),
        ("linux", "x86_64") => Some("ytdlp-native-linux-x86_64"),
        _ => None,
    }
}

/// `fetch_cli` native (FR-13): the published release from one GET.
/// Python reads the version out of the same source bytes it installs;
/// a compiled image cannot, so the version and the binding digest come
/// from the API response that names the asset - the same single-request
/// property, one layer up.
pub fn fetch_cli(ctx: &mut UpdateCtx, timeout: u64) -> Result<Release, String> {
    let body = ctx.http.text(RELEASES_API, timeout)?;
    parse_release(&body, release_asset(ctx.platform, ctx.arch))
}

/// The yt-dlp release API: one GET carries the newest tag, the onedir
/// zip for a shipped bundle platform and the zip's published digest.
pub const YTDLP_RELEASES_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";

/// The onedir zip yt-dlp publishes for each platform whose bundles ship
/// its payload - the same assets bundles.yml stages.
pub fn ytdlp_tree_asset(platform: &str, arch: &str) -> Option<&'static str> {
    match (platform, arch) {
        ("windows", "x86_64") => Some("yt-dlp_win.zip"),
        ("linux", "x86_64") => Some("yt-dlp_linux.zip"),
        ("macos", "aarch64") => Some("yt-dlp_macos.zip"),
        _ => None,
    }
}

/// The archive member that becomes `bin/yt-dlp[.exe]`.
fn ytdlp_tree_member(asset: &str) -> Option<&'static str> {
    match asset {
        "yt-dlp_win.zip" => Some("yt-dlp.exe"),
        "yt-dlp_linux.zip" => Some("yt-dlp_linux"),
        "yt-dlp_macos.zip" => Some("yt-dlp_macos"),
        _ => None,
    }
}

/// `fetch_yt_dlp`: the same single-GET contract for the bundled yt-dlp
/// tree. Tag, platform zip and digest come from one answer, so the bytes
/// can only be accepted for the release that named them.
pub fn fetch_yt_dlp(ctx: &mut UpdateCtx) -> Result<Release, String> {
    let body = ctx.http.text(YTDLP_RELEASES_API, net::FORCED_TIMEOUT)?;
    parse_release(&body, ytdlp_tree_asset(ctx.platform, ctx.arch))
}

/// One release answer parsed for one asset name.
fn parse_release(body: &str, asset: Option<&str>) -> Result<Release, String> {
    let root = settings_json::parse(body)
        .map_err(|e| format!("the release metadata did not parse: {e}"))?;
    let obj = match &root {
        Json::Obj(fields) => fields,
        _ => return Err("the release metadata was not an object".to_string()),
    };
    let mut release = Release {
        version: match config::get(obj, "tag_name") {
            Some(Json::Str(tag)) => Some(tag.strip_prefix('v').unwrap_or(tag).to_string()),
            _ => None,
        },
        ..Release::default()
    };
    if let (Some(asset), Some(Json::Arr(assets))) = (asset, config::get(obj, "assets")) {
        for item in assets {
            let Json::Obj(fields) = item else { continue };
            let Some(Json::Str(name)) = config::get(fields, "name") else {
                continue;
            };
            if name != asset {
                continue;
            }
            let Some(Json::Str(url)) = config::get(fields, "browser_download_url") else {
                continue;
            };
            // the api digest is `sha256:<hex>` or null. an asset without
            // one is kept with its url so the caller can name the digest
            // refusal instead of the missing-build one (FR-13)
            release.asset = Some(name.clone());
            release.asset_url = Some(url.clone());
            release.digest = match config::get(fields, "digest") {
                Some(Json::Str(text)) => {
                    Some(text.strip_prefix("sha256:").unwrap_or(text).to_lowercase())
                }
                _ => None,
            };
            break;
        }
    }
    Ok(release)
}

/// `install_cli` native (FR-13): gate the downloaded image, then place
/// it. Nothing existing is touched until the gates pass, and the running
/// binary is renamed aside rather than overwritten - legal on both
/// platforms, where overwriting a running image is not. Returns
/// `(ok, message)` like python.
pub fn install_cli(ctx: &mut UpdateCtx, staged: &str) -> (bool, String) {
    let target = ctx.self_path.to_string();
    let selftest = (ctx.selftest)(staged);
    if selftest != 0 {
        pyfs::remove_quiet(staged);
        return (
            false,
            format!("the new build did not pass its own checks (rc={selftest})"),
        );
    }
    let old = pyfs::with_suffix(&target, ".old");
    // a leftover .old from an earlier run must not block the rename
    pyfs::remove_quiet(&old);
    if let Err(e) = pyfs::replace(&target, &old) {
        pyfs::remove_quiet(staged);
        return (
            false,
            format!(
                "could not move the running image aside: {}",
                ascii_safe(&e.to_string())
            ),
        );
    }
    if let Err(e) = pyfs::replace(staged, &target) {
        // put the working image back before reporting
        let _ = pyfs::replace(&old, &target);
        pyfs::remove_quiet(staged);
        return (
            false,
            format!(
                "could not place the new file: {}",
                ascii_safe(&e.to_string())
            ),
        );
    }
    (true, target)
}

/// `remove_previous_image`: the native-only half of FR-13's install
/// contract. Python has no rename step, so it has no `.old` to clear;
/// the native image removes its predecessor on the next successful
/// start, which is also when a half-finished install would finally be
/// noticed. Best effort and silent, like python's `unlink` in a
/// `try`.
pub fn remove_previous_image(self_path: &str) {
    pyfs::remove_quiet(&pyfs::with_suffix(self_path, ".old"));
}

/// The native download gate: the asset lands beside the target as
/// `<target>.new` through the `.part` staging, then its digest must
/// equal the one the same API response published. A failure removes both
/// staged names, so a later run cannot mistake them for the real image.
fn download_release(ctx: &mut UpdateCtx, release: &Release, staged: &str) -> (bool, String) {
    let part = pyfs::with_suffix(staged, ".part");
    let drop_staged = || {
        pyfs::remove_quiet(staged);
        pyfs::remove_quiet(&part);
    };
    let Some(url) = release.asset_url.clone() else {
        return (
            false,
            "the published release has no build for this platform".into(),
        );
    };
    let Some(digest) = release.digest.clone() else {
        return (
            false,
            "the published release has no digest for this build".into(),
        );
    };
    if let Err(e) = ctx.http.file(&url, staged, net::DL_TIMEOUT, None) {
        drop_staged();
        return (false, format!("the download failed: {}", ascii_safe(&e)));
    }
    match sha256::sha256_file(staged) {
        Ok(got) if got == digest => (true, String::new()),
        Ok(_) => {
            drop_staged();
            (
                false,
                "the downloaded build did not match the published digest".into(),
            )
        }
        Err(e) => {
            drop_staged();
            (
                false,
                format!(
                    "could not read the downloaded build: {}",
                    ascii_safe(&e.to_string())
                ),
            )
        }
    }
}

/// `update_yt_dlp(s)`: in-place self-update of the resolved yt-dlp
/// binary. The bundled payload updates as a tree - tag, zip and digest
/// from one releases API GET, verify, extract, swap the binary and
/// `_internal` with rollback - because `-U` on a onedir build replaces
/// the launcher with a onefile image and leaves the tree behind. A
/// user-set `yt_dlp_path` keeps `<binary> -U`; that copy is not ours to
/// restructure. Returns true when the update ran or the payload was
/// already current.
pub fn update_yt_dlp(s: &Obj, ctx: &mut UpdateCtx) -> bool {
    let bin_dir = resolve::py_path_join(ctx.root, "bin");
    let binary = match resolve::find_yt_dlp(s, &bin_dir, ctx.platform, ctx.which) {
        Ok(Some(binary)) => binary,
        Ok(None) => {
            tty::emit_line(&ctx.sink, "yt-dlp binary not found.");
            tty::emit_line(
                &ctx.sink,
                "put yt-dlp in bin/ or on PATH, or set yt_dlp_path in config/settings.json",
            );
            return false;
        }
        Err(shape) => {
            tty::write_stderr(&format!(
                "ytdlp: settings value refused with python shape {}\n",
                shape.token()
            ));
            return false;
        }
    };
    let (old, rc) = (ctx.probe)(&binary);
    if rc != 0 || old.is_empty() {
        tty::emit_line(
            &ctx.sink,
            &format!("yt-dlp binary check failed (rc={rc}): {old}"),
        );
        return false;
    }
    tty::emit_line(&ctx.sink, &format!("binary      : {binary}"));
    tty::emit_line(&ctx.sink, &format!("version now : {old}"));
    let saved = config::get(s, "yt_dlp_path").is_some_and(config::py_truthy);
    let bundled = resolve::py_path_join(&bin_dir, resolve::platform_bin_name(ctx.platform));
    if !saved && binary == bundled {
        return update_bundled_yt_dlp(ctx, &binary, &old);
    }
    let update_rc = (ctx.call)(&binary, &["-U".to_string()]);
    if crate::exec::interrupt_pending() {
        // python's `except KeyboardInterrupt` around the call
        tty::emit_line(&ctx.sink, "\ncancelled by user");
        return false;
    }
    let (new, _) = (ctx.probe)(&binary);
    tty::emit_line(&ctx.sink, &format!("version after: {new}"));
    update_rc == 0
}

/// The bundled payload's tree update. The running tree is only touched
/// after the archive's digest matches the one the API published and both
/// the binary and `_internal` are staged; `swap_payload` then renames
/// old aside, new in, and restores the old pair on any failure. The
/// stage lives inside `bin/` so every move is a same-volume rename.
fn update_bundled_yt_dlp(ctx: &mut UpdateCtx, binary: &str, old: &str) -> bool {
    let bin_dir = resolve::py_path_join(ctx.root, "bin");
    let Some(asset) = ytdlp_tree_asset(ctx.platform, ctx.arch) else {
        tty::emit_line(
            &ctx.sink,
            &format!("no published yt-dlp tree for {}/{}", ctx.platform, ctx.arch),
        );
        return false;
    };
    let release = match fetch_yt_dlp(ctx) {
        Ok(release) => release,
        Err(e) => {
            tty::emit_line(
                &ctx.sink,
                &format!("could not reach the update source: {}", ascii_safe(&e)),
            );
            return false;
        }
    };
    let Some(newver) = release.version.clone() else {
        tty::emit_line(&ctx.sink, "no version found in the published file");
        return false;
    };
    let Some(url) = release.asset_url.clone() else {
        tty::emit_line(&ctx.sink, &format!("the published release has no {asset}"));
        return false;
    };
    let Some(digest) = release.digest.clone() else {
        tty::emit_line(
            &ctx.sink,
            &format!("the published release has no digest for {asset}"),
        );
        return false;
    };
    if version_tuple(Some(&newver)) <= version_tuple(Some(old)) {
        tty::emit_line(&ctx.sink, &format!("version after: {old}"));
        return true;
    }
    let Some(member) = ytdlp_tree_member(asset) else {
        tty::emit_line(&ctx.sink, &format!("no published yt-dlp tree for {asset}"));
        return false;
    };
    let stage = match pyfs::mkdtemp(".yt-dlp-", &bin_dir) {
        Ok(stage) => stage,
        Err(e) => {
            tty::emit_line(
                &ctx.sink,
                &format!(
                    "could not create the update folder: {}",
                    ascii_safe(&e.to_string())
                ),
            );
            return false;
        }
    };
    let outcome = stage_and_swap(ctx, &stage, &url, &digest, asset, member, binary);
    pyfs::rmtree_quiet(&stage);
    match outcome {
        Ok(()) => {
            let (new, _) = (ctx.probe)(binary);
            tty::emit_line(&ctx.sink, &format!("version after: {new}"));
            true
        }
        Err(msg) => {
            tty::emit_line(&ctx.sink, &msg);
            false
        }
    }
}

/// Download, verify, extract and swap one tree. Split out so the stage
/// is removed on every path by the caller.
fn stage_and_swap(
    ctx: &mut UpdateCtx,
    stage: &str,
    url: &str,
    digest: &str,
    asset: &str,
    member: &str,
    binary: &str,
) -> Result<(), String> {
    let zip_path = resolve::py_path_join(stage, asset);
    if let Err(e) = ctx.http.file(url, &zip_path, net::DL_TIMEOUT, None) {
        return Err(format!("the download failed: {}", ascii_safe(&e)));
    }
    match sha256::sha256_file(&zip_path) {
        Ok(got) if got == digest => {}
        Ok(_) => return Err("the downloaded build did not match the published digest".into()),
        Err(e) => {
            return Err(format!(
                "could not read the downloaded build: {}",
                ascii_safe(&e.to_string())
            ));
        }
    }
    let tree = resolve::py_path_join(stage, "tree");
    if let Err(e) = crate::archive::extract_zip_all(&zip_path, &tree) {
        return Err(format!(
            "the downloaded archive was unreadable: {}",
            ascii_safe(&e)
        ));
    }
    let staged_exe = resolve::py_path_join(&tree, member);
    let staged_internal = resolve::py_path_join(&tree, "_internal");
    let internal_target =
        resolve::py_path_join(&resolve::py_path_join(ctx.root, "bin"), "_internal");
    swap_payload(binary, &internal_target, &staged_exe, &staged_internal)
        .map_err(|e| format!("could not replace the yt-dlp tree: {e}"))
}

/// Rename the current binary and `_internal` aside, place the staged
/// pair, then drop the old pair. Any failure restores the pair that was
/// there, so a failed update leaves the previous payload runnable.
fn swap_payload(
    binary: &str,
    internal_target: &str,
    staged_exe: &str,
    staged_internal: &str,
) -> Result<(), String> {
    if !std::path::Path::new(staged_exe).is_file()
        || !std::path::Path::new(staged_internal).is_dir()
    {
        return Err("the downloaded yt-dlp tree is incomplete".into());
    }
    pyfs::chmod_755(staged_exe).map_err(|e| format!("could not set the binary executable: {e}"))?;
    let exe_old = pyfs::with_suffix(binary, ".old");
    let internal_old = pyfs::with_suffix(internal_target, ".old");
    pyfs::remove_quiet(&exe_old);
    pyfs::rmtree_quiet(&internal_old);
    let had_exe = std::path::Path::new(binary).exists();
    let had_internal = std::path::Path::new(internal_target).exists();
    if had_exe {
        pyfs::replace(binary, &exe_old)
            .map_err(|e| format!("could not move the current binary aside: {e}"))?;
    }
    if had_internal && let Err(e) = pyfs::replace(internal_target, &internal_old) {
        if had_exe {
            let _ = pyfs::replace(&exe_old, binary);
        }
        return Err(format!("could not move the current payload aside: {e}"));
    }
    if let Err(e) = pyfs::replace(staged_exe, binary) {
        if had_internal {
            let _ = pyfs::replace(&internal_old, internal_target);
        }
        if had_exe {
            let _ = pyfs::replace(&exe_old, binary);
        }
        return Err(format!("could not place the new binary: {e}"));
    }
    if let Err(e) = pyfs::replace(staged_internal, internal_target) {
        // the freshly placed binary goes before the old pair returns
        pyfs::remove_quiet(binary);
        if had_internal {
            let _ = pyfs::replace(&internal_old, internal_target);
        }
        if had_exe {
            let _ = pyfs::replace(&exe_old, binary);
        }
        return Err(format!("could not place the new payload: {e}"));
    }
    pyfs::remove_quiet(&exe_old);
    pyfs::rmtree_quiet(&internal_old);
    Ok(())
}

/// `auto_update_check(s, force)`: the startup notice for a newer
/// published version, and the forced form `--update` and menu item 13
/// run. It asks before it touches anything and, unless forced,
/// throttles itself to one network check a day and stays silent when the
/// network is gone. Returns true only when the image was replaced.
///
/// PORT NOTE (native, from FR-13): the digest gate, the `--selftest`
/// probe, the rename-then-place install and the human release page are
/// the native contract decided on 2026-10-14; every other line and gate
/// is python's. The two native refusal lines print only when forced,
/// like python's.
pub fn auto_update_check(s: &mut Obj, force: bool, ctx: &mut UpdateCtx) -> Result<bool, UpdErr> {
    if !force {
        if !config::get(s, "auto_update").is_some_and(config::py_truthy) {
            return Ok(false);
        }
        if !ctx.stdin_tty {
            return Ok(false);
        }
        let last = match config::get(s, "auto_update_last_check") {
            Some(Value::Int(n)) => *n as f64,
            Some(Value::Float(f)) => *f,
            Some(Value::Bool(false)) | Some(Value::Null) | None => 0.0,
            // python's `float()` refuses anything else and the
            // except resets the stamp to zero
            Some(_) => 0.0,
        };
        if (ctx.now)() - last < 86400.0 {
            return Ok(false);
        }
    }
    let release = match fetch_cli(
        ctx,
        if force {
            net::FORCED_TIMEOUT
        } else {
            net::CHECK_TIMEOUT
        },
    ) {
        Ok(release) => release,
        Err(e) => {
            if force {
                tty::emit_line(
                    &ctx.sink,
                    &format!("  could not reach the update source: {}", ascii_safe(&e)),
                );
            }
            return Ok(false);
        }
    };
    if !force {
        config::set(s, "auto_update_last_check", Value::Int((ctx.now)() as i64));
    }
    let Some(newver) = release.version.clone() else {
        if force {
            tty::emit_line(&ctx.sink, "  no version found in the published file");
        }
        return Ok(false);
    };
    if version_tuple(Some(&newver)) <= version_tuple(Some(crate::VERSION)) {
        if force {
            tty::emit_line(
                &ctx.sink,
                &format!(
                    "  published v{newver}, this is v{} - already current",
                    crate::VERSION
                ),
            );
        }
        return Ok(false);
    }
    // FR-13: a platform without a published asset, or an asset without a
    // digest, is a refusal - and it is refused before the notice, so the
    // user is never asked about an update that cannot happen
    if release.asset_url.is_none() || release.asset.is_none() {
        if force {
            tty::emit_line(
                &ctx.sink,
                &format!("  no published cli build for {}", ctx.platform),
            );
        }
        return Ok(false);
    }
    if release.digest.is_none() {
        if force {
            tty::emit_line(
                &ctx.sink,
                &format!(
                    "  the published release has no digest for {}",
                    release.asset.unwrap_or_default()
                ),
            );
        }
        return Ok(false);
    }
    tty::emit_line(
        &ctx.sink,
        &format!(
            "  a newer version is published: v{newver} (this is v{})",
            crate::VERSION
        ),
    );
    tty::emit_line(&ctx.sink, &format!("  {RELEASES_PAGE}"));
    if !prompt_yes(ctx, "  update now? y/n (enter = no): ")? {
        tty::emit_line(&ctx.sink, "  not updated");
        return Ok(false);
    }
    if !force {
        config::set(s, "auto_update_last_check", Value::Int((ctx.now)() as i64));
    }
    let staged = pyfs::with_suffix(ctx.self_path, ".new");
    let (ok, msg) = match download_release(ctx, &release, &staged) {
        (true, _) => {
            let (ok, msg) = install_cli(ctx, &staged);
            (ok, msg)
        }
        (false, msg) => (false, msg),
    };
    tty::emit_line(&ctx.sink, &format!("  {msg}"));
    if ok {
        tty::emit_line(
            &ctx.sink,
            "  updated - the new version runs from the next start",
        );
    }
    Ok(ok)
}

/// `update_all(s)`: menu item 13 and `--update`, both updatable halves.
/// Returns true when either one moved.
pub fn update_all(s: &mut Obj, ctx: &mut UpdateCtx) -> Result<bool, UpdErr> {
    tty::emit_line(&ctx.sink, "  yt-dlp");
    let moved_yt = update_yt_dlp(s, ctx);
    tty::emit_line(&ctx.sink, "  cli");
    let moved_cli = auto_update_check(s, true, ctx)?;
    Ok(moved_yt || moved_cli)
}

/// `read_esc` with python's answer rule: a y/yes goes on, everything
/// else - including escape - is no. EOF and Ctrl+C are raised, as
/// python raises them.
fn prompt_yes(ctx: &mut UpdateCtx, prompt: &str) -> Result<bool, UpdErr> {
    match ctx.reader.read_line(prompt, None, ctx.out) {
        ReadOutcome::Line(line) => {
            let answer = crate::pytext::py_strip(&line).to_lowercase();
            Ok(answer == "y" || answer == "yes")
        }
        ReadOutcome::Escape => Ok(false),
        ReadOutcome::Eof => Err(UpdErr::Eof),
        ReadOutcome::Interrupted => Err(UpdErr::Interrupted),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::defaults;
    use crate::testfake::{FakeHttp, LinesOut, scratch};
    use crate::tty::{RecordingOut, ScriptItem, ScriptedReader};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    const GYAN: &str = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
    const BTBN: &str = "afa2b69c9965919e0b519a633520321f838d74690f8fb28c76393c54a9448d60";

    /// A release answer for the windows asset, digest and all.
    fn release_json(tag: &str, asset: &str, digest: Option<&str>) -> String {
        let digest_field = match digest {
            Some(d) => format!("\"digest\": \"sha256:{d}\""),
            None => "\"digest\": null".to_string(),
        };
        format!(
            "{{\"tag_name\": \"{tag}\", \"assets\": [{{\"name\": \"ytdlp-native-linux-x86_64\", \
             \"browser_download_url\": \"https://example.invalid/linux\", \
             \"digest\": \"sha256:{BTBN}\"}}, \
             {{\"name\": \"{asset}\", \
             \"browser_download_url\": \"https://example.invalid/asset\", {digest_field}}}]}}"
        )
    }

    /// One updater run's world: a scripted network, a scripted console,
    /// a scripted probe and a clock that does not move.
    struct Harness {
        dir: String,
        self_path: String,
        lines: Arc<Mutex<Vec<String>>>,
        http: FakeHttp,
        rec: RecordingOut,
        reader: ScriptedReader,
        probes: VecDeque<(String, i64)>,
        probed: Vec<String>,
        selftested: Vec<String>,
        selftest_rc: i64,
        updates: Vec<String>,
        stamp: f64,
    }

    impl Harness {
        fn new(tag: &str) -> Self {
            let dir = scratch(tag);
            let self_path = format!("{dir}/ytdlp.exe");
            std::fs::write(&self_path, b"the running image").unwrap();
            let lines = Arc::new(Mutex::new(Vec::new()));
            Self {
                dir,
                self_path,
                lines,
                http: FakeHttp::new(),
                rec: RecordingOut::default(),
                reader: ScriptedReader::new(vec![]),
                probes: VecDeque::new(),
                probed: Vec::new(),
                selftested: Vec::new(),
                selftest_rc: 0,
                updates: Vec::new(),
                stamp: 1_700_000_000.0,
            }
        }

        fn sink(&self) -> SharedSink {
            tty::SharedOut::new(LinesOut {
                lines: Arc::clone(&self.lines),
            })
            .sink()
        }

        fn printed(&self) -> Vec<String> {
            self.lines.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }

        /// Run `auto_update_check` with the harness world.
        fn check(&mut self, s: &mut Obj, force: bool) -> Result<bool, UpdErr> {
            let sink = self.sink();
            let Self {
                dir,
                self_path,
                http,
                rec,
                reader,
                probes,
                probed,
                selftested,
                selftest_rc,
                updates,
                stamp,
                ..
            } = self;
            let mut probe = |binary: &str| {
                probed.push(binary.to_string());
                probes
                    .pop_front()
                    .unwrap_or_else(|| ("1.5.0".to_string(), 0))
            };
            let mut selftest = |binary: &str| {
                selftested.push(binary.to_string());
                *selftest_rc
            };
            let mut call = |binary: &str, args: &[String]| {
                updates.push(format!("{binary} {}", args.join(" ")));
                0
            };
            let mut now = || *stamp;
            let mut which = |_: &str| None;
            let mut ctx = UpdateCtx {
                platform: "windows",
                arch: "x86_64",
                self_path,
                root: dir,
                is_windows: true,
                use_color: false,
                stdin_tty: true,
                sink,
                http,
                probe: &mut probe,
                selftest: &mut selftest,
                call: &mut call,
                now: &mut now,
                which: &mut which,
                out: rec,
                reader,
            };
            auto_update_check(s, force, &mut ctx)
        }

        /// Run `update_all` (menu item 13 and `--update`).
        fn all(&mut self, s: &mut Obj) -> Result<bool, UpdErr> {
            let sink = self.sink();
            let Self {
                dir,
                self_path,
                http,
                rec,
                reader,
                probes,
                probed,
                selftested,
                selftest_rc,
                updates,
                stamp,
                ..
            } = self;
            let mut probe = |binary: &str| {
                probed.push(binary.to_string());
                probes
                    .pop_front()
                    .unwrap_or_else(|| ("1.5.0".to_string(), 0))
            };
            let mut selftest = |binary: &str| {
                selftested.push(binary.to_string());
                *selftest_rc
            };
            let mut call = |binary: &str, args: &[String]| {
                updates.push(format!("{binary} {}", args.join(" ")));
                0
            };
            let mut now = || *stamp;
            let mut which = |_: &str| None;
            let mut ctx = UpdateCtx {
                platform: "windows",
                arch: "x86_64",
                self_path,
                root: dir,
                is_windows: true,
                use_color: false,
                stdin_tty: true,
                sink,
                http,
                probe: &mut probe,
                selftest: &mut selftest,
                call: &mut call,
                now: &mut now,
                which: &mut which,
                out: rec,
                reader,
            };
            update_all(s, &mut ctx)
        }
    }

    fn settings() -> Obj {
        defaults()
    }

    /// A release that publishes a newer version for this platform, with
    /// the asset bytes hashed into the answer.
    fn newer_release(asset_bytes: &[u8]) -> String {
        let digest = crate::sha256::sha256_hex(asset_bytes);
        release_json("v9.9.9", "ytdlp-native-windows-x86_64.exe", Some(&digest))
    }

    /// The yt-dlp releases answer: one asset, the tree zip.
    fn tree_release(tag: &str, asset: &str, digest: Option<&str>) -> String {
        let digest_field = match digest {
            Some(d) => format!("\"digest\": \"sha256:{d}\""),
            None => "\"digest\": null".to_string(),
        };
        format!(
            "{{\"tag_name\": \"{tag}\", \"assets\": [{{\"name\": \"{asset}\", \
             \"browser_download_url\": \"https://example.invalid/tree\", {digest_field}}}]}}"
        )
    }

    /// A yt-dlp onedir zip: the launcher plus one `_internal` file.
    fn tree_zip(member: &str) -> Vec<u8> {
        crate::archive::zip_with(&[
            (member, b"the new tree binary"),
            ("_internal/python3.dll", b"the new dll"),
        ])
    }

    #[test]
    fn ytdlp_tree_assets_name_the_shipped_platform_zips() {
        assert_eq!(
            ytdlp_tree_asset("windows", "x86_64"),
            Some("yt-dlp_win.zip")
        );
        assert_eq!(
            ytdlp_tree_asset("linux", "x86_64"),
            Some("yt-dlp_linux.zip")
        );
        assert_eq!(
            ytdlp_tree_asset("macos", "aarch64"),
            Some("yt-dlp_macos.zip")
        );
        assert_eq!(ytdlp_tree_asset("macos", "x86_64"), None);
        assert_eq!(ytdlp_tree_asset("linux", "aarch64"), None);
        assert_eq!(ytdlp_tree_asset("freebsd", "x86_64"), None);
        assert_eq!(ytdlp_tree_member("yt-dlp_win.zip"), Some("yt-dlp.exe"));
        assert_eq!(ytdlp_tree_member("yt-dlp_linux.zip"), Some("yt-dlp_linux"));
        assert_eq!(ytdlp_tree_member("yt-dlp_macos.zip"), Some("yt-dlp_macos"));
        assert_eq!(ytdlp_tree_member("yt-dlp.exe"), None);
    }

    #[test]
    fn fetch_yt_dlp_reads_the_tag_and_the_zip_digest() {
        let sink = tty::SharedOut::new(RecordingOut::default()).sink();
        let body = tree_release("2026.09.01", "yt-dlp_win.zip", Some(GYAN));
        let mut http = FakeHttp::new().with_text(YTDLP_RELEASES_API, &body);
        let mut reader = ScriptedReader::new(vec![]);
        let mut rec = RecordingOut::default();
        let mut probe = |_: &str| (String::new(), 0);
        let mut selftest = |_: &str| 0;
        let mut call = |_: &str, _: &[String]| 0;
        let mut now = || 0.0;
        let mut which = |_: &str| None;
        let mut ctx = UpdateCtx {
            platform: "windows",
            arch: "x86_64",
            self_path: "x",
            root: ".",
            is_windows: true,
            use_color: false,
            stdin_tty: true,
            sink,
            http: &mut http,
            probe: &mut probe,
            selftest: &mut selftest,
            call: &mut call,
            now: &mut now,
            which: &mut which,
            out: &mut rec,
            reader: &mut reader,
        };
        let release = fetch_yt_dlp(&mut ctx).unwrap();
        assert_eq!(release.version.as_deref(), Some("2026.09.01"));
        assert_eq!(release.asset.as_deref(), Some("yt-dlp_win.zip"));
        assert_eq!(
            release.asset_url.as_deref(),
            Some("https://example.invalid/tree")
        );
        assert_eq!(release.digest.as_deref(), Some(GYAN));
        assert_eq!(http.calls(), vec![format!("text {YTDLP_RELEASES_API} 60")]);
    }

    /// The bundled payload downloads, verifies, extracts and swaps the
    /// tree; no `-U` runs and the old pair is gone.
    #[test]
    fn a_bundled_tree_updates_from_the_release_zip() {
        let mut h = Harness::new("tree-update");
        let bin = resolve::py_path_join(&h.dir, "bin");
        std::fs::create_dir_all(&bin).unwrap();
        let binary = resolve::py_path_join(&bin, "yt-dlp.exe");
        std::fs::write(&binary, b"the old onefile").unwrap();
        let zip = tree_zip("yt-dlp.exe");
        let digest = crate::sha256::sha256_hex(&zip);
        h.http = FakeHttp::new()
            .with_text(
                YTDLP_RELEASES_API,
                &tree_release("2026.09.01", "yt-dlp_win.zip", Some(&digest)),
            )
            .with_file("https://example.invalid/tree", &zip)
            .with_text(
                RELEASES_API,
                &release_json(
                    &format!("v{}", crate::VERSION),
                    "ytdlp-native-windows-x86_64.exe",
                    Some(GYAN),
                ),
            );
        h.probes.push_back(("2026.08.19".to_string(), 0));
        h.probes.push_back(("2026.09.01".to_string(), 0));
        let mut s = settings();
        assert!(h.all(&mut s).unwrap());
        assert_eq!(h.updates, Vec::<String>::new(), "no -U on a bundled tree");
        assert_eq!(
            std::fs::read(&binary).unwrap(),
            b"the new tree binary",
            "the launcher is the published member"
        );
        assert_eq!(
            std::fs::read(format!("{bin}/_internal/python3.dll")).unwrap(),
            b"the new dll"
        );
        assert!(!std::path::Path::new(&format!("{binary}.old")).exists());
        assert!(!std::path::Path::new(&format!("{bin}/_internal.old")).exists());
        assert!(
            std::fs::read_dir(&bin).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".yt-dlp-")),
            "the stage is gone"
        );
        let calls = h.http.calls();
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert_eq!(calls[0], format!("text {YTDLP_RELEASES_API} 60"));
        assert!(
            calls[1].starts_with(&format!("file https://example.invalid/tree {bin}")),
            "{calls:?}"
        );
        assert!(calls[1].ends_with(" 1800"), "{calls:?}");
        assert_eq!(calls[2], format!("text {RELEASES_API} 60"));
        let printed = h.printed();
        assert_eq!(printed[0], "  yt-dlp");
        assert_eq!(printed[1], format!("binary      : {binary}"));
        assert_eq!(printed[2], "version now : 2026.08.19");
        assert_eq!(printed[3], "version after: 2026.09.01");
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn release_asset_names_the_platform_binary() {
        assert_eq!(
            release_asset("windows", "x86_64"),
            Some("ytdlp-native-windows-x86_64.exe")
        );
        assert_eq!(
            release_asset("linux", "x86_64"),
            Some("ytdlp-native-linux-x86_64")
        );
        // macOS has no bare image (owner call 2026-10-17h)
        assert_eq!(release_asset("macos", "aarch64"), None);
        assert_eq!(release_asset("macos", "x86_64"), None);
        assert_eq!(release_asset("linux", "riscv64"), None);
        assert_eq!(release_asset("freebsd", "x86_64"), None);
    }

    #[test]
    fn expected_sha_prefers_the_named_line_then_the_first_digest() {
        let sink = tty::SharedOut::new(RecordingOut::default()).sink();
        // the multi-asset file: the line naming this build wins
        let text = format!("aaa  other.zip\n{BTBN}  ffmpeg.exe\nbbb  third.zip\n");
        let mut http = FakeHttp::new().with_text("sha", &text);
        assert_eq!(
            expected_sha("url", "sha", "ffmpeg.exe", &mut http, &sink),
            Some(BTBN.to_string())
        );
        // the single-asset file: the first digest field
        let mut http = FakeHttp::new().with_text("sha", &format!("{GYAN}\n"));
        assert_eq!(
            expected_sha("url", "sha", "ffmpeg.exe", &mut http, &sink),
            Some(GYAN.to_string())
        );
    }

    #[test]
    fn expected_sha_prints_both_failure_reasons() {
        let recorder = RecordingOut::default();
        let sink = tty::SharedOut::new(recorder.clone()).sink();
        let mut http = FakeHttp::new().with_text_err("sha", "boom");
        assert_eq!(
            expected_sha("url", "sha", "ffmpeg.exe", &mut http, &sink),
            None
        );
        let mut http = FakeHttp::new().with_text("sha", "nothing here");
        assert_eq!(
            expected_sha("url", "sha", "ffmpeg.exe", &mut http, &sink),
            None
        );
        let text = recorder.text();
        assert!(
            text.contains("  could not read the published checksum: boom"),
            "{text}"
        );
        assert!(
            text.contains("  the published checksum did not name this build"),
            "{text}"
        );
    }

    #[test]
    fn fetch_cli_reads_the_tag_and_the_binding_digest() {
        let sink = tty::SharedOut::new(RecordingOut::default()).sink();
        let body = release_json("v9.9.9", "ytdlp-native-windows-x86_64.exe", Some(GYAN));
        let mut http = FakeHttp::new().with_text(RELEASES_API, &body);
        let mut reader = ScriptedReader::new(vec![]);
        let mut rec = RecordingOut::default();
        let mut cache = crate::resolve::VersionCache::default();
        let mut probe = |_: &str| (String::new(), 0);
        let mut selftest = |_: &str| 0;
        let mut call = |_: &str, _: &[String]| 0;
        let mut now = || 0.0;
        let mut which = |_: &str| None;
        let mut ctx = UpdateCtx {
            platform: "windows",
            arch: "x86_64",
            self_path: "x",
            root: ".",
            is_windows: true,
            use_color: false,
            stdin_tty: true,
            sink,
            http: &mut http,
            probe: &mut probe,
            selftest: &mut selftest,
            call: &mut call,
            now: &mut now,
            which: &mut which,
            out: &mut rec,
            reader: &mut reader,
        };
        let release = fetch_cli(&mut ctx, CHECK_TIMEOUT_FOR_TEST).unwrap();
        // the leading v is stripped: the compare is against a product
        // version that carries none
        assert_eq!(release.version.as_deref(), Some("9.9.9"));
        assert_eq!(
            release.asset.as_deref(),
            Some("ytdlp-native-windows-x86_64.exe")
        );
        assert_eq!(
            release.asset_url.as_deref(),
            Some("https://example.invalid/asset")
        );
        assert_eq!(release.digest.as_deref(), Some(GYAN));
        assert_eq!(http.calls(), vec![format!("text {RELEASES_API} 6")]);
        let _ = &mut cache;
    }

    /// The forced fetch keeps the 60 s ceiling, the quiet one the 6 s
    /// audit-F3 value.
    const CHECK_TIMEOUT_FOR_TEST: u64 = 6;

    #[test]
    fn a_release_without_the_platform_asset_is_a_refusal() {
        let mut h = Harness::new("no-asset");
        let body = release_json("v9.9.9", "ytdlp-native-macos-arm64", Some(GYAN));
        h.http = FakeHttp::new().with_text(RELEASES_API, &body);
        h.reader = ScriptedReader::new(vec![ScriptItem::Line("y".to_string())]);
        let mut s = settings();
        assert!(!h.check(&mut s, true).unwrap());
        let printed = h.printed();
        assert!(
            printed
                .iter()
                .any(|l| l == "  no published cli build for windows"),
            "{printed:?}"
        );
        // refused before the notice: nothing was downloaded and the
        // prompt never ran
        assert_eq!(h.http.calls().len(), 1);
        assert_eq!(h.selftested, Vec::<String>::new());
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn a_release_without_a_digest_is_a_refusal() {
        let mut h = Harness::new("no-digest");
        let body = release_json("v9.9.9", "ytdlp-native-windows-x86_64.exe", None);
        h.http = FakeHttp::new().with_text(RELEASES_API, &body);
        h.reader = ScriptedReader::new(vec![ScriptItem::Line("y".to_string())]);
        let mut s = settings();
        assert!(!h.check(&mut s, true).unwrap());
        let printed = h.printed();
        assert!(
            printed.iter().any(|l| l
                == "  the published release has no digest for ytdlp-native-windows-x86_64.exe"),
            "{printed:?}"
        );
        assert_eq!(h.selftested, Vec::<String>::new());
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn an_unreachable_source_is_silent_unless_forced() {
        let mut h = Harness::new("unreachable");
        h.http = FakeHttp::new().with_text_err(RELEASES_API, "host not found");
        let mut s = settings();
        // quiet: the gate is a tty menu with auto_update on, and the
        // answer is silence plus a stamp
        config::set(&mut s, "auto_update", Value::Bool(true));
        assert!(!h.check(&mut s, false).unwrap());
        assert_eq!(h.printed(), Vec::<String>::new());
        // forced: python's line, with the error text
        assert!(!h.check(&mut s, true).unwrap());
        assert_eq!(
            h.printed(),
            vec!["  could not reach the update source: host not found"]
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn the_quiet_check_throttles_to_one_a_day_and_needs_a_tty() {
        let mut h = Harness::new("throttle");
        let bytes = b"a new image".to_vec();
        let body = newer_release(&bytes);
        h.http = FakeHttp::new().with_text(RELEASES_API, &body);
        // one declined answer on the pad: the gate cases below never
        // reach the prompt, the due case does
        h.reader = ScriptedReader::new(vec![ScriptItem::Line(String::new())]);
        let mut s = settings();
        // auto_update off: no contact at all
        config::set(&mut s, "auto_update", Value::Bool(false));
        assert!(!h.check(&mut s, false).unwrap());
        assert_eq!(h.http.calls(), Vec::<String>::new());
        // on, and stamped a moment ago: still no contact
        config::set(&mut s, "auto_update", Value::Bool(true));
        config::set(&mut s, "auto_update_last_check", Value::Int(1_700_000_000));
        assert!(!h.check(&mut s, false).unwrap());
        assert_eq!(h.http.calls(), Vec::<String>::new());
        // stamped a day and a half ago: one contact, then the stamp moves
        config::set(
            &mut s,
            "auto_update_last_check",
            Value::Int(1_700_000_000 - 130_000),
        );
        // the check runs on to the notice and is declined there
        assert!(!h.check(&mut s, false).unwrap());
        assert_eq!(h.http.calls(), vec![format!("text {RELEASES_API} 6")]);
        assert_eq!(
            config::get(&s, "auto_update_last_check"),
            Some(&Value::Int(1_700_000_000))
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn a_declined_notice_leaves_the_image_alone() {
        let mut h = Harness::new("declined");
        let bytes = b"a new image".to_vec();
        let body = newer_release(&bytes);
        h.http = FakeHttp::new()
            .with_text(RELEASES_API, &body)
            .with_file("https://example.invalid/asset", &bytes);
        h.reader = ScriptedReader::new(vec![ScriptItem::Line(String::new())]);
        let mut s = settings();
        assert!(!h.check(&mut s, true).unwrap());
        let printed = h.printed();
        assert_eq!(
            printed,
            vec![
                format!(
                    "  a newer version is published: v9.9.9 (this is v{})",
                    crate::VERSION
                ),
                format!("  {RELEASES_PAGE}"),
                "  not updated".to_string(),
            ]
        );
        // no download, no probe, the running image still there
        assert_eq!(h.http.calls().len(), 1);
        assert_eq!(h.selftested, Vec::<String>::new());
        assert_eq!(std::fs::read(&h.self_path).unwrap(), b"the running image");
        assert!(!std::path::Path::new(&format!("{}.new", h.self_path)).exists());
        assert_eq!(
            h.reader.prompts,
            vec!["  update now? y/n (enter = no): ".to_string()]
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn a_new_image_is_gated_on_its_own_selftest_then_placed() {
        let mut h = Harness::new("install");
        let bytes = b"a new image".to_vec();
        let body = newer_release(&bytes);
        h.http = FakeHttp::new()
            .with_text(RELEASES_API, &body)
            .with_file("https://example.invalid/asset", &bytes);
        h.reader = ScriptedReader::new(vec![ScriptItem::Line("y".to_string())]);
        let mut s = settings();
        assert!(h.check(&mut s, true).unwrap());
        // the probe ran on the staged .new, not on the running image
        assert_eq!(h.selftested, vec![format!("{}.new", h.self_path)]);
        // the running image was renamed aside and the new one placed
        assert_eq!(std::fs::read(&h.self_path).unwrap(), bytes);
        assert_eq!(
            std::fs::read(format!("{}.old", h.self_path)).unwrap(),
            b"the running image"
        );
        assert!(!std::path::Path::new(&format!("{}.new", h.self_path)).exists());
        assert!(!std::path::Path::new(&format!("{}.new.part", h.self_path)).exists());
        let printed = h.printed();
        assert_eq!(printed[2], format!("  {}", h.self_path));
        assert_eq!(
            printed[3],
            "  updated - the new version runs from the next start"
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn a_build_that_fails_its_own_checks_is_never_installed() {
        let mut h = Harness::new("gated");
        let bytes = b"a broken image".to_vec();
        let body = newer_release(&bytes);
        h.http = FakeHttp::new()
            .with_text(RELEASES_API, &body)
            .with_file("https://example.invalid/asset", &bytes);
        h.reader = ScriptedReader::new(vec![ScriptItem::Line("y".to_string())]);
        // phase 9 lands --selftest, so this is exactly what the shipped
        // binary answers today: the install is inert by construction
        h.selftest_rc = 2;
        let mut s = settings();
        assert!(!h.check(&mut s, true).unwrap());
        assert_eq!(
            std::fs::read(&h.self_path).unwrap(),
            b"the running image",
            "the running image is untouched"
        );
        assert!(!std::path::Path::new(&format!("{}.new", h.self_path)).exists());
        assert!(!std::path::Path::new(&format!("{}.old", h.self_path)).exists());
        let printed = h.printed();
        assert!(
            printed
                .iter()
                .any(|l| l == "  the new build did not pass its own checks (rc=2)"),
            "{printed:?}"
        );
        assert!(!printed.iter().any(|l| l.contains("updated -")));
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn a_digest_mismatch_changes_nothing_on_disk() {
        let mut h = Harness::new("digest");
        let bytes = b"the wrong bytes".to_vec();
        // the answer names a digest that is not these bytes
        let body = newer_release(b"other bytes");
        h.http = FakeHttp::new()
            .with_text(RELEASES_API, &body)
            .with_file("https://example.invalid/asset", &bytes);
        h.reader = ScriptedReader::new(vec![ScriptItem::Line("y".to_string())]);
        let mut s = settings();
        assert!(!h.check(&mut s, true).unwrap());
        assert_eq!(std::fs::read(&h.self_path).unwrap(), b"the running image");
        for suffix in [".new", ".new.part", ".old"] {
            assert!(
                !std::path::Path::new(&format!("{}{suffix}", h.self_path)).exists(),
                "{suffix} left behind"
            );
        }
        assert_eq!(h.selftested, Vec::<String>::new());
        let printed = h.printed();
        assert!(
            printed
                .iter()
                .any(|l| l == "  the downloaded build did not match the published digest"),
            "{printed:?}"
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    /// OQ-N4, the live two-image dance: everything above scripts the
    /// `<new> --selftest` gate; this one runs it for real against a
    /// published release image. Ignored by default because it needs that
    /// artifact: run with
    /// `YTDLP_LIVE_IMAGE=<published bare image> cargo test --lib live_two_image_dance -- --ignored`
    #[test]
    #[ignore = "needs a published image; set YTDLP_LIVE_IMAGE and run with --ignored"]
    fn live_two_image_dance_against_a_published_image() {
        let image = match std::env::var("YTDLP_LIVE_IMAGE") {
            Ok(p) => p,
            Err(_) => {
                eprintln!("SKIP: YTDLP_LIVE_IMAGE is not set");
                return;
            }
        };
        let dir = scratch("live-dance");
        let self_path = format!("{dir}/ytdlp");
        std::fs::write(&self_path, b"the previous running image").unwrap();
        let staged = format!("{self_path}.new");
        std::fs::copy(&image, &staged).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let want = crate::sha256::sha256_file(&staged).unwrap();

        let lines = Arc::new(Mutex::new(Vec::new()));
        let mut http = FakeHttp::new();
        let mut rec = RecordingOut::default();
        let mut reader = ScriptedReader::new(vec![]);
        let mut probe = |_: &str| ("0".to_string(), 0);
        // the LIVE gate: a real `<staged> --selftest` spawn, the rc
        let mut selftest = |binary: &str| {
            std::process::Command::new(binary)
                .arg("--selftest")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|st| st.code().unwrap_or(-1) as i64)
                .unwrap_or(-1)
        };
        let mut call = |_: &str, _: &[String]| 0;
        let mut now = || 1_700_000_000.0;
        let mut which = |_: &str| None;
        let sink = tty::SharedOut::new(LinesOut {
            lines: Arc::clone(&lines),
        })
        .sink();
        let mut ctx = UpdateCtx {
            platform: if cfg!(windows) { "windows" } else { "linux" },
            arch: "x86_64",
            self_path: &self_path,
            root: &dir,
            is_windows: cfg!(windows),
            stdin_tty: false,
            use_color: false,
            sink,
            http: &mut http,
            probe: &mut probe,
            selftest: &mut selftest,
            call: &mut call,
            now: &mut now,
            which: &mut which,
            out: &mut rec,
            reader: &mut reader,
        };

        // 1. the live dance: real spawn gate, real renames
        let (ok, msg) = install_cli(&mut ctx, &staged);
        assert!(ok, "install refused: {msg}");
        assert_eq!(
            crate::sha256::sha256_file(&self_path).unwrap(),
            want,
            "the placed image is the published one, byte for byte"
        );
        assert_eq!(
            std::fs::read(format!("{self_path}.old")).unwrap(),
            b"the previous running image"
        );
        assert!(!std::path::Path::new(&staged).exists());

        // 2. the placed image really runs
        let rc = std::process::Command::new(&self_path)
            .arg("--selftest")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("the placed image is runnable")
            .code()
            .unwrap_or(-1);
        assert_eq!(rc, 0, "the placed image answers --selftest");

        // 3. the fresh-start cleanup removes .old
        remove_previous_image(&self_path);
        assert!(!std::path::Path::new(&format!("{self_path}.old")).exists());

        // 4. the live refusal: a staged file whose real spawn cannot pass
        std::fs::write(&self_path, b"still the running image").unwrap();
        std::fs::write(&staged, b"not a runnable image").unwrap();
        let (ok, msg) = install_cli(&mut ctx, &staged);
        assert!(!ok, "a dead staged file must be refused");
        assert!(msg.contains("did not pass its own checks"), "{msg}");
        assert_eq!(
            std::fs::read(&self_path).unwrap(),
            b"still the running image",
            "refused: the running image is untouched"
        );
        assert!(!std::path::Path::new(&staged).exists());
        assert!(!std::path::Path::new(&format!("{self_path}.old")).exists());

        std::fs::remove_dir_all(&dir).unwrap();
        println!("live two-image dance PASS against {image}");
    }

    #[test]
    fn the_previous_image_goes_on_the_next_successful_start() {
        let h = Harness::new("old-gone");
        std::fs::write(format!("{}.old", h.self_path), b"the previous image").unwrap();
        assert!(std::path::Path::new(&format!("{}.old", h.self_path)).exists());
        remove_previous_image(&h.self_path);
        assert!(!std::path::Path::new(&format!("{}.old", h.self_path)).exists());
        // and the running image is untouched
        assert_eq!(std::fs::read(&h.self_path).unwrap(), b"the running image");
        // a start with nothing to clear is a no-op
        remove_previous_image(&h.self_path);
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    /// Same tag as the installed tree: no download, and the half still
    /// answers success like `-U` does when it is already current.
    #[test]
    fn a_current_bundled_tree_is_not_downloaded() {
        let mut h = Harness::new("tree-current");
        let bin = resolve::py_path_join(&h.dir, "bin");
        std::fs::create_dir_all(&bin).unwrap();
        let binary = resolve::py_path_join(&bin, "yt-dlp.exe");
        std::fs::write(&binary, b"the current tree").unwrap();
        h.http = FakeHttp::new()
            .with_text(
                YTDLP_RELEASES_API,
                &tree_release("2026.08.19", "yt-dlp_win.zip", Some(GYAN)),
            )
            .with_text(
                RELEASES_API,
                &release_json(
                    &format!("v{}", crate::VERSION),
                    "ytdlp-native-windows-x86_64.exe",
                    Some(GYAN),
                ),
            );
        h.probes.push_back(("2026.08.19".to_string(), 0));
        let mut s = settings();
        assert!(h.all(&mut s).unwrap());
        assert_eq!(std::fs::read(&binary).unwrap(), b"the current tree");
        assert_eq!(h.updates, Vec::<String>::new());
        let calls = h.http.calls();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert!(calls.iter().all(|c| !c.starts_with("file ")), "{calls:?}");
        let printed = h.printed();
        assert_eq!(printed[3], "version after: 2026.08.19");
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    /// The published digest is the gate: a zip with other bytes never
    /// reaches the swap.
    #[test]
    fn a_tree_zip_with_other_bytes_changes_nothing() {
        let mut h = Harness::new("tree-digest");
        let bin = resolve::py_path_join(&h.dir, "bin");
        std::fs::create_dir_all(&bin).unwrap();
        let binary = resolve::py_path_join(&bin, "yt-dlp.exe");
        std::fs::write(&binary, b"the running tree").unwrap();
        let zip = tree_zip("yt-dlp.exe");
        let wrong = crate::sha256::sha256_hex(b"other bytes");
        h.http = FakeHttp::new()
            .with_text(
                YTDLP_RELEASES_API,
                &tree_release("2026.09.01", "yt-dlp_win.zip", Some(&wrong)),
            )
            .with_file("https://example.invalid/tree", &zip)
            .with_text(
                RELEASES_API,
                &release_json(
                    &format!("v{}", crate::VERSION),
                    "ytdlp-native-windows-x86_64.exe",
                    Some(GYAN),
                ),
            );
        h.probes.push_back(("2026.08.19".to_string(), 0));
        let mut s = settings();
        assert!(!h.all(&mut s).unwrap());
        assert_eq!(std::fs::read(&binary).unwrap(), b"the running tree");
        assert!(!std::path::Path::new(&format!("{binary}.old")).exists());
        assert!(!std::path::Path::new(&format!("{bin}/_internal")).exists());
        assert!(
            std::fs::read_dir(&bin).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".yt-dlp-")),
            "the stage is gone"
        );
        assert!(
            h.printed()
                .iter()
                .any(|l| l == "the downloaded build did not match the published digest"),
            "{:?}",
            h.printed()
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    /// No published digest, no download.
    #[test]
    fn a_tree_release_without_a_digest_is_a_refusal() {
        let mut h = Harness::new("tree-no-digest");
        let bin = resolve::py_path_join(&h.dir, "bin");
        std::fs::create_dir_all(&bin).unwrap();
        let binary = resolve::py_path_join(&bin, "yt-dlp.exe");
        std::fs::write(&binary, b"the running tree").unwrap();
        h.http = FakeHttp::new()
            .with_text(
                YTDLP_RELEASES_API,
                &tree_release("2026.09.01", "yt-dlp_win.zip", None),
            )
            .with_text(
                RELEASES_API,
                &release_json(
                    &format!("v{}", crate::VERSION),
                    "ytdlp-native-windows-x86_64.exe",
                    Some(GYAN),
                ),
            );
        h.probes.push_back(("2026.08.19".to_string(), 0));
        let mut s = settings();
        assert!(!h.all(&mut s).unwrap());
        assert_eq!(std::fs::read(&binary).unwrap(), b"the running tree");
        let calls = h.http.calls();
        assert!(calls.iter().all(|c| !c.starts_with("file ")), "{calls:?}");
        assert!(
            h.printed()
                .iter()
                .any(|l| l == "the published release has no digest for yt-dlp_win.zip"),
            "{:?}",
            h.printed()
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    /// A half-moved swap puts the previous pair back: the final rename is
    /// forced to fail by aiming the internal target into a missing
    /// folder.
    #[test]
    fn a_failed_swap_restores_the_previous_payload() {
        let dir = scratch("swap-rollback");
        let binary = format!("{dir}/yt-dlp.exe");
        let internal = format!("{dir}/_internal");
        std::fs::create_dir_all(&internal).unwrap();
        std::fs::write(&binary, b"old exe").unwrap();
        std::fs::write(format!("{internal}/old.dll"), b"old dll").unwrap();
        let staged_exe = format!("{dir}/new-exe");
        let staged_internal = format!("{dir}/new-internal");
        std::fs::create_dir_all(&staged_internal).unwrap();
        std::fs::write(&staged_exe, b"new exe").unwrap();
        std::fs::write(format!("{staged_internal}/new.dll"), b"new dll").unwrap();
        let bad_target = format!("{dir}/gone/_internal");
        let err = swap_payload(&binary, &bad_target, &staged_exe, &staged_internal).unwrap_err();
        assert!(err.contains("could not place the new payload"), "{err}");
        assert_eq!(std::fs::read(&binary).unwrap(), b"old exe");
        assert_eq!(
            std::fs::read(format!("{internal}/old.dll")).unwrap(),
            b"old dll"
        );
        assert!(!std::path::Path::new(&format!("{binary}.old")).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A zip that is not a onedir tree is refused before the target is
    /// touched.
    #[test]
    fn an_incomplete_tree_is_refused_before_the_swap() {
        let dir = scratch("swap-incomplete");
        let binary = format!("{dir}/yt-dlp.exe");
        let internal = format!("{dir}/_internal");
        std::fs::create_dir_all(&internal).unwrap();
        std::fs::write(&binary, b"old exe").unwrap();
        std::fs::write(format!("{internal}/old.dll"), b"old dll").unwrap();
        let staged_exe = format!("{dir}/new-exe");
        std::fs::write(&staged_exe, b"new exe").unwrap();
        let err = swap_payload(
            &binary,
            &internal,
            &staged_exe,
            &format!("{dir}/no-internal"),
        )
        .unwrap_err();
        assert_eq!(err, "the downloaded yt-dlp tree is incomplete");
        assert_eq!(std::fs::read(&binary).unwrap(), b"old exe");
        assert_eq!(
            std::fs::read(format!("{internal}/old.dll")).unwrap(),
            b"old dll"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn update_all_runs_both_halves() {
        let mut h = Harness::new("both");
        let yt = resolve::py_path_join(&h.dir, "yt-dlp.exe");
        std::fs::write(&yt, b"yt-dlp").unwrap();
        let mut s = settings();
        config::set(&mut s, "yt_dlp_path", Value::Str(yt.clone()));
        // the yt-dlp half answers -U and reports a new version; the cli
        // half finds itself current
        h.probes.push_back(("1.5.0".to_string(), 0));
        h.probes.push_back(("2026.10.10".to_string(), 0));
        h.probes.push_back(("1.5.0".to_string(), 0));
        let body = release_json(
            &format!("v{}", crate::VERSION),
            "ytdlp-native-windows-x86_64.exe",
            Some(GYAN),
        );
        h.http = FakeHttp::new().with_text(RELEASES_API, &body);
        // the yt-dlp half answers -U with rc 0, which is python's "moved"
        assert!(h.all(&mut s).unwrap());
        assert_eq!(h.updates, vec![format!("{yt} -U")]);
        let printed = h.printed();
        assert_eq!(printed[0], "  yt-dlp");
        assert_eq!(printed[1], format!("binary      : {yt}"));
        assert_eq!(printed[2], "version now : 1.5.0");
        assert_eq!(printed[3], "version after: 2026.10.10");
        assert_eq!(printed[4], "  cli");
        assert_eq!(
            printed[5],
            format!(
                "  published v{}, this is v{} - already current",
                crate::VERSION,
                crate::VERSION
            )
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn a_missing_yt_dlp_binary_is_reported_and_nothing_is_spawned() {
        let mut h = Harness::new("no-ytdlp");
        let mut s = settings();
        config::set(&mut s, "yt_dlp_path", Value::Str(format!("{}/gone", h.dir)));
        assert!(!h.all(&mut s).unwrap());
        assert_eq!(h.updates, Vec::<String>::new());
        let printed = h.printed();
        assert_eq!(printed[0], "  yt-dlp");
        assert_eq!(printed[1], "yt-dlp binary not found.");
        assert_eq!(
            printed[2],
            "put yt-dlp in bin/ or on PATH, or set yt_dlp_path in config/settings.json"
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn a_yt_dlp_that_fails_its_pre_check_is_not_started() {
        let mut h = Harness::new("bad-ytdlp");
        let yt = resolve::py_path_join(&h.dir, "yt-dlp.exe");
        std::fs::write(&yt, b"yt-dlp").unwrap();
        let mut s = settings();
        config::set(&mut s, "yt_dlp_path", Value::Str(yt));
        h.probes.push_back((String::new(), 127));
        assert!(!h.all(&mut s).unwrap());
        assert_eq!(h.updates, Vec::<String>::new());
        assert!(
            h.printed()
                .iter()
                .any(|l| l == "yt-dlp binary check failed (rc=127): "),
            "{:?}",
            h.printed()
        );
        std::fs::remove_dir_all(&h.dir).unwrap();
    }

    #[test]
    fn version_tuple_matches_selftest_pins() {
        assert!(version_tuple(Some("1.10.0")) > version_tuple(Some("1.9.0")));
        assert!(version_tuple(Some("1.2.0")) < version_tuple(Some("1.3")));
        assert_eq!(version_tuple(Some("")), vec![0]);
        assert_eq!(version_tuple(None), vec![0]);
        assert_eq!(version_tuple(Some("1.3")), vec![1, 3]);
        assert!(version_tuple(Some("1.3-beta")) >= version_tuple(Some("1.3")));
        assert_eq!(version_tuple(Some("dev")), vec![0]);
        assert_eq!(version_tuple(Some("1..2")), vec![1, 2]);
        assert_eq!(version_tuple(Some("v1.2+3")), vec![0, 2, 3]);
    }

    #[test]
    fn version_piece_beyond_i64_saturates() {
        let big = "9".repeat(30);
        assert_eq!(version_tuple(Some(&format!("1.{big}"))), vec![1, i64::MAX]);
    }

    #[test]
    fn parse_sha256_matches_selftest_pins() {
        assert_eq!(parse_sha256(GYAN), Some(GYAN.to_string()));
        assert_eq!(
            parse_sha256(&format!("{GYAN}  ffmpeg-9.0.2.zip")),
            Some(GYAN.to_string())
        );
        assert_eq!(parse_sha256("no digest here"), None);
        assert_eq!(parse_sha256(""), None);
        assert_eq!(parse_sha256("tooshort"), None);
        assert_eq!(parse_sha256(&"g".repeat(64)), None);
        assert_eq!(
            parse_sha256(&format!("{}  *ffmpeg.zip", GYAN.to_uppercase())),
            Some(GYAN.to_string())
        );
    }

    #[test]
    fn pick_checksum_matches_selftest_pins() {
        let text =
            format!("aaa  x.zip\n{BTBN}  ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz\nbbb  y.zip\n");
        assert_eq!(
            pick_checksum(&text, "ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz"),
            Some(BTBN.to_string())
        );
        assert_eq!(pick_checksum("aaa  x.zip\nbbb  y.zip\n", "z.zip"), None);
        assert_eq!(pick_checksum("", "z.zip"), None);
    }
}
