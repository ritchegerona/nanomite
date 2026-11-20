//! Tool registry and integration for Nanomite agent.

pub mod registry;

// Tool specification for nanomite integration
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub module: String,
    pub description: String,
    pub parameters: serde_json::Value,
}