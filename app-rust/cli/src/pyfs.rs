//! Filesystem primitives with python's semantics (specsheet section 4,
//! layer 7; the phase-8 port notes): `os.replace`,
//! `tempfile.mkdtemp`, `shutil.rmtree(ignore_errors=True)`, the
//! `os.chmod(target, 0o755)` step and
//! `Path.with_name(name + ".part")`.
//!
//! Only the updaters and the ffmpeg install use these; every other path
//! in the port is rendered by `resolve`. The differences from the std
//! calls are the ones python's behaviour depends on:
//! `os.replace` must overwrite an existing destination (windows'
//! `rename` refuses), `mkdtemp` must hand out a name nobody else holds,
//! and `rmtree` must swallow every error because it runs in a `finally`
//! that must not mask the real result.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};

/// `os.replace(src, dst)`: rename over whatever is already there.
/// `std::fs::rename` fails on windows when the destination exists,
/// which would turn "replace the old ffmpeg" and "place the new cli"
/// into refusals.
pub fn replace(src: &str, dst: &str) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        fn wide(path: &str) -> Vec<u16> {
            std::ffi::OsStr::new(path)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }
        let (from, to) = (wide(src), wide(dst));
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING,
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(src, dst)
    }
}

/// Serial suffix for `mkdtemp`; the atomic `create_dir` is what makes a
/// collision impossible, this only keeps the names apart.
static MKDTEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// `tempfile.mkdtemp(prefix, dir)`: a fresh directory whose name starts
/// with `prefix`, created inside `dir`.
pub fn mkdtemp(prefix: &str, dir: &str) -> io::Result<String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    for _ in 0..64 {
        let seq = MKDTEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = crate::resolve::py_path_join(dir, &format!("{prefix}{stamp:x}-{seq:x}"));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            // taken by someone else: another name, same guarantee
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free temp name"))
}

/// `shutil.rmtree(path, ignore_errors=True)`: best effort, every error
/// swallowed, symlinks unlinked rather than followed.
pub fn rmtree_quiet(path: &str) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            rmtree_quiet(&entry.path().to_string_lossy());
        }
    }
    let _ = std::fs::remove_dir(path);
}

/// `os.chmod(target, 0o755)`, which python only calls when
/// `os.name != "nt"`.
pub fn chmod_755(path: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// `dest.with_name(dest.name + suffix)`: the sibling name. Since the
/// suffix is appended to the file name and the directory is kept, this
/// is the path plus the suffix.
pub fn with_suffix(path: &str, suffix: &str) -> String {
    format!("{path}{suffix}")
}

/// `Path.unlink(missing_ok=True)`.
pub fn remove_quiet(path: &str) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir()
            .join(format!("ytdlp-pyfs-test-{}-{name}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn with_suffix_takes_the_last_component() {
        assert_eq!(with_suffix("/a/b/blob.zip", ".part"), "/a/b/blob.zip.part");
        assert_eq!(with_suffix("blob.zip", ".part"), "blob.zip.part");
        assert_eq!(
            with_suffix("/a/b/ffmpeg.exe", ".new"),
            "/a/b/ffmpeg.exe.new"
        );
    }

    #[test]
    fn replace_overwrites_an_existing_file() {
        let dir = scratch("replace");
        let old = format!("{dir}/old");
        let new = format!("{dir}/new");
        std::fs::write(&old, b"old bytes").unwrap();
        std::fs::write(&new, b"new bytes").unwrap();
        replace(&new, &old).unwrap();
        assert_eq!(std::fs::read(&old).unwrap(), b"new bytes");
        assert!(!std::path::Path::new(&new).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mkdtemp_hands_out_distinct_directories() {
        let dir = scratch("mkdtemp");
        let mut made = Vec::new();
        for _ in 0..4 {
            let path = mkdtemp(".ff-", &dir).unwrap();
            assert!(
                std::path::Path::new(&path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".ff-"),
                "prefix kept: {path}"
            );
            assert!(!made.contains(&path), "fresh name: {path}");
            made.push(path);
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 4);
        rmtree_quiet(&dir);
        assert!(!std::path::Path::new(&dir).exists());
    }

    #[test]
    fn rmtree_quiet_swallows_everything() {
        let dir = scratch("rmtree");
        let nested = format!("{dir}/.ff-1/bin");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(format!("{nested}/ffmpeg"), b"x").unwrap();
        rmtree_quiet(&dir);
        assert!(!std::path::Path::new(&dir).exists());
        // a second pass and a missing path are both no-ops
        rmtree_quiet(&dir);
        rmtree_quiet(&format!("{dir}/gone"));
    }

    #[test]
    fn chmod_755_leaves_the_file_readable() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("chmod");
        let path = format!("{dir}/ffmpeg");
        std::fs::write(&path, b"x").unwrap();
        chmod_755(&path).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        #[cfg(unix)]
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
        #[cfg(not(unix))]
        assert!(meta.len() == 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
