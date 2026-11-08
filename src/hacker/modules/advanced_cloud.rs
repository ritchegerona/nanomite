//! Advanced Cloud/Container: AWS/Azure/GCP enumeration, k8s RBAC analysis, container escape checks.
//!
//! All operations are scope-gated and use injected fetchers/clients.

use std::collections::HashMap;
use std::time::Duration;

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// Max resources per cloud provider query.
pub const MAX_CLOUD_RESOURCES: usize = 200;
/// Max k8s resources to enumerate.
pub const MAX_K8S_RESOURCES: usize = 300;
/// HTTP timeout.
pub const ADV_CLOUD_TIMEOUT: Duration = Duration::from_secs(30);

/// Minimal response shape.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Fetched {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|v| v.as_str())
    }
}

/// Fetches responses. Production uses reqwest; tests inject a stub.
pub trait Fetcher: Send + Sync {
    fn fetch(
        &self,
        url: &str,
        method: &str,
        body: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Result<Fetched, String>;
}

pub struct ReqwestFetcher;

impl Fetcher for ReqwestFetcher {
    fn fetch(
        &self,
        url: &str,
        method: &str,
        body: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Result<Fetched, String> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(ADV_CLOUD_TIMEOUT)
            .user_agent("nanomite-hacker-adv-cloud/0.1")
            .build()
            .map_err(|e| format!("http client: {e}"))?;

        let mut req = match method.to_uppercase().as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            _ => return Err(format!("unsupported method {method}")),
        };

        for (k, v) in headers {
            req = req.header(*k, *v);
        }

        if let Some(b) = body {
            req = req.body(b.to_string());
        }

        let response = req.send().map_err(|e| format!("request failed: {e}"))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let body = response.text().unwrap_or_default();

        Ok(Fetched {
            status,
            headers,
            body,
        })
    }
}

