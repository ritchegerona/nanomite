//! Web security: HTTP response / security-header analysis.
//!
//! The analysis itself is pure ([`analyze_headers`]); the fetch is injected so
//! tests never touch the network. Redirects are **not** followed - a redirect
//! to another host would escape the validated scope.

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// Minimal response shape the analyzer needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

impl Fetched {
    /// Case-insensitive header lookup.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Fetches response headers for a URL. Production does a single, no-redirect
/// request; tests inject a stub.
pub trait Fetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Fetched, String>;
}

/// Real fetcher: one GET, redirects disabled, body not read.
pub struct ReqwestFetcher;

impl Fetcher for ReqwestFetcher {
    fn fetch(&self, url: &str) -> Result<Fetched, String> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("nanomite-hacker/0.1")
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        let response = client
            .get(url)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;
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
        Ok(Fetched { status, headers })
    }
}

/// Analysis of one response, `url` used only for the https check and display.
///
/// Every missing header becomes a structured finding; version disclosure and
/// cookie-flag gaps are informational. The `Set-Cookie` **value** is never
/// stored or reported - only which flags are absent.
pub fn analyze_headers(url: &str, fetched: &Fetched) -> ModuleReport {
    let https = url.trim().to_ascii_lowercase().starts_with("https://");
    let present = |name: &str| fetched.header(name).is_some();

    // (rendered label, finding title, ok, severity)
    let csp_frame_ancestors = fetched
        .header("Content-Security-Policy")
        .is_some_and(|v| v.to_ascii_lowercase().contains("frame-ancestors"));
    let mut checks: Vec<(&'static str, &'static str, bool, RiskLevel)> = vec![
        (
            "content-security-policy",
            "missing content-security-policy",
            present("Content-Security-Policy"),
            RiskLevel::Medium,
        ),
        (
            "x-content-type-options",
            "missing x-content-type-options",
            present("X-Content-Type-Options"),
            RiskLevel::Low,
        ),
        (
            "clickjacking protection (x-frame-options or csp frame-ancestors)",
            "missing clickjacking protection",
            present("X-Frame-Options") || csp_frame_ancestors,
            RiskLevel::Medium,
        ),
        (
            "referrer-policy",
            "missing referrer-policy",
            present("Referrer-Policy"),
            RiskLevel::Low,
        ),
        (
            "permissions-policy",
            "missing permissions-policy",
            present("Permissions-Policy"),
            RiskLevel::Low,
        ),
    ];
    if https {
        checks.push((
            "strict-transport-security (hsts)",
            "missing strict-transport-security",
            present("Strict-Transport-Security"),
            RiskLevel::Medium,
        ));
    }

    let missing = checks.iter().filter(|(_, _, ok, _)| !ok).count();
    let summary = if missing == 0 {
        format!("{} · no missing security headers", fetched.status)
    } else {
        format!("{} · {missing} missing security header(s)", fetched.status)
    };
    let mut report = ModuleReport::new("web", "headers", url, summary);

    for (label, title, ok, severity) in &checks {
        report = report.line(format!("{} {label}", if *ok { "✓" } else { "✗" }));
        if !*ok {
            // An absent header is directly observed, so it is `Confirmed`.
            report = report.finding(
                Finding::new("web", "headers", url, *title, *severity)
                    .confidence(Confidence::Confirmed)
                    .detail(format!("response from {url} did not include `{label}`")),
            );
        }
    }

    // Informational: version disclosure and cookie flags.
    if let Some(server) = fetched.header("Server") {
        report = report.line(format!("· server discloses `{server}`"));
        report = report.finding(
            Finding::new(
                "web",
                "headers",
                url,
                "server version disclosure",
                RiskLevel::Informational,
            )
            .confidence(Confidence::Confirmed)
            .detail(format!("Server: {server}")),
        );
    }
    if let Some(cookie) = fetched.header("Set-Cookie") {
        let lower = cookie.to_ascii_lowercase();
        let mut absent = Vec::new();
        if !lower.contains("secure") {
            absent.push("secure");
        }
        if !lower.contains("httponly") {
            absent.push("httponly");
        }
        if !lower.contains("samesite") {
            absent.push("samesite");
        }
        if !absent.is_empty() {
            let joined = absent.join(", ");
            report = report.line(format!("· cookie missing: {joined}"));
            // Never store the cookie value: only which flags are absent.
            report = report.finding(
                Finding::new(
                    "web",
                    "headers",
                    url,
                    format!("cookie missing flags: {joined}"),
                    RiskLevel::Low,
                )
                .confidence(Confidence::Confirmed)
                .detail(format!("a Set-Cookie response is missing: {joined}")),
            );
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetched(status: u16, headers: &[(&str, &str)]) -> Fetched {
        Fetched {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn flags_missing_headers_and_passes_hardened_responses() {
        let report = analyze_headers("https://example.com", &fetched(200, &[("Server", "nginx")]));
        // HSTS, CSP, x-content-type-options, referrer-policy, permissions-policy
        // all missing; clickjacking protection also missing.
        assert!(report.summary.contains("missing security header(s)"));
        assert!(report.lines.iter().any(|l| l.starts_with("✗")));
        assert!(report.lines.iter().any(|l| l.contains("server discloses")));

        let hardened = analyze_headers(
            "https://example.com",
            &fetched(
                200,
                &[
                    ("Strict-Transport-Security", "max-age=63072000"),
                    ("Content-Security-Policy", "default-src 'self'"),
                    ("X-Content-Type-Options", "nosniff"),
                    ("X-Frame-Options", "DENY"),
                    ("Referrer-Policy", "no-referrer"),
                    ("Permissions-Policy", "geolocation=()"),
                ],
            ),
        );
        assert!(hardened.summary.contains("no missing security headers"));
    }

    #[test]
    fn observed_gaps_are_confirmed_findings_and_hits_are_informational() {
        use crate::hacker::findings::Confidence;
        let report = analyze_headers("https://example.com", &fetched(200, &[("Server", "nginx")]));
        assert!(report.findings.len() >= 2);
        assert!(report
            .findings
            .iter()
            .all(|f| f.confidence == Confidence::Confirmed));
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "missing content-security-policy"));
        let disclosure = report
            .findings
            .iter()
            .find(|f| f.title == "server version disclosure")
            .expect("disclosure finding");
        assert_eq!(disclosure.severity, RiskLevel::Informational);
        // The hardened response has no missing-header findings.
        let hardened = analyze_headers(
            "https://example.com",
            &fetched(
                200,
                &[
                    ("Strict-Transport-Security", "max-age=1"),
                    ("Content-Security-Policy", "default-src 'self'"),
                    ("X-Content-Type-Options", "nosniff"),
                    ("X-Frame-Options", "DENY"),
                    ("Referrer-Policy", "no-referrer"),
                    ("Permissions-Policy", "geolocation=()"),
                ],
            ),
        );
        assert!(hardened.findings.is_empty());
    }

    #[test]
    fn csp_frame_ancestors_satisfies_clickjacking() {
        let report = analyze_headers(
            "https://x",
            &fetched(
                200,
                &[("Content-Security-Policy", "frame-ancestors 'none'")],
            ),
        );
        assert!(report
            .lines
            .iter()
            .any(|l| l.starts_with('✓') && l.contains("clickjacking")));
    }

    #[test]
    fn plaintext_http_does_not_require_hsts() {
        let report = analyze_headers("http://x", &fetched(200, &[]));
        assert!(!report.lines.iter().any(|l| l.contains("hsts")));
    }

    #[test]
    fn cookie_flags_are_reported() {
        let report = analyze_headers("https://x", &fetched(200, &[("Set-Cookie", "sid=1")]));
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("cookie missing") && l.contains("secure")));
    }
}
