//! Vulnerability Scanning Module - Blue Team scanning tools.

pub mod nuclei;
pub mod openvas;
pub mod trivy;
pub mod checkov;
pub mod zap;
pub mod burp;
pub mod schemathesis;
pub mod prowler;

use tracing::info;

pub async fn init() -> anyhow::Result<()> {
    info!("Vulnerability Scanning Module initialized (Blue Team)");
    Ok(())
}