/// AWS metadata service check (SSRF target).
pub fn aws_metadata_check(fetcher: &dyn Fetcher) -> Result<ModuleReport, String> {
    let endpoints = vec![
        (
            "instance-id",
            "http://169.254.169.254/latest/meta-data/instance-id",
        ),
        (
            "iam-role",
            "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
        ),
        (
            "iam-creds",
            "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
        ),
        ("user-data", "http://169.254.169.254/latest/user-data"),
        (
            "identity-doc",
            "http://169.254.169.254/latest/dynamic/instance-identity/document",
        ),
    ];

    let mut report = ModuleReport::new(
        "advanced_cloud",
        "aws-metadata",
        "169.254.169.254",
        "testing AWS metadata endpoints",
    );

    let mut leaked = Vec::new();
    for (name, url) in endpoints {
        match fetcher.fetch(url, "GET", None, &[("Metadata", "true")]) {
            Ok(fetched) if fetched.status == 200 && !fetched.body.is_empty() => {
                report = report.line(format!(
                    "⚠ {} - ACCESSIBLE ({} bytes)",
                    name,
                    fetched.body.len()
                ));
                leaked.push((name, fetched.body.chars().take(200).collect::<String>()));
            }
            Ok(fetched) => {
                report = report.line(format!(
                    "✓ {} - HTTP {} ({} bytes)",
                    name,
                    fetched.status,
                    fetched.body.len()
                ));
            }
            Err(e) => {
                report = report.line(format!("✓ {} - error: {}", name, e));
            }
        }
    }

    if !leaked.is_empty() {
        report = report.finding(
            Finding::new(
                "advanced_cloud",
                "aws-metadata",
                "169.254.169.254",
                "aws metadata service accessible - credential leak",
                RiskLevel::Critical,
            )
            .confidence(Confidence::Confirmed)
            .detail(format!(
                "AWS metadata endpoints leaked: {}",
                leaked
                    .iter()
                    .map(|(n, v)| format!("{}={}", n, v))
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
        );
        report.summary = "CRITICAL: AWS metadata accessible".to_string();
    } else {
        report.summary = "No AWS metadata leakage detected".to_string();
    }

    Ok(report)
}

/// GCP metadata service check.
pub fn gcp_metadata_check(fetcher: &dyn Fetcher) -> Result<ModuleReport, String> {
    let endpoints = vec![
        ("project-id", "http://metadata.google.internal/computeMetadata/v1/project/project-id"),
        ("instance-id", "http://metadata.google.internal/computeMetadata/v1/instance/id"),
        ("service-account", "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token"),
        ("scopes", "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/scopes"),
        ("ssh-keys", "http://metadata.google.internal/computeMetadata/v1/project/attributes/ssh-keys"),
    ];

    let mut report = ModuleReport::new(
        "advanced_cloud",
        "gcp-metadata",
        "metadata.google.internal",
        "testing GCP metadata endpoints",
    );

    let mut leaked = Vec::new();
    for (name, url) in endpoints {
        match fetcher.fetch(url, "GET", None, &[("Metadata-Flavor", "Google")]) {
            Ok(fetched) if fetched.status == 200 && !fetched.body.is_empty() => {
                report = report.line(format!(
                    "⚠ {} - ACCESSIBLE ({} bytes)",
                    name,
                    fetched.body.len()
                ));
                leaked.push((name, fetched.body.chars().take(200).collect::<String>()));
            }
            Ok(fetched) => {
                report = report.line(format!(
                    "✓ {} - HTTP {} ({} bytes)",
                    name,
                    fetched.status,
                    fetched.body.len()
                ));
            }
            Err(e) => {
                report = report.line(format!("✓ {} - error: {}", name, e));
            }
        }
    }

    if !leaked.is_empty() {
        report = report.finding(
            Finding::new(
                "advanced_cloud",
                "gcp-metadata",
                "metadata.google.internal",
                "gcp metadata service accessible - token leak",
                RiskLevel::Critical,
            )
            .confidence(Confidence::Confirmed)
            .detail(format!(
                "GCP metadata endpoints leaked: {}",
                leaked
                    .iter()
                    .map(|(n, v)| format!("{}={}", n, v))
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
        );
        report.summary = "CRITICAL: GCP metadata accessible".to_string();
    } else {
        report.summary = "No GCP metadata leakage detected".to_string();
    }

    Ok(report)
}

/// Azure metadata service check.
pub fn azure_metadata_check(fetcher: &dyn Fetcher) -> Result<ModuleReport, String> {
    let endpoints = vec![
        ("instance", "http://169.254.169.254/metadata/instance?api-version=2021-02-01"),
        ("identity", "http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https://management.azure.com/"),
        ("attested", "http://169.254.169.254/metadata/attested/document?api-version=2021-02-01"),
    ];

    let mut report = ModuleReport::new(
        "advanced_cloud",
        "azure-metadata",
        "169.254.169.254",
        "testing Azure metadata endpoints",
    );

    let mut leaked = Vec::new();
    for (name, url) in endpoints {
        match fetcher.fetch(url, "GET", None, &[("Metadata", "true")]) {
            Ok(fetched) if fetched.status == 200 && !fetched.body.is_empty() => {
                report = report.line(format!(
                    "⚠ {} - ACCESSIBLE ({} bytes)",
                    name,
                    fetched.body.len()
                ));
                leaked.push((name, fetched.body.chars().take(200).collect::<String>()));
            }
            Ok(fetched) => {
                report = report.line(format!(
                    "✓ {} - HTTP {} ({} bytes)",
                    name,
                    fetched.status,
                    fetched.body.len()
                ));
            }
            Err(e) => {
                report = report.line(format!("✓ {} - error: {}", name, e));
            }
        }
    }

    if !leaked.is_empty() {
        report = report.finding(
            Finding::new(
                "advanced_cloud",
                "azure-metadata",
                "169.254.169.254",
                "azure metadata service accessible - token leak",
                RiskLevel::Critical,
            )
            .confidence(Confidence::Confirmed)
            .detail(format!(
                "Azure metadata endpoints leaked: {}",
                leaked
                    .iter()
                    .map(|(n, v)| format!("{}={}", n, v))
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
        );
        report.summary = "CRITICAL: Azure metadata accessible".to_string();
    } else {
        report.summary = "No Azure metadata leakage detected".to_string();
    }

    Ok(report)
}

/// Kubernetes API server enumeration (requires network access to k8s API).
#[derive(Debug, Clone)]
pub struct K8sResource {
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,
    pub labels: HashMap<String, String>,
}

pub fn k8s_api_enum(
    api_server: &str,
    token: Option<&str>,
    fetcher: &dyn Fetcher,
) -> Result<ModuleReport, String> {
    let api_server = api_server.trim();
    if api_server.is_empty() {
        return Err("advanced_cloud/k8s needs an API server URL".into());
    }

    let auth_header = token.map(|t| format!("Bearer {t}"));
    let mut headers = vec![("Accept", "application/json")];
    if let Some(auth) = &auth_header {
        headers.push(("Authorization", auth.as_str()));
    }

    let endpoints = vec![
        ("namespaces", "/api/v1/namespaces"),
        ("pods", "/api/v1/pods"),
        ("services", "/api/v1/services"),
        ("nodes", "/api/v1/nodes"),
        ("deployments", "/apis/apps/v1/deployments"),
        ("statefulsets", "/apis/apps/v1/statefulsets"),
        ("daemonsets", "/apis/apps/v1/daemonsets"),
        ("configmaps", "/api/v1/configmaps"),
        ("secrets", "/api/v1/secrets"),
        ("roles", "/apis/rbac.authorization.k8s.io/v1/roles"),
        (
            "rolebindings",
            "/apis/rbac.authorization.k8s.io/v1/rolebindings",
        ),
        (
            "clusterroles",
            "/apis/rbac.authorization.k8s.io/v1/clusterroles",
        ),
        (
            "clusterrolebindings",
            "/apis/rbac.authorization.k8s.io/v1/clusterrolebindings",
        ),
        ("serviceaccounts", "/api/v1/serviceaccounts"),
        ("ingresses", "/apis/networking.k8s.io/v1/ingresses"),
        (
            "networkpolicies",
            "/apis/networking.k8s.io/v1/networkpolicies",
        ),
        (
            "crds",
            "/apis/apiextensions.k8s.io/v1/customresourcedefinitions",
        ),
    ];

    let mut report = ModuleReport::new(
        "advanced_cloud",
        "k8s-enum",
        api_server.to_string(),
        format!("enumerating {} k8s resource types", endpoints.len()),
    );

    let mut total_resources = 0;
    let mut sensitive_findings = Vec::new();

    for (kind, path) in endpoints.iter().take(MAX_K8S_RESOURCES / 10) {
        let url = format!("{}{}", api_server.trim_end_matches('/'), path);
        match fetcher.fetch(&url, "GET", None, &headers) {
            Ok(fetched) if fetched.status == 200 => {
                if let Ok(list) = serde_json::from_str::<serde_json::Value>(&fetched.body) {
                    if let Some(items) = list.get("items").and_then(|v| v.as_array()) {
                        let count = items.len();
                        total_resources += count;
                        report = report.line(format!("  {} ({}) - {} items", kind, path, count));

                        // Check for sensitive resources
                        match *kind {
                            "secrets" => {
                                for item in items.iter().take(10) {
                                    if let Some(name) = item
                                        .get("metadata")
                                        .and_then(|m| m.get("name"))
                                        .and_then(|v| v.as_str())
                                    {
                                        if let Some(ns) = item
                                            .get("metadata")
                                            .and_then(|m| m.get("namespace"))
                                            .and_then(|v| v.as_str())
                                        {
                                            sensitive_findings
                                                .push(format!("Secret: {}/{}", ns, name));
                                        }
                                    }
                                }
                            }
                            "serviceaccounts" => {
                                for item in items.iter().take(10) {
                                    if let Some(name) = item
                                        .get("metadata")
                                        .and_then(|m| m.get("name"))
                                        .and_then(|v| v.as_str())
                                    {
                                        if let Some(ns) = item
                                            .get("metadata")
                                            .and_then(|m| m.get("namespace"))
                                            .and_then(|v| v.as_str())
                                        {
                                            // Check for automountServiceAccountToken
                                            if item
                                                .get("automountServiceAccountToken")
                                                .and_then(|v| v.as_bool())
                                                != Some(false)
                                            {
                                                sensitive_findings.push(format!(
                                                    "SA with token auto-mount: {}/{}",
                                                    ns, name
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                            "clusterrolebindings" | "rolebindings" => {
                                for item in items.iter().take(10) {
                                    if let Some(subjects) =
                                        item.get("subjects").and_then(|v| v.as_array())
                                    {
                                        for subj in subjects {
                                            if let Some(kind) =
                                                subj.get("kind").and_then(|v| v.as_str())
                                            {
                                                if kind == "ServiceAccount" {
                                                    if let Some(name) =
                                                        subj.get("name").and_then(|v| v.as_str())
                                                    {
                                                        if let Some(ns) = subj
                                                            .get("namespace")
                                                            .and_then(|v| v.as_str())
                                                        {
                                                            sensitive_findings.push(format!(
                                                                "Binding: {}/{} -> {}",
                                                                ns,
                                                                name,
                                                                item.get("metadata")
                                                                    .and_then(|m| m.get("name"))
                                                                    .and_then(|v| v.as_str())
                                                                    .unwrap_or("?")
                                                            ));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(fetched) => {
                report = report.line(format!("  {} ({}) - HTTP {}", kind, path, fetched.status));
            }
            Err(e) => {
                report = report.line(format!("  {} ({}) - error: {}", kind, path, e));
            }
        }
    }

    if !sensitive_findings.is_empty() {
        report = report.line("");
        report = report.line("⚠ Sensitive RBAC/Secret findings:");
        for f in sensitive_findings.iter().take(20) {
            report = report.line(format!("  {}", f));
        }
        report = report.finding(
            Finding::new(
                "advanced_cloud",
                "k8s-enum",
                api_server.to_string(),
                "kubernetes sensitive resources exposed",
                RiskLevel::High,
            )
            .confidence(Confidence::Confirmed)
            .detail(sensitive_findings.join("; ")),
        );
    }

    report.summary = format!(
        "enumerated {} k8s resource types, {} total resources",
        endpoints.len(),
        total_resources
    );
    Ok(report)
}

/// Container runtime escape checks (run inside container).
pub fn container_escape_checks() -> ModuleReport {
    let mut report = ModuleReport::new(
        "advanced_cloud",
        "container-escape",
        "local",
        "container escape vector checks",
    );

    // Check for privileged indicators
    let checks = vec![
        (
            "--privileged flag",
            std::path::Path::new("/proc/self/status").exists() && {
                std::fs::read_to_string("/proc/self/status")
                    .map(|s| s.contains("CapEff:\t0000003fffffffff"))
                    .unwrap_or(false)
            },
        ),
        (
            "Docker socket mounted",
            std::path::Path::new("/var/run/docker.sock").exists(),
        ),
        (
            "Containerd socket",
            std::path::Path::new("/run/containerd/containerd.sock").exists(),
        ),
        (
            "CRI-O socket",
            std::path::Path::new("/var/run/crio/crio.sock").exists(),
        ),
        (
            "Host PID namespace",
            std::path::Path::new("/proc/1/ns/pid").exists() && {
                std::fs::read_link("/proc/1/ns/pid")
                    .map(|p| p != std::fs::read_link("/proc/self/ns/pid").unwrap_or_default())
                    .unwrap_or(false)
            },
        ),
        (
            "Host network namespace",
            std::path::Path::new("/proc/1/ns/net").exists() && {
                std::fs::read_link("/proc/1/ns/net")
                    .map(|p| p != std::fs::read_link("/proc/self/ns/net").unwrap_or_default())
                    .unwrap_or(false)
            },
        ),
        (
            "SYS_ADMIN capability",
            std::path::Path::new("/proc/self/status").exists() && {
                std::fs::read_to_string("/proc/self/status")
                    .map(|s| {
                        s.lines().any(|l| {
                            l.starts_with("CapEff:") && {
                                let cap = l.split_whitespace().nth(1).unwrap_or("");
                                // Check for SYS_ADMIN (bit 21)
                                u64::from_str_radix(cap, 16)
                                    .map(|c| c & (1 << 21) != 0)
                                    .unwrap_or(false)
                            }
                        })
                    })
                    .unwrap_or(false)
            },
        ),
        (
            "CAP_DAC_OVERRIDE",
            std::path::Path::new("/proc/self/status").exists() && {
                std::fs::read_to_string("/proc/self/status")
                    .map(|s| {
                        s.lines().any(|l| {
                            l.starts_with("CapEff:") && {
                                let cap = l.split_whitespace().nth(1).unwrap_or("");
                                u64::from_str_radix(cap, 16)
                                    .map(|c| c & (1 << 1) != 0)
                                    .unwrap_or(false)
                            }
                        })
                    })
                    .unwrap_or(false)
            },
        ),
        (
            "/proc writable",
            std::fs::metadata("/proc")
                .map(|m| !m.permissions().readonly())
                .unwrap_or(false),
        ),
        (
            "/sys writable",
            std::fs::metadata("/sys")
                .map(|m| !m.permissions().readonly())
                .unwrap_or(false),
        ),
        (
            "cgroup v1 release_agent",
            std::path::Path::new("/sys/fs/cgroup/release_agent").exists(),
        ),
        (
            "AppArmor profile",
            std::path::Path::new("/proc/self/attr/current").exists() && {
                std::fs::read_to_string("/proc/self/attr/current")
                    .map(|s| s.trim() == "unconfined")
                    .unwrap_or(false)
            },
        ),
        (
            "Seccomp disabled",
            std::path::Path::new("/proc/self/status").exists() && {
                std::fs::read_to_string("/proc/self/status")
                    .map(|s| {
                        s.lines()
                            .any(|l| l.starts_with("Seccomp:") && l.contains("0"))
                    })
                    .unwrap_or(false)
            },
        ),
    ];

    let mut risky = 0;
    for (name, result) in &checks {
        let status = if *result {
            "⚠ PRESENT"
        } else {
            "✓ not detected"
        };
        report = report.line(format!("{} - {}", name, status));
        if *result {
            risky += 1;
        }
    }

    if risky > 0 {
        report = report.finding(
            Finding::new("advanced_cloud", "container-escape", "local",
                format!("container escape vectors detected: {}", risky),
                RiskLevel::High)
            .confidence(Confidence::Suspected)
            .detail(format!("{} container escape indicators present. Running in privileged or misconfigured container.", risky)),
        );
        report.summary = format!("{} escape vectors detected", risky);
    } else {
        report.summary = "No obvious container escape vectors".to_string();
    }

    report
}

/// Comprehensive cloud sweep.
pub fn full_sweep(target: &str, fetcher: &dyn Fetcher) -> Result<ModuleReport, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("advanced_cloud/sweep needs a target".into());
    }

    let mut combined =
        ModuleReport::new("advanced_cloud", "sweep", target.to_string(), String::new());

    // Cloud metadata checks
    for (name, check_fn) in [
        (
            "AWS",
            aws_metadata_check as fn(&dyn Fetcher) -> Result<ModuleReport, String>,
        ),
        ("GCP", gcp_metadata_check),
        ("Azure", azure_metadata_check),
    ] {
        match check_fn(fetcher) {
            Ok(r) => {
                combined
                    .lines
                    .push(format!("=== {} Metadata ({}) ===", name, r.summary));
                combined.lines.extend(r.lines);
                combined.findings.extend(r.findings);
            }
            Err(e) => combined
                .lines
                .push(format!("{} metadata error: {}", name, e)),
        }
    }

    // Container escape (only meaningful inside container)
    let container_report = container_escape_checks();
    combined.lines.push(format!(
        "=== Container Escape ({}) ===",
        container_report.summary
    ));
    combined.lines.extend(container_report.lines);
    combined.findings.extend(container_report.findings);

    combined.summary = "completed cloud metadata + container checks".to_string();
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct StubFetcher(HashMap<String, Fetched>);
    impl Fetcher for StubFetcher {
        fn fetch(
            &self,
            url: &str,
            _method: &str,
            _body: Option<&str>,
            _headers: &[(&str, &str)],
        ) -> Result<Fetched, String> {
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no stub for {url}"))
        }
    }

    fn fetched(status: u16, body: &str) -> Fetched {
        Fetched {
            status,
            headers: HashMap::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn aws_metadata_detects_leak() {
        let fetcher = StubFetcher(
            [
                (
                    "http://169.254.169.254/latest/meta-data/instance-id".into(),
                    fetched(200, "i-123456"),
                ),
                (
                    "http://169.254.169.254/latest/meta-data/iam/security-credentials/".into(),
                    fetched(200, "role-name"),
                ),
            ]
            .into(),
        );

        let report = aws_metadata_check(&fetcher).unwrap();
        assert!(report.summary.contains("CRITICAL"));
        assert!(report
            .findings
            .iter()
            .any(|f| f.severity == RiskLevel::Critical));
    }

    #[test]
    fn gcp_metadata_detects_token_leak() {
        let fetcher = StubFetcher([
            ("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token".into(), fetched(200, "{\"access_token\":\"ya29...\"}")),
        ].into());

        let report = gcp_metadata_check(&fetcher).unwrap();
        assert!(report.summary.contains("CRITICAL"));
        assert!(report
            .findings
            .iter()
            .any(|f| f.severity == RiskLevel::Critical));
    }

    #[test]
    fn azure_metadata_detects_leak() {
        let fetcher = StubFetcher([
            ("http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https://management.azure.com/".into(), fetched(200, "{\"access_token\":\"eyJ...\"}")),
        ].into());

        let report = azure_metadata_check(&fetcher).unwrap();
        assert!(report.summary.contains("CRITICAL"));
    }

    #[test]
    fn k8s_enum_enumerates_resources() {
        let pods_json = serde_json::json!({
            "items": [
                {"metadata": {"name": "pod1", "namespace": "default"}},
                {"metadata": {"name": "pod2", "namespace": "kube-system"}}
            ]
        })
        .to_string();

        let secrets_json = serde_json::json!({
            "items": [
                {"metadata": {"name": "secret1", "namespace": "default"}}
            ]
        })
        .to_string();

        let fetcher = StubFetcher(
            [
                (
                    "https://k8s.example.com/api/v1/pods".into(),
                    fetched(200, &pods_json),
                ),
                (
                    "https://k8s.example.com/api/v1/secrets".into(),
                    fetched(200, &secrets_json),
                ),
            ]
            .into(),
        );

        let report = k8s_api_enum("https://k8s.example.com", Some("token"), &fetcher).unwrap();
        assert!(report.lines.iter().any(|l| l.contains("pods")));
        assert!(report.lines.iter().any(|l| l.contains("secrets")));
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "kubernetes sensitive resources exposed"));
    }

    #[test]
    fn container_escape_checks_runs() {
        let report = container_escape_checks();
        assert!(!report.lines.is_empty());
        assert!(report.lines.iter().any(|l| l.contains("Docker socket")
            || l.contains("privileged")
            || l.contains("capability")));
    }
}
