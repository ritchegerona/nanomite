//! C2/Implant Simulation: Beacon profiling, traffic analysis, detection engineering.
//!
//! This module provides frameworks for simulating and detecting C2 traffic patterns.
//! All content is DETECTION-FOCUSED - no functional implants or C2 servers.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// std-only pseudo-random source for rule IDs and jitter (not security-sensitive).
fn pseudo_random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

/// Max beacon profiles to analyze.
pub const MAX_BEACON_PROFILES: usize = 50;
/// Max traffic samples per profile.
pub const MAX_TRAFFIC_SAMPLES: usize = 1000;

/// C2 beacon profile (detection-focused).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BeaconProfile {
    pub name: String,
    pub c2_framework: String, // Cobalt Strike, Sliver, Mythic, Brute Ratel, Havoc, custom
    pub protocol: String,     // HTTP, HTTPS, DNS, TCP, UDP, SMB, named pipe
    pub profile_config: BeaconConfig,
    pub indicators: BeaconIndicators,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BeaconConfig {
    pub sleep_time: u32,      // milliseconds
    pub jitter: u8,           // percentage
    pub max_dns: Option<u32>, // for DNS beacons
    pub user_agent: String,
    pub headers: HashMap<String, String>,
    pub uri_paths: Vec<String>,
    pub port: u16,
    pub ssl: bool,
    pub proxy_aware: bool,
    pub kill_date: Option<u64>,
    pub watermark: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BeaconIndicators {
    pub ja3_fingerprint: Option<String>,
    pub hassh_fingerprint: Option<String>,
    pub default_headers: Vec<String>,
    pub uri_patterns: Vec<String>,
    pub timing_pattern: String,     // "regular", "jittered", "human-like"
    pub byte_patterns: Vec<String>, // magic bytes, XOR keys, etc.
    pub dns_query_types: Vec<String>, // for DNS beacons
    pub unique_artifacts: Vec<String>, // specific to framework
}

/// Pre-defined beacon profiles for major C2 frameworks.
pub fn builtin_beacon_profiles() -> Vec<BeaconProfile> {
    vec![
        BeaconProfile {
            name: "Cobalt Strike HTTP (default)".into(),
            c2_framework: "Cobalt Strike".into(),
            protocol: "HTTP".into(),
            profile_config: BeaconConfig {
                sleep_time: 60000,
                jitter: 37,
                max_dns: None,
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".into(),
                headers: [
                    ("Accept".into(), "*/*".into()),
                    ("Accept-Language".into(), "en-US,en;q=0.9".into()),
                ].into(),
                uri_paths: vec!["/submit.php".into(), "/api/".into()],
                port: 80,
                ssl: false,
                proxy_aware: true,
                kill_date: None,
                watermark: Some(0x12345678),
            },
            indicators: BeaconIndicators {
                ja3_fingerprint: Some("771,4865-4867-4866-49195-49199-52393-52392-49196-49200-49162-49161-49171-49172-156-157-47-53,0-23-65281-10-11-35-16-5-13-18-51-45-43-27-21,29-23-24-25-256-257".into()),
                hassh_fingerprint: None,
                default_headers: vec!["Accept: */*".into(), "Accept-Language: en-US,en;q=0.9".into()],
                uri_patterns: vec!["/submit.php".into(), "/api/".into()],
                timing_pattern: "jittered".into(),
                byte_patterns: vec!["0x12345678 watermark".into(), "XOR key in metadata".into()],
                dns_query_types: vec![],
                unique_artifacts: vec!["stage0 .text section".into(), "Reflective loader".into(), "Malleable profile artifacts".into()],
            },
        },
        BeaconProfile {
            name: "Cobalt Strike HTTPS (Malleable)".into(),
            c2_framework: "Cobalt Strike".into(),
            protocol: "HTTPS".into(),
            profile_config: BeaconConfig {
                sleep_time: 60000,
                jitter: 37,
                max_dns: None,
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".into(),
                headers: [
                    ("Accept".into(), "text/html,application/xhtml+xml".into()),
                    ("Accept-Encoding".into(), "gzip, deflate".into()),
                ].into(),
                uri_paths: vec!["/search".into(), "/login".into(), "/dashboard".into()],
                port: 443,
                ssl: true,
                proxy_aware: true,
                kill_date: None,
                watermark: None,
            },
            indicators: BeaconIndicators {
                ja3_fingerprint: Some("771,4865-4867-4866-49195-49199-52393-52392-49196-49200-49162-49161-49171-49172-156-157-47-53,0-23-65281-10-11-35-16-5-13-18-51-45-43-27-21,29-23-24-25-256-257".into()),
                hassh_fingerprint: None,
                default_headers: vec!["Accept: text/html,application/xhtml+xml".into()],
                uri_patterns: vec!["/search".into(), "/login".into()],
                timing_pattern: "jittered".into(),
                byte_patterns: vec!["Malleable profile dependent".into()],
                dns_query_types: vec![],
                unique_artifacts: vec!["Custom SSL cert".into(), "Malleable profile transforms".into()],
            },
        },
        BeaconProfile {
            name: "Cobalt Strike DNS".into(),
            c2_framework: "Cobalt Strike".into(),
            protocol: "DNS".into(),
            profile_config: BeaconConfig {
                sleep_time: 60000,
                jitter: 37,
                max_dns: Some(255),
                user_agent: "".into(),
                headers: HashMap::new(),
                uri_paths: vec![],
                port: 53,
                ssl: false,
                proxy_aware: false,
                kill_date: None,
                watermark: None,
            },
            indicators: BeaconIndicators {
                ja3_fingerprint: None,
                hassh_fingerprint: None,
                default_headers: vec![],
                uri_patterns: vec![],
                timing_pattern: "jittered".into(),
                byte_patterns: vec!["DNS TXT/AAAA/A records with encoded data".into()],
                dns_query_types: vec!["TXT".into(), "AAAA".into(), "A".into(), "MX".into()],
                unique_artifacts: vec!["DNS beacon encoding".into(), "Subdomain brute patterns".into()],
            },
        },
        BeaconProfile {
            name: "Sliver HTTP".into(),
            c2_framework: "Sliver".into(),
            protocol: "HTTP".into(),
            profile_config: BeaconConfig {
                sleep_time: 60000,
                jitter: 20,
                max_dns: None,
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".into(),
                headers: HashMap::new(),
                uri_paths: vec!["/".into(), "/api/health".into()],
                port: 80,
                ssl: false,
                proxy_aware: true,
                kill_date: None,
                watermark: None,
            },
            indicators: BeaconIndicators {
                ja3_fingerprint: Some("771,4865-4867-4866-49195-49199-52393-52392-49196-49200-49162-49161-49171-49172-156-157-47-53,0-23-65281-10-11-35-16-5-13-18-51-45-43-27-21,29-23-24-25-256-257".into()),
                hassh_fingerprint: None,
                default_headers: vec![],
                uri_patterns: vec!["/".into(), "/api/health".into()],
                timing_pattern: "jittered".into(),
                byte_patterns: vec!["Mutual TLS (mTLS)".into(), "WireGuard-style noise".into()],
                dns_query_types: vec![],
                unique_artifacts: vec!["mTLS by default".into(), "ARM64 support".into(), "gRPC/HTTP2".into()],
            },
        },
        BeaconProfile {
            name: "Sliver HTTPS/mTLS".into(),
            c2_framework: "Sliver".into(),
            protocol: "HTTPS".into(),
            profile_config: BeaconConfig {
                sleep_time: 60000,
                jitter: 20,
                max_dns: None,
                user_agent: "".into(),
                headers: HashMap::new(),
                uri_paths: vec![],
                port: 443,
                ssl: true,
                proxy_aware: true,
                kill_date: None,
                watermark: None,
            },
            indicators: BeaconIndicators {
                ja3_fingerprint: Some("771,4865-4867-4866-49195-49199-52393-52392-49196-49200-49162-49161-49171-49172-156-157-47-53,0-23-65281-10-11-35-16-5-13-18-51-45-43-27-21,29-23-24-25-256-257".into()),
                hassh_fingerprint: Some("hassh fingerprint for Sliver mTLS".into()),
                default_headers: vec![],
                uri_patterns: vec![],
                timing_pattern: "jittered".into(),
                byte_patterns: vec!["mTLS cert verification".into()],
                dns_query_types: vec![],
                unique_artifacts: vec!["Mutual TLS required".into(), "Certificate pinning".into()],
            },
        },
        BeaconProfile {
            name: "Mythic HTTP (Apfell)".into(),
            c2_framework: "Mythic".into(),
            protocol: "HTTP".into(),
            profile_config: BeaconConfig {
                sleep_time: 60000,
                jitter: 10,
                max_dns: None,
                user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)".into(),
                headers: HashMap::new(),
                uri_paths: vec!["/api/v1.4/".into()],
                port: 80,
                ssl: false,
                proxy_aware: true,
                kill_date: None,
                watermark: None,
            },
            indicators: BeaconIndicators {
                ja3_fingerprint: None,
                hassh_fingerprint: None,
                default_headers: vec![],
                uri_patterns: vec!["/api/v1.4/".into()],
                timing_pattern: "regular".into(),
                byte_patterns: vec!["JSON-based C2".into(), "Agent UUID in headers".into()],
                dns_query_types: vec![],
                unique_artifacts: vec!["Container-based agents".into(), "GraphQL-like API".into()],
            },
        },
        BeaconProfile {
            name: "Brute Ratel HTTPS".into(),
            c2_framework: "Brute Ratel".into(),
            protocol: "HTTPS".into(),
            profile_config: BeaconConfig {
                sleep_time: 60000,
                jitter: 20,
                max_dns: None,
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)".into(),
                headers: [
                    ("Accept".into(), "*/*".into()),
                ].into(),
                uri_paths: vec!["/api/".into()],
                port: 443,
                ssl: true,
                proxy_aware: true,
                kill_date: None,
                watermark: None,
            },
            indicators: BeaconIndicators {
                ja3_fingerprint: None,
                hassh_fingerprint: None,
                default_headers: vec!["Accept: */*".into()],
                uri_patterns: vec!["/api/".into()],
                timing_pattern: "jittered".into(),
                byte_patterns: vec!["Custom encryption".into(), "No reflective loader".into()],
                dns_query_types: vec![],
                unique_artifacts: vec!["Badger (C# implant)".into(), "Custom crypto".into(), "ETW patching".into()],
            },
        },
        BeaconProfile {
            name: "Havoc HTTPS".into(),
            c2_framework: "Havoc".into(),
            protocol: "HTTPS".into(),
            profile_config: BeaconConfig {
                sleep_time: 60000,
                jitter: 30,
                max_dns: None,
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)".into(),
                headers: HashMap::new(),
                uri_paths: vec!["/".into()],
                port: 443,
                ssl: true,
                proxy_aware: true,
                kill_date: None,
                watermark: None,
            },
            indicators: BeaconIndicators {
                ja3_fingerprint: None,
                hassh_fingerprint: None,
                default_headers: vec![],
                uri_patterns: vec!["/".into()],
                timing_pattern: "jittered".into(),
                byte_patterns: vec!["Golang implant".into(), "Custom protocol".into()],
                dns_query_types: vec![],
                unique_artifacts: vec!["Demon (Golang implant)".into(), "Qt-based UI".into()],
            },
        },
        BeaconProfile {
            name: "PoshC2 (PowerShell)".into(),
            c2_framework: "PoshC2".into(),
            protocol: "HTTPS".into(),
            profile_config: BeaconConfig {
                sleep_time: 60000,
                jitter: 20,
                max_dns: None,
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)".into(),
                headers: [
                    ("Authorization".into(), "Bearer <token>".into()),
                ].into(),
                uri_paths: vec!["/api/".into()],
                port: 443,
                ssl: true,
                proxy_aware: true,
                kill_date: None,
                watermark: None,
            },
            indicators: BeaconIndicators {
                ja3_fingerprint: None,
                hassh_fingerprint: None,
                default_headers: vec!["Authorization: Bearer".into()],
                uri_patterns: vec!["/api/".into()],
                timing_pattern: "regular".into(),
                byte_patterns: vec!["PowerShell agent".into(), "AMSI bypass artifacts".into()],
                dns_query_types: vec![],
                unique_artifacts: vec!["PowerShell-based".into(), "SharpSploit integration".into()],
            },
        },
    ]
}

