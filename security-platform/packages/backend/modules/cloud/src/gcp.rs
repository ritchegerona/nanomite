//! GCP cloud security assessment.

use serde::{Deserialize, Serialize};
use security_platform_core::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCPResult {
    pub project_id: String,
    pub findings: Vec<serde_json::Value>,
}

pub async fn assess(_project: String) -> Result<GCPResult> {
    Ok(GCPResult {
        project_id: String::new(),
        findings: vec![],
    })
}
