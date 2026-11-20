//! Incident Response & Forensics Module - Blue Team.

pub mod disk_forensics;
pub mod memory_forensics;
pub mod network_forensics;
pub mod malware_analysis;
pub mod timeline;

use tracing::info;

pub async fn init() -> anyhow::Result<()> {
    info!("Incident Response Module initialized (Blue Team)");
    Ok(())
}
