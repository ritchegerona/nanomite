//! Tool run endpoints.

use axum::{routing::{get, post}, Router, Json};
use serde_json::json;

pub fn routes() -> Router {
    Router::new()
        .route("/tool-runs", post(create_tool_run).get(list_tool_runs))
}

async fn create_tool_run() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn list_tool_runs() -> Json<serde_json::Value> {
    Json(json!({"items": []}))
}