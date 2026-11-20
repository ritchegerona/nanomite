//! API route definitions.

pub mod health;
pub mod auth;
pub mod organizations;
pub mod projects;
pub mod assessments;
pub mod findings;
pub mod tool_runs;
pub mod agent;

use axum::Router;

pub fn router() -> Router {
    Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(organizations::routes())
        .merge(projects::routes())
        .merge(assessments::routes())
        .merge(findings::routes())
        .merge(tool_runs::routes())
        .merge(agent::routes())
}