//! SIEM Module - Security Information and Event Management (Blue Team).

pub mod ingestion;
pub mod sigma_rules;
pub mod correlation;
pub mod alerting;
pub mod threat_hunting;
pub mod mitre;

use tracing::info;

pub async fn init() -> anyhow::Result<()> {
    info!("SIEM Module initialized");
    Ok(())
}