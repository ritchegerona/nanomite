//! Organization endpoints.

use axum::{routing::{get, post}, Router, Json};
use serde_json::json;

pub fn routes() -> Router {
    Router::new()
        .route("/organizations", post(create_organization).get(list_organizations))
}

async fn create_organization() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn list_organizations() -> Json<serde_json::Value> {
    Json(json!({"items": []}))
}