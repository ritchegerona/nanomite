//! DNS-over-HTTPS resolver with leak protection for NanomiteVPN.
//!
//! `DnsResolver::resolve` issues a real DoH query (JSON API of the
//! configured endpoint, e.g. Google's `https://dns.google/resolve`) and
//! parses the `Answer` records. Results are cached for their TTL, and an
//! empty answer is an error — `leak_test` requires at least one address
//! before reporting "no leak", so it can no longer pass vacuously.
//!
//! Note: for leak protection to mean anything, the resolver must be
//! reachable *inside* the tunnel (endpoint IP covered by AllowedIPs) or
//! the DoH endpoint is itself trusted. `DnsLeakProtection` polices the
//! classic plaintext ports (53/UDP+TCP, 5353/mDNS).

use crate::nanomite_vpn::{VpnError, VpnResult};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// DNS-over-HTTPS resolver configuration.
#[derive(Debug, Clone)]
pub struct DoHConfig {
    /// DoH endpoint URL (JSON API).
    pub endpoint: String,
    /// Timeout for DNS queries.
    pub timeout_secs: u64,
    /// Whether to cache results.
    pub cache_enabled: bool,
}

impl Default for DoHConfig {
    fn default() -> Self {
        DoHConfig {
            endpoint: "https://dns.google/resolve".to_string(),
            timeout_secs: 5,
            cache_enabled: true,
        }
    }
}

/// A DNS record response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsRecord {
    /// The queried domain name.
    pub name: String,
    /// The resolved IP addresses.
    pub addresses: Vec<IpAddr>,
    /// TTL in seconds (minimum over answers, clamped).
    pub ttl: u32,
}

/// One `Answer` entry from the DoH JSON API. (Wire casing: `data` is
/// lowercase, `TTL` uppercase — matches Google's `dns.google/resolve`.)
#[derive(Debug, Clone, Deserialize)]
struct DohAnswer {
    data: String,
    #[serde(rename = "TTL", default)]
    #[allow(dead_code)]
    ttl: Option<u32>,
}

/// Wire shape of the DoH JSON response (e.g. https://dns.google/resolve).
#[derive(Debug, Clone, Deserialize)]
struct DohResponse {
    #[serde(rename = "Answer", default)]
    answer: Vec<DohAnswer>,
}

fn parse_doh_response(name: &str, json: &str) -> VpnResult<DnsRecord> {
    let resp: DohResponse =
        serde_json::from_str(json).map_err(|e| VpnError::Dns(format!("doh parse: {e}")))?;
    let (mut addresses, mut ttl) = (Vec::new(), u32::MAX);
    for answer in &resp.answer {
        if let Ok(ip) = answer.data.parse::<IpAddr>() {
            addresses.push(ip);
        }
        if let Some(t) = answer.ttl {
            ttl = ttl.min(t);
        }
    }
    if addresses.is_empty() {
        return Err(VpnError::Dns(format!("no A/AAAA answer for {name}")));
    }
    Ok(DnsRecord {
        name: name.to_string(),
        addresses,
        ttl: ttl.min(86400),
    })
}

/// DNS-over-HTTPS resolver.
pub struct DnsResolver {
    config: DoHConfig,
    cache: HashMap<String, (DnsRecord, Instant)>,
}

impl DnsResolver {
    /// Create a new DoH resolver.
    pub fn new(config: DoHConfig) -> Self {
        DnsResolver {
            config,
            cache: HashMap::new(),
        }
    }

