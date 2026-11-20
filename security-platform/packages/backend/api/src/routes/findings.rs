//! Finding endpoints.

use axum::{routing::{get, post}, Router, Json};
use serde_json::json;

pub fn routes() -> Router {
    Router::new()
        .route("/findings", post(create_finding).get(list_findings))
}

async fn create_finding() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn list_findings() -> Json<serde_json::Value> {
    Json(json!({"items": []}))
}