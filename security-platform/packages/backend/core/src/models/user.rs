//! User model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub role: String, // admin, analyst, viewer, operator, lead
    pub mfa_secret: Option<String>,
    pub last_login: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUser {
    pub org_id: Uuid,
    pub email: String,
    pub password: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUser {
    pub email: Option<String>,
    pub role: Option<String>,
    pub mfa_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    pub role: String,
    pub last_login: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserResponse {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            org_id: u.org_id,
            email: u.email,
            role: u.role,
            last_login: u.last_login,
            created_at: u.created_at,
        }
    }
}

// Team roles for Blue/Red/Purple access control
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TeamRole {
    BlueAnalyst,
    BlueLead,
    RedOperator,
    RedLead,
    PurpleEngineer,
    Ciso,
    Auditor,
}

impl std::str::FromStr for TeamRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "blue_analyst" => Ok(TeamRole::BlueAnalyst),
            "blue_lead" => Ok(TeamRole::BlueLead),
            "red_operator" => Ok(TeamRole::RedOperator),
            "red_lead" => Ok(TeamRole::RedLead),
            "purple_engineer" => Ok(TeamRole::PurpleEngineer),
            "ciso" => Ok(TeamRole::Ciso),
            "auditor" => Ok(TeamRole::Auditor),
            _ => Err(format!("Unknown team role: {}", s)),
        }
    }
}

impl std::fmt::Display for TeamRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TeamRole::BlueAnalyst => write!(f, "blue_analyst"),
            TeamRole::BlueLead => write!(f, "blue_lead"),
            TeamRole::RedOperator => write!(f, "red_operator"),
            TeamRole::RedLead => write!(f, "red_lead"),
            TeamRole::PurpleEngineer => write!(f, "purple_engineer"),
            TeamRole::Ciso => write!(f, "ciso"),
            TeamRole::Auditor => write!(f, "auditor"),
        }
    }
}