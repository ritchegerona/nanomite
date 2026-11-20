//! Cloud & Container Security Module - Blue Team.

pub mod aws;
pub mod azure;
pub mod gcp;
pub mod kubernetes;
pub mod falco;

use tracing::info;

pub async fn init() -> anyhow::Result<()> {
    info!("Cloud & Container Security Module initialized (Blue Team)");
    Ok(())
}
