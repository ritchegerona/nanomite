//! Tool run model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ToolRun {
    pub id: Uuid,
    pub assessment_id: Uuid,
    pub tool_name: String,
    pub module: String, // osint, pentest, vulnscan, siem, socmed, etc.
    pub team: String, // blue, red, purple
    pub command: String,
    pub args: serde_json::Value,
    pub status: String, // pending, running, completed, failed, timeout, cancelled
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub exit_code: Option<i32>,
    pub artifacts: serde_json::Value, // [{path, type, hash, size}]
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateToolRun {
    pub assessment_id: Uuid,
    pub tool_name: String,
    pub module: String,
    pub team: String,
    pub command: String,
    pub args: Option<serde_json::Value>,
}