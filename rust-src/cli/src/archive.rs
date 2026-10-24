//! Archive member extraction (specsheet section 4, layer 7; FR-14).
//!
//! `extract_member` lifts the one needed binary out of a downloaded
//! archive. Both providers nest one versioned folder with
//! `bin/<binname>` inside, so python matches on the last path component
//! and writes only that member - no path handling, no zip-slip surface,
//! and nothing else from the archive touches disk.
//!
//! Port notes (phase 8): zip 7.2.0 and tar 0.4.46 come with default
//! features off, so only decode paths are compiled, and both
//! decompress through pure-rust crates (flate2's miniz_oxide, and
//! lzma-rust2 in place of the C xz2 - OQ-N3). python's rule that a
//! header anywhere in the archive must parse before anything is
//! written (`getmembers` first) is kept by reading every member header
//! in one pass, teeing the matching member to `<out>.partial` during
//! the same decode and renaming only after the pass succeeds; a
//! truncated archive therefore reports the same "unreadable" as
//! python, the partial is removed, and nothing is left behind.

use std::io::Write;

use crate::pyfs;
use crate::resolve::py_path_join;
use crate::tty::{self, SharedSink};

/// `extract_member(blob, how, binname, outdir)`: write the member whose
/// last path component is `binname` into `outdir`, or None with
/// python's two possible reasons printed.
pub fn extract_member(
    blob: &str,
    how: &str,
    binname: &str,
    outdir: &str,
    sink: &SharedSink,
) -> Option<String> {
    let out = py_path_join(outdir, binname);
    let result = match how {
        "zip" => from_zip(blob, binname, &out),
        "tar.xz" => from_tar_xz(blob, binname, &out),
        "tar.gz" => from_tar_gz(blob, binname, &out),
        // python's `else: return None` inside the try: no message here,
        // the caller decides what a missing member means
        _ => return None,
    };
    match result {
        Ok(found) => found,
        Err(e) => {
            tty::emit_line(
                sink,
                &format!("  the downloaded archive was unreadable: {e}"),
            );
            None
        }
    }
}

/// python's member rule: the last path component equals the binary name,
/// and the entry is not a directory.
fn member_hit(name: &str, binname: &str) -> bool {
    !name.ends_with('/') && name.rsplit('/').next() == Some(binname)
}

/// Extract every member of a zip into `dest`, keeping the archive's
/// relative paths. The yt-dlp onedir zips are the consumer: the binary
/// plus `_internal` are the tree the CLI runs, so unlike `extract_member`
/// this writes the whole archive. Every member name is validated before
/// anything is written (the same headers-first rule as
/// `extract_member`), and a name that is absolute, carries a drive or
/// backslash, or holds a `.`/`..`/empty component is refused - no
/// zip-slip surface.
pub fn extract_zip_all(blob: &str, dest: &str) -> Result<(), String> {
    let file = std::fs::File::open(blob).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut plan: Vec<(usize, String, bool)> = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|e| e.to_string())?;
        plan.push((index, safe_member_path(entry.name())?, entry.is_dir()));
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for (index, rel, is_dir) in plan {
        let out = std::path::Path::new(dest).join(&rel);
        if is_dir {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let out_text = out.to_string_lossy().into_owned();
        write_member(&mut entry, &out_text)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(())
}

/// The zip member name as a safe relative path, or the refusal text.
fn safe_member_path(name: &str) -> Result<String, String> {
    let refuse = || format!("refused member name: {name}");
    if name.contains('\\') || name.contains(':') || name.starts_with('/') {
        return Err(refuse());
    }
    let trimmed = name.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(refuse());
    }
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(refuse());
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

fn write_member<R: std::io::Read>(src: &mut R, out: &str) -> Result<(), String> {
    let mut fh = std::fs::File::create(out).map_err(|e| e.to_string())?;
    std::io::copy(src, &mut fh).map_err(|e| e.to_string())?;
    fh.flush().map_err(|e| e.to_string())
}

fn from_zip(blob: &str, binname: &str, out: &str) -> Result<Option<String>, String> {
    let file = std::fs::File::open(blob).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    // the central directory first (python's namelist()), so a truncated
    // archive fails before any write
    let mut hit: Option<usize> = None;
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|e| e.to_string())?
            .name()
            .to_string();
        if member_hit(&name, binname) {
            hit = Some(index);
            break;
        }
    }
    let Some(index) = hit else {
        return Ok(None);
    };
    let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
    write_member(&mut entry, out)?;
    Ok(Some(out.to_string()))
}

