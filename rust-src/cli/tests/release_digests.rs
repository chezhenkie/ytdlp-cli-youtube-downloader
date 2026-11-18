//! G2 addition (phase 2): the bundle digests pinned in
//! `RELEASE-NOTES.md` are checked with the hand-rolled sha256 whenever
//! the release artifacts exist locally. `releases/` is gitignored, so
//! the hashing part skips in CI and on fresh clones. The README carries
//! no digest rows: the same file ships inside every bundle, so a
//! release's numbers live in that release's notes only.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use ytdlp_cli::sha256::Sha256;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Streaming hash, 1 MiB chunks, mirroring the python `sha256_file`.
fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xF) as usize] as char);
    }
    out
}

/// Digest rows in the notes look like `<64 hex>  <artifact file name>`;
/// the name embeds the release tag.
fn digest_rows(text: &str) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(digest), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let is_digest = digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit());
        if is_digest && name.starts_with("ytdlp-cli-v") {
            rows.push((digest.to_string(), name.to_string()));
        }
    }
    rows
}

/// A README that ships inside every bundle must not pin any release, in
/// any shape: a versioned artifact name or a digest token counts, even
/// when the line is a table row rather than a checksum block.
fn release_pin(text: &str) -> Option<String> {
    for line in text.lines() {
        for token in line.split(|c: char| {
            !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '<' || c == '>')
        }) {
            let lower = token.to_ascii_lowercase();
            let versioned = lower.starts_with("ytdlp-cli-v")
                && lower
                    .as_bytes()
                    .get("ytdlp-cli-v".len())
                    .is_some_and(|b| b.is_ascii_digit());
            let digest = token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit());
            if versioned || digest {
                return Some(line.to_string());
            }
        }
    }
    None
}

/// Every file under `releases/`, at any depth: the release tree keeps
/// the published files under per-flavour bundle folders.
fn release_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            release_files(&path, found);
        } else {
            found.push(path);
        }
    }
}

#[test]
fn notes_pinned_digests_match_when_releases_exist() {
    let root = repo_root();
    let readme = std::fs::read_to_string(root.join("README.md")).expect("README.md readable");
    if let Some(line) = release_pin(&readme) {
        panic!("README.md ships in every bundle and must not pin a release: {line}");
    }

    let notes =
        std::fs::read_to_string(root.join("RELEASE-NOTES.md")).expect("RELEASE-NOTES.md readable");
    let rows = digest_rows(&notes);
    assert!(!rows.is_empty(), "RELEASE-NOTES.md carries no digest rows");

    let mut files = Vec::new();
    release_files(&root.join("releases"), &mut files);
    if files.is_empty() {
        println!("releases/ absent: RELEASE-NOTES digest check skipped (dev-only, per the plan)");
        return;
    }
    let mut present = 0;
    for (digest, name) in &rows {
        let path = files.iter().find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy() == name.as_str())
                .unwrap_or(false)
        });
        let Some(path) = path else {
            continue;
        };
        present += 1;
        let got = sha256_file(path).unwrap_or_else(|e| panic!("hash {}: {e}", path.display()));
        assert_eq!(&got, digest, "digest mismatch for {name}");
    }
    assert_eq!(
        present,
        rows.len(),
        "some pinned artifacts are missing locally: {present} of {} found",
        rows.len()
    );
    println!("{present} release artifacts hashed and matched");
}
