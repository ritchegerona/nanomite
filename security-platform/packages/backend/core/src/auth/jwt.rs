//! JWT token creation and validation.

use chrono::{Utc, TimeDelta};
use jsonwebtoken::{encode, decode, Header, EncodingKey, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub org_id: Uuid,
    pub role: String,
    pub team: String,
    pub exp: i64,
    pub iat: i64,
    pub iss: String,
    pub aud: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

pub fn create_tokens(
    user_id: Uuid,
    org_id: Uuid,
    role: &str,
    team: &str,
    config: &crate::config::AuthConfig,
) -> anyhow::Result<TokenResponse> {
    let now = Utc::now();
    let access_exp = now + TimeDelta::try_seconds(config.access_token_ttl_secs as i64).unwrap();
    let refresh_exp = now + TimeDelta::try_seconds(config.refresh_token_ttl_secs as i64).unwrap();

    let access_claims = Claims {
        sub: user_id,
        org_id,
        role: role.to_string(),
        team: team.to_string(),
        exp: access_exp.timestamp(),
        iat: now.timestamp(),
        iss: config.jwt_issuer.clone(),
        aud: config.jwt_audience.clone(),
    };

    let refresh_claims = Claims {
        sub: user_id,
        org_id,
        role: role.to_string(),
        team: team.to_string(),
        exp: refresh_exp.timestamp(),
        iat: now.timestamp(),
        iss: config.jwt_issuer.clone(),
        aud: config.jwt_audience.clone(),
    };

    let access_token = encode(
        &Header::default(),
        &access_claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )?;

    let refresh_token = encode(
        &Header::default(),
        &refresh_claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )?;

    Ok(TokenResponse {
        access_token,
        refresh_token,
        expires_in: config.access_token_ttl_secs,
    })
}

pub fn validate_token(token: &str, config: &crate::config::AuthConfig) -> anyhow::Result<Claims> {
    let mut validation = Validation::default();
    validation.set_issuer(&[&config.jwt_issuer]);
    validation.set_audience(&[config.jwt_audience.clone()]);

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &validation,
    )?;

    Ok(token_data.claims)
}