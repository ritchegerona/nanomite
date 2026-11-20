//! Assessment model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Assessment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub assessment_type: String, // osint, pentest, vulnscan, siem, red_team, comprehensive
    pub team: String, // blue, red, purple
    pub status: String, // draft, running, paused, completed, failed, cancelled
    pub config: serde_json::Value,
    pub scope_validation: serde_json::Value, // authorization, ROE, constraints
    pub created_by: Uuid,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAssessment {
    pub project_id: Uuid,
    pub name: String,
    pub assessment_type: String,
    pub team: String,
    pub config: Option<serde_json::Value>,
    pub scope_validation: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAssessment {
    pub name: Option<String>,
    pub status: Option<String>,
    pub config: Option<serde_json::Value>,
}

// Authorization document for Red Team engagements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationDocument {
    pub document_id: String,
    pub signed_by: String,
    pub signed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub scope: Vec<String>,
    pub rules_of_engagement: Vec<String>,
}