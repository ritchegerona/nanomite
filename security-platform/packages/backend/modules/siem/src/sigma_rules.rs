//! Sigma rule management.

use serde::{Deserialize, Serialize};
use security_platform_core::error::Result;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigmaRule {
    pub id: String,
    pub title: String,
    pub rule_yaml: String,
    pub level: String,
    pub status: String,
    pub mitre_technique: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSigmaArgs {
    pub title: String,
    pub rule_yaml: String,
    pub level: Option<String>,
    pub mitre_technique: Option<String>,
}

pub async fn create_rule(args: CreateSigmaArgs) -> Result<SigmaRule> {
    info!("Creating Sigma rule: {}", args.title);
    Ok(SigmaRule {
        id: Uuid::new_v4().to_string(),
        title: args.title,
        rule_yaml: args.rule_yaml,
        level: args.level.unwrap_or("medium".to_string()),
        status: "draft".to_string(),
        mitre_technique: args.mitre_technique,
    })
}

pub fn tool_spec() -> serde_json::Value {
    serde_json::json!({
        "name": "sigma_create",
        "module": "siem",
        "description": "Create Sigma detection rule",
        "parameters": {
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "rule_yaml": {"type": "string"},
                "level": {"type": "string"},
                "mitre_technique": {"type": "string"}
            },
            "required": ["title", "rule_yaml"]
        }
    })
}