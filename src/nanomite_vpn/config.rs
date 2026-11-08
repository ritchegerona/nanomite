//! Configuration management for NanomiteVPN.
//!
//! Handles peer configurations, endpoint parsing, key serialization,
//! and TOML/YAML config file loading/saving.

use crate::nanomite_vpn::{VpnError, VpnResult};
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

/// Top-level VPN configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Local key pair (generated if not provided).
    #[serde(skip)]
    pub keypair: Option<crate::nanomite_vpn::crypto::KeyPair>,
    /// Local listen port.
    pub listen_port: u16,
    /// TUN interface configuration.
    pub tun: TunConfig,
    /// Peer configurations.
    pub peers: Vec<PeerConfig>,
    /// Kill switch configuration.
    pub kill_switch: KillSwitchConfig,
    /// DNS-over-HTTPS configuration.
    pub doh: DnsConfig,
    /// NAT traversal configuration.
    pub nat: NatConfig,
}

/// TUN interface configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunConfig {
    /// Interface name.
    pub name: String,
    /// IPv4 address.
    pub address: Ipv4Addr,
    /// CIDR prefix length.
    pub prefix_len: u8,
    /// MTU.
    pub mtu: u16,
    /// DNS servers pushed to peers.
    pub dns_servers: Vec<Ipv4Addr>,
}

impl Default for TunConfig {
    fn default() -> Self {
        TunConfig {
            name: "nanomite0".to_string(),
            address: Ipv4Addr::new(10, 8, 0, 1),
            prefix_len: 24,
            mtu: 1400,
            dns_servers: vec![Ipv4Addr::new(10, 8, 0, 1)],
        }
    }
}

/// Peer configuration for connecting to another VPN node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    /// Peer's public key (base64 or hex encoded).
    pub public_key: String,
    /// Peer's endpoint (host:port).
    pub endpoint: String,
    /// Allowed IPs for this peer (CIDR).
    pub allowed_ips: Vec<String>,
    /// Persistent keepalive interval (seconds).
    pub persistent_keepalive: Option<u32>,
    /// Optional pre-shared key (base64 encoded).
    pub preshared_key: Option<String>,
}

/// Kill switch configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillSwitchConfig {
    /// Whether kill switch is enabled.
    pub enabled: bool,
    /// VPN server endpoint.
    pub vpn_server: String,
    /// TUN subnet.
    pub tun_subnet: String,
    /// Local subnet allowed through kill switch.
    pub local_subnet: Option<String>,
    /// Allow DNS to VPN server.
    pub allow_dns: bool,
}

impl Default for KillSwitchConfig {
    fn default() -> Self {
        KillSwitchConfig {
            enabled: true,
            vpn_server: "127.0.0.1:51820".to_string(),
            tun_subnet: "10.8.0.0/24".to_string(),
            local_subnet: Some("192.168.0.0/16".to_string()),
            allow_dns: true,
        }
    }
}

/// DNS-over-HTTPS configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsConfig {
    /// DoH endpoint URL.
    pub endpoint: String,
    /// Timeout in seconds.
    pub timeout_secs: u64,
    /// Whether leak protection is enabled.
    pub leak_protection: bool,
}

impl Default for DnsConfig {
    fn default() -> Self {
        DnsConfig {
            endpoint: "https://dns.google/resolve".to_string(),
            timeout_secs: 5,
            leak_protection: true,
        }
    }
}

/// NAT traversal configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatConfig {
    /// STUN server address.
    pub stun_server: String,
    /// Whether hole punching is enabled.
    pub hole_punching: bool,
}

impl Default for NatConfig {
    fn default() -> Self {
        NatConfig {
            stun_server: "stun.l.google.com:19302".to_string(),
            hole_punching: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            keypair: None,
            listen_port: 51820,
            tun: TunConfig::default(),
            peers: Vec::new(),
            kill_switch: KillSwitchConfig::default(),
            doh: DnsConfig::default(),
            nat: NatConfig::default(),
        }
    }
}

