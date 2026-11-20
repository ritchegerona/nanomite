//! Evidence processor - chain of custody, hashing, signing.

use tracing::info;

pub async fn init() -> anyhow::Result<()> {
    info!("Evidence processor initialized");
    Ok(())
}
