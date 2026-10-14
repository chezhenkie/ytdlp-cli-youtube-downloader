//! Process execution text surfaces (specsheet section 4, layer 5; FR-10
//! verdicts and the IF-4 parse-miss helpers): the frozen failure
//! sentence and the subtitle verdict lines. All pure so the selftest can
//! pin them without a download run, and so the differential harness can
//! byte-compare them.

use crate::term::ascii_safe;

/// Frozen sentence for a failed run: the rc always, yt-dlp's error line
/// when there is one, full length - the old `err[:WIDTH]` cut kept the
/// error class but dropped the tail (FR-10).
pub fn fail_sentence(rc: i64, err: &str) -> String {
    if err.is_empty() {
        format!("downloading - failed (rc={rc})")
    } else {
        format!("downloading - failed (rc={rc}): {err}")
    }
}

/// Short reason for a failed subtitle pass, classified from yt-dlp's
/// last error line. None when nothing is recognizable - the caller then
/// shows no second line.
pub fn sub_fail_reason(err: Option<&str>) -> Option<String> {
    let low = err.unwrap_or("").to_lowercase();
    if low.contains("429") || low.contains("too many requests") {
        return Some("the subtitle api answered 429 too many requests".to_string());
    }
    if low.contains("timed out") || low.contains("timeout") {
        return Some("the subtitle request timed out".to_string());
    }
    if low.contains("connection") || low.contains("network") || low.contains("unreachable") {
        return Some("the network was unreachable".to_string());
    }
    None
}

/// Verdict lines for a subtitle pass that exited 0: (verdict, miss_note).
/// miss_note is empty when codes were read, else the IF-4 parse-miss line:
/// yt-dlp succeeded but the stdout scrape read nothing, which points at
/// reworded upstream output rather than missing subs.
pub fn sub_success_lines(codes: &[String]) -> (String, String) {
    if !codes.is_empty() {
        let joined = codes
            .iter()
            .map(|code| ascii_safe(code))
            .collect::<Vec<_>>()
            .join(", ");
        (format!("subtitles - {joined}"), String::new())
    } else {
        (
            "subtitles - none available".to_string(),
            "note: yt-dlp reported success but no subtitle files were read \
             - wording may have changed, please report"
                .to_string(),
        )
    }
}

/// Verdict lines for a failed subtitle pass: the not-fetched verdict plus
/// the classified reason, or the IF-4 note when the error line matched no
/// known wording.
pub fn sub_fail_lines(err: Option<&str>) -> Vec<String> {
    let mut lines =
        vec!["subtitles - not fetched (video kept). unable to download subtitles".to_string()];
    match sub_fail_reason(err) {
        Some(reason) => lines.push(format!("subtitle error - {reason}")),
        None => {
            lines.push("subtitle error - yt-dlp wording not recognized - please report".to_string())
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::WIDTH;

    #[test]
    fn fail_sentence_matches_selftest_pins() {
        assert_eq!(fail_sentence(7, ""), "downloading - failed (rc=7)");
        let long_err = "ERROR: unable to rename file: [WinError 32] The process cannot \
                        access the file because it is being used by another process";
        assert_eq!(
            fail_sentence(1, long_err),
            "downloading - failed (rc=1): ERROR: unable to rename file: [WinError 32] \
             The process cannot access the file because it is being used by another process"
        );
        assert!(fail_sentence(1, long_err).len() as i64 > WIDTH);
    }

    #[test]
    fn sub_reasons_match_selftest_pins() {
        let r429_src =
            "ERROR: Unable to download video subtitles for 'en': HTTP Error 429: Too Many Requests";
        assert_eq!(
            sub_fail_reason(Some(r429_src)).as_deref(),
            Some("the subtitle api answered 429 too many requests")
        );
        assert_eq!(sub_fail_reason(Some("SOMETHING ODD")), None);
        assert_eq!(sub_fail_reason(None), None);
        assert_eq!(
            sub_fail_reason(Some("HTTP Error 429")).as_deref(),
            Some("the subtitle api answered 429 too many requests")
        );
        assert_eq!(
            sub_fail_reason(Some("The request timed out")).as_deref(),
            Some("the subtitle request timed out")
        );
        assert_eq!(
            sub_fail_reason(Some("connection reset by peer")).as_deref(),
            Some("the network was unreachable")
        );
    }

    #[test]
    fn sub_success_lines_match_selftest_pins() {
        let (verdict, miss) = sub_success_lines(&["en".to_string(), "en-GB".to_string()]);
        assert_eq!(verdict, "subtitles - en, en-GB");
        assert_eq!(miss, "");
        let (verdict, miss) = sub_success_lines(&[]);
        assert_eq!(verdict, "subtitles - none available");
        assert_eq!(
            miss,
            "note: yt-dlp reported success but no subtitle files were read \
             - wording may have changed, please report"
        );
    }

    #[test]
    fn sub_fail_lines_match_selftest_pins() {
        let r429_src =
            "ERROR: Unable to download video subtitles for 'en': HTTP Error 429: Too Many Requests";
        let lines = sub_fail_lines(Some(r429_src));
        assert_eq!(
            lines[0],
            "subtitles - not fetched (video kept). unable to download subtitles"
        );
        assert_eq!(
            lines[1],
            "subtitle error - the subtitle api answered 429 too many requests"
        );
        let lines = sub_fail_lines(Some("SOMETHING ODD"));
        assert_eq!(
            lines[1],
            "subtitle error - yt-dlp wording not recognized - please report"
        );
    }
}
