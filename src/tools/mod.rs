pub mod catalog;
pub mod document_tools;
pub mod exec_tools;
pub mod file_tools;
pub mod improve_tools;
pub mod memory_tools;

pub mod web_tools;

use crate::improve::ImproveEngine;
use crate::memory::MemoryEngine;
use std::sync::Arc;

/// The full capability set handed to the agent runtime.
pub struct ToolSet {
    pub files: crate::tools::file_tools::FileTools,
    pub exec: crate::tools::exec_tools::ExecTools,
    pub web: crate::tools::web_tools::WebTools,
    /// Document extraction (pdf, docx, xlsx, odt, rtf, csv, …). Always
    /// present: the registry is built in-process and needs no engine.
    pub documents: crate::tools::document_tools::DocumentTools,
    /// Persistent memory. `None` when the engine could not be opened (e.g. a
    /// read-only config directory): the agent keeps working, without memory.
    pub memory: Option<Arc<MemoryEngine>>,
    /// Self-improvement engine. Always attached in production (opening it only
    /// creates a data directory); the two self-edit tools are otherwise inert,
    /// because staging a proposal changes nothing on disk.
    pub improve: Option<Arc<ImproveEngine>>,
    /// Anchor paths the engine validates against. Travels with the engine so
    /// tool dispatch needs nothing from the runtime.
    pub improve_roots: Option<crate::improve::Roots>,
}

impl ToolSet {
    pub fn new(workspace: std::path::PathBuf, max_file_bytes: usize) -> Self {
        Self {
            files: crate::tools::file_tools::FileTools::new(workspace.clone(), max_file_bytes),
            exec: crate::tools::exec_tools::ExecTools::new(workspace.clone()),
            web: crate::tools::web_tools::WebTools,
            documents: crate::tools::document_tools::DocumentTools::new(
                workspace.clone(),
                max_file_bytes,
            ),
            memory: None,
            improve: None,
            improve_roots: None,
        }
    }
}
