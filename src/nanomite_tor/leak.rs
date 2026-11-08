//! Advisory leak protection.
//!
//! When Tor is active, application traffic should go through the SOCKS proxy
//! and DNS should be resolved by the proxy (remote DNS). This guard makes that
//! policy explicit and testable: it classifies a destination as direct (local)
//! or Tor-routed, refuses accidental direct egress, and checks whether a DNS
//! server is acceptable.
//!
//! It is *advisory*: it does not install firewall rules. OS-level enforcement is
//! a separate concern (see `nanomite_vpn::killswitch` for the VPN analogue).

use crate::nanomite_tor::{TorError, TorResult};
use std::net::{IpAddr, SocketAddr};

/// How a destination should be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDecision {
    /// Loopback/private destination, or Tor is inactive.
    Direct,
    /// Public destination while Tor is active.
    ViaTor,
}

/// In-process leak policy.
#[derive(Debug, Clone)]
pub struct TorLeakGuard {
    active: bool,
    socks: SocketAddr,
    dns: Option<SocketAddr>,
}

impl TorLeakGuard {
    pub fn new(socks: SocketAddr) -> Self {
        Self {
            active: false,
            socks,
            dns: None,
        }
    }

    /// Also accept the given DNS endpoint as non-leaking.
    pub fn with_dns(mut self, dns: SocketAddr) -> Self {
        self.dns = Some(dns);
        self
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn socks(&self) -> SocketAddr {
        self.socks
    }

    /// Classify a destination.
    pub fn route(&self, target: SocketAddr) -> RouteDecision {
        if !self.active || is_local(target.ip()) || self.is_tor_endpoint(target) {
            RouteDecision::Direct
        } else {
            RouteDecision::ViaTor
        }
    }

    /// True when a DNS server is safe to use: anything while Tor is inactive,
    /// otherwise only the configured DNS endpoint or a local resolver.
    pub fn dns_allowed(&self, server: SocketAddr) -> bool {
        if !self.active {
            return true;
        }
        is_local(server.ip()) || self.dns == Some(server)
    }

    /// Refuse a *direct* connection to a public address while Tor is active.
    pub fn enforce_direct(&self, target: SocketAddr) -> TorResult<()> {
        if self.route(target) == RouteDecision::ViaTor {
            return Err(TorError::Leak(format!(
                "direct egress to {target} refused while Tor is active"
            )));
        }
        Ok(())
    }

    fn is_tor_endpoint(&self, target: SocketAddr) -> bool {
        target == self.socks || self.dns == Some(target)
    }
}

/// Loopback, unspecified, link-local, private or unique-local addresses are
/// considered local and never routed through Tor.
fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unspecified(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(active: bool) -> TorLeakGuard {
        let mut guard = TorLeakGuard::new("127.0.0.1:9050".parse().unwrap());
        guard.set_active(active);
        guard
    }

    #[test]
    fn inactive_routes_everything_direct() {
        let g = guard(false);
        assert_eq!(
            g.route("93.184.216.34:443".parse().unwrap()),
            RouteDecision::Direct
        );
        assert!(g.dns_allowed("8.8.8.8:53".parse().unwrap()));
        assert!(g
            .enforce_direct("93.184.216.34:443".parse().unwrap())
            .is_ok());
    }

    #[test]
    fn active_routes_public_through_tor_but_not_local() {
        let g = guard(true);
        assert_eq!(
            g.route("93.184.216.34:443".parse().unwrap()),
            RouteDecision::ViaTor
        );
        assert_eq!(
            g.route("127.0.0.1:8080".parse().unwrap()),
            RouteDecision::Direct
        );
        assert_eq!(
            g.route("192.168.1.10:22".parse().unwrap()),
            RouteDecision::Direct
        );
        assert_eq!(
            g.route("10.0.0.5:80".parse().unwrap()),
            RouteDecision::Direct
        );
    }

    #[test]
    fn active_blocks_direct_public_egress() {
        let g = guard(true);
        let error = g
            .enforce_direct("93.184.216.34:443".parse().unwrap())
            .unwrap_err();
        assert!(matches!(error, TorError::Leak(_)));
        assert!(g.enforce_direct("127.0.0.1:9050".parse().unwrap()).is_ok());
    }

    #[test]
    fn dns_is_restricted_while_active() {
        let g = guard(true).with_dns("127.0.0.1:5353".parse().unwrap());
        assert!(g.dns_allowed("127.0.0.1:5353".parse().unwrap()));
        assert!(g.dns_allowed("127.0.0.1:53".parse().unwrap()));
        assert!(!g.dns_allowed("8.8.8.8:53".parse().unwrap()));
    }

    #[test]
    fn tor_endpoints_are_always_direct() {
        let g = guard(true);
        assert_eq!(
            g.route("127.0.0.1:9050".parse().unwrap()),
            RouteDecision::Direct
        );
    }
}
