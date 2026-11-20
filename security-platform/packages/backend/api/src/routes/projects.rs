//! Project endpoints.

use axum::{routing::{get, post}, Router, Json};
use serde_json::json;

pub fn routes() -> Router {
    Router::new()
        .route("/projects", post(create_project).get(list_projects))
}

async fn create_project() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn list_projects() -> Json<serde_json::Value> {
    Json(json!({"items": []}))
}