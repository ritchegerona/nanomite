//! Findings: the structured, persistable output of an assessment operation.
//!
//! A [`ModuleReport`](super::modules::ModuleReport) is display text; a
//! [`Finding`] is the durable record a report is built from. Findings are
//! **untrusted data**: their title/detail come from response headers, tool
//! output and CVE metadata, so they are bounded here and scrubbed again before
//! they are written (see [`super::store`]).
//!
//! Severity reuses [`RiskLevel`] (the ascending informational→critical scale)
//! so the crate keeps one severity vocabulary rather than a second parallel
//! enum.

use super::risk::RiskLevel;

/// Maximum characters kept in a finding title.
pub const MAX_TITLE_CHARS: usize = 200;
/// Maximum characters kept in a finding detail body.
pub const MAX_DETAIL_CHARS: usize = 2_000;

/// How well a finding has been established.
///
/// Version/header heuristics can only ever produce [`Confidence::Suspected`];
/// `Confirmed` means a validated reproduction and is never inferred from a
/// version match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    Suspected,
    Likely,
    Confirmed,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::Suspected => "suspected",
            Confidence::Likely => "likely",
            Confidence::Confirmed => "confirmed",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "suspected" => Some(Confidence::Suspected),
            "likely" => Some(Confidence::Likely),
            "confirmed" => Some(Confidence::Confirmed),
            _ => None,
        }
    }
}

/// Triage state of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingStatus {
    Open,
    Triaged,
    Accepted,
    Closed,
}

impl FindingStatus {
    pub fn label(self) -> &'static str {
        match self {
            FindingStatus::Open => "open",
            FindingStatus::Triaged => "triaged",
            FindingStatus::Accepted => "accepted",
            FindingStatus::Closed => "closed",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "open" => Some(FindingStatus::Open),
            "triaged" => Some(FindingStatus::Triaged),
            "accepted" => Some(FindingStatus::Accepted),
            "closed" => Some(FindingStatus::Closed),
            _ => None,
        }
    }
}

/// One durable finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub module: &'static str,
    pub operation: &'static str,
    pub target: String,
    pub title: String,
    pub severity: RiskLevel,
    pub confidence: Confidence,
    pub status: FindingStatus,
    /// Evidence / rationale, bounded and scrubbed before it reaches disk.
    pub detail: String,
}

impl Finding {
    /// A new finding: `Suspected`, `Open`, no detail. Heuristic producers must
    /// leave the confidence here unless they have real validation.
    pub fn new(
        module: &'static str,
        operation: &'static str,
        target: impl Into<String>,
        title: impl Into<String>,
        severity: RiskLevel,
    ) -> Self {
        Self {
            module,
            operation,
            target: truncate(target.into(), MAX_TITLE_CHARS),
            title: truncate(title.into(), MAX_TITLE_CHARS),
            severity,
            confidence: Confidence::Suspected,
            status: FindingStatus::Open,
            detail: String::new(),
        }
    }

    pub fn confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn status(mut self, status: FindingStatus) -> Self {
        self.status = status;
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = truncate(detail.into(), MAX_DETAIL_CHARS);
        self
    }

    /// One-line, transcript-friendly rendering.
    pub fn line(&self) -> String {
        format!(
            "[{}] {} · {} · {} ({})",
            self.severity.label(),
            self.title,
            self.target,
            self.confidence.label(),
            self.module
        )
    }
}

/// Truncates on a character boundary, appending an ellipsis marker when cut.
fn truncate(text: String, max: usize) -> String {
    if text.chars().count() <= max {
        return text;
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_finding_is_suspected_and_open() {
        let f = Finding::new(
            "vuln",
            "correlate",
            "widget 1.2.4",
            "CVE-1 in widget",
            RiskLevel::High,
        );
        assert_eq!(f.confidence, Confidence::Suspected);
        assert_eq!(f.status, FindingStatus::Open);
        assert_eq!(f.severity, RiskLevel::High);
        assert!(f.line().contains("high"));
        assert!(f.line().contains("suspected"));
    }

    #[test]
    fn confidence_and_status_round_trip() {
        for c in [
            Confidence::Suspected,
            Confidence::Likely,
            Confidence::Confirmed,
        ] {
            assert_eq!(Confidence::parse(c.label()), Some(c));
        }
        for s in [
            FindingStatus::Open,
            FindingStatus::Triaged,
            FindingStatus::Accepted,
            FindingStatus::Closed,
        ] {
            assert_eq!(FindingStatus::parse(s.label()), Some(s));
        }
        assert_eq!(Confidence::parse("nope"), None);
        assert_eq!(FindingStatus::parse("nope"), None);
    }

    #[test]
    fn title_and_detail_are_bounded_on_a_char_boundary() {
        let long = "x".repeat(MAX_DETAIL_CHARS + 500);
        let f = Finding::new("web", "headers", "t", "title", RiskLevel::Low).detail(long);
        assert_eq!(f.detail.chars().count(), MAX_DETAIL_CHARS);
        assert!(f.detail.ends_with('…'));

        // Multi-byte characters must not be split.
        let wide = "é".repeat(MAX_TITLE_CHARS + 10);
        let f = Finding::new("web", "headers", "t", wide, RiskLevel::Low);
        assert_eq!(f.title.chars().count(), MAX_TITLE_CHARS);
    }
}
