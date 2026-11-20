//! Tool executor - runs tools directly on the host.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionRequest {
    pub run_id: uuid::Uuid,
    pub tool_name: String,
    pub module: String,
    pub team: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: Option<String>,
    pub timeout_secs: u64,
    pub max_memory_mb: u64,
    pub max_cpu_percent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    pub run_id: uuid::Uuid,
    pub status: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_secs: u64,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub name: String,
    pub path: String,
    pub hash: String,
    pub size: u64,
}

pub struct ToolExecutor;

impl ToolExecutor {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, request: ToolExecutionRequest) -> anyhow::Result<ToolExecutionResult> {
        info!("Executing tool {} (run {})", request.tool_name, request.run_id);

        let start = std::time::Instant::now();

        let mut cmd = self.build_direct_command(&request)?;

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;

        let output = match tokio::time::timeout(
            std::time::Duration::from_secs(request.timeout_secs),
            child.wait_with_output()
        ).await {
            Ok(result) => result?,
            Err(_) => {
                warn!("Tool {} timed out after {}s", request.tool_name, request.timeout_secs);
                return Ok(ToolExecutionResult {
                    run_id: request.run_id,
                    status: "timeout".to_string(),
                    stdout: String::new(),
                    stderr: format!("Command timed out after {} seconds", request.timeout_secs),
                    exit_code: Some(-1),
                    duration_secs: request.timeout_secs,
                    artifacts: vec![],
                });
            }
        };

        let duration = start.elapsed();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(ToolExecutionResult {
            run_id: request.run_id,
            status: if output.status.success() { "completed".to_string() } else { "failed".to_string() },
            stdout,
            stderr,
            exit_code: output.status.code(),
            duration_secs: duration.as_secs(),
            artifacts: vec![],
        })
    }

    fn build_direct_command(&self, request: &ToolExecutionRequest) -> anyhow::Result<Command> {
        let mut cmd = Command::new(&request.command);
        cmd.args(&request.args);
        if let Some(dir) = &request.working_dir {
            cmd.current_dir(dir);
        }
        for (k, v) in &request.env {
            cmd.env(k, v);
        }
        Ok(cmd)
    }
}