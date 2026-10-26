//! Test support utilities - only compiled when running tests.

#[cfg(test)]
pub mod agent {
    pub use crate::agent::testdouble::ScriptedProvider;
}

#[cfg(test)]
pub mod app {
    pub use crate::app::App;
    pub use crate::app::Entry;
    pub use crate::app::EntryRole;
    pub use crate::app::Phase;
}

#[cfg(test)]
pub mod memory {
    pub use crate::memory::policy::MemoryPolicy;
    pub use crate::memory::MemoryEngine;
}

#[cfg(test)]
pub mod tools {
    pub use crate::tools::ToolSet;
}

#[cfg(test)]
pub mod config {
    pub use crate::config::Config;
}

#[cfg(test)]
pub mod session {
    pub use crate::session::list;
    pub use crate::session::load;
    pub use crate::session::SessionMeta;
    pub use crate::session::StoredSession;
}
