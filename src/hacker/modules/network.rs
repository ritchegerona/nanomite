//! Network security: bounded, authorized port/service reachability checks.
//!
//! Connects are injected through [`Connector`] so tests never open sockets.
//! The caller resolves and scope-checks addresses *before* calling (see the
//! coordinator); this module additionally bounds the address and port counts.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use super::ModuleReport;

/// Addresses probed per run (bounded).
pub const MAX_IPS: usize = 8;
/// Ports probed per run (bounded).
pub const MAX_PORTS: usize = 32;
/// Per-connect timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(800);

/// Opens a TCP connection. Production uses a real socket; tests inject a stub.
pub trait Connector: Send + Sync {
    fn connect(&self, addr: SocketAddr, timeout: Duration) -> bool;
}

/// Real TCP connector.
pub struct TcpConnector;

impl Connector for TcpConnector {
    fn connect(&self, addr: SocketAddr, timeout: Duration) -> bool {
        std::net::TcpStream::connect_timeout(&addr, timeout).is_ok()
    }
}

/// A small, conservative default port set: common management and web/service
/// ports. Callers may pass their own bounded list.
pub fn default_ports() -> Vec<u16> {
    vec![22, 80, 443, 3306, 5432, 6379, 8080, 8443]
}

/// Probes `ports` on each address. Returns the open endpoints, bounded by
/// [`MAX_IPS`] × [`MAX_PORTS`].
pub fn scan_ports(
    host: &str,
    ips: &[IpAddr],
    ports: &[u16],
    connector: &dyn Connector,
) -> Result<ModuleReport, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("network/ports needs a host".into());
    }
    if ips.is_empty() {
        return Err("network/ports needs at least one resolved address".into());
    }
    let ips = &ips[..ips.len().min(MAX_IPS)];
    let ports = &ports[..ports.len().min(MAX_PORTS)];

    let mut open: Vec<String> = Vec::new();
    for ip in ips {
        for &port in ports {
            let addr = SocketAddr::new(*ip, port);
            if connector.connect(addr, DEFAULT_TIMEOUT) {
                open.push(format!("{ip}:{port} open"));
            }
        }
    }

    let probed = ips.len() * ports.len();
    let summary = if open.is_empty() {
        format!("no open ports across {probed} probe(s)")
    } else {
        format!("{} open across {probed} probe(s)", open.len())
    };
    let mut report = ModuleReport::new("network", "ports", host, summary);
    for line in open {
        report = report.line(line);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A connector that reports the given ports open on the first address.
    struct Stub {
        open: HashSet<(IpAddr, u16)>,
    }
    impl Connector for Stub {
        fn connect(&self, addr: SocketAddr, _timeout: Duration) -> bool {
            self.open.contains(&(addr.ip(), addr.port()))
        }
    }

    #[test]
    fn reports_only_open_ports() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let open = HashSet::from([(ip, 22), (ip, 443)]);
        let report = scan_ports("example.com", &[ip], &[22, 80, 443], &Stub { open }).unwrap();
        assert_eq!(report.summary, "2 open across 3 probe(s)");
        assert!(report.lines.contains(&"10.0.0.1:22 open".to_string()));
        assert!(report.lines.contains(&"10.0.0.1:443 open".to_string()));
        assert!(!report.lines.iter().any(|l| l.contains(":80")));
    }

    #[test]
    fn refuses_without_addresses_or_host() {
        let stub = Stub {
            open: HashSet::new(),
        };
        assert!(scan_ports("example.com", &[], &[80], &stub).is_err());
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(scan_ports("  ", &[ip], &[80], &stub).is_err());
    }

    #[test]
    fn probe_count_is_bounded() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let ports: Vec<u16> = (1..=1000).collect();
        let report = scan_ports(
            "h",
            &[ip],
            &ports,
            &Stub {
                open: HashSet::new(),
            },
        )
        .unwrap();
        assert!(report.summary.contains(&format!("{} probe", MAX_PORTS)));
    }
}
