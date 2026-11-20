//! OSINT Module - Open Source Intelligence gathering tools.

pub mod subdomain_enum;
pub mod port_scan;
pub mod tech_fingerprint;
pub mod ssl_tls;
pub mod dns_enum;
pub mod email_harvest;
pub mod whois;
pub mod cloud_enum;
pub mod git_recon;
pub mod metadata;

use tracing::info;

pub async fn init() -> anyhow::Result<()> {
    info!("OSINT Module initialized");
    Ok(())
}
