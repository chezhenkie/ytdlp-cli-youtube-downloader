//! Update machinery, pure half (specsheet section 4, layer 6; FR-13):
//! version comparison and the two checksum-file parsers, so the updater's
//! decisions are testable without a network.

/// A version string as a comparable list. Non-numeric pieces count as 0
/// rather than raising, because the published string is not under our
/// control.
///
/// PORT NOTE (recorded for the spec change history): python splits on
/// `[.\-+]` and converts a piece with `p.isdigit()` - a unicode test - and
/// unbounded `int()`. The port takes ASCII digits only and saturates a
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

#[cfg(test)]
mod tests {
    use super::*;

    const GYAN: &str = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
    const BTBN: &str = "afa2b69c9965919e0b519a633520321f838d74690f8fb28c76393c54a9448d60";

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
