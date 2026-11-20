//! Scheduled tasks - cron jobs for scans, hunts, compliance checks.

use tracing::info;

pub async fn init() -> anyhow::Result<()> {
    info!("Scheduled tasks initialized");
    Ok(())
}
