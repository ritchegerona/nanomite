//! Alerting - Slack, PagerDuty, email, webhook.

use serde::{Deserialize, Serialize};
use security_platform_core::error::Result;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub title: String,
    pub severity: String,
    pub status: String,
    pub mitre_technique: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAlertArgs {
    pub title: String,
    pub severity: String,
    pub mitre_technique: Option<String>,
}

pub async fn create_alert(args: CreateAlertArgs) -> Result<Alert> {
    info!("Creating alert: {}", args.title);
    Ok(Alert {
        id: Uuid::new_v4().to_string(),
        title: args.title,
        severity: args.severity,
        status: "new".to_string(),
        mitre_technique: args.mitre_technique,
    })
}

pub fn tool_spec() -> serde_json::Value {
    serde_json::json!({
        "name": "siem_alert",
        "module": "siem",
        "description": "Create SIEM alert",
        "parameters": {
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "severity": {"type": "string"},
                "mitre_technique": {"type": "string"}
            },
            "required": ["title", "severity"]
        }
    })
}