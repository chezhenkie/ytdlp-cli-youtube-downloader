//! The packaged selftest (phase 9): python's `selftest()` as a runtime
//! command.
//!
//! Check names, order and count follow the python selftest one to one,
//! plus the four native stripper checks the menu row 8 stage added and
//! the info-field checks the menu row 9 stage added and the two
//! description-rename checks the 2.5.2 stage added and the two
//! output-extension checks the 2.5.3 stage added (190 total),
//! so `rust-src/tools/selftest_compare.py` can run both
//! reports and diff the decoded name/verdict stream. Where python
//! monkeypatched a module global this module injects the seam the
//! ported function already takes - the same rule the crate tests and
//! the phase-6 harness arms follow. The deviations with no native
//! counterpart are each recorded in the specsheet conformance map
//! (section 10).
//!
//! The report itself is plain text - python's `print` never colored it -
//! and the production `Out` handles windows' newline translation, like
//! python's `print`. Captured menu subcalls go through a local capture
//! `Out`; where python printed a subcall straight into the report
//! (item 13's hold), this module does the same.

use crate::tty::Out;
use crate::ui::WIDTH;

mod plumbing;
mod sections;

use plumbing::*;
use sections::*;

/// The entry point: python's `selftest()` - the 190-check report, rc 0
/// when all pass and rc 1 like python's return.
pub fn run_selftest(
    version: &str,
    root: &str,
    config_path: &str,
    image_path: &str,
    out: &mut dyn Out,
) -> i64 {
    let capture = CaptureOut::default();
    let mut c = Checker::new(out);
    let auto = real_auto_outdir();
    let is_windows = cfg!(windows);

    // python's dispatch order: history, subs, args, progress, spinners,
    // args tail, partials, info field, urls, menus+updaters pins,
    // parsers, archive probes, langs, item 7, block, holds, batch, then
    // the audit set
    section_history(&mut c, &capture);
    section_subs(&mut c);
    section_args(&mut c, root, config_path, auto.as_deref());
    {
        let _ = capture; // the args block captures nothing
    }
    section_progress(&mut c);
    section_spinners(&mut c, &capture);
    section_args2(&mut c, root, auto.as_deref());
    section_partials(&mut c);
    section_info_field(&mut c);
    section_misc(&mut c);
    section_menu(&mut c, root, config_path, auto.as_deref(), is_windows);
    section_parsers(&mut c, root, image_path, is_windows);
    section_archive(&mut c);
    section_langs(&mut c);
    section_lang_input(&mut c);
    section_item7(&mut c, version, root, config_path, auto.as_deref());
    section_block(&mut c);
    section_holds(&mut c, version, root, config_path, auto.as_deref());
    section_batch(&mut c, version, root, config_path, auto.as_deref());
    section_audit(&mut c, version, root, config_path, auto.as_deref());

    let count = c.count;
    let failed = c.fails.len() as i64;
    finish_report(count, failed, c.report_out())
}

/// Python's closing pair (`"-" * WIDTH`, then the count summary); the
/// rc rule is 1 when any check failed (python's return).
fn finish_report(count: i64, failed: i64, out: &mut dyn Out) -> i64 {
    out.write(&"-".repeat(WIDTH as usize));
    out.write("\n");
    out.write(&format!("selftest: {count} checks, {failed} failed"));
    out.write("\n");
    out.flush();
    if failed > 0 { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sink's shape pinned: the report stays byte-equal to python's
    /// `%-4s %s%s` formatting.
    #[test]
    fn the_sink_formats_pass_fail() {
        let capture = CaptureOut::default();
        let mut echoed = capture.clone();
        let mut checker = Checker::new(&mut echoed);
        checker.check("one good", true, NO_GOT);
        checker.check("one bad", false, NO_GOT);
        assert_eq!(
            capture.text(),
            "PASS one good\nFAIL one bad  got: ''\n",
            "the sink text must be python's format"
        );
        assert_eq!(checker.count, 2);
        assert_eq!(checker.fails, vec!["one bad".to_string()]);
    }

    /// The report's summary line and rc rule: 0 when nothing failed, 1
    /// when any check did (python's return).
    #[test]
    fn the_summary_line_and_rc_rule() {
        let capture_a = CaptureOut::default();
        let mut echoed_a = capture_a.clone();
        let mut clean = Checker::new(&mut echoed_a);
        clean.check("good", true, NO_GOT);
        assert_eq!(
            finish_report(clean.count, clean.fails.len() as i64, &mut echoed_a),
            0
        );
        assert!(
            capture_a
                .text()
                .ends_with("\nselftest: 1 checks, 0 failed\n")
        );

        let capture_b = CaptureOut::default();
        let mut echoed_b = capture_b.clone();
        let mut failing = Checker::new(&mut echoed_b);
        failing.check("good", true, NO_GOT);
        failing.check("bad", false, NO_GOT);
        assert_eq!(
            finish_report(failing.count, failing.fails.len() as i64, &mut echoed_b),
            1
        );
        assert!(
            capture_b
                .text()
                .ends_with("\nselftest: 2 checks, 1 failed\n")
        );
    }
}
