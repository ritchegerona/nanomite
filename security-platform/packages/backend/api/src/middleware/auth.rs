//! Authentication middleware - extracts JWT from headers.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
    http::HeaderMap,
};
use security_platform_core::config::AppConfig;
use security_platform_core::auth::jwt;

pub async fn auth_middleware(
    State(config): State<AppConfig>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, axum::response::Response> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match auth_header {
        Some(token) => {
            match jwt::validate_token(token, &config.auth) {
                Ok(claims) => {
                    request.extensions_mut().insert(claims);
                    Ok(next.run(request).await)
                }
                Err(_) => Err(axum::response::Response::builder()
                    .status(401)
                    .body("Invalid token".into())
                    .unwrap()),
            }
        }
        None => Err(axum::response::Response::builder()
            .status(401)
            .body("Missing authorization header".into())
            .unwrap()),
    }
}