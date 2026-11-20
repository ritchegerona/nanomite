//! Health check endpoints.

use axum::{
    routing::get,
    Router,
    Json,
};
use serde_json::json;

pub fn routes() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/live", get(live))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "healthy",
        "service": "security-platform-api",
        "version": "0.1.0",
    }))
}

async fn ready() -> Json<serde_json::Value> {
    Json(json!({"status": "ready"}))
}

async fn live() -> Json<serde_json::Value> {
    Json(json!({"status": "alive"}))
}