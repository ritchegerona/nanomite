pub mod catalog;
pub mod document_tools;
pub mod exec_tools;
pub mod external_tools;
pub mod file_tools;
pub mod git_tools;
pub mod improve_tools;
pub mod index_tools;
pub mod memory_tools;
pub mod plan_tools;
pub mod test_tools;

pub mod web_tools;

use crate::improve::ImproveEngine;
use crate::memory::MemoryEngine;
use std::sync::Arc;

/// The full capability set handed to the agent runtime.
#[derive(Clone)]
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
    /// Git tools. Always present: the registry is built in-process and needs no engine.
    pub git: crate::tools::git_tools::GitTools,
    /// Test runner tools. Always present.
    pub test: crate::tools::test_tools::TestTools,
    /// User-defined external tools (empty unless manifests are configured).
    pub external: std::sync::Arc<crate::tools::external_tools::ExternalTools>,
    /// Symbol index tools. Always present.
    pub index: crate::tools::index_tools::IndexTools,
    /// Plan/task list tools. Always present.
    pub plan: crate::tools::plan_tools::PlanTools,
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
            git: crate::tools::git_tools::GitTools::new(workspace.clone()),
            test: crate::tools::test_tools::TestTools::new(workspace.clone()),
            external: std::sync::Arc::new(crate::tools::external_tools::ExternalTools::default()),
            index: crate::tools::index_tools::IndexTools::new(workspace.clone()),
            plan: crate::tools::plan_tools::PlanTools::new(workspace.clone()),
            improve_roots: None,
        }
    }
}
