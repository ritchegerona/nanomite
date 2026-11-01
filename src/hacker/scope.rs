//! Assessment scope: typed targets and the matching rules that decide whether
//! a resolved destination is inside an authorized assessment.
//!
//! Scope is validated against the *resolved* destination, never against the
//! hostname alone: a permitted hostname whose DNS resolves outside the
//! allowed CIDRs is refused. Parsing and matching are pure so they are cheap
//! to test; DNS resolution itself is performed by the caller and handed in.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::context::AssessmentContext;

/// A parsed IPv4/IPv6 CIDR block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpNet {
    V4 { net: u32, prefix: u8 },
    V6 { net: u128, prefix: u8 },
}

impl IpNet {
    /// The network address as an `IpAddr`.
    pub fn network(&self) -> IpAddr {
        match *self {
            IpNet::V4 { net, .. } => IpAddr::V4(Ipv4Addr::from(net)),
            IpNet::V6 { net, .. } => IpAddr::V6(Ipv6Addr::from(net)),
        }
    }

    pub fn prefix(&self) -> u8 {
        match *self {
            IpNet::V4 { prefix, .. } | IpNet::V6 { prefix, .. } => prefix,
        }
    }

    /// Whether `ip` falls inside this block. Mismatched families never match.
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (*self, ip) {
            (IpNet::V4 { net, prefix }, IpAddr::V4(ip)) => {
                let mask = v4_mask(prefix);
                (u32::from(ip) & mask) == net
            }
            (IpNet::V6 { net, prefix }, IpAddr::V6(ip)) => {
                let mask = v6_mask(prefix);
                (u128::from(ip) & mask) == net
            }
            _ => false,
        }
    }
}

fn v4_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix.min(32))
    }
}

fn v6_mask(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix.min(128))
    }
}

/// Parses `"10.0.0.0/8"`, `"192.168.1.5/32"`, `"::1/128"`, or a bare IP
/// (treated as a /32 or /128 host route). Anything malformed is rejected.
pub fn parse_cidr(raw: &str) -> Result<IpNet, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty CIDR".into());
    }
    let (addr_part, prefix_part) = match raw.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (raw, None),
    };
    let ip: IpAddr = addr_part
        .parse()
        .map_err(|_| format!("invalid IP address `{addr_part}`"))?;
    match ip {
        IpAddr::V4(v4) => {
            let prefix = match prefix_part {
                Some(p) => p
                    .parse::<u8>()
                    .map_err(|_| format!("invalid prefix `{p}`"))?,
                None => 32,
            };
            if prefix > 32 {
                return Err(format!("IPv4 prefix {prefix} out of range 0..=32"));
            }
            let mask = v4_mask(prefix);
            Ok(IpNet::V4 {
                net: u32::from(v4) & mask,
                prefix,
            })
        }
        IpAddr::V6(v6) => {
            let prefix = match prefix_part {
                Some(p) => p
                    .parse::<u8>()
                    .map_err(|_| format!("invalid prefix `{p}`"))?,
                None => 128,
            };
            if prefix > 128 {
                return Err(format!("IPv6 prefix {prefix} out of range 0..=128"));
            }
            let mask = v6_mask(prefix);
            Ok(IpNet::V6 {
                net: u128::from(v6) & mask,
                prefix,
            })
        }
    }
}

/// Normalizes a hostname or IP literal: trimmed, lowercased, trailing dot
/// removed. Empty input yields `None`.
pub fn normalize_host(raw: &str) -> Option<String> {
    let host = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// `host` equals `allowed` or is a subdomain of it. Never a substring match
/// (`evil-example.com` is not inside `example.com`).
pub fn domain_in_scope(host: &str, allowed: &[String]) -> bool {
    allowed
        .iter()
        .any(|a| host == a || host.strip_suffix(a).is_some_and(|rest| rest.ends_with('.')))
}

/// Classification of an address that shapes scope decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpClass {
    Public,
    Private,
    Loopback,
    LinkLocal,
    /// The cloud instance-metadata address `169.254.169.254`.
    Metadata,
}

/// Classifies an address. Metadata is singled out because reaching it is a
/// common SSRF escalation and must never be implied by a broad CIDR.
pub fn classify_ip(ip: IpAddr) -> IpClass {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() {
                IpClass::Loopback
            } else {
                IpClass::Public
            }
        }
    }
}

fn classify_v4(ip: Ipv4Addr) -> IpClass {
    if ip == Ipv4Addr::new(169, 254, 169, 254) {
        return IpClass::Metadata;
    }
    if ip.is_loopback() || ip.is_unspecified() {
        return IpClass::Loopback;
    }
    if ip.is_private() {
        return IpClass::Private;
    }
    if ip.is_link_local() {
        return IpClass::LinkLocal;
    }
    IpClass::Public
}

/// Outcome of a scope check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeMatch {
    InScope,
    OutOfScope,
    /// Not enough information to decide (empty resolution, unparsed target).
    Ambiguous,
}

impl ScopeMatch {
    pub fn is_in_scope(self) -> bool {
        matches!(self, ScopeMatch::InScope)
    }
}

/// Checks a single normalized host/IP against an assessment's allowed set.
/// The metadata endpoint is only in scope when an allowed CIDR literally
/// covers it (i.e. the assessment explicitly included it).
pub fn host_in_scope(host: &str, ctx: &AssessmentContext) -> ScopeMatch {
    let Some(host) = normalize_host(host) else {
        return ScopeMatch::Ambiguous;
    };
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip_in_scope(ip, ctx);
    }
    if domain_in_scope(&host, &ctx.allowed_domains) {
        ScopeMatch::InScope
    } else {
        ScopeMatch::OutOfScope
    }
}