/// Analyzes network traffic for beacon-like patterns.
#[derive(Debug, Clone)]
pub struct TrafficSample {
    pub timestamp: u64, // Unix epoch ms
    pub src_ip: String,
    pub dst_ip: String,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: String, // TCP, UDP, HTTP, HTTPS, DNS
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub payload_hash: Option<String>, // SHA256 of first N bytes
    pub ja3: Option<String>,
    pub sni: Option<String>,
    pub http_host: Option<String>,
    pub http_uri: Option<String>,
    pub http_user_agent: Option<String>,
    pub dns_query: Option<String>,
    pub dns_qtype: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BeaconAnalysisResult {
    pub suspected_beacons: Vec<SuspectedBeacon>,
    pub statistics: TrafficStatistics,
}

#[derive(Debug, Clone)]
pub struct SuspectedBeacon {
    pub src_ip: String,
    pub dst_ip: String,
    pub dst_port: u16,
    pub profile_match: Option<String>, // Matched profile name
    pub confidence: f32,               // 0.0 - 1.0
    pub reasons: Vec<String>,
    pub sample_count: usize,
    pub avg_interval: f64,
    pub jitter_detected: bool,
    pub first_seen: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone)]
pub struct TrafficStatistics {
    pub total_connections: usize,
    pub unique_src_ips: HashMap<String, usize>,
    pub unique_dst_ips: HashMap<String, usize>,
    pub protocols: HashMap<String, usize>,
    pub ports: HashMap<u16, usize>,
    pub time_span: Duration,
}

