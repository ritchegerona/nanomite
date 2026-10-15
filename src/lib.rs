//! nanomite library crate.
//!
//! The binary (`main.rs`) owns the terminal; everything else lives here so the
//! agent runtime, the providers, the tools and the memory engine can be driven
//! directly by tests.

pub mod agent;
pub mod app;
pub mod commands;
pub mod config;
pub mod extract;
// The memory engine exposes a deliberate API superset of what the TUI consumes
// today (backup/restore, consolidation, listing helpers); unused members of a
// published library surface are not dead code.
#[allow(dead_code)]
pub mod memory;
pub mod session;
pub mod tools;
pub mod ui;
