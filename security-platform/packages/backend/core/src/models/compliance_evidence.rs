//! Compliance evidence model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ComplianceFramework {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub controls: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ComplianceEvidence {
    pub id: Uuid,
    pub framework_id: Uuid,
    pub control_id: String,
    pub finding_id: Option<Uuid>,
    pub status: String, // pending, satisfied, not_applicable
    pub collected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateComplianceEvidence {
    pub framework_id: Uuid,
    pub control_id: String,
    pub finding_id: Option<Uuid>,
    pub status: Option<String>,
}