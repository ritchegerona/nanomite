//! Asset model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Asset {
    pub id: Uuid,
    pub project_id: Uuid,
    pub asset_type: String, // domain, ip, cidr, cloud_resource, container, host
    pub identifier: String,
    pub metadata: serde_json::Value,
    pub discovered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAsset {
    pub project_id: Uuid,
    pub asset_type: String,
    pub identifier: String,
    pub metadata: Option<serde_json::Value>,
}