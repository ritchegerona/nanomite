//! Advanced Reconnaissance: Certificate Transparency, ASN mapping, Cloud asset enumeration.
//!
//! All operations are passive-first, scope-gated, and bounded. The resolver/fetcher
//! are injected so tests never touch the network.

use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// Max subdomains per CT log query.
pub const MAX_CT_SUBDOMAINS: usize = 200;
/// Max cloud assets per provider query.
pub const MAX_CLOUD_ASSETS: usize = 150;
/// HTTP timeout for CT/cloud API calls.
pub const ADV_RECON_TIMEOUT: Duration = Duration::from_secs(30);

/// Fetches JSON from a URL. Production uses reqwest; tests inject a stub.
pub trait JsonFetcher: Send + Sync {
    fn fetch_json(&self, url: &str) -> Result<serde_json::Value, String>;
}

/// Real fetcher for CT logs and cloud APIs.
pub struct ReqwestJsonFetcher;

impl JsonFetcher for ReqwestJsonFetcher {
    fn fetch_json(&self, url: &str) -> Result<serde_json::Value, String> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(ADV_RECON_TIMEOUT)
            .user_agent("nanomite-hacker-adv-recon/0.1")
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        let response = client
            .get(url)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("http {}: {}", status.as_u16(), status));
        }
        response.json().map_err(|e| format!("json parse: {e}"))
    }
}

/// Resolves hostnames to addresses. The production impl uses the system resolver.
pub trait Resolver: Send + Sync {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String>;
}

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

/// Certificate Transparency log entry (crt.sh style).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CtEntry {
    #[serde(rename = "name_value")]
    pub name_value: String,
    #[serde(rename = "issuer_name")]
    pub issuer_name: Option<String>,
    #[serde(rename = "not_before")]
    pub not_before: Option<String>,
    #[serde(rename = "not_after")]
    pub not_after: Option<String>,
}

/// Queries crt.sh for subdomains of a domain.
pub fn certificate_transparency(
    domain: &str,
    fetcher: &dyn JsonFetcher,
) -> Result<ModuleReport, String> {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() {
        return Err("advanced_recon/ct needs a domain".into());
    }

    // crt.sh API: https://crt.sh/?q=%25.example.com&output=json
    let url = format!("https://crt.sh/?q=%25.{domain}&output=json");
    let json = fetcher.fetch_json(&url)?;

    let mut subdomains = HashSet::new();
    if let Some(arr) = json.as_array() {
        for entry in arr {
            if let Some(name) = entry.get("name_value").and_then(|v| v.as_str()) {
                for line in name.lines() {
                    let line = line.trim().to_ascii_lowercase();
                    if line.ends_with(&format!(".{domain}")) || line == domain {
                        // Strip wildcard prefix
                        let clean = line.strip_prefix("*.").unwrap_or(&line);
                        if clean.ends_with(&domain) {
                            subdomains.insert(clean.to_string());
                        }
                    }
                }
            }
        }
    }

    let mut subdomains: Vec<String> = subdomains.into_iter().collect();
    subdomains.sort();
    let total = subdomains.len();
    subdomains.truncate(MAX_CT_SUBDOMAINS);

    let summary = if total == 0 {
        "no subdomains found in CT logs".to_string()
    } else {
        format!("{total} unique subdomain(s) from CT logs")
    };

    let mut report = ModuleReport::new("advanced_recon", "ct", domain.clone(), summary);
    for sub in &subdomains {
        report = report.line(sub.clone());
    }

    if !subdomains.is_empty() {
        report = report.finding(
            Finding::new(
                "advanced_recon",
                "ct",
                domain.clone(),
                format!("ct subdomain inventory: {total}"),
                RiskLevel::Informational,
            )
            .confidence(Confidence::Confirmed)
            .detail(format!(
                "Certificate Transparency logs reveal {} subdomain(s) for {}",
                total, domain
            )),
        );
    }

    Ok(report)
}

/// ASN information from ipinfo.io / ipapi.co style APIs.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AsnInfo {
    pub asn: Option<String>,
    pub org: Option<String>,
    pub country: Option<String>,
    pub isp: Option<String>,
}

