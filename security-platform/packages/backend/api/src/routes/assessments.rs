//! Assessment endpoints.

use axum::{routing::{get, post}, Router, Json};
use serde_json::json;

pub fn routes() -> Router {
    Router::new()
        .route("/assessments", post(create_assessment).get(list_assessments))
}

async fn create_assessment() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn list_assessments() -> Json<serde_json::Value> {
    Json(json!({"items": []}))
}