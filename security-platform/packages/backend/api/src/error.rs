//! API error handling.

use axum::{
    response::{Response, Json, IntoResponse},
    http::StatusCode,
};
use serde_json::json;

pub fn handle_error(error: anyhow::Error) -> Response {
    let status = StatusCode::INTERNAL_SERVER_ERROR;
    let body = Json(json!({
        "success": false,
        "error": error.to_string(),
        "meta": {
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }
    }));
    (status, body).into_response()
}