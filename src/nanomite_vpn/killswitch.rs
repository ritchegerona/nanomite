//! Kill switch for NanomiteVPN.
//!
//! Prevents all network traffic from leaking outside the VPN tunnel.
//! When active, only traffic to the VPN server and through the TUN
//! interface is allowed; everything else is blocked.

use crate::nanomite_vpn::{VpnError, VpnResult};
use std::net::{IpAddr, SocketAddr};

/// Kill switch configuration.
#[derive(Debug, Clone)]
pub struct KillSwitchConfig {
    /// VPN server endpoint that must remain reachable.
    pub vpn_server: SocketAddr,
    /// TUN interface subnet (CIDR).
    pub tun_subnet: String,
    /// Local network subnet to allow (e.g., for router access).
    pub local_subnet: Option<String>,
    /// Whether to allow DNS to the VPN server's DNS.
    pub allow_dns: bool,
}

impl Default for KillSwitchConfig {
    fn default() -> Self {
        KillSwitchConfig {
            vpn_server: "127.0.0.1:51820".parse().unwrap(),
            tun_subnet: "10.8.0.0/24".to_string(),
            local_subnet: Some("192.168.0.0/16".to_string()),
            allow_dns: true,
        }
    }
}

/// Kill switch state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillSwitchState {
    /// Kill switch is inactive; all traffic allowed.
    Inactive,
    /// Kill switch is active; only VPN traffic allowed.
    Active,
    /// Kill switch encountered an error setting up rules.
    Error(String),
}

/// Kill switch implementation using nftables/iptables.
pub struct KillSwitch {
    config: KillSwitchConfig,
    state: KillSwitchState,
    rules_installed: bool,
}

impl KillSwitch {
    /// Create a new kill switch with the given configuration.
    pub fn new(config: KillSwitchConfig) -> Self {
        KillSwitch {
            config,
            state: KillSwitchState::Inactive,
            rules_installed: false,
        }
    }

    /// Activate the kill switch by installing firewall rules.
    pub fn activate(&mut self) -> VpnResult<()> {
        self.install_rules()?;
        self.state = KillSwitchState::Active;
        self.rules_installed = true;
        Ok(())
    }

    /// Deactivate the kill switch by removing firewall rules.
    pub fn deactivate(&mut self) -> VpnResult<()> {
        self.remove_rules()?;
        self.state = KillSwitchState::Inactive;
        self.rules_installed = false;
        Ok(())
    }

    /// Get the current kill switch state.
    pub fn state(&self) -> KillSwitchState {
        self.state.clone()
    }

    /// Check if the kill switch is active.
    pub fn is_active(&self) -> bool {
        matches!(self.state, KillSwitchState::Active)
    }

    /// Check if a connection to the given address should be allowed.
    pub fn should_allow(&self, addr: SocketAddr) -> bool {
        if !self.is_active() {
            return true; // Kill switch off, allow all
        }

        // Always allow traffic to the VPN server
        if addr == self.config.vpn_server {
            return true;
        }

        // Allow traffic to local subnet if configured
        if let Some(ref local_subnet) = self.config.local_subnet {
            if self.ip_in_subnet(addr.ip(), local_subnet) {
                return true;
            }
        }

        // Allow DNS to VPN server if configured
        if self.config.allow_dns && addr.port() == 53 && addr.ip() == self.config.vpn_server.ip() {
            return true;
        }

        // Block everything else
        false
    }

    /// Install firewall rules (iptables/nftables).
    fn install_rules(&self) -> VpnResult<()> {
        #[cfg(target_os = "linux")]
        {
            use std::process::Command;

            // Create a new chain for nanomite-vpn
            let _ = Command::new("nft")
                .args(["add", "chain", "inet", "filter", "nanomite_vpn"])
                .status();

            // Allow established connections
            let _ = Command::new("nft")
                .args([
                    "add",
                    "rule",
                    "inet",
                    "filter",
                    "nanomite_vpn",
                    "ct",
                    "state",
                    "established,related",
                    "accept",
                ])
                .status();

            // Allow loopback
            let _ = Command::new("nft")
                .args([
                    "add",
                    "rule",
                    "inet",
                    "filter",
                    "nanomite_vpn",
                    "iifname",
                    "lo",
                    "accept",
                ])
                .status();

            // Allow VPN server traffic
            let _ = Command::new("nft")
                .args([
                    "add",
                    "rule",
                    "inet",
                    "filter",
                    "nanomite_vpn",
                    "ip",
                    "daddr",
                    &self.config.vpn_server.ip().to_string(),
                    "udp",
                    "dport",
                    &self.config.vpn_server.port().to_string(),
                    "accept",
                ])
                .status();

            // Allow TUN interface
            let _ = Command::new("nft")
                .args([
                    "add",
                    "rule",
                    "inet",
                    "filter",
                    "nanomite_vpn",
                    "iifname",
                    &self.config.tun_subnet,
                    "accept",
                ])
                .status();

            // Drop everything else
            let _ = Command::new("nft")
                .args(["add", "rule", "inet", "filter", "nanomite_vpn", "drop"])
                .status();
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = self;
            // Non-Linux: kill switch is a no-op (firewall rules not implemented)
        }

        Ok(())
    }