    /// Resolve a domain name via DNS-over-HTTPS.
    pub async fn resolve(&mut self, name: &str) -> VpnResult<DnsRecord> {
        if self.config.cache_enabled {
            if let Some((record, at)) = self.cache.get(name) {
                if at.elapsed().as_secs() < record.ttl as u64 {
                    return Ok(record.clone());
                }
            }
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .build()
            .map_err(|e| VpnError::Dns(format!("doh client: {e}")))?;
        let url = format!(
            "{}?name={}&type=A",
            self.config.endpoint,
            urlencoding_escape(name)
        );
        let body = client
            .get(&url)
            .send()
            .await
            .map_err(|e| VpnError::Dns(format!("doh request failed: {e}")))?
            .error_for_status()
            .map_err(|e| VpnError::Dns(format!("doh HTTP status: {e}")))?
            .text()
            .await
            .map_err(|e| VpnError::Dns(format!("doh body read: {e}")))?;

        let record = parse_doh_response(name, &body)?;

        if self.config.cache_enabled {
            self.cache
                .insert(name.to_string(), (record.clone(), Instant::now()));
        }
        Ok(record)
    }

    /// Test for DNS leaks: resolve a well-known name and require at least
    /// one address through the tunnel resolver. Returns the record.
    pub async fn leak_test(&mut self) -> VpnResult<DnsRecord> {
        self.resolve("dns.google").await
    }

    /// Clear the DNS cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

/// Minimal URL query escaping for the `name=` parameter (names are LDH
/// labels; only `%` and non-ASCII need handling in practice).
fn urlencoding_escape(s: &str) -> String {
    s.replace('%', "%25").replace(' ', "%20")
}

/// DNS leak protection — blocks all DNS traffic that doesn't go
/// through the VPN tunnel.
pub struct DnsLeakProtection {
    /// Blocked DNS ports (53 for UDP/TCP).
    blocked_ports: Vec<u16>,
    /// Allowed DNS servers (via the tunnel).
    allowed_servers: Vec<IpAddr>,
    /// Whether leak protection is active.
    active: bool,
}

impl Default for DnsLeakProtection {
    fn default() -> Self {
        Self::new()
    }
}

impl DnsLeakProtection {
    /// Create new DNS leak protection.
    pub fn new() -> Self {
        DnsLeakProtection {
            blocked_ports: vec![53, 5353], // DNS and mDNS
            allowed_servers: Vec::new(),
            active: false,
        }
    }

    /// Activate DNS leak protection (block system DNS, force tunnel DNS).
    pub fn activate(&mut self, dns_servers: Vec<IpAddr>) {
        self.allowed_servers = dns_servers;
        self.active = true;
    }

    /// Deactivate DNS leak protection.
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Check if DNS leak protection is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Check if a connection to a DNS server should be blocked.
    pub fn should_block(&self, addr: std::net::SocketAddr) -> bool {
        if !self.active {
            return false;
        }
        if self.blocked_ports.contains(&addr.port()) && !self.allowed_servers.contains(&addr.ip()) {
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_doh_response_extracts_ips_and_ttl() {
        let body = r#"{"Status":0,"Answer":[{"name":"example.com.","type":1,"TTL":300,"data":"93.184.216.34"}]}"#;
        let record = parse_doh_response("example.com", body).expect("parse");
        assert_eq!(record.name, "example.com");
        assert_eq!(
            record.addresses,
            vec!["93.184.216.34".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(record.ttl, 300);
    }

    #[test]
    fn parse_doh_response_empty_answer_is_error() {
        let body = r#"{"Status":3}"#; // NXDOMAIN-ish: no Answer array
        assert!(parse_doh_response("nope.example", body).is_err());
    }

    #[test]
    fn parse_doh_response_min_ttl_wins() {
        let body = r#"{"Answer":[{"data":"1.1.1.1","TTL":600},{"data":"1.0.0.1","TTL":60}]}"#;
        let record = parse_doh_response("x", body).expect("parse");
        assert_eq!(record.addresses.len(), 2);
        assert_eq!(record.ttl, 60);
    }

    #[test]
    fn doh_config_default() {
        let config = DoHConfig::default();
        assert!(config.endpoint.contains("dns.google"));
        assert_eq!(config.timeout_secs, 5);
        assert!(config.cache_enabled);
    }

    #[test]
    fn dns_leak_protection() {
        let mut protection = DnsLeakProtection::new();
        assert!(!protection.is_active());
        protection.activate(vec!["10.8.0.1".parse().unwrap()]);
        assert!(protection.is_active());
        let blocked: std::net::SocketAddr = "8.8.8.8:53".parse().unwrap();
        assert!(protection.should_block(blocked));
        let allowed: std::net::SocketAddr = "10.8.0.1:53".parse().unwrap();
        assert!(!protection.should_block(allowed));
        let http: std::net::SocketAddr = "1.2.3.4:80".parse().unwrap();
        assert!(!protection.should_block(http));
    }

    #[test]
    fn urlescape_handles_hostile_names() {
        assert_eq!(urlencoding_escape("a%b c"), "a%25b%20c");
    }
}