/// Analyzes traffic samples for C2 beacon patterns.
pub fn analyze_beacon_traffic(
    samples: &[TrafficSample],
    profiles: &[BeaconProfile],
) -> BeaconAnalysisResult {
    let mut stats = TrafficStatistics {
        total_connections: samples.len(),
        unique_src_ips: HashMap::new(),
        unique_dst_ips: HashMap::new(),
        protocols: HashMap::new(),
        ports: HashMap::new(),
        time_span: Duration::from_secs(0),
    };

    if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
        stats.time_span = Duration::from_millis(last.timestamp.saturating_sub(first.timestamp));
    }

    for s in samples {
        *stats.unique_src_ips.entry(s.src_ip.clone()).or_default() += 1;
        *stats.unique_dst_ips.entry(s.dst_ip.clone()).or_default() += 1;
        *stats.protocols.entry(s.protocol.clone()).or_default() += 1;
        *stats.ports.entry(s.dst_port).or_default() += 1;
    }

    // Group by (src_ip, dst_ip, dst_port) for beacon detection
    let mut groups: HashMap<(String, String, u16), Vec<&TrafficSample>> = HashMap::new();
    for s in samples {
        groups
            .entry((s.src_ip.clone(), s.dst_ip.clone(), s.dst_port))
            .or_default()
            .push(s);
    }

    let mut suspected = Vec::new();

    for ((src, dst, port), group_samples) in groups {
        if group_samples.len() < 3 {
            continue; // Need at least 3 samples for pattern
        }

        let mut reasons = Vec::new();
        let mut confidence: f32 = 0.0;
        let mut profile_match = None;
        let mut jitter_detected = false;

        // Check timing regularity
        let intervals: Vec<u64> = group_samples
            .windows(2)
            .map(|w| w[1].timestamp.saturating_sub(w[0].timestamp))
            .collect();

        let avg_interval = intervals.iter().sum::<u64>() as f64 / intervals.len() as f64;
        let variance = intervals
            .iter()
            .map(|i| (*i as f64 - avg_interval).powi(2))
            .sum::<f64>()
            / intervals.len() as f64;
        let std_dev = variance.sqrt();
        let cv = if avg_interval > 0.0 {
            std_dev / avg_interval
        } else {
            0.0
        };

        // Low coefficient of variation = regular beaconing
        if cv < 0.1 && avg_interval > 1000.0 {
            reasons.push(format!(
                "Regular interval: {:.0}ms (CV={:.2})",
                avg_interval, cv
            ));
            confidence += 0.3;
        } else if cv < 0.5 && avg_interval > 1000.0 {
            reasons.push(format!(
                "Jittered interval: {:.0}ms ±{:.0}ms (CV={:.2})",
                avg_interval, std_dev, cv
            ));
            confidence += 0.2;
            jitter_detected = true;
        }

        // Check for consistent payload sizes
        let sent_sizes: Vec<u64> = group_samples.iter().map(|s| s.bytes_sent).collect();
        let sent_cv = coefficient_of_variation(&sent_sizes);

        if sent_cv < 0.1 && sent_sizes.iter().all(|&s| s > 0) {
            reasons.push(format!("Consistent outbound size: {} bytes", sent_sizes[0]));
            confidence += 0.2;
        }

        // JA3 matching
        if let Some(ja3) = group_samples[0].ja3.as_ref() {
            for profile in profiles {
                if profile.indicators.ja3_fingerprint.as_ref() == Some(ja3) {
                    profile_match = Some(profile.name.clone());
                    reasons.push(format!("JA3 matches {}", profile.name));
                    confidence += 0.4;
                    break;
                }
            }
        }

        // HTTP URI matching
        if let Some(uri) = group_samples[0].http_uri.as_ref() {
            for profile in profiles {
                if profile
                    .indicators
                    .uri_patterns
                    .iter()
                    .any(|p| uri.contains(p))
                {
                    if profile_match.is_none() {
                        profile_match = Some(profile.name.clone());
                    }
                    reasons.push(format!("URI matches {}", profile.name));
                    confidence += 0.2;
                }
            }
        }

        // User-Agent matching
        if let Some(ua) = group_samples[0].http_user_agent.as_ref() {
            for profile in profiles {
                if profile.profile_config.user_agent == *ua {
                    reasons.push(format!("User-Agent matches {}", profile.name));
                    confidence += 0.1;
                }
            }
        }

        // DNS beacon detection
        if port == 53 && group_samples[0].dns_query.is_some() {
            reasons.push("DNS beacon candidate".into());
            confidence += 0.3;
            for profile in profiles {
                if profile.protocol == "DNS" {
                    profile_match = Some(profile.name.clone());
                }
            }
        }

        // High frequency to single destination
        if group_samples.len() > 100 && group_samples.iter().all(|s| s.dst_ip == dst) {
            reasons.push(format!(
                "High frequency to single destination ({} connections)",
                group_samples.len()
            ));
            confidence += 0.2;
        }

        confidence = confidence.min(1.0);

        if confidence > 0.3 {
            suspected.push(SuspectedBeacon {
                src_ip: src,
                dst_ip: dst,
                dst_port: port,
                profile_match,
                confidence,
                reasons,
                sample_count: group_samples.len(),
                avg_interval,
                jitter_detected,
                first_seen: group_samples[0].timestamp,
                last_seen: group_samples.last().unwrap().timestamp,
            });
        }
    }

    suspected.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

    BeaconAnalysisResult {
        suspected_beacons: suspected,
        statistics: stats,
    }
}

