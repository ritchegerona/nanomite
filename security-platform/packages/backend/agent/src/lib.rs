//! Nanomite AI Agent integration service.
//! Embeds nanomite-core as a Rust library for intelligent orchestration.

pub mod config;
pub mod service;
pub mod stream;
pub mod tools;

use config::AgentConfig;
use service::NanomiteService;
use std::sync::Arc;

pub async fn init(config: AgentConfig) -> anyhow::Result<Arc<NanomiteService>> {
    let service = NanomiteService::new(config).await?;
    Ok(Arc::new(service))
}