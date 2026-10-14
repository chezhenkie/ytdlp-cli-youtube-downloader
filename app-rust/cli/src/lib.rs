//! ytdlp cli - native implementation of the ytdlp-cli specification.
//!
//! Ported layer by layer from `app-python/src/ytdlp.py`, which stays the
//! behavioural oracle: every ported function is byte-compared through
//! `app-rust/tools/harness.py` before it counts (RUST-PORT-PLAN.md,
//! specsheet NFR-4). Layer names follow specsheet section 4: term,
//! config, resolve, args, exec, update, ui.

pub mod args;
pub mod config;
pub mod exec;
pub mod rng;
pub mod settings_json;
pub mod sha256;
pub mod term;
pub mod ui;
pub mod update;
pub mod width_table;

/// Product version, single-sourced from this package's Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
