//! Authentication endpoints.

use axum::{routing::post, Router, Json};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub expires_in: u64,
}

pub fn routes() -> Router {
    Router::new().route("/auth/login", post(login))
}

async fn login(_payload: Json<LoginRequest>) -> Json<LoginResponse> {
    Json(LoginResponse {
        access_token: "placeholder".to_string(),
        expires_in: 900,
    })
}