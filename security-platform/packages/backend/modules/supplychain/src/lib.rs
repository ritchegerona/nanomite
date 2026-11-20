//! Supply Chain & SBOM Module.

pub mod sbom;
pub mod dependency_confusion;
pub mod provenance;
pub mod license;

use tracing::info;

pub async fn init() -> anyhow::Result<()> {
    info!("Supply Chain & SBOM Module initialized");
    Ok(())
}