fn from_tar_xz(blob: &str, binname: &str, out: &str) -> Result<Option<String>, String> {
    from_tar(
        || {
            let file = std::fs::File::open(blob).map_err(|e| e.to_string())?;
            Ok(lzma_rust2::XzReader::new(file, false))
        },
        binname,
        out,
    )
}

/// The cli bundles are gzipped tars (`tar -czf` in bundles.yml), so the
/// self-update reads `.tar.gz` beside the provider `.tar.xz` path.
fn from_tar_gz(blob: &str, binname: &str, out: &str) -> Result<Option<String>, String> {
    from_tar(
        || {
            let file = std::fs::File::open(blob).map_err(|e| e.to_string())?;
            Ok(flate2::read::GzDecoder::new(file))
        },
        binname,
        out,
    )
}

/// The single-pass tar read: every member header validates the whole
/// stream, and the first matching file member tees straight out during
/// the same decode - the xz stream is read once, not twice (audit perf
/// 7). The tee lands as `<out>.partial` and is renamed only after the
/// header pass succeeded, so a truncated archive still writes nothing.
fn from_tar<R: std::io::Read>(
    open: impl Fn() -> Result<R, String>,
    binname: &str,
    out: &str,
) -> Result<Option<String>, String> {
    let partial = format!("{out}.partial");
    let mut hit = false;
    // pass 1: every member header (python's getmembers), comparing the
    // raw name bytes - a path python would decode with surrogateescape
    // is compared here as bytes
    let pass = (|| -> Result<(), String> {
        let mut archive = tar::Archive::new(open()?);
        let entries = archive.entries().map_err(|e| e.to_string())?;
        for entry in entries {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let is_file = entry.header().entry_type().is_file();
            let name = entry.path_bytes().into_owned();
            if !hit && is_file && name.rsplit(|b| *b == b'/').next() == Some(binname.as_bytes()) {
                hit = true;
                let mut fh = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
                std::io::copy(&mut entry, &mut fh).map_err(|e| e.to_string())?;
                fh.flush().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })();
    if let Err(e) = pass {
        if hit {
            pyfs::remove_quiet(&partial);
        }
        return Err(e);
    }
    if !hit {
        return Ok(None);
    }
    // the header pass ran to the end: the archive is whole, promote the
    // tee'd member (the rename preserves the file's 0644; the caller
    // sets the exec mode)
    if let Err(e) = pyfs::replace(&partial, out) {
        pyfs::remove_quiet(&partial);
        return Err(e.to_string());
    }
    Ok(Some(out.to_string()))
}

// fixture builders, shipped: the packaged selftest builds the same
// fixtures at runtime (nothing carries archive bytes through a vector)

/// The published zip layout (gyan essentials and the BtbN win64 build
/// share it): one versioned folder, `bin/` inside it, the binary at
/// `bin/ffmpeg.exe`.
pub(crate) fn zip_with(names: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};
    let mut buf = Cursor::new(Vec::new());
    {
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let mut w = ZipWriter::new(&mut buf);
        for (name, body) in names {
            w.start_file(*name, options).unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// Same nesting in a tar.xz, encoded at build time like python's
/// `tarfile.open("w:xz")` fixture does - the packaged selftest and the
/// tests share the builder (the `encoder` feature is a main
/// dependency now, G8's decode-only build is recorded as amended).
pub(crate) fn tar_xz_with(names: &[(&str, &[u8])]) -> Vec<u8> {
    let raw = tar_raw(names);
    let mut out = Vec::new();
    let mut enc =
        lzma_rust2::XzWriter::new(&mut out, lzma_rust2::XzOptions::with_preset(1)).unwrap();
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap();
    out
}

/// Same nesting gzipped: the shape the cli's own linux lean tar carries
/// (`ytdlp-cli-v<ver>/ytdlp`). Test-only: the packaged selftest keeps
/// its providers' zip and tar.xz fixtures, the gz path is pinned by the
/// crate test below.
#[cfg(test)]
pub(crate) fn tar_gz_with(names: &[(&str, &[u8])]) -> Vec<u8> {
    let raw = tar_raw(names);
    let mut out = Vec::new();
    let mut enc = flate2::write::GzEncoder::new(&mut out, flate2::Compression::fast());
    enc.write_all(&raw).unwrap();
    enc.finish().unwrap();
    out
}

fn tar_raw(names: &[(&str, &[u8])]) -> Vec<u8> {
    let mut raw = Vec::new();
    {
        let mut b = tar::Builder::new(&mut raw);
        for (name, body) in names {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            b.append_data(&mut header, name, *body).unwrap();
        }
        b.finish().unwrap();
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve;
    use std::sync::Arc;
    use std::sync::Mutex;

    /// A recording sink plus its log, so a test can read the printed
    /// lines without a terminal.
    fn recording() -> (SharedSink, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = tty::SharedOut::new(LineOut {
            lines: Arc::clone(&lines),
        })
        .sink();
        (sink, lines)
    }

    /// Records one text as one line: `emit_line` writes the text and
    /// then the newline, so a recorder keyed on lines is easier to
    /// assert than the raw event log.
    struct LineOut {
        lines: Arc<Mutex<Vec<String>>>,
    }

    impl tty::Out for LineOut {
        fn write(&mut self, text: &str) {
            if text == "\n" {
                return;
            }
            self.lines.lock().unwrap().push(text.to_string());
        }
        fn flush(&mut self) {}
    }

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir()
            .join(format!("ytdlp-arch-test-{}-{name}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The tests reuse the shipped fixture builders (the packaged
    /// selftest builds the same bytes).

    #[test]
    fn zip_lifts_the_nested_binary_and_skips_the_rest() {
        let dir = scratch("zip-ok");
        let blob = format!("{dir}/archive.zip");
        let bytes = zip_with(&[
            ("ffmpeg-9.0.2-essentials_build/bin/other.exe", b"other"),
            (
                "ffmpeg-9.0.2-essentials_build/bin/ffmpeg.exe",
                b"the binary",
            ),
            ("ffmpeg-9.0.2-essentials_build/doc.txt", b"doc"),
        ]);
        std::fs::write(&blob, &bytes).unwrap();
        let out_dir = format!("{dir}/out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let (sink, lines) = recording();
        let out = extract_member(&blob, "zip", "ffmpeg.exe", &out_dir, &sink);
        assert_eq!(out, Some(resolve::py_path_join(&out_dir, "ffmpeg.exe")));
        assert_eq!(std::fs::read(out.unwrap()).unwrap(), b"the binary");
        assert!(lines.lock().unwrap().is_empty());
        // nothing else from the archive landed
        assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tar_xz_lifts_the_nested_binary() {
        let dir = scratch("tar-ok");
        let blob = format!("{dir}/archive.tar.xz");
        let bytes = tar_xz_with(&[
            ("ffmpeg-n9.0/bin/ffprobe", b"probe"),
            ("ffmpeg-n9.0/bin/ffmpeg", b"the tar binary"),
        ]);
        std::fs::write(&blob, &bytes).unwrap();
        let out_dir = format!("{dir}/out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let (sink, lines) = recording();
        let out = extract_member(&blob, "tar.xz", "ffmpeg", &out_dir, &sink);
        assert_eq!(out, Some(resolve::py_path_join(&out_dir, "ffmpeg")));
        assert_eq!(std::fs::read(out.unwrap()).unwrap(), b"the tar binary");
        assert!(lines.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tar_gz_lifts_the_nested_binary() {
        let dir = scratch("targz-ok");
        let blob = format!("{dir}/archive.tar.gz");
        let bytes = tar_gz_with(&[
            ("ytdlp-cli-v2.6.1/README.md", b"doc"),
            ("ytdlp-cli-v2.6.1/ytdlp", b"the cli image"),
        ]);
        std::fs::write(&blob, &bytes).unwrap();
        let out_dir = format!("{dir}/out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let (sink, lines) = recording();
        let out = extract_member(&blob, "tar.gz", "ytdlp", &out_dir, &sink);
        assert_eq!(out, Some(resolve::py_path_join(&out_dir, "ytdlp")));
        assert_eq!(std::fs::read(out.unwrap()).unwrap(), b"the cli image");
        assert!(lines.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_member_that_is_not_there_is_none_and_quiet() {
        let dir = scratch("missing");
        let blob = format!("{dir}/archive.zip");
        std::fs::write(&blob, zip_with(&[("a/bin/other", b"x")])).unwrap();
        let out_dir = format!("{dir}/out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let (sink, lines) = recording();
        assert_eq!(
            extract_member(&blob, "zip", "ffmpeg", &out_dir, &sink),
            None
        );
        assert!(lines.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_truncated_archive_reports_unreadable_and_writes_nothing() {
        let dir = scratch("truncated");
        let cases = [
            ("zip", zip_with(&[("a/bin/ffmpeg", b"0123456789")])),
            ("tar.xz", tar_xz_with(&[("a/bin/ffmpeg", b"0123456789")])),
            ("tar.gz", tar_gz_with(&[("a/bin/ffmpeg", b"0123456789")])),
        ];
        for (how, whole) in cases {
            let blob = format!("{dir}/{how}.blob");
            std::fs::write(&blob, &whole[..whole.len() / 2]).unwrap();
            let out_dir = format!("{dir}/out-{how}");
            std::fs::create_dir_all(&out_dir).unwrap();
            let (sink, lines) = recording();
            assert_eq!(extract_member(&blob, how, "ffmpeg", &out_dir, &sink), None);
            let printed = lines.lock().unwrap().clone();
            assert_eq!(printed.len(), 1, "{how}: {printed:?}");
            assert!(
                printed[0].starts_with("  the downloaded archive was unreadable: "),
                "{how}: {printed:?}"
            );
            assert_eq!(
                std::fs::read_dir(&out_dir).unwrap().count(),
                0,
                "{how}: nothing written"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn zip_all_writes_the_whole_tree() {
        let dir = scratch("zip-all");
        let blob = format!("{dir}/archive.zip");
        let bytes = zip_with(&[
            ("yt-dlp.exe", b"the binary"),
            ("_internal/python3.dll", b"the dll"),
            ("_internal/lib/sub/file.txt", b"nested"),
        ]);
        std::fs::write(&blob, &bytes).unwrap();
        let out_dir = format!("{dir}/out");
        extract_zip_all(&blob, &out_dir).unwrap();
        assert_eq!(
            std::fs::read(format!("{out_dir}/yt-dlp.exe")).unwrap(),
            b"the binary"
        );
        assert_eq!(
            std::fs::read(format!("{out_dir}/_internal/python3.dll")).unwrap(),
            b"the dll"
        );
        assert_eq!(
            std::fs::read(format!("{out_dir}/_internal/lib/sub/file.txt")).unwrap(),
            b"nested"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn zip_all_refuses_an_unsafe_member_before_writing() {
        let dir = scratch("zip-slip");
        for name in ["../escape.txt", "/abs.txt", "a/../../b.txt", "a\\b.txt"] {
            let blob = format!("{dir}/bad.zip");
            std::fs::write(&blob, zip_with(&[("ok.txt", b"x"), (name, b"bad")])).unwrap();
            let out_dir = format!("{dir}/out");
            let err = extract_zip_all(&blob, &out_dir).unwrap_err();
            assert!(err.starts_with("refused member name: "), "{name}: {err}");
            assert!(!std::path::Path::new(&out_dir).exists(), "{name}: wrote");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn zip_all_reports_a_truncated_archive() {
        let dir = scratch("zip-all-truncated");
        let blob = format!("{dir}/archive.zip");
        let bytes = zip_with(&[("yt-dlp.exe", b"0123456789")]);
        std::fs::write(&blob, &bytes[..bytes.len() / 2]).unwrap();
        let err = extract_zip_all(&blob, &format!("{dir}/out")).unwrap_err();
        assert!(!err.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unknown_shape_is_none_and_quiet() {
        let dir = scratch("shape");
        let blob = format!("{dir}/archive.rar");
        std::fs::write(&blob, b"x").unwrap();
        let (sink, lines) = recording();
        assert_eq!(
            extract_member(&blob, "rar", "ffmpeg", dir.as_str(), &sink),
            None
        );
        assert!(lines.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
