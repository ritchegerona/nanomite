//! Advanced Supply Chain: SBOM analysis, dependency confusion, repo jacking detection.
//!
//! All operations are passive/local-first and bounded.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// Max dependencies to analyze.
pub const MAX_DEPENDENCIES: usize = 1000;
/// Max SBOM size.
pub const MAX_SBOM_SIZE: usize = 10_000_000;
/// HTTP timeout.
pub const SUPPLY_CHAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// SBOM document (CycloneDX / SPDX subset).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum SbomDocument {
    CycloneDx(CycloneDxSbom),
    Spdx(SpdxSbom),
    Generic(serde_json::Value),
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CycloneDxSbom {
    #[serde(rename = "bomFormat")]
    pub bom_format: Option<String>,
    #[serde(rename = "specVersion")]
    pub spec_version: Option<String>,
    pub components: Option<Vec<CycloneDxComponent>>,
    pub dependencies: Option<Vec<CycloneDxDependency>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CycloneDxComponent {
    #[serde(rename = "type")]
    pub component_type: Option<String>,
    pub name: String,
    pub version: Option<String>,
    #[serde(rename = "group")]
    pub group: Option<String>,
    pub purl: Option<String>, // Package URL
    pub licenses: Option<Vec<CycloneDxLicense>>,
    pub hashes: Option<Vec<CycloneDxHash>>,
    #[serde(rename = "externalReferences")]
    pub external_references: Option<Vec<CycloneDxExternalRef>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CycloneDxDependency {
    #[serde(rename = "ref")]
    pub ref_: String,
    #[serde(rename = "dependsOn")]
    pub depends_on: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CycloneDxLicense {
    pub license: CycloneDxLicenseInner,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CycloneDxLicenseInner {
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CycloneDxHash {
    pub alg: String,
    pub content: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CycloneDxExternalRef {
    #[serde(rename = "type")]
    pub ref_type: String,
    pub url: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SpdxSbom {
    #[serde(rename = "spdxVersion")]
    pub spdx_version: Option<String>,
    pub packages: Option<Vec<SpdxPackage>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SpdxPackage {
    #[serde(rename = "SPDXID")]
    pub spdx_id: Option<String>,
    pub name: String,
    #[serde(rename = "versionInfo")]
    pub version_info: Option<String>,
    #[serde(rename = "downloadLocation")]
    pub download_location: Option<String>,
    #[serde(rename = "licenseConcluded")]
    pub license_concluded: Option<String>,
    #[serde(rename = "licenseDeclared")]
    pub license_declared: Option<String>,
    #[serde(rename = "externalRefs")]
    pub external_refs: Option<Vec<SpdxExternalRef>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SpdxExternalRef {
    #[serde(rename = "referenceCategory")]
    pub reference_category: String,
    #[serde(rename = "referenceType")]
    pub reference_type: String,
    #[serde(rename = "referenceLocator")]
    pub reference_locator: String,
}

/// Dependency entry (normalized from SBOM).
#[derive(Debug, Clone)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub ecosystem: String, // npm, pypi, maven, cargo, go, nuget, etc.
    pub purl: Option<String>,
    pub licenses: Vec<String>,
    pub hashes: HashMap<String, String>,
    pub source_repo: Option<String>,
    pub is_dev: bool,
}

/// Parses an SBOM file (CycloneDX or SPDX) into normalized dependencies.
/// Picks the SBOM dialect by its discriminator fields.
///
/// The formats are *not* auto-detected by trial deserialization: both
/// document structs use optional fields, so an untagged enum would match the
/// first variant for every document (an SPDX file parsed as CycloneDX yields
/// zero components).
fn classify_sbom(raw: serde_json::Value) -> Result<SbomDocument, String> {
    if raw.get("spdxVersion").is_some() {
        return serde_json::from_value::<SpdxSbom>(raw)
            .map(SbomDocument::Spdx)
            .map_err(|e| format!("parse spdx sbom: {e}"));
    }
    if raw.get("bomFormat").is_some() {
        return serde_json::from_value::<CycloneDxSbom>(raw)
            .map(SbomDocument::CycloneDx)
            .map_err(|e| format!("parse cyclonedx sbom: {e}"));
    }
    if let Ok(spdx) = serde_json::from_value::<SpdxSbom>(raw.clone()) {
        if spdx.packages.is_some() {
            return Ok(SbomDocument::Spdx(spdx));
        }
    }
    if let Ok(cdx) = serde_json::from_value::<CycloneDxSbom>(raw.clone()) {
        if cdx.components.is_some() {
            return Ok(SbomDocument::CycloneDx(cdx));
        }
    }
    Ok(SbomDocument::Generic(raw))
}

pub fn parse_sbom(path: &Path) -> Result<Vec<Dependency>, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("read sbom: {e}"))?;

    if content.len() > MAX_SBOM_SIZE {
        return Err("SBOM too large".into());
    }

    let raw: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("parse sbom: {e}"))?;
    let sbom = classify_sbom(raw)?;

    let mut deps = Vec::new();

    match sbom {
        SbomDocument::CycloneDx(cdx) => {
            if let Some(components) = cdx.components {
                for comp in components {
                    let ecosystem = detect_ecosystem_from_purl(&comp.purl);
                    let dep = Dependency {
                        name: comp.name,
                        version: comp.version.unwrap_or_default(),
                        ecosystem,
                        purl: comp.purl,
                        licenses: comp
                            .licenses
                            .unwrap_or_default()
                            .into_iter()
                            .filter_map(|l| l.license.id.or(l.license.name))
                            .collect(),
                        hashes: comp
                            .hashes
                            .unwrap_or_default()
                            .into_iter()
                            .map(|h| (h.alg, h.content))
                            .collect(),
                        source_repo: comp
                            .external_references
                            .unwrap_or_default()
                            .into_iter()
                            .find(|r| r.ref_type == "vcs")
                            .map(|r| r.url),
                        is_dev: false,
                    };
                    deps.push(dep);
                }
            }
        }
        SbomDocument::Spdx(spdx) => {
            if let Some(packages) = spdx.packages {
                for pkg in packages {
                    let ecosystem = detect_ecosystem_from_spdx_refs(&pkg.external_refs);
                    let dep = Dependency {
                        name: pkg.name,
                        version: pkg.version_info.unwrap_or_default(),
                        ecosystem,
                        purl: pkg.external_refs.as_ref().and_then(|refs| {
                            refs.iter()
                                .find(|r| r.reference_type == "purl")
                                .map(|r| r.reference_locator.clone())
                        }),
                        licenses: vec![
                            pkg.license_concluded.unwrap_or_default(),
                            pkg.license_declared.unwrap_or_default(),
                        ]
                        .into_iter()
                        .filter(|s| !s.is_empty() && s != "NOASSERTION")
                        .collect(),
                        hashes: HashMap::new(),
                        source_repo: pkg.external_refs.as_ref().and_then(|refs| {
                            refs.iter()
                                .find(|r| {
                                    r.reference_category == "PERSISTENT-ID"
                                        && r.reference_type == "vcs"
                                })
                                .map(|r| r.reference_locator.clone())
                        }),
                        is_dev: false,
                    };
                    deps.push(dep);
                }
            }
        }
        SbomDocument::Generic(_) => {
            return Err("generic SBOM format not supported; use CycloneDX or SPDX".into());
        }
    }

    deps.truncate(MAX_DEPENDENCIES);
    Ok(deps)
}

fn detect_ecosystem_from_purl(purl: &Option<String>) -> String {
    purl.as_ref()
        .and_then(|p| {
            // purl format: pkg:type/namespace/name@version
            p.strip_prefix("pkg:")
                .and_then(|s| s.split('/').next())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn detect_ecosystem_from_spdx_refs(refs: &Option<Vec<SpdxExternalRef>>) -> String {
    refs.as_ref()
        .and_then(|refs| {
            refs.iter()
                .find(|r| r.reference_type == "purl")
                .and_then(|r| r.reference_locator.strip_prefix("pkg:"))
                .and_then(|s| s.split('/').next())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Analyzes SBOM for supply chain risks.
pub fn sbom_analyze(path: &Path) -> Result<ModuleReport, String> {
    let deps = parse_sbom(path)?;
    let total = deps.len();

    let mut report = ModuleReport::new(
        "supply_chain",
        "sbom",
        path.display().to_string(),
        format!("analyzed {} dependencies", total),
    );

    // Group by ecosystem
    let mut by_ecosystem: HashMap<String, Vec<&Dependency>> = HashMap::new();
    for dep in &deps {
        by_ecosystem
            .entry(dep.ecosystem.clone())
            .or_default()
            .push(dep);
    }

    for (eco, eco_deps) in &by_ecosystem {
        report = report.line(format!("  {}: {} deps", eco, eco_deps.len()));
    }

    // Check for known risky patterns
    let mut risks = Vec::new();

    // 1. Unpinned versions
    let unpinned: Vec<_> = deps
        .iter()
        .filter(|d| {
            d.version.contains("latest")
                || d.version.contains("*")
                || d.version.contains("^")
                || d.version.contains("~")
        })
        .collect();
    if !unpinned.is_empty() {
        risks.push(format!(
            "{} unpinned/version-range dependencies",
            unpinned.len()
        ));
        report = report.line(format!("⚠ {} unpinned versions", unpinned.len()));
    }

    // 2. Missing source repository
    let no_source: Vec<_> = deps.iter().filter(|d| d.source_repo.is_none()).collect();
    if !no_source.is_empty() {
        risks.push(format!(
            "{} dependencies without source repository",
            no_source.len()
        ));
    }

    // 3. Suspicious package names (typosquatting candidates)
    let popular = [
        "lodash",
        "react",
        "axios",
        "express",
        "request",
        "moment",
        "chalk",
        "commander",
        "debug",
        "async",
        "django",
        "flask",
        "requests",
        "numpy",
        "pandas",
        "pytest",
        "celery",
        "redis",
        "sqlalchemy",
        "spring-boot",
        "jackson",
        "gson",
        "log4j",
        "guava",
        "netty",
        "kafka",
        "zookeeper",
        "goog",
        "google",
        "microsoft",
        "aws",
        "azure",
        "kubernetes",
        "docker",
        "helm",
        "rust",
        "cargo",
        "tokio",
        "serde",
        "clap",
        "rand",
        "regex",
        "chrono",
        "uuid",
        "express",
        "koa",
        "fastify",
        "nest",
        "next",
        "nuxt",
        "vite",
        "webpack",
        "babel",
        "eslint",
        "prettier",
        "jest",
        "mocha",
        "cypress",
        "playwright",
        "puppeteer",
        "selenium",
    ];
    let suspicious: Vec<_> = deps
        .iter()
        .filter(|d| {
            let name = d.name.to_ascii_lowercase();
            popular.iter().any(|p| {
                let p = p.to_ascii_lowercase();
                (name == p
                    || name == format!("{}-", p)
                    || name == format!("-{}", p)
                    || name == format!("{}_", p)
                    || name == format!("_{}", p)
                    || name == format!("{}js", p)
                    || name == format!("{}py", p)
                    || name == format!("{}rs", p))
                    && name != p
            })
        })
        .collect();
    if !suspicious.is_empty() {
        risks.push(format!(
            "{} potential typosquatting packages",
            suspicious.len()
        ));
        for s in suspicious.iter().take(10) {
            report = report.line(format!(
                "  ⚠ Typosquat candidate: {} ({})",
                s.name, s.ecosystem
            ));
        }
        report = report.finding(
            Finding::new(
                "supply_chain",
                "sbom",
                path.display().to_string(),
                "potential typosquatting dependencies",
                RiskLevel::High,
            )
            .confidence(Confidence::Suspected)
            .detail(
                suspicious
                    .iter()
                    .map(|d| format!("{} ({})", d.name, d.ecosystem))
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
        );
    }

    // 4. Known malicious package patterns
    let malicious_patterns = [
        "-stealer",
        "-grabber",
        "-logger",
        "-keylog",
        "-inject",
        "-exploit",
        "-backdoor",
        "-rat-",
        "-trojan",
        "-miner",
        "-cryptominer",
    ];
    let malicious: Vec<_> = deps
        .iter()
        .filter(|d| {
            malicious_patterns
                .iter()
                .any(|p| d.name.to_ascii_lowercase().contains(p))
        })
        .collect();
    if !malicious.is_empty() {
        risks.push(format!(
            "{} packages with malicious name patterns",
            malicious.len()
        ));
        for m in malicious.iter().take(10) {
            report = report.line(format!(
                "  ⚠ Malicious pattern: {} ({})",
                m.name, m.ecosystem
            ));
        }
        report = report.finding(
            Finding::new(
                "supply_chain",
                "sbom",
                path.display().to_string(),
                "dependencies with malicious name patterns",
                RiskLevel::Critical,
            )
            .confidence(Confidence::Suspected)
            .detail(
                malicious
                    .iter()
                    .map(|d| format!("{} ({})", d.name, d.ecosystem))
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
        );
    }

    // 5. License risks
    let risky_licenses = [
        "gpl-3.0",
        "agpl-3.0",
        "sspl",
        "bsl",
        "elv2",
        "commons-clause",
    ];
    let license_risk: Vec<_> = deps
        .iter()
        .filter(|d| {
            d.licenses.iter().any(|l| {
                risky_licenses
                    .iter()
                    .any(|rl| l.to_ascii_lowercase().contains(rl))
            })
        })
        .collect();
    if !license_risk.is_empty() {
        risks.push(format!(
            "{} dependencies with restrictive/copyleft licenses",
            license_risk.len()
        ));
        report = report.finding(
            Finding::new(
                "supply_chain",
                "sbom",
                path.display().to_string(),
                "restrictive license dependencies",
                RiskLevel::Medium,
            )
            .confidence(Confidence::Confirmed)
            .detail(
                license_risk
                    .iter()
                    .map(|d| format!("{} ({})", d.name, d.licenses.join(",")))
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
        );
    }

    // 6. Old/unmaintained packages (version age - would need external API)
    // Placeholder: flag versions that look old (0.x, or very old semver)
    let old_versions: Vec<_> = deps
        .iter()
        .filter(|d| {
            let v = d.version.trim_start_matches('v');
            v.starts_with("0.")
                || v.split('.')
                    .next()
                    .map(|m| m.parse::<u32>().unwrap_or(0) < 1)
                    .unwrap_or(false)
        })
        .collect();
    if !old_versions.is_empty() {
        risks.push(format!(
            "{} pre-1.0 or potentially old versions",
            old_versions.len()
        ));
    }

    if risks.is_empty() {
        report = report.line("✓ No obvious supply chain risks detected in SBOM");
        report.summary = format!("{} deps analyzed - no risks", total);
    } else {
        report.summary = format!("{} deps - {} risk categories", total, risks.len());
    }

    Ok(report)
}

/// Dependency confusion: checks if internal package names exist on public registries.
/// This is a passive check - only queries public registry APIs for existence.
pub fn dependency_confusion_check(
    package_names: &[String],
    ecosystems: &[String], // same length as package_names
) -> Result<ModuleReport, String> {
    if package_names.len() != ecosystems.len() {
        return Err("package_names and ecosystems must have same length".into());
    }
    if package_names.is_empty() {
        return Err("dependency_confusion needs at least one package name".into());
    }

    let mut report = ModuleReport::new(
        "supply_chain",
        "dep-confusion",
        "internal-packages",
        format!(
            "checking {} internal packages against public registries",
            package_names.len()
        ),
    );

    // In production, this would query npm, pypi, cargo, maven, etc. APIs
    // For now, this is a scaffold showing the approach
    report = report.line("Public registry queries that would be performed:");
    for (name, eco) in package_names.iter().zip(ecosystems.iter()) {
        let registry_url = match eco.as_str() {
            "npm" => format!("https://registry.npmjs.org/{}", name),
            "pypi" => format!("https://pypi.org/pypi/{}/json", name),
            "cargo" => format!("https://crates.io/api/v1/crates/{}", name),
            "maven" => format!(
                "https://search.maven.org/solrsearch/select?q=g:{}+AND+a:{}",
                name, name
            ),
            "go" => format!("https://proxy.golang.org/{}/@v/list", name),
            "nuget" => format!(
                "https://api.nuget.org/v3/registration5-gz-semver2/{}/index.json",
                name.to_lowercase()
            ),
            _ => format!("unknown registry for {}", eco),
        };
        report = report.line(format!("  [{}] {} -> {}", eco, name, registry_url));
    }

    report = report.line("");
    report = report.line("⚠ This is a scaffold - production implementation would:");
    report = report.line("  - Query each registry API for package existence");
    report = report.line("  - Compare versions: if public version > private, confusion possible");
    report = report.line("  - Check for typosquatting variants (hyphen/underscore/prefix/suffix)");
    report = report.line("  - Verify package ownership (scope/org matching)");
    report = report.line("  - Flag any internal name that exists publicly with different owner");

    report = report.finding(
        Finding::new("supply_chain", "dep-confusion", "internal-packages",
            "dependency confusion check scaffold",
            RiskLevel::Informational)
        .confidence(Confidence::Suspected)
        .detail("Production implementation requires registry API queries. See report lines for endpoints."),
    );

    Ok(report)
}

/// Repo jacking detection: checks if GitHub/GitLab repos referenced in deps have been renamed/transferred.
pub fn repo_jacking_check(source_repos: &[String]) -> Result<ModuleReport, String> {
    if source_repos.is_empty() {
        return Err("repo_jacking needs at least one source repository".into());
    }

    let mut report = ModuleReport::new(
        "supply_chain",
        "repo-jacking",
        "source-repos",
        format!(
            "checking {} source repositories for jacking indicators",
            source_repos.len()
        ),
    );

    report = report.line("GitHub/GitLab API queries that would be performed:");
    for repo in source_repos {
        // Normalize to GitHub format
        let gh_repo = repo
            .strip_prefix("https://github.com/")
            .or_else(|| repo.strip_prefix("http://github.com/"))
            .or_else(|| repo.strip_prefix("git@github.com:"))
            .or_else(|| repo.strip_prefix("github.com/"))
            .unwrap_or(repo)
            .trim_end_matches(".git");

        report = report.line(format!("  {} -> GitHub API: /repos/{}", repo, gh_repo));
        report = report
            .line("    Checks: repo exists, not archived, not transferred, owner matches expected");
    }

    report = report.line("");
    report = report.line("⚠ Scaffold - production implementation would:");
    report = report.line("  - Query GitHub/GitLab API for each repo");
    report = report.line("  - Check: 404 (deleted/renamed), archived=true, owner changed");
    report = report.line("  - Compare current owner with expected (from SBOM/package.json)");
    report = report.line("  - Check for repo transfer events in audit log");
    report = report.line("  - Flag any repo that no longer matches expected ownership");

    report = report.finding(
        Finding::new(
            "supply_chain",
            "repo-jacking",
            "source-repos",
            "repo jacking check scaffold",
            RiskLevel::Informational,
        )
        .confidence(Confidence::Suspected)
        .detail("Production implementation requires GitHub/GitLab API access with token."),
    );

    Ok(report)
}

/// Comprehensive supply chain sweep.
pub fn full_sweep(
    sbom_path: Option<&Path>,
    internal_packages: Option<(&[String], &[String])>, // (names, ecosystems)
    source_repos: Option<&[String]>,
) -> Result<ModuleReport, String> {
    let mut combined = ModuleReport::new("supply_chain", "sweep", "supply-chain", String::new());

    if let Some(path) = sbom_path {
        match sbom_analyze(path) {
            Ok(r) => {
                combined
                    .lines
                    .push(format!("=== SBOM Analysis ({}) ===", r.summary));
                combined.lines.extend(r.lines);
                combined.findings.extend(r.findings);
            }
            Err(e) => combined.lines.push(format!("SBOM error: {e}")),
        }
    }

    if let Some((names, ecos)) = internal_packages {
        match dependency_confusion_check(names, ecos) {
            Ok(r) => {
                combined
                    .lines
                    .push(format!("=== Dependency Confusion ({}) ===", r.summary));
                combined.lines.extend(r.lines);
                combined.findings.extend(r.findings);
            }
            Err(e) => combined.lines.push(format!("Dep confusion error: {e}")),
        }
    }

    if let Some(repos) = source_repos {
        match repo_jacking_check(repos) {
            Ok(r) => {
                combined
                    .lines
                    .push(format!("=== Repo Jacking ({}) ===", r.summary));
                combined.lines.extend(r.lines);
                combined.findings.extend(r.findings);
            }
            Err(e) => combined.lines.push(format!("Repo jacking error: {e}")),
        }
    }

    combined.summary = "completed supply chain sweep".to_string();
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_cyclonedx_sbom() {
        let sbom_json = serde_json::json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.4",
            "components": [
                {
                    "type": "library",
                    "name": "lodash",
                    "version": "4.17.21",
                    "purl": "pkg:npm/lodash@4.17.21",
                    "licenses": [{"license": {"id": "MIT"}}],
                    "hashes": [{"alg": "SHA-256", "content": "abc123"}],
                    "externalReferences": [{"type": "vcs", "url": "https://github.com/lodash/lodash"}]
                },
                {
                    "type": "library",
                    "name": "express",
                    "version": "4.18.2",
                    "purl": "pkg:npm/express@4.18.2"
                }
            ]
        }).to_string();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bom.json");
        fs::write(&path, sbom_json).unwrap();

        let deps = parse_sbom(&path).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "lodash");
        assert_eq!(deps[0].version, "4.17.21");
        assert_eq!(deps[0].ecosystem, "npm");
        assert_eq!(deps[0].licenses, vec!["MIT"]);
        assert_eq!(
            deps[0].source_repo,
            Some("https://github.com/lodash/lodash".to_string())
        );
    }

    #[test]
    fn parse_spdx_sbom() {
        let sbom_json = serde_json::json!({
            "spdxVersion": "SPDX-2.3",
            "packages": [
                {
                    "SPDXID": "SPDXRef-Package-1",
                    "name": "requests",
                    "versionInfo": "2.28.1",
                    "downloadLocation": "https://pypi.org/project/requests/2.28.1/",
                    "licenseConcluded": "Apache-2.0",
                    "licenseDeclared": "Apache-2.0",
                    "externalRefs": [
                        {"referenceCategory": "PACKAGE-MANAGER", "referenceType": "purl", "referenceLocator": "pkg:pypi/requests@2.28.1"},
                        {"referenceCategory": "PERSISTENT-ID", "referenceType": "vcs", "referenceLocator": "https://github.com/psf/requests"}
                    ]
                }
            ]
        }).to_string();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spdx.json");
        fs::write(&path, sbom_json).unwrap();

        let deps = parse_sbom(&path).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "requests");
        assert_eq!(deps[0].version, "2.28.1");
        assert_eq!(deps[0].ecosystem, "pypi");
        assert!(deps[0].licenses.contains(&"Apache-2.0".to_string()));
        assert_eq!(
            deps[0].source_repo,
            Some("https://github.com/psf/requests".to_string())
        );
    }

    #[test]
    fn sbom_analyze_detects_typosquatting() {
        let sbom_json = serde_json::json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.4",
            "components": [
                {"type": "library", "name": "lodash", "version": "4.17.21", "purl": "pkg:npm/lodash@4.17.21"},
                {"type": "library", "name": "lodashjs", "version": "1.0.0", "purl": "pkg:npm/lodashjs@1.0.0"},
                {"type": "library", "name": "express", "version": "4.18.2", "purl": "pkg:npm/express@4.18.2"},
                {"type": "library", "name": "express-js", "version": "1.0.0", "purl": "pkg:npm/express-js@1.0.0"}
            ]
        }).to_string();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bom.json");
        fs::write(&path, sbom_json).unwrap();

        let report = sbom_analyze(&path).unwrap();
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "potential typosquatting dependencies"));
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("Typosquat candidate")));
    }

    #[test]
    fn sbom_analyze_detects_malicious_patterns() {
        let sbom_json = serde_json::json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.4",
            "components": [
                {"type": "library", "name": "password-stealer", "version": "1.0.0", "purl": "pkg:npm/password-stealer@1.0.0"},
                {"type": "library", "name": "keylogger-tool", "version": "1.0.0", "purl": "pkg:npm/keylogger-tool@1.0.0"}
            ]
        }).to_string();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bom.json");
        fs::write(&path, sbom_json).unwrap();

        let report = sbom_analyze(&path).unwrap();
        assert!(report
            .findings
            .iter()
            .any(|f| f.severity == RiskLevel::Critical));
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "dependencies with malicious name patterns"));
    }

    #[test]
    fn dependency_confusion_check_validates_input() {
        assert!(
            dependency_confusion_check(&["pkg1".into()], &["npm".into(), "pypi".into()]).is_err()
        );
        assert!(dependency_confusion_check(&[], &[]).is_err());
    }

    #[test]
    fn repo_jacking_check_validates_input() {
        assert!(repo_jacking_check(&[]).is_err());
    }
}
