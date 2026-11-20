//! Main entry point for Security Platform API server.

use security_platform_core::config::AppConfig;
use security_platform_api::run;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = AppConfig::from_env()?;

    tracing::info!("Starting Security Platform API v0.1.0");

    run(config).await?;

    Ok(())
}