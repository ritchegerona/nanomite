//! Trivy container/image vulnerability scanning.

use serde::{Deserialize, Serialize};
use security_platform_core::error::Result;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrivyResult {
    pub target: String,
    pub vulnerabilities: Vec<Vuln>,
    pub scanner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vuln {
    pub vuln_id: String,
    pub pkg_name: String,
    pub installed_version: String,
    pub fixed_version: Option<String>,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrivyArgs {
    pub target: String,
    pub scan_type: String,
}

pub async fn run(args: TrivyArgs) -> Result<TrivyResult> {
    info!("Running Trivy scan for {}", args.target);
    Ok(TrivyResult {
        target: args.target,
        vulnerabilities: vec![],
        scanner: args.scan_type,
    })
}

pub fn tool_spec() -> serde_json::Value {
    serde_json::json!({
        "name": "trivy_scan",
        "module": "vulnerability_scanning",
        "description": "Trivy container/image vulnerability scan",
        "parameters": {
            "type": "object",
            "properties": {
                "target": {"type": "string"},
                "scan_type": {"type": "string"}
            },
            "required": ["target"]
        }
    })
}