/// Parsed peer configuration (with resolved keys and addresses).
#[derive(Debug, Clone)]
pub struct ResolvedPeer {
    /// Peer's public key bytes.
    pub public_key: [u8; 32],
    /// Peer's endpoint address.
    pub endpoint: SocketAddr,
    /// Allowed IPs (parsed CIDR).
    pub allowed_ips: Vec<String>,
    /// Persistent keepalive interval.
    pub persistent_keepalive: Option<u32>,
    /// Optional pre-shared key bytes.
    pub preshared_key: Option<[u8; 32]>,
}

impl Config {
    /// Create a new default configuration.
    pub fn new() -> Self {
        Config::default()
    }

    /// Load configuration from a TOML file.
    pub fn from_file(path: &std::path::Path) -> VpnResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| VpnError::Config(format!("failed to read config: {e}")))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| VpnError::Config(format!("failed to parse config: {e}")))?;
        Ok(config)
    }

    /// Save configuration to a TOML file.
    pub fn to_file(&self, path: &std::path::Path) -> VpnResult<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| VpnError::Config(format!("failed to serialize config: {e}")))?;
        std::fs::write(path, content)
            .map_err(|e| VpnError::Config(format!("failed to write config: {e}")))?;
        Ok(())
    }

    /// Load configuration from a TOML string. (Named `from_toml_str`
    /// to avoid shadowing the `FromStr` trait method.)
    pub fn from_toml_str(content: &str) -> VpnResult<Self> {
        toml::from_str(content)
            .map_err(|e| VpnError::Config(format!("failed to parse config: {e}")))
    }

    /// Serialize configuration to a TOML string.
    pub fn to_string(&self) -> VpnResult<String> {
        toml::to_string_pretty(self)
            .map_err(|e| VpnError::Config(format!("failed to serialize config: {e}")))
    }

    /// Generate a new key pair and set it in the config.
    pub fn generate_keypair(&mut self) -> VpnResult<[u8; 32]> {
        let keypair = crate::nanomite_vpn::crypto::KeyPair::generate()?;
        let public = keypair.public;
        self.keypair = Some(keypair);
        Ok(public)
    }

    /// Get the local public key from config.
    pub fn public_key(&self) -> VpnResult<[u8; 32]> {
        self.keypair
            .as_ref()
            .map(|kp| kp.public)
            .ok_or_else(|| VpnError::Config("no keypair in config".to_string()))
    }

    /// Get the local fingerprint.
    pub fn fingerprint(&self) -> VpnResult<String> {
        let public = self.public_key()?;
        Ok(crate::nanomite_vpn::crypto::peer_fingerprint(&public))
    }

    /// Add a peer to the configuration.
    pub fn add_peer(&mut self, peer: PeerConfig) {
        self.peers.push(peer);
    }

    /// Remove a peer by public key.
    pub fn remove_peer(&mut self, public_key: &str) {
        self.peers.retain(|p| p.public_key != public_key);
    }

    /// Get the default config path for the platform.
    pub fn default_config_path() -> VpnResult<PathBuf> {
        let dir = dirs::config_dir()
            .ok_or_else(|| VpnError::Config("cannot determine config dir".to_string()))?;
        let vpn_dir = dir.join("nanomite");
        std::fs::create_dir_all(&vpn_dir)
            .map_err(|e| VpnError::Config(format!("cannot create config dir: {e}")))?;
        Ok(vpn_dir.join("vpn.toml"))
    }

    /// Parse a peer's public key from hex or base64.
    pub fn parse_public_key(key_str: &str) -> VpnResult<[u8; 32]> {
        // Try hex first
        if let Ok(bytes) = hex::decode(key_str) {
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return Ok(key);
            }
        }

        // Try base64 (unpadded first, then padded), base64 0.22 Engine API.
        for engine in [
            base64::engine::general_purpose::STANDARD_NO_PAD,
            base64::engine::general_purpose::STANDARD,
        ] {
            use base64::Engine as _;
            if let Ok(bytes) = engine.decode(key_str) {
                if bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    return Ok(key);
                }
            }
        }

        Err(VpnError::Config(format!(
            "invalid public key: {key_str} (expected 32-byte hex or base64)"
        )))
    }

    /// Parse a peer endpoint string into a SocketAddr.
    pub fn parse_endpoint(endpoint_str: &str) -> VpnResult<SocketAddr> {
        endpoint_str
            .parse::<SocketAddr>()
            .map_err(|e| VpnError::Config(format!("invalid endpoint '{endpoint_str}': {e}")))
    }

    /// Resolve all peers in the config into ResolvedPeer structs.
    pub fn resolve_peers(&self) -> VpnResult<Vec<ResolvedPeer>> {
        self.peers
            .iter()
            .map(|p| {
                let public_key = Self::parse_public_key(&p.public_key)?;
                let endpoint = Self::parse_endpoint(&p.endpoint)?;
                let preshared_key = p
                    .preshared_key
                    .as_ref()
                    .map(|k| Self::parse_public_key(k))
                    .transpose()?;
                Ok(ResolvedPeer {
                    public_key,
                    endpoint,
                    allowed_ips: p.allowed_ips.clone(),
                    persistent_keepalive: p.persistent_keepalive,
                    preshared_key,
                })
            })
            .collect()
    }
}

