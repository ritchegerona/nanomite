//! Azure cloud security assessment.

use serde::{Deserialize, Serialize};
use security_platform_core::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureResult {
    pub subscription_id: String,
    pub findings: Vec<serde_json::Value>,
}

pub async fn assess(_subscription: String) -> Result<AzureResult> {
    Ok(AzureResult {
        subscription_id: String::new(),
        findings: vec![],
    })
}
