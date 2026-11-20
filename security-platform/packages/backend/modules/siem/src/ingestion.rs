//! Log ingestion - syslog, winlogbeat, fluentd, vector, opentelemetry.

use serde::{Deserialize, Serialize};
use security_platform_core::error::Result;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSource {
    pub id: String,
    pub name: String,
    pub source_type: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub eps_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestArgs {
    pub source_id: String,
    pub log_data: serde_json::Value,
}

pub async fn ingest(args: IngestArgs) -> Result<serde_json::Value> {
    info!("Ingesting log from source {}", args.source_id);
    Ok(serde_json::json!({"status": "ingested", "events": 1}))
}

pub fn tool_spec() -> serde_json::Value {
    serde_json::json!({
        "name": "siem_ingest",
        "module": "siem",
        "description": "Ingest log events into SIEM",
        "parameters": {
            "type": "object",
            "properties": {
                "source_id": {"type": "string"},
                "log_data": {"type": "object"}
            },
            "required": ["source_id", "log_data"]
        }
    })
}