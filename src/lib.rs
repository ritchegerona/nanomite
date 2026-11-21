//! nanomite library crate.
//!
//! The binary (`main.rs`) owns the terminal; everything else lives here so the
//! agent runtime, the providers, the tools and the memory engine can be driven
//! directly by tests.

/// Single source of truth for the running version. Displayed in the TUI and
/// embedded in generated reports; `Cargo.toml` remains the only place to bump.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod agent;
pub mod app;
pub mod board;
pub mod commands;
pub mod config;
pub mod debug;
pub mod evidence;
pub mod extract;
pub mod hacker;
pub mod herdr;
pub mod hooks;
pub mod improve;
pub mod learn;
#[cfg(feature = "tor")]
pub mod nanomite_tor;
#[cfg(feature = "vpn")]
pub mod nanomite_vpn;
pub mod reporting;
// The memory engine exposes a deliberate API superset of what the TUI consumes
// today (backup/restore, consolidation, listing helpers); unused members of a
// published library surface are not dead code.
#[allow(dead_code)]
pub mod memory;
pub mod neural;
pub mod privacy;
pub mod session;
pub mod telemetry;
pub mod tools;
pub mod ui;
pub mod update;
pub mod update_signing;
pub mod worktree;

/// Test support utilities - re-exports internal test types for integration tests.
#[cfg(test)]
pub mod test_support;
