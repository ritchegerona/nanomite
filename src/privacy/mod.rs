//! In-process control surface for the NanomiteVPN and NanomiteTOR modules.
//!
//! The modules themselves are self-contained libraries; this module is the
//! thin supervisor that lets the running application (and the agent) start,
//! stop and inspect them:
//!
//! - [`vpn`] owns one [`VpnEngine`] at a time, keeps its run loop alive and
//!   reports peer state.
//! - [`tor`] owns one managed `tor` daemon and reports bootstrap progress.
//!
//! Both are feature-gated (`vpn`, `tor`). When a feature is off the functions
//! stay callable and return a clear "not compiled in" error, so call sites
//! never need their own `cfg` branches.
//!
//! Every operation that touches the network runs off the UI thread: the
//! caller either awaits [`vpn::start`] / [`tor::start`] inside a task or uses
//! the blocking wrappers, which drive the same futures on a runtime handle.

#[cfg(feature = "tor")]
pub mod tor;
#[cfg(feature = "vpn")]
pub mod vpn;

/// Message shown when a module is not part of this build.
pub const NOT_COMPILED: &str =
    "this build has no privacy modules - rebuild with `cargo build --release --features \"vpn,tor\"`";
