//! G2 addition (phase 2): the README-pinned bundle
//! digests are checked with the hand-rolled sha256 whenever the release
//! artifacts exist locally. `releases/` is gitignored, so this test
//! skips silently in CI and on fresh clones.

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

/// Digest rows in the README look like
/// `<64 hex>  <artifact file name>`; the name embeds the release tag.
fn readme_digest_rows(text: &str) -> Vec<(String, String)> {
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

#[test]
fn readme_pinned_digests_match_when_releases_exist() {
    let root = repo_root();
    let readme = std::fs::read_to_string(root.join("README.md")).expect("README.md readable");
    let rows = readme_digest_rows(&readme);
    assert!(!rows.is_empty(), "README carries no digest rows");

    let mut present = 0;
    for (digest, name) in &rows {
        // ytdlp-cli-v1.5.0-windows-lean.zip -> release tag v1.5.0
        let version = name
            .split('-')
            .find(|part| {
                part.starts_with('v') && part[1..].starts_with(|c: char| c.is_ascii_digit())
            })
            .expect("artifact names embed the version");
        let path = root.join("releases").join(version).join(name);
        if !path.exists() {
            continue;
        }
        present += 1;
        let got = sha256_file(&path).unwrap_or_else(|e| panic!("hash {}: {e}", path.display()));
        assert_eq!(&got, digest, "digest mismatch for {name}");
    }
    if present == 0 {
        println!("releases/ absent: README digest check skipped (dev-only, per the plan)");
    } else {
        assert_eq!(
            present,
            rows.len(),
            "some pinned artifacts are missing locally"
        );
        println!("{present} release artifacts hashed and matched");
    }
}
