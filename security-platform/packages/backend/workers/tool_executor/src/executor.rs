//! Tool executor - runs security tools in sandboxed containers.

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

pub struct ToolExecutor {
    sandbox_enabled: bool,
}

impl ToolExecutor {
    pub fn new(sandbox_enabled: bool) -> Self {
        Self { sandbox_enabled }
    }

    pub async fn execute(&self, request: ToolExecutionRequest) -> anyhow::Result<ToolExecutionResult> {
        info!("Executing tool {} (run {})", request.tool_name, request.run_id);

        let start = std::time::Instant::now();

        let mut cmd: Command = if self.sandbox_enabled {
            self.build_sandbox_command(&request)?
        } else {
            self.build_direct_command(&request)?
        };

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

    fn build_sandbox_command(&self, request: &ToolExecutionRequest) -> anyhow::Result<Command> {
        let mut cmd = Command::new("podman");
        cmd.arg("run")
            .arg("--rm")
            .arg("--memory").arg(format!("{}m", request.max_memory_mb))
            .arg("--cpus").arg(format!("{}", request.max_cpu_percent as f64 / 100.0))
            .arg("--pids-limit").arg("100")
            .arg("--read-only")
            .arg("--tmpfs").arg("/tmp:noexec,nosuid,size=100m")
            .arg("--network").arg("none")
            .arg("--user").arg("1000:1000")
            .arg("--cap-drop").arg("ALL")
            .arg("--security-opt").arg("no-new-privileges:true");

        let image = self.tool_image(&request.tool_name);
        cmd.arg(&image);
        cmd.arg(&request.command);
        cmd.args(&request.args);

        Ok(cmd)
    }

    fn tool_image(&self, tool_name: &str) -> String {
        match tool_name {
            "nmap" | "masscan" | "naabu" => "instrumentisto/nmap:latest",
            "nikto" => "sullo/nikto:latest",
            "nuclei" => "projectdiscovery/nuclei:latest",
            "sqlmap" => "kalilinux/kali-linux-docker:latest",
            "amass" => "caffix/amass:latest",
            "subfinder" => "projectdiscovery/subfinder:latest",
            "hashcat" => "hashcat/hashcat:latest",
            "john" => "kalilinux/kali-linux-docker:latest",
            "aircrack-ng" => "kalilinux/kali-linux-docker:latest",
            "metasploit" => "metasploitframework/metasploit-framework:latest",
            "trivy" => "aquasec/trivy:latest",
            "checkov" => "bridgecrew/checkov:latest",
            "prowler" => "toniblyx/prowler:latest",
            _ => "alpine:latest",
        }.to_string()
    }
}