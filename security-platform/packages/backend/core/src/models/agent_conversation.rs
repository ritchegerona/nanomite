//! Agent conversation model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentConversation {
    pub id: Uuid,
    pub assessment_id: Option<Uuid>,
    pub user_id: Uuid,
    pub messages: serde_json::Value,
    pub context: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentConversation {
    pub assessment_id: Option<Uuid>,
    pub user_id: Uuid,
    pub messages: Option<serde_json::Value>,
    pub context: Option<serde_json::Value>,
}