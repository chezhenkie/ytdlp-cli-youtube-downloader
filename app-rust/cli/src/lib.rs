//! ytdlp cli - native implementation of the ytdlp-cli specification.
//!
//! Phase 1 skeleton: the workspace, the differential harness and CI are
//! proven before behaviour is ported. Layers and functions land per
//! RUST-PORT-PLAN.md phases 2 onwards; the layer boundaries and the
//! behaviour they must reproduce are pinned in ytdlp-cli-specsheet.md
//! sections 4 and 6.

/// Product version, single-sourced from this package's Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
