//! Nanomite AI Agent endpoints.

use axum::{routing::{get, post}, Router, Json};
use serde_json::json;

pub fn routes() -> Router {
    Router::new()
        .route("/agent/chat", post(chat))
        .route("/agent/plan", post(plan))
        .route("/agent/execute", post(execute))
        .route("/agent/skills", get(list_skills))
}

async fn chat() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn plan() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn execute() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

async fn list_skills() -> Json<serde_json::Value> {
    Json(json!({"items": []}))
}