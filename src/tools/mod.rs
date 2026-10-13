pub mod exec_tools;
pub mod file_tools;

pub mod web_tools;

/// The full capability set handed to the agent runtime.
pub struct ToolSet {
    pub files: crate::tools::file_tools::FileTools,
    pub exec: crate::tools::exec_tools::ExecTools,
    pub web: crate::tools::web_tools::WebTools,
}

impl ToolSet {
    pub fn new(workspace: std::path::PathBuf, max_file_bytes: usize) -> Self {
        Self {
            files: crate::tools::file_tools::FileTools::new(workspace.clone(), max_file_bytes),
            exec: crate::tools::exec_tools::ExecTools::new(workspace),
            web: crate::tools::web_tools::WebTools,
        }
    }
}
