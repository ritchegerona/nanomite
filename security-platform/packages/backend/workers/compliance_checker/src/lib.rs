//! Compliance checker - OSCAL evidence, NIST/ISO/PCI mapping.

use tracing::info;

pub async fn init() -> anyhow::Result<()> {
    info!("Compliance checker initialized");
    Ok(())
}
