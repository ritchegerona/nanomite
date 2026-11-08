//! NanomiteVPN binary entry point.
//!
//! Build with: cargo build --features vpn --bin nanomite-vpn
//! Run with:   cargo run --features vpn --bin nanomite-vpn -- <subcommand>

use nanomite::nanomite_vpn::cli::run_cli;
use std::process;

#[tokio::main]
async fn main() {
    if let Err(e) = run_cli().await {
        eprintln!("nanomite-vpn: {e}");
        process::exit(1);
    }
}
