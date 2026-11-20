//! Agent service configuration.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub enabled: bool,
    pub binary_path: String,
    pub library_path: String,
    pub models_dir: String,
    pub skills_dir: String,
    pub memory_dir: String,
    pub max_concurrent_sessions: u32,
    pub default_max_steps: u32,
    pub reasoning_enabled: bool,
    pub planning_enabled: bool,
    pub memory_enabled: bool,
    pub skills_auto_install: bool,
    pub tool_timeout_secs: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            binary_path: String::new(),
            library_path: String::new(),
            models_dir: "./data/nanomite/models".to_string(),
            skills_dir: "./data/nanomite/skills".to_string(),
            memory_dir: "./data/nanomite/memory".to_string(),
            max_concurrent_sessions: 100,
            default_max_steps: 20,
            reasoning_enabled: true,
            planning_enabled: true,
            memory_enabled: true,
            skills_auto_install: true,
            tool_timeout_secs: 300,
        }
    }
}