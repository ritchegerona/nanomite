//! Tool registry for Nanomite agent skills.

use super::ToolSpec;

pub struct ToolRegistry {
    tools: Vec<ToolSpec>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: ToolSpec) {
        self.tools.push(tool);
    }

    pub fn list(&self) -> &[ToolSpec] {
        &self.tools
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}