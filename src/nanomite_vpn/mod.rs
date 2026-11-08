//! NanomiteVPN — WireGuard-inspired personal VPN for the nanomite project.
//!
//! A lightweight, modern VPN tunnel built on old-laptop-friendly
//! primitives: Ed25519 identity signatures, one-shot X25519 ephemeral
//! key agreement (forward-secret, burned after use), ChaCha20-Poly1305
//! encryption with WireGuard-style wire nonces, BLAKE3 transcript/fingerprint
//! hashing, TUN/TAP virtual interfaces, NAT traversal via STUN hole
//! punching, DNS-over-HTTPS with leak protection, and a system kill
//! switch. See `crypto.rs` for why identity is signing-based rather than
//! static X25519.
//!
//! # Architecture
//!
//! ```text
//! Peer A                          Peer B
//!   |                               |
//!   |--- HandshakeInit  ----------> |
//!   |<--- HandshakeResponse --------|
//!   |--- SessionEstablished ------> |
//!   |                               |
//!   |--- Encrypted IP packets <---->|
//!   |    (TUN <-> UDP socket)       |
//! ```
//!
//! # Feature gate
//!
//! This module is compiled only when the `vpn` feature is enabled:
//!
//! ```toml
//! [features]
//! vpn = ["dep:ring", "dep:blake3", "dep:getrandom", "dep:tokio-tun", ...]
//! ```
//!
//! # Quick start
//!
//! ```no_run
//! use nanomite::nanomite_vpn::{VpnEngine, VpnConfig, KeyPair};
//! use nanomite::nanomite_vpn::engine::PeerConfig;
//! use std::net::SocketAddr;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut engine = VpnEngine::new(VpnConfig::default()).await?;
//! engine.bind("0.0.0.0:51820".parse()?).await?;
//! engine.add_peer(PeerConfig {
//!     public_key: [1u8; 32],
//!     endpoint: "203.0.113.7:51820".parse()?,
//!     allowed_ips: vec!["10.8.0.0/24".to_string()],
//!     persistent_keepalive: Some(15),
//!     preshared_key: None,
//! }).await?;
//! engine.connect(&[1u8; 32]).await?;
//! # Ok(())
//! # }
//! ```

pub mod cli;
pub mod config;
pub mod crypto;
pub mod dns;
pub mod engine;
pub mod events;
pub mod killswitch;
pub mod nat;
pub mod protocol;
pub mod tun;

// Public re-exports for convenience
pub use config::Config;
pub use crypto::KeyPair;
pub use engine::{TunnelEvent, TunnelState, VpnConfig, VpnEngine};
pub use events::VpnEvent;
pub use tun::TunDevice;

/// Top-level result type for all NanomiteVPN operations.
pub type VpnResult<T> = Result<T, VpnError>;

/// Errors that can occur during VPN operation.
#[derive(Debug)]
pub enum VpnError {
    Crypto(String),
    Protocol(String),
    Tun(String),
    Network(String),
    Nat(String),
    Dns(String),
    KillSwitch(String),
    Config(String),
    Io(std::io::Error),
    Serde(serde_json::Error),
    NotEstablished,
    PeerNotFound(String),
    SessionExpired,
}

impl std::fmt::Display for VpnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VpnError::Crypto(s) => write!(f, "crypto error: {s}"),
            VpnError::Protocol(s) => write!(f, "protocol error: {s}"),
            VpnError::Tun(s) => write!(f, "tun device error: {s}"),
            VpnError::Network(s) => write!(f, "network error: {s}"),
            VpnError::Nat(s) => write!(f, "nat traversal error: {s}"),
            VpnError::Dns(s) => write!(f, "dns error: {s}"),
            VpnError::KillSwitch(s) => write!(f, "kill switch error: {s}"),
            VpnError::Config(s) => write!(f, "configuration error: {s}"),
            VpnError::Io(e) => write!(f, "io error: {e}"),
            VpnError::Serde(e) => write!(f, "serde error: {e}"),
            VpnError::NotEstablished => write!(f, "tunnel not established"),
            VpnError::PeerNotFound(p) => write!(f, "peer not found: {p}"),
            VpnError::SessionExpired => write!(f, "session expired"),
        }
    }
}

impl std::error::Error for VpnError {}

impl From<std::io::Error> for VpnError {
    fn from(e: std::io::Error) -> Self {
        VpnError::Io(e)
    }
}

impl From<serde_json::Error> for VpnError {
    fn from(e: serde_json::Error) -> Self {
        VpnError::Serde(e)
    }
}

impl From<ring::error::Unspecified> for VpnError {
    fn from(e: ring::error::Unspecified) -> Self {
        VpnError::Crypto(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vpn_result_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VpnResult<()>>();
    }
}
