//! Reporting: turn persisted findings into a bounded summary or an export.
//!
//! Pure renderers only — the store read/write is injected by the coordinator
//! (see [`super::super::coordinator`]). Nothing here reaches the network or
//! re-reads a target, so both operations are `Passive`.

use std::path::Path;

use super::ModuleReport;
use crate::hacker::store::{ReportFormat, ReportSummary};

/// Renders the finding counts for one assessment.
pub fn summary_report(assessment_id: &str, summary: &ReportSummary) -> ModuleReport {
    let headline = if summary.total == 0 {
        "no findings recorded yet".to_string()
    } else {
        format!("{} finding(s)", summary.total)
    };
    let mut report = ModuleReport::new("reporting", "summary", assessment_id, headline);
    report = report.line(format!(
        "by severity: {}",
        render_counts(&summary.by_severity)
    ));
    report = report.line(format!("by status: {}", render_counts(&summary.by_status)));
    report
}

/// Renders the outcome of a report export.
pub fn export_report(assessment_id: &str, format: ReportFormat, path: &Path) -> ModuleReport {
    ModuleReport::new(
        "reporting",
        "export",
        assessment_id,
        format!("{} report written", format.extension()),
    )
    .line(format!("path: {}", path.display()))
}

fn render_counts(counts: &[(String, usize)]) -> String {
    if counts.is_empty() {
        return "(none)".to_string();
    }
    counts
        .iter()
        .map(|(label, n)| format!("{label}={n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_summary_is_explicit() {
        let report = summary_report("asm-1", &ReportSummary::default());
        assert_eq!(report.summary, "no findings recorded yet");
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("by severity: (none)")));
        assert!(report.lines.iter().any(|l| l.contains("by status: (none)")));
        assert!(report.findings.is_empty());
    }

    #[test]
    fn populated_summary_lists_every_bucket() {
        let summary = ReportSummary {
            total: 4,
            by_severity: vec![("critical".into(), 1), ("low".into(), 3)],
            by_status: vec![("closed".into(), 1), ("open".into(), 3)],
        };
        let report = summary_report("asm-2", &summary);
        assert_eq!(report.summary, "4 finding(s)");
        assert_eq!(report.target, "asm-2");
        assert!(report
            .lines
            .iter()
            .any(|l| l == "by severity: critical=1 low=3"));
        assert!(report
            .lines
            .iter()
            .any(|l| l == "by status: closed=1 open=3"));
    }

    #[test]
    fn export_report_names_the_format_and_path() {
        let path = std::path::Path::new("/tmp/reports/asm-1.json");
        let report = export_report("asm-1", ReportFormat::Json, path);
        assert!(report.summary.contains("json report written"));
        assert!(report.lines.iter().any(|l| l.contains("asm-1.json")));
    }
}