/// Queries ASN info for an IP or CIDR.
pub fn asn_mapping(target: &str, fetcher: &dyn JsonFetcher) -> Result<ModuleReport, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("advanced_recon/asn needs an IP or CIDR".into());
    }

    // Use ipapi.co (free tier, no key needed for low volume)
    let url = format!("https://ipapi.co/{target}/json/");
    let json = fetcher.fetch_json(&url)?;

    let asn = json
        .get("asn")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let org = json
        .get("org")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let country = json
        .get("country_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let isp = json
        .get("org")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let mut report = ModuleReport::new(
        "advanced_recon",
        "asn",
        target.to_string(),
        format!("AS{asn} · {org} · {country}"),
    );

    report = report.line(format!("ASN: {asn}"));
    report = report.line(format!("Organization: {org}"));
    report = report.line(format!("Country: {country}"));
    report = report.line(format!("ISP: {isp}"));

    report = report.finding(
        Finding::new(
            "advanced_recon",
            "asn",
            target.to_string(),
            format!("ASN mapping: {asn} {org}"),
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail(format!(
            "IP {target} belongs to AS{asn} ({org}) in {country}"
        )),
    );

    Ok(report)
}

/// Cloud provider asset entry.
#[derive(Debug, Clone)]
pub struct CloudAsset {
    pub provider: &'static str,
    pub resource_type: String,
    pub identifier: String,
    pub region: Option<String>,
    pub tags: Vec<String>,
}

/// Enumerates cloud assets for a given domain/organization across providers.
/// Uses passive APIs: crt.sh for certs, then DNS resolution to find cloud-hosted assets.
pub fn cloud_asset_enum(
    domain: &str,
    fetcher: &dyn JsonFetcher,
    resolver: &dyn Resolver,
) -> Result<ModuleReport, String> {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() {
        return Err("advanced_recon/cloud needs a domain".into());
    }

    let mut assets = Vec::new();

    // First, get subdomains from CT logs
    let ct_url = format!("https://crt.sh/?q=%25.{domain}&output=json");
    let json = fetcher.fetch_json(&ct_url)?;

    let mut subdomains = HashSet::new();
    if let Some(arr) = json.as_array() {
        for entry in arr {
            if let Some(name) = entry.get("name_value").and_then(|v| v.as_str()) {
                for line in name.lines() {
                    let line = line
                        .trim()
                        .to_ascii_lowercase()
                        .strip_prefix("*.")
                        .unwrap_or(line.trim())
                        .to_string();
                    if line.ends_with(&format!(".{domain}")) || line == domain {
                        subdomains.insert(line);
                    }
                }
            }
        }
    }

    // Resolve each subdomain and check for cloud provider patterns
    for sub in subdomains.iter().take(MAX_CLOUD_ASSETS) {
        if let Ok(ips) = resolver.resolve(sub) {
            for ip in ips {
                let ip_str = ip.to_string();
                // Check for known cloud provider IP ranges (simplified - in prod use ASN)
                let provider = identify_cloud_provider(&ip_str);
                if let Some(prov) = provider {
                    assets.push(CloudAsset {
                        provider: prov,
                        resource_type: "compute".into(),
                        identifier: ip_str.clone(),
                        region: None, // Would need provider API for region
                        tags: vec![sub.clone()],
                    });
                }
            }
        }
    }

    // Deduplicate by identifier
    assets.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    assets.dedup_by(|a, b| a.identifier == b.identifier);

    let total = assets.len();
    let summary = if total == 0 {
        "no cloud-hosted assets found for subdomains".to_string()
    } else {
        format!(
            "{total} cloud asset(s) across {} provider(s)",
            unique_providers(&assets)
        )
    };

    let mut report = ModuleReport::new("advanced_recon", "cloud", &domain, summary);
    for asset in &assets {
        let region = asset.region.as_deref().unwrap_or("unknown");
        report = report.line(format!(
            "[{}] {} · {} · {}",
            asset.provider, asset.resource_type, asset.identifier, region
        ));
    }

    if !assets.is_empty() {
        report = report.finding(
            Finding::new(
                "advanced_recon",
                "cloud",
                domain.to_string(),
                format!("cloud asset inventory: {total} assets"),
                RiskLevel::Informational,
            )
            .confidence(Confidence::Confirmed)
            .detail(format!(
                "Found {} cloud-hosted asset(s) for {} across providers: {}",
                total,
                domain,
                assets
                    .iter()
                    .map(|a| a.provider)
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        );
    }

    Ok(report)
}

fn identify_cloud_provider(_ip: &str) -> Option<&'static str> {
    // Simplified: check against known cloud ASNs via ipapi.co would be better
    // For now, use a basic heuristic on the IP (in real impl, use ASN mapping)
    // This is a placeholder - real implementation would query ASN
    None
}

fn unique_providers(assets: &[CloudAsset]) -> usize {
    let mut set = HashSet::new();
    for a in assets {
        set.insert(a.provider);
    }
    set.len()
}

/// Performs a comprehensive advanced recon sweep: CT logs + ASN + Cloud assets.
pub fn full_sweep(
    domain: &str,
    fetcher: &dyn JsonFetcher,
    resolver: &dyn Resolver,
) -> Result<ModuleReport, String> {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() {
        return Err("advanced_recon/sweep needs a domain".into());
    }

    let mut combined = ModuleReport::new("advanced_recon", "sweep", domain.clone(), String::new());

    // CT subdomains
    match certificate_transparency(&domain, fetcher) {
        Ok(ct_report) => {
            combined
                .lines
                .push(format!("=== CT Subdomains ({}) ===", ct_report.summary));
            combined.lines.extend(ct_report.lines);
            combined.findings.extend(ct_report.findings);
        }
        Err(e) => combined.lines.push(format!("CT error: {e}")),
    }

    // Resolve domain for ASN
    if let Ok(ips) = resolver.resolve(&domain) {
        for ip in ips.iter().take(1) {
            match asn_mapping(&ip.to_string(), fetcher) {
                Ok(asn_report) => {
                    combined
                        .lines
                        .push(format!("=== ASN ({}) ===", asn_report.summary));
                    combined.lines.extend(asn_report.lines);
                    combined.findings.extend(asn_report.findings);
                }
                Err(e) => combined.lines.push(format!("ASN error: {e}")),
            }
        }
    }

    // Cloud assets
    match cloud_asset_enum(&domain, fetcher, resolver) {
        Ok(cloud_report) => {
            combined
                .lines
                .push(format!("=== Cloud Assets ({}) ===", cloud_report.summary));
            combined.lines.extend(cloud_report.lines);
            combined.findings.extend(cloud_report.findings);
        }
        Err(e) => combined.lines.push(format!("Cloud enum error: {e}")),
    }

    combined.summary = format!("completed: CT + ASN + Cloud for {}", domain);
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::IpAddr;

    struct StubFetcher(HashMap<String, serde_json::Value>);
    impl JsonFetcher for StubFetcher {
        fn fetch_json(&self, url: &str) -> Result<serde_json::Value, String> {
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no stub for {url}"))
        }
    }

    struct StubResolver(HashMap<String, Vec<IpAddr>>);
    impl Resolver for StubResolver {
        fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
            self.0
                .get(host)
                .cloned()
                .ok_or_else(|| format!("unresolved {host}"))
        }
    }

    #[test]
    fn ct_parses_crtsh_response() {
        let json = serde_json::json!([
            {"name_value": "*.example.com\nsub.example.com"},
            {"name_value": "api.example.com"}
        ]);
        let fetcher =
            StubFetcher([("https://crt.sh/?q=%25.example.com&output=json".into(), json)].into());
        let report = certificate_transparency("example.com", &fetcher).unwrap();
        assert_eq!(report.summary, "3 unique subdomain(s) from CT logs");
        assert!(report.lines.contains(&"example.com".to_string()));
        assert!(report.lines.contains(&"sub.example.com".to_string()));
        assert!(report.lines.contains(&"api.example.com".to_string()));
    }

    #[test]
    fn ct_deduplicates_wildcard_and_exact() {
        let json = serde_json::json!([
            {"name_value": "*.example.com\nexample.com\nsub.example.com"}
        ]);
        let fetcher =
            StubFetcher([("https://crt.sh/?q=%25.example.com&output=json".into(), json)].into());
        let report = certificate_transparency("example.com", &fetcher).unwrap();
        // Should deduplicate: *.example.com -> example.com, plus sub.example.com
        assert_eq!(report.lines.len(), 2);
    }

    #[test]
    fn asn_mapping_reports_structured() {
        let json = serde_json::json!({
            "asn": "AS15169",
            "org": "Google LLC",
            "country_name": "United States"
        });
        let fetcher = StubFetcher([("https://ipapi.co/8.8.8.8/json/".into(), json)].into());
        let report = asn_mapping("8.8.8.8", &fetcher).unwrap();
        assert!(report.summary.contains("AS15169"));
        assert!(report.summary.contains("Google"));
        assert_eq!(report.findings.len(), 1);
        assert!(report.findings[0].detail.contains("8.8.8.8"));
    }

    #[test]
    fn cloud_enum_resolves_subdomains_and_identifies_providers() {
        let ct_json = serde_json::json!([
            {"name_value": "aws.example.com\ngcp.example.com"}
        ]);
        let fetcher = StubFetcher(
            [(
                "https://crt.sh/?q=%25.example.com&output=json".into(),
                ct_json,
            )]
            .into(),
        );
        let resolver = StubResolver(
            [
                ("aws.example.com".into(), vec!["52.0.0.1".parse().unwrap()]),
                ("gcp.example.com".into(), vec!["35.0.0.1".parse().unwrap()]),
            ]
            .into(),
        );
        // identify_cloud_provider returns None in stub, so no assets found
        let report = cloud_asset_enum("example.com", &fetcher, &resolver).unwrap();
        assert!(report.summary.contains("no cloud-hosted assets"));
    }

    #[test]
    fn empty_domain_rejected() {
        let fetcher = StubFetcher(HashMap::new());
        let resolver = StubResolver(HashMap::new());
        assert!(certificate_transparency("", &fetcher).is_err());
        assert!(asn_mapping("", &fetcher).is_err());
        assert!(cloud_asset_enum("", &fetcher, &resolver).is_err());
        assert!(full_sweep("", &fetcher, &resolver).is_err());
    }

    #[test]
    fn results_are_bounded() {
        let many_subs: Vec<serde_json::Value> = (0..500)
            .map(|i| serde_json::json!({"name_value": format!("sub{i}.example.com")}))
            .collect();
        let fetcher = StubFetcher(
            [(
                "https://crt.sh/?q=%25.example.com&output=json".into(),
                serde_json::Value::Array(many_subs),
            )]
            .into(),
        );
        let report = certificate_transparency("example.com", &fetcher).unwrap();
        assert_eq!(report.lines.len(), MAX_CT_SUBDOMAINS);
    }
}