impl Default for ResolvedPeer {
    fn default() -> Self {
        ResolvedPeer {
            public_key: [0u8; 32],
            endpoint: "0.0.0.0:0".parse().unwrap(),
            allowed_ips: Vec::new(),
            persistent_keepalive: None,
            preshared_key: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let config = Config::default();
        assert_eq!(config.listen_port, 51820);
        assert!(config.peers.is_empty());
        assert!(config.kill_switch.enabled);
    }

    #[test]
    fn config_roundtrip_toml() {
        let mut config = Config::default();
        let _ = config.generate_keypair().expect("keypair gen");

        let toml_str = config.to_string().expect("serialize");
        let config2 = Config::from_toml_str(&toml_str).expect("deserialize");
        assert_eq!(config.listen_port, config2.listen_port);
        assert_eq!(config.tun.name, config2.tun.name);
    }

    #[test]
    fn parse_public_key_hex() {
        let key_bytes = [0x42u8; 32];
        let hex_str = hex::encode(key_bytes);
        let parsed = Config::parse_public_key(&hex_str).expect("parse hex");
        assert_eq!(parsed, key_bytes);
    }

    #[test]
    fn parse_public_key_invalid() {
        let result = Config::parse_public_key("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn parse_endpoint_valid() {
        let ep = Config::parse_endpoint("1.2.3.4:51820").expect("parse endpoint");
        assert_eq!(ep.port(), 51820);
    }

    #[test]
    fn parse_endpoint_invalid() {
        let result = Config::parse_endpoint("not-an-address");
        assert!(result.is_err());
    }

    #[test]
    fn add_and_remove_peer() {
        let mut config = Config::default();
        let peer = PeerConfig {
            public_key: hex::encode([0x11u8; 32]),
            endpoint: "1.2.3.4:51820".to_string(),
            allowed_ips: vec!["0.0.0.0/0".to_string()],
            persistent_keepalive: Some(15),
            preshared_key: None,
        };
        config.add_peer(peer.clone());
        assert_eq!(config.peers.len(), 1);

        config.remove_peer(&peer.public_key);
        assert_eq!(config.peers.len(), 0);
    }

    #[test]
    fn fingerprint_generation() {
        let mut config = Config::default();
        let _ = config.generate_keypair().expect("keypair");
        let fp = config.fingerprint().expect("fingerprint");
        assert_eq!(fp.len(), 16);
    }
}
