//! Falco runtime security integration.

use serde::{Deserialize, Serialize};
use security_platform_core::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FalcoEvent {
    pub rule: String,
    pub priority: String,
    pub output: String,
}

pub async fn ingest(_events: Vec<FalcoEvent>) -> Result<serde_json::Value> {
    Ok(serde_json::json!({"status": "ingested"}))
}
