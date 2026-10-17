//! G3 addition (RUST-PORT-PLAN.md phase 3): the tracked settings.json
//! round-trips through the rust state layer byte-exact. The tracked file
//! itself is only read; a copy in a temp dir carries the load plus no-op
//! save cycle, which is the mechanical meaning of "no-op save leaves
//! git diff empty". Runs in CI like the other workspace tests.

use std::path::{Path, PathBuf};
use ytdlp_cli::config::{LoadOutcome, load_settings, render_settings_file, save_settings};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn tracked_settings_json_roundtrips_byte_exact() {
    let source = repo_root()
        .join("deprecated-py-src")
        .join("config")
        .join("settings.json");
    if !source.exists() {
        println!("tracked settings.json absent: roundtrip check skipped (dev-only)");
        return;
    }
    let original = std::fs::read(&source).expect("tracked settings readable");

    let work =
        std::env::temp_dir().join(format!("ytdlp-settings-roundtrip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).expect("temp dir");
    let copy = work.join("settings.json");
    std::fs::write(&copy, &original).expect("copy settings");

    let LoadOutcome::Loaded { settings, notes } = load_settings(&copy).expect("load") else {
        panic!("the tracked settings.json must load");
    };
    assert!(
        notes.is_empty(),
        "the tracked settings.json carries no coercion notes: {notes:?}"
    );
    assert_eq!(
        render_settings_file(&settings).as_bytes(),
        original.as_slice(),
        "the tracked file must already be exactly what the writer produces"
    );
    save_settings(&copy, &settings).expect("save");
    assert_eq!(
        std::fs::read(&copy).expect("read back"),
        original,
        "a no-op save must not change a byte"
    );

    let _ = std::fs::remove_dir_all(&work);
}
