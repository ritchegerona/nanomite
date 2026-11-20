//! Finding model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Finding {
    pub id: Uuid,
    pub assessment_id: Uuid,
    pub asset_id: Option<Uuid>,
    pub title: String,
    pub severity: String, // critical, high, medium, low, info
    pub cvss_vector: Option<String>,
    pub cvss_score: Option<f64>,
    pub epss_score: Option<f64>,
    pub kev_listed: bool,
    pub description: String,
    pub evidence: serde_json::Value,
    pub status: String, // open, triaged, in_progress, fixed, false_positive, accepted_risk
    pub assignee_id: Option<Uuid>,
    pub team: String, // blue, red, purple
    pub mitre_technique: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFinding {
    pub assessment_id: Uuid,
    pub asset_id: Option<Uuid>,
    pub title: String,
    pub severity: String,
    pub cvss_vector: Option<String>,
    pub cvss_score: Option<f64>,
    pub epss_score: Option<f64>,
    pub kev_listed: Option<bool>,
    pub description: String,
    pub evidence: Option<serde_json::Value>,
    pub team: String,
    pub mitre_technique: Option<String>,
}