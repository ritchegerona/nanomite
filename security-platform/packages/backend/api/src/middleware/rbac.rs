//! RBAC middleware - enforces Blue/Red/Purple team access control.

use axum::{
    extract::State,
    middleware::Next,
    response::{Response, IntoResponse},
    http::{HeaderMap, StatusCode},
};
use std::str::FromStr;
use security_platform_core::config::AppConfig;
use security_platform_core::auth::rbac;
use security_platform_core::models::user::TeamRole;
use security_platform_core::auth::jwt;

pub async fn rbac_middleware(
    State(config): State<AppConfig>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let module = headers
        .get("x-security-module")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("shared");

    let claims = request.extensions().get::<jwt::Claims>().cloned();

    match claims {
        Some(claims) => {
            let role = match TeamRole::from_str(&claims.role) {
                Ok(r) => r,
                Err(_) => return Err(StatusCode::FORBIDDEN),
            };

            let access = rbac::can_access_module(&role, module);
            match access {
                rbac::TeamAccess::None => Err(StatusCode::FORBIDDEN),
                rbac::TeamAccess::Read | rbac::TeamAccess::Write | rbac::TeamAccess::Admin => {
                    Ok(next.run(request).await)
                }
            }
        }
        None => Err(StatusCode::UNAUTHORIZED),
    }
}