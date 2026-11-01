//! Vulnerability analysis: version→CVE correlation.
//!
//! **A version match is never a confirmed vulnerability.** Every hit is
//! reported as `Suspected (version-only)`; confirmation is a later, separate
//! step. The engine takes a caller-provided database so this crate ships no
//! stale CVE data: an absent database means "nothing to correlate", not
//! "no vulnerabilities".

use serde::{Deserialize, Serialize};

use super::ModuleReport;
use crate::hacker::findings::Finding;
use crate::hacker::risk::RiskLevel;

/// Severity, coarse and CVSS-aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }
}

/// One database entry. Ranges are half-open: `[at_least, below)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnEntry {
    pub id: String,
    pub component: String,
    pub summary: String,
    #[serde(default)]
    pub severity: Option<Severity>,
    /// Inclusive lower bound as a dotted version (e.g. `"1.2.0"`).
    #[serde(default)]
    pub at_least: Option<String>,
    /// Exclusive upper bound (first fixed version).
    #[serde(default)]
    pub below: Option<String>,
}

/// Parses a dotted version, tolerating suffixes (`9.6p1`, `1.2.3-rc1`).
pub fn parse_version(raw: &str) -> Option<(u32, u32, u32)> {
    let core = raw
        .trim()
        .trim_start_matches('v')
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .next()
        .unwrap_or("");
    if core.is_empty() {
        return None;
    }
    let mut parts = core.split('.').map(|p| p.parse::<u32>().ok());
    let major = parts.next().flatten().unwrap_or(0);
    let minor = parts.next().flatten().unwrap_or(0);
    let patch = parts.next().flatten().unwrap_or(0);
    Some((major, minor, patch))
}

fn in_range(version: (u32, u32, u32), at_least: Option<&str>, below: Option<&str>) -> bool {
    if let Some(lo) = at_least.and_then(parse_version) {
        if version < lo {
            return false;
        }
    }
    if let Some(hi) = below.and_then(parse_version) {
        if version >= hi {
            return false;
        }
    }
    true
}

/// Correlates one component/version against the database. Unknown or
/// unparsable input returns an empty result rather than an error.
pub fn correlate(component: &str, version: &str, db: &[VulnEntry]) -> ModuleReport {
    let target = component.trim().to_ascii_lowercase();
    let parsed = parse_version(version);
    let mut hits: Vec<&VulnEntry> = Vec::new();
    if let Some(v) = parsed {
        for entry in db {
            if !entry.component.eq_ignore_ascii_case(&target) {
                continue;
            }
            if in_range(v, entry.at_least.as_deref(), entry.below.as_deref()) {
                hits.push(entry);
            }
        }
    }

    let summary = match parsed {
        None => format!("{component} {version}: unparsable version"),
        Some(_) if db.is_empty() => "no vulnerability database configured".to_string(),
        Some(_) if hits.is_empty() => format!("{component} {version}: no matching entries"),
        Some(_) => format!("{} suspected (version-only, unconfirmed)", hits.len()),
    };
    let mut report = ModuleReport::new(
        "vuln",
        "correlate",
        format!("{component} {version}"),
        summary,
    );
    for entry in &hits {
        let sev = entry.severity.unwrap_or(Severity::Medium).label();
        report = report.line(format!(
            "{} [{sev}] {} - Suspected (version-only; validate before reporting)",
            entry.id, entry.summary
        ));
    }
    // One structured finding per hit. Confidence stays `Suspected`: a version
    // match is never a confirmed vulnerability.
    for entry in &hits {
        report = report.finding(
            Finding::new(
                "vuln",
                "correlate",
                format!("{component} {version}"),
                format!("{} in {}", entry.id, entry.component),
                severity_risk(entry.severity),
            )
            .detail(format!(
                "{} - Suspected (version-only; validate before reporting)",
                entry.summary
            )),
        );
    }
    report
}

/// Maps the CVE database severity onto the shared ascending risk scale.
fn severity_risk(severity: Option<Severity>) -> RiskLevel {
    match severity {
        Some(Severity::Low) => RiskLevel::Low,
        Some(Severity::High) => RiskLevel::High,
        Some(Severity::Critical) => RiskLevel::Critical,
        Some(Severity::Medium) | None => RiskLevel::Medium,
    }
}

/// Loads a JSON array of entries from `path`.
pub fn load_db(path: &std::path::Path) -> Result<Vec<VulnEntry>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Vec<VulnEntry> {
        vec![VulnEntry {
            id: "CVE-0000-0001".into(),
            component: "widget".into(),
            summary: "buffer overflow in widget".into(),
            severity: Some(Severity::High),
            at_least: Some("1.0.0".into()),
            below: Some("1.2.5".into()),
        }]
    }

    #[test]
    fn version_parsing_tolerates_suffixes() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v9.6p1"), Some((9, 6, 0)));
        assert_eq!(parse_version("2.4"), Some((2, 4, 0)));
        assert_eq!(parse_version("nonsense"), None);
    }

    #[test]
    fn matches_only_inside_the_range() {
        let report = correlate("widget", "1.2.4", &db());
        assert!(report.summary.contains("suspected"));
        assert!(report.lines[0].contains("CVE-0000-0001"));
        // Confirmed language must never appear.
        assert!(report.lines[0].contains("Suspected"));
        assert!(!report.lines[0].to_lowercase().contains("confirmed"));

        // At the fixed version: no match.
        let fixed = correlate("widget", "1.2.5", &db());
        assert!(fixed.summary.contains("no matching entries"));

        // Below the floor: no match.
        let old = correlate("widget", "0.9.0", &db());
        assert!(old.summary.contains("no matching entries"));
    }

    #[test]
    fn empty_db_reports_no_database_not_no_vulns() {
        let report = correlate("widget", "1.2.4", &[]);
        assert!(report.summary.contains("no vulnerability database"));
        assert!(report.lines.is_empty());
    }

    #[test]
    fn unknown_component_and_bad_version_are_handled() {
        assert!(correlate("other", "1.2.4", &db())
            .summary
            .contains("no matching"));
        assert!(correlate("widget", "wat", &db())
            .summary
            .contains("unparsable"));
    }

    #[test]
    fn db_round_trips_json() {
        let dir = std::env::temp_dir().join(format!("nanomite-vulndb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("db.json");
        std::fs::write(
            &path,
            r#"[{"id":"CVE-1","component":"nginx","summary":"x","severity":"high","below":"1.25.0"}]"#,
        )
        .unwrap();
        let loaded = load_db(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].component, "nginx");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
