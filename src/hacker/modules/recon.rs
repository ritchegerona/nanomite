//! Reconnaissance: authorized domain/DNS inventory.
//!
//! Passive-first: the operation resolves a configured in-scope host and
//! reports its addresses. The resolver is injected so tests never touch DNS.

use std::net::IpAddr;

use super::ModuleReport;

/// Upper bound on addresses reported for one host.
pub const MAX_ADDRESSES: usize = 32;

/// Resolves hostnames to addresses. The production impl uses the system
/// resolver; tests inject a stub.
pub trait Resolver: Send + Sync {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String>;
}

/// The system resolver (`std::net::ToSocketAddrs`).
pub struct SystemResolver;

impl Resolver for SystemResolver {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        use std::net::ToSocketAddrs;
        let addrs = (host, 0)
            .to_socket_addrs()
            .map_err(|e| format!("DNS resolution failed: {e}"))?;
        Ok(addrs.map(|s| s.ip()).collect())
    }
}

/// Resolves `host` and builds a report. Addresses are de-duplicated and
/// sorted; the list is bounded to [`MAX_ADDRESSES`].
pub fn dns(host: &str, resolver: &dyn Resolver) -> Result<ModuleReport, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("recon/dns needs a host".into());
    }
    let ips = resolver.resolve(host)?;
    Ok(dns_with(host, &ips))
}

/// Builds the report from an already-resolved address list. The coordinator
/// resolves once (so it can scope-check each address) and passes them here.
pub fn dns_with(host: &str, ips: &[IpAddr]) -> ModuleReport {
    let mut ips = ips.to_vec();
    ips.sort();
    ips.dedup();
    let total = ips.len();
    ips.truncate(MAX_ADDRESSES);

    let summary = if total == 0 {
        "no addresses resolved".to_string()
    } else {
        format!("{total} address(es)")
    };
    let mut report = ModuleReport::new("recon", "dns", host.trim(), summary);
    for ip in &ips {
        report = report.line(ip.to_string());
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub(Vec<IpAddr>);
    impl Resolver for Stub {
        fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn resolves_dedups_and_sorts() {
        let stub = Stub(vec![
            "10.0.0.2".parse().unwrap(),
            "10.0.0.1".parse().unwrap(),
            "10.0.0.1".parse().unwrap(),
        ]);
        let report = dns("example.com", &stub).unwrap();
        assert_eq!(report.summary, "2 address(es)");
        assert_eq!(report.lines, vec!["10.0.0.1", "10.0.0.2"]);
    }

    #[test]
    fn empty_host_is_refused() {
        let stub = Stub(vec![]);
        assert!(dns("  ", &stub).is_err());
    }

    #[test]
    fn address_list_is_bounded() {
        let many: Vec<IpAddr> = (0..100)
            .map(|i| IpAddr::from([10, 0, 0, i as u8]))
            .collect();
        let report = dns("example.com", &Stub(many)).unwrap();
        assert_eq!(report.lines.len(), MAX_ADDRESSES);
    }
}