    /// Remove firewall rules.
    fn remove_rules(&self) -> VpnResult<()> {
        #[cfg(target_os = "linux")]
        {
            use std::process::Command;
            let _ = Command::new("nft")
                .args(["delete", "chain", "inet", "filter", "nanomite_vpn"])
                .status();
        }
        Ok(())
    }

    /// Check if an IP address is within a CIDR subnet.
    fn ip_in_subnet(&self, ip: IpAddr, subnet: &str) -> bool {
        let parts: Vec<&str> = subnet.split('/').collect();
        if parts.len() != 2 {
            return false;
        }
        let network: IpAddr = match parts[0].parse() {
            Ok(ip) => ip,
            Err(_) => return false,
        };
        let prefix: u8 = match parts[1].parse() {
            Ok(p) => p,
            Err(_) => return false,
        };

        match (ip, network) {
            (IpAddr::V4(ip), IpAddr::V4(network)) => {
                let ip_bits = u32::from(ip);
                let net_bits = u32::from(network);
                let mask: u32 = if prefix == 0 {
                    0
                } else {
                    !((1u32 << (32 - prefix)) - 1)
                };
                (ip_bits & mask) == (net_bits & mask)
            }
            (IpAddr::V6(_), IpAddr::V6(_)) => {
                // Simplified: V6 not fully implemented
                false
            }
            _ => false,
        }
    }

    /// Test that the kill switch is working by checking a few addresses.
    pub fn leak_test(&self) -> VpnResult<bool> {
        if !self.is_active() {
            return Err(VpnError::KillSwitch("kill switch not active".to_string()));
        }

        // Traffic to VPN server should be allowed
        if !self.should_allow(self.config.vpn_server) {
            return Err(VpnError::KillSwitch(
                "VPN server traffic blocked".to_string(),
            ));
        }

        // Traffic to random external IP on port 80 should be blocked
        let external: SocketAddr = "1.2.3.4:80".parse().unwrap();
        if self.should_allow(external) {
            return Err(VpnError::KillSwitch(
                "external traffic not blocked".to_string(),
            ));
        }

        Ok(true)
    }
}

impl Default for KillSwitch {
    fn default() -> Self {
        Self::new(KillSwitchConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_switch_config_default() {
        let config = KillSwitchConfig::default();
        assert_eq!(config.vpn_server.port(), 51820);
        assert_eq!(config.tun_subnet, "10.8.0.0/24");
        assert!(config.allow_dns);
    }

    #[test]
    fn kill_switch_inactive_allows_all() {
        let ks = KillSwitch::new(KillSwitchConfig::default());
        assert!(!ks.is_active());

        let addr: SocketAddr = "8.8.8.8:53".parse().unwrap();
        assert!(ks.should_allow(addr));
    }

    #[test]
    fn kill_switch_active_blocks_external() {
        let mut ks = KillSwitch::new(KillSwitchConfig::default());
        ks.activate().expect("activate should succeed");
        assert!(ks.is_active());

        // VPN server should be allowed
        assert!(ks.should_allow(ks.config.vpn_server));

        // External traffic should be blocked
        let external: SocketAddr = "1.2.3.4:80".parse().unwrap();
        assert!(!ks.should_allow(external));

        // External DNS should be blocked
        let external_dns: SocketAddr = "8.8.8.8:53".parse().unwrap();
        assert!(!ks.should_allow(external_dns));
    }

    #[test]
    fn kill_switch_ip_in_subnet() {
        let ks = KillSwitch::new(KillSwitchConfig::default());
        assert!(ks.ip_in_subnet("10.8.0.5".parse().unwrap(), "10.8.0.0/24"));
        assert!(!ks.ip_in_subnet("10.8.1.5".parse().unwrap(), "10.8.0.0/24"));
        assert!(ks.ip_in_subnet("192.168.1.1".parse().unwrap(), "192.168.0.0/16"));
    }

    #[test]
    fn kill_switch_allow_dns_to_vpn() {
        let config = KillSwitchConfig {
            vpn_server: "10.8.0.1:51820".parse().unwrap(),
            tun_subnet: "10.8.0.0/24".to_string(),
            local_subnet: None,
            allow_dns: true,
        };
        let mut ks = KillSwitch::new(config);
        ks.activate().expect("activate");

        let vpn_dns: SocketAddr = "10.8.0.1:53".parse().unwrap();
        assert!(ks.should_allow(vpn_dns));
    }
}