/// Checks one IP against the allowed CIDRs.
pub fn ip_in_scope(ip: IpAddr, ctx: &AssessmentContext) -> ScopeMatch {
    if ctx.allowed_nets().iter().any(|net| net.contains(ip)) {
        ScopeMatch::InScope
    } else {
        ScopeMatch::OutOfScope
    }
}

/// Validates a resolved target: every resolved IP must be inside scope, and
/// there must be at least one. A permitted hostname that resolves partly
/// outside the allowed CIDRs is refused (DNS rebinding / round-robin guard).
pub fn resolved_in_scope(resolved: &[IpAddr], ctx: &AssessmentContext) -> ScopeMatch {
    if resolved.is_empty() {
        return ScopeMatch::Ambiguous;
    }
    if resolved
        .iter()
        .all(|ip| ip_in_scope(*ip, ctx).is_in_scope())
    {
        ScopeMatch::InScope
    } else {
        ScopeMatch::OutOfScope
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn ctx_with(cidrs: &[&str], domains: &[&str]) -> AssessmentContext {
        let mut ctx = AssessmentContext::new("a1", "test assessment");
        ctx.allowed_cidrs = cidrs.iter().map(|s| s.to_string()).collect();
        ctx.allowed_domains = domains.iter().map(|s| s.to_string()).collect();
        ctx.start = SystemTime::now();
        ctx.expiry = ctx.start + Duration::from_secs(3600);
        ctx
    }

    #[test]
    fn cidr_parsing_and_matching() {
        let net = parse_cidr("10.0.0.0/8").unwrap();
        assert!(net.contains("10.1.2.3".parse().unwrap()));
        assert!(!net.contains("11.0.0.1".parse().unwrap()));

        let host = parse_cidr("192.168.1.5").unwrap();
        assert_eq!(host.prefix(), 32);
        assert!(host.contains("192.168.1.5".parse().unwrap()));
        assert!(!host.contains("192.168.1.6".parse().unwrap()));

        let v6 = parse_cidr("2001:db8::/32").unwrap();
        assert!(v6.contains("2001:db8::1".parse().unwrap()));
        assert!(!v6.contains("2001:dead::1".parse().unwrap()));
    }

    #[test]
    fn cidr_parsing_rejects_bad_input() {
        assert!(parse_cidr("").is_err());
        assert!(parse_cidr("nonsense").is_err());
        assert!(parse_cidr("10.0.0.0/33").is_err());
        assert!(parse_cidr("::/200").is_err());
    }

    #[test]
    fn domain_matching_is_exact_or_subdomain_only() {
        let allowed = vec!["example.com".to_string(), "lab.internal".to_string()];
        assert!(domain_in_scope("example.com", &allowed));
        assert!(domain_in_scope("api.example.com", &allowed));
        assert!(domain_in_scope("a.b.example.com", &allowed));
        // Substring impostors are refused.
        assert!(!domain_in_scope("evil-example.com", &allowed));
        assert!(!domain_in_scope("example.com.evil.net", &allowed));
        assert!(!domain_in_scope("other.com", &allowed));
    }

    #[test]
    fn metadata_endpoint_is_flagged() {
        assert_eq!(
            classify_ip("169.254.169.254".parse().unwrap()),
            IpClass::Metadata
        );
        assert_eq!(classify_ip("127.0.0.1".parse().unwrap()), IpClass::Loopback);
        assert_eq!(classify_ip("10.0.0.1".parse().unwrap()), IpClass::Private);
        assert_eq!(classify_ip("8.8.8.8".parse().unwrap()), IpClass::Public);
    }

    #[test]
    fn host_scope_uses_domains_for_names_and_cidrs_for_ips() {
        let ctx = ctx_with(&["10.0.0.0/8"], &["example.com"]);
        assert!(host_in_scope("api.example.com", &ctx).is_in_scope());
        assert_eq!(host_in_scope("evil.com", &ctx), ScopeMatch::OutOfScope);
        assert!(host_in_scope("10.1.1.1", &ctx).is_in_scope());
        assert_eq!(host_in_scope("8.8.8.8", &ctx), ScopeMatch::OutOfScope);
        assert_eq!(host_in_scope("", &ctx), ScopeMatch::Ambiguous);
    }

    #[test]
    fn resolved_addresses_all_must_be_in_scope() {
        let ctx = ctx_with(&["10.0.0.0/8"], &["example.com"]);
        let all_in = vec!["10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap()];
        assert!(resolved_in_scope(&all_in, &ctx).is_in_scope());

        // One address outside the CIDR refuses the whole target (rebinding).
        let mixed = vec!["10.0.0.1".parse().unwrap(), "8.8.8.8".parse().unwrap()];
        assert_eq!(resolved_in_scope(&mixed, &ctx), ScopeMatch::OutOfScope);

        // No resolution is ambiguous, never assumed in scope.
        assert_eq!(resolved_in_scope(&[], &ctx), ScopeMatch::Ambiguous);
    }

    #[test]
    fn metadata_is_out_of_scope_unless_a_cidr_covers_it() {
        let ctx = ctx_with(&["10.0.0.0/8"], &["example.com"]);
        assert_eq!(
            host_in_scope("169.254.169.254", &ctx),
            ScopeMatch::OutOfScope
        );

        let explicit = ctx_with(&["169.254.169.254/32"], &[]);
        assert!(host_in_scope("169.254.169.254", &explicit).is_in_scope());
    }
}
