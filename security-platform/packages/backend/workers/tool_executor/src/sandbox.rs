//! Sandbox configuration for tool execution.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub runtime: String,
    pub network_enabled: bool,
    pub filesystem_read_only: bool,
    pub allowed_paths: Vec<String>,
    pub blocked_paths: Vec<String>,
    pub max_memory_mb: u64,
    pub max_cpu_percent: u32,
    pub max_pids: u32,
    pub timeout_secs: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            runtime: "podman".to_string(),
            network_enabled: false,
            filesystem_read_only: true,
            allowed_paths: vec!["/tmp".to_string(), "/workspace".to_string()],
            blocked_paths: vec!["/etc".to_string(), "/root".to_string(), "/home".to_string()],
            max_memory_mb: 1024,
            max_cpu_percent: 50,
            max_pids: 100,
            timeout_secs: 300,
        }
    }
}
