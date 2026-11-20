//! Agent plan model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentPlan {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub goal: String,
    pub steps: serde_json::Value,
    pub status: String, // draft, pending_approval, approved, executing, completed, failed
    pub approved_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentPlan {
    pub conversation_id: Uuid,
    pub goal: String,
    pub steps: Option<serde_json::Value>,
}