fn coefficient_of_variation(values: &[u64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<u64>() as f64 / values.len() as f64;
    if mean == 0.0 {
        return 0.0;
    }
    let variance = values
        .iter()
        .map(|v| (*v as f64 - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt() / mean
}

/// Generates detection rules (Sigma, Suricata, Zeek) for a beacon profile.
pub fn generate_detection_rules(profile: &BeaconProfile) -> ModuleReport {
    let mut report = ModuleReport::new(
        "c2_simulation",
        "detection-rules",
        &profile.name,
        format!("detection rules for {}", profile.name),
    );

    report = report.line("=== Sigma Rule (Windows Event Logs) ===");
    report = report.line(format!("title: {} Beacon Detection", profile.c2_framework));
    report = report.line("id: {generated-uuid}");
    report = report.line("status: experimental");
    report = report.line("author: nanomite");
    report = report.line("date: 2024/01/01");
    report = report.line("logsource:");
    report = report.line("  category: network_connection");
    report = report.line("  product: windows");
    report = report.line("detection:");
    report = report.line("  selection:");
    report = report.line(format!(
        "    DestinationPort: {}",
        profile.profile_config.port
    ));
    if profile.profile_config.ssl {
        report = report.line("    Protocol: TLS");
    }
    if let Some(ja3) = &profile.indicators.ja3_fingerprint {
        report = report.line(format!("    JA3: '{}'", ja3));
    }
    if !profile.profile_config.user_agent.is_empty() {
        report = report.line(format!(
            "    UserAgent: '{}'",
            profile.profile_config.user_agent
        ));
    }
    report = report.line("  condition: selection");
    report = report.line("level: medium");
    report = report.line("tags:");
    report = report.line("  - attack.command_and_control");
    report = report.line("  - attack.t1071");

    report = report.line("");
    report = report.line("=== Suricata Rule ===");
    let sid = format!("100000{}", pseudo_random_u64() % 90000 + 10000);
    let mut rule = format!(
        "alert http {} any -> any {}",
        if profile.profile_config.ssl {
            "tls"
        } else {
            "tcp"
        },
        profile.profile_config.port
    );
    rule.push_str(&format!(" (msg:\"{} Beacon\"; ", profile.c2_framework));
    rule.push_str(&format!("sid:{}; rev:1; ", sid));
    if !profile.profile_config.user_agent.is_empty() {
        rule.push_str(&format!(
            "content:\"{}\"; http_user_agent; ",
            escape_suricata(&profile.profile_config.user_agent)
        ));
    }
    for uri in &profile.profile_config.uri_paths {
        rule.push_str(&format!("content:\"{}\"; http_uri; ", escape_suricata(uri)));
    }
    rule.push_str("classtype:trojan-activity; metadata:attack.t1071;)");
    report = report.line(rule);

    report = report.line("");
    report = report.line("=== Zeek Script (Intel Framework) ===");
    report = report.line("@load base/frameworks/intel");
    for uri in &profile.profile_config.uri_paths {
        report = report.line(format!("Intel::create_intel([$indicator=\"{}\", $type=Intel::URL, $meta=[$source=\"nanomite\", $desc=\"{} beacon URI\"]]);", uri, profile.c2_framework));
    }
    if let Some(ja3) = &profile.indicators.ja3_fingerprint {
        report = report.line(format!("Intel::create_intel([$indicator=\"{}\", $type=Intel::SOFTWARE, $meta=[$source=\"nanomite\", $desc=\"{} JA3\"]]);", ja3, profile.c2_framework));
    }

    report = report.line("");
    report = report.line("=== Elastic/Winlogbeat Query ===");
    report = report.line(format!(
        "destination.port:{} AND {}",
        profile.profile_config.port,
        if profile.profile_config.ssl {
            "network.transport:tls"
        } else {
            "network.transport:tcp"
        }
    ));
    if let Some(ja3) = &profile.indicators.ja3_fingerprint {
        report = report.line(format!("AND tls.ja3:\"{}\"", ja3));
    }

    report = report.finding(
        Finding::new(
            "c2_simulation",
            "detection-rules",
            &profile.name,
            format!("generated detection rules for {}", profile.name),
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail("Rules are templates - tune for environment before deployment"),
    );

    report
}

fn escape_suricata(s: &str) -> String {
    s.replace('"', "\\\"")
        .replace(';', "\\;")
        .replace(':', "\\:")
}

/// Traffic generation simulation (for testing detection rules only).
/// Returns a description of what traffic would look like - NO ACTUAL TRAFFIC GENERATED.
pub fn simulate_beacon_traffic(profile: &BeaconProfile, count: usize) -> ModuleReport {
    let count = count.min(MAX_TRAFFIC_SAMPLES);
    let mut report = ModuleReport::new(
        "c2_simulation",
        "traffic-sim",
        &profile.name,
        format!("simulated {} beacon requests (description only)", count),
    );

    report = report.line("=== Simulation Parameters ===");
    report = report.line(format!("Framework: {}", profile.c2_framework));
    report = report.line(format!("Protocol: {}", profile.protocol));
    report = report.line(format!(
        "Sleep: {}ms ± {}%",
        profile.profile_config.sleep_time, profile.profile_config.jitter
    ));
    report = report.line(format!("Port: {}", profile.profile_config.port));
    report = report.line(format!("SSL: {}", profile.profile_config.ssl));
    report = report.line(format!("User-Agent: {}", profile.profile_config.user_agent));

    report = report.line("");
    report = report.line("=== Expected Traffic Pattern ===");
    for i in 0..count.min(5) {
        let jitter = ((pseudo_random_u64() as f64 / u64::MAX as f64) - 0.5)
            * 2.0
            * (profile.profile_config.jitter as f64 / 100.0);
        let interval = profile.profile_config.sleep_time as f64 * (1.0 + jitter);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + (i as u64 * interval as u64);
        let uri = &profile.profile_config.uri_paths[i % profile.profile_config.uri_paths.len()];
        report = report.line(format!(
            "[{}] {}:{} -> {}:{} {} {} UA={}",
            timestamp,
            "10.0.0.100",
            49152 + i,
            "192.168.1.50",
            profile.profile_config.port,
            if profile.profile_config.ssl {
                "HTTPS"
            } else {
                "HTTP"
            },
            uri,
            profile
                .profile_config
                .user_agent
                .chars()
                .take(30)
                .collect::<String>()
        ));
    }
    if count > 5 {
        report = report.line(format!("... and {} more requests", count - 5));
    }

    report = report.line("");
    report = report.line("=== Network Indicators ===");
    if let Some(ja3) = &profile.indicators.ja3_fingerprint {
        report = report.line(format!("JA3: {}", ja3));
    }
    if let Some(hassh) = &profile.indicators.hassh_fingerprint {
        report = report.line(format!("HASSH: {}", hassh));
    }
    for artifact in &profile.indicators.unique_artifacts {
        report = report.line(format!("Artifact: {}", artifact));
    }

    report = report.finding(
        Finding::new(
            "c2_simulation",
            "traffic-sim",
            &profile.name,
            "beacon traffic simulation description (no actual traffic)",
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail("Use to validate detection rules against expected patterns"),
    );

    report
}

/// Comprehensive C2 simulation analysis.
pub fn full_analysis() -> ModuleReport {
    let mut combined = ModuleReport::new("c2_simulation", "sweep", "catalog", String::new());

    let profiles = builtin_beacon_profiles();
    for profile in &profiles {
        let rules = generate_detection_rules(profile);
        combined.lines.push(format!(
            "=== {} ({}) ===",
            profile.name, profile.c2_framework
        ));
        combined.lines.extend(rules.lines);
        combined.findings.extend(rules.findings);
    }

    combined.summary = format!(
        "generated detection rules for {} beacon profiles",
        profiles.len()
    );
    combined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_have_entries() {
        let profiles = builtin_beacon_profiles();
        assert!(!profiles.is_empty());
        assert!(profiles.iter().any(|p| p.c2_framework == "Cobalt Strike"));
        assert!(profiles.iter().any(|p| p.c2_framework == "Sliver"));
        assert!(profiles.iter().any(|p| p.c2_framework == "Mythic"));
        assert!(profiles.iter().any(|p| p.protocol == "DNS"));
    }

    #[test]
    fn analyze_beacon_traffic_detects_regular() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let samples = (0..10).map(|i| TrafficSample {
            timestamp: now + i * 60000,
            src_ip: "10.0.0.100".into(),
            dst_ip: "192.168.1.50".into(),
            src_port: 49152,
            dst_port: 443,
            protocol: "HTTPS".into(),
            bytes_sent: 200,
            bytes_received: 500,
            payload_hash: None,
            ja3: Some("771,4865-4867-4866-49195-49199-52393-52392-49196-49200-49162-49161-49171-49172-156-157-47-53,0-23-65281-10-11-35-16-5-13-18-51-45-43-27-21,29-23-24-25-256-257".into()),
            sni: Some("c2.example.com".into()),
            http_host: Some("c2.example.com".into()),
            http_uri: Some("/submit.php".into()),
            http_user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".into()),
            dns_query: None,
            dns_qtype: None,
        }).collect::<Vec<_>>();

        let profiles = builtin_beacon_profiles();
        let result = analyze_beacon_traffic(&samples, &profiles);
        assert!(!result.suspected_beacons.is_empty());
        assert!(result.suspected_beacons[0].confidence > 0.5);
        assert!(result.suspected_beacons[0].profile_match.is_some());
    }

    #[test]
    fn analyze_beacon_traffic_detects_jittered() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let samples = (0..10)
            .map(|i| TrafficSample {
                // Alternating 80s/40s gaps: mean 60s, CV ~0.33 -> jittered beacon.
                timestamp: now + i * 60000 + if i % 2 == 0 { 0 } else { 20000 },
                src_ip: "10.0.0.100".into(),
                dst_ip: "192.168.1.50".into(),
                src_port: 49152,
                dst_port: 443,
                protocol: "HTTPS".into(),
                bytes_sent: 200,
                bytes_received: 500,
                payload_hash: None,
                ja3: None,
                sni: None,
                http_host: None,
                http_uri: Some("/submit.php".into()),
                http_user_agent: Some(
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".into(),
                ),
                dns_query: None,
                dns_qtype: None,
            })
            .collect::<Vec<_>>();

        let profiles = builtin_beacon_profiles();
        let result = analyze_beacon_traffic(&samples, &profiles);
        assert!(!result.suspected_beacons.is_empty());
        assert!(result.suspected_beacons[0].jitter_detected);
    }

    #[test]
    fn generate_detection_rules_creates_sigma() {
        let profiles = builtin_beacon_profiles();
        let profile = &profiles[0];
        let report = generate_detection_rules(profile);
        assert!(report.lines.iter().any(|l| l.contains("Sigma Rule")));
        assert!(report.lines.iter().any(|l| l.contains("Suricata Rule")));
        assert!(report.lines.iter().any(|l| l.contains("Zeek Script")));
    }

    #[test]
    fn simulate_traffic_describes_pattern() {
        let profiles = builtin_beacon_profiles();
        let profile = &profiles[0];
        let report = simulate_beacon_traffic(profile, 3);
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("Simulation Parameters")));
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("Expected Traffic Pattern")));
    }
}
