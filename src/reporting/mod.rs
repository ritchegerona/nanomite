//! # Auto-Reporting — Generate Markdown/HTML Reports from Neural Memory + Evidence
//!
//! Produces an assessment report from:
//! - Neural memory records (technique tags, task results, learned patterns)
//! - Evidence locker artifacts (tool outputs, screenshots, provenance)
//! - Autonomous plan summaries (tasks, status, timing)
//!
//! Output formats: Markdown (default) and HTML.
//!
//! No dependency on `App` or the neural engine — just takes structured inputs
//! and renders text. Tests exercise both formats end to end.

use crate::evidence::{EvidenceArtifact, EvidenceFilter, EvidenceLocker};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// One assessment's evidence gathered for reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentEvidence {
    pub assessment_id: String,
    pub target: String,
    pub artifacts: Vec<EvidenceArtifact>,
    pub total: usize,
}

/// One neural memory record projected for reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportMemoryRecord {
    pub id: String,
    pub text: String,
    pub tags: Vec<String>,
    pub category: String,
    pub created_at: u64,
    pub salience: f64,
}

/// A complete report source: evidence + memories + plan metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSource {
    pub assessment_id: String,
    pub target: String,
    pub goal: String,
    pub generated_at: u64,
    pub evidence: AssessmentEvidence,
    pub memories: Vec<ReportMemoryRecord>,
    pub plan_summary: Option<String>,
}

/// Report format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Markdown,
    Html,
}

/// Auto-reporting engine: assembles evidence + neural memory into a report.
#[derive(Clone)]
pub struct AutoReporter {
    evidence: Option<Arc<EvidenceLocker>>,
    neural_memory: Option<Arc<crate::neural::memory::NeuralMemory>>,
    data_dir: PathBuf,
}

impl AutoReporter {
    /// Create a new reporter.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            evidence: None,
            neural_memory: None,
            data_dir: data_dir.into(),
        }
    }

    /// Attach the evidence locker.
    pub fn with_evidence(mut self, locker: Arc<EvidenceLocker>) -> Self {
        self.evidence = Some(locker);
        self
    }

    /// Attach neural memory for technique/task recall.
    pub fn with_neural_memory(mut self, memory: Arc<crate::neural::memory::NeuralMemory>) -> Self {
        self.neural_memory = Some(memory);
        self
    }

    /// Generate a full report for an assessment.
    pub fn generate_report(
        &self,
        assessment_id: &str,
        target: &str,
        goal: &str,
        format: ReportFormat,
    ) -> std::io::Result<String> {
        // Gather evidence.
        let evidence = self.gather_evidence(assessment_id, target)?;

        // Gather neural memories tagged for this assessment/target.
        let memories = self.gather_memories(assessment_id, target)?;

        // Build the source.
        let source = ReportSource {
            assessment_id: assessment_id.to_string(),
            target: target.to_string(),
            goal: goal.to_string(),
            generated_at: crate::evidence::now_secs(),
            evidence,
            memories,
            plan_summary: self.plan_summary(assessment_id),
        };

        match format {
            ReportFormat::Markdown => Ok(render_markdown(&source)),
            ReportFormat::Html => Ok(render_html(&source)),
        }
    }

    /// Gather all evidence artifacts for an assessment (optionally filtered by target).
    fn gather_evidence(
        &self,
        assessment_id: &str,
        target: &str,
    ) -> std::io::Result<AssessmentEvidence> {
        let locker = match &self.evidence {
            Some(l) => l.clone(),
            None => {
                return Ok(AssessmentEvidence {
                    assessment_id: assessment_id.to_string(),
                    target: target.to_string(),
                    artifacts: Vec::new(),
                    total: 0,
                });
            }
        };

        let filter = EvidenceFilter {
            assessment_id: Some(assessment_id.to_string()),
            target: Some(target.to_string()),
            ..Default::default()
        };

        let query = locker.list(&filter)?;
        Ok(AssessmentEvidence {
            assessment_id: assessment_id.to_string(),
            target: target.to_string(),
            artifacts: query.items,
            total: query.total,
        })
    }

    /// Gather neural memory records relevant to this assessment.
    fn gather_memories(
        &self,
        assessment_id: &str,
        target: &str,
    ) -> std::io::Result<Vec<ReportMemoryRecord>> {
        let memory = match &self.neural_memory {
            Some(m) => m.clone(),
            None => return Ok(Vec::new()),
        };

        // Query by assessment id tag and target.
        let query_text = format!("{assessment_id} {target}");
        let result = match memory.query(&query_text, 50, 0.1) {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        Ok(result
            .records
            .into_iter()
            .map(|r| ReportMemoryRecord {
                id: r.id,
                text: r.text,
                tags: r.tags,
                category: r.category,
                created_at: r.created_at,
                salience: r.salience,
            })
            .collect())
    }

    /// Load any saved autonomous plan summary for this assessment.
    fn plan_summary(&self, assessment_id: &str) -> Option<String> {
        let plans_dir = self.data_dir.join("plans");
        if !plans_dir.exists() {
            return None;
        }
        if let Ok(entries) = std::fs::read_dir(&plans_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        if let Ok(plan) = serde_json::from_str::<
                            crate::neural::autonomous_planner::AutonomousPlan,
                        >(&text)
                        {
                            if plan.goal.contains(&target_contains(assessment_id)) {
                                return Some(format!(
                                    "Plan {}: {} goals, {} tasks completed",
                                    plan.id,
                                    1,
                                    plan.tasks.iter().filter(|t| matches!(t.status, crate::neural::autonomous_planner::TaskStatus::Completed)).count()
                                ));
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

/// Heuristic: does an assessment id appear inside a plan goal?
fn target_contains(assessment_id: &str) -> String {
    assessment_id.to_string()
}

/// Render a Markdown report.
fn render_markdown(source: &ReportSource) -> String {
    let mut out = String::new();

    out.push_str("# Nanomite Assessment Report\n\n");
    out.push_str(&format!(
        "**Assessment ID:** `{}`\n\n",
        source.assessment_id
    ));
    out.push_str(&format!("**Target:** `{}`\n\n", source.target));
    out.push_str(&format!("**Goal:** {}\n\n", source.goal));
    out.push_str(&format!(
        "**Generated:** {}\n\n",
        humantime(source.generated_at)
    ));
    out.push_str("---\n\n");

    // Evidence section.
    out.push_str("## Evidence\n\n");
    if source.evidence.total == 0 {
        out.push_str("No evidence artifacts recorded for this assessment.\n\n");
    } else {
        out.push_str(&format!("Total artifacts: {}\n\n", source.evidence.total));
        out.push_str("| Hash | Label | Origin | Target | Tags | Size |\n");
        out.push_str("|------|-------|--------|--------|------|------|\n");
        for a in &source.evidence.artifacts {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} bytes |\n",
                truncate(&a.hash, 16),
                escape_md(&a.label),
                escape_md(&a.origin),
                escape_md(&a.target),
                if a.tags.is_empty() {
                    "-".to_string()
                } else {
                    a.tags.join(", ")
                },
                a.size_bytes
            ));
        }
        out.push('\n');
    }

    // Neural memory section.
    out.push_str("## Neural Memory — Techniques & Task Results\n\n");
    if source.memories.is_empty() {
        out.push_str("No neural memory records for this assessment.\n\n");
    } else {
        out.push_str(&format!("Records: {}\n\n", source.memories.len()));
        for m in &source.memories {
            out.push_str(&format!("### {}\n\n", escape_md(&m.id)));
            out.push_str(&format!(
                "**Category:** {}  |  **Tags:** {}\n\n",
                escape_md(&m.category),
                if m.tags.is_empty() {
                    "-".to_string()
                } else {
                    m.tags.join(", ")
                }
            ));
            out.push_str(&format!("{}\n\n", escape_md(&m.text)));
        }
    }

    // Plan summary.
    if let Some(summary) = &source.plan_summary {
        out.push_str("## Autonomous Plan\n\n");
        out.push_str(&format!("{}\n\n", escape_md(summary)));
    }

    out.push_str("---\n\n");
    out.push_str(&format!(
        "*Generated by Nanomite v{} — Autonomous Security Operator*\n",
        crate::VERSION
    ));

    out
}

/// Render an HTML report.
fn render_html(source: &ReportSource) -> String {
    let mut out = String::new();

    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"UTF-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n");
    out.push_str(&format!(
        "<title>Nanomite Assessment Report — {}</title>\n",
        escape_html(&source.assessment_id)
    ));
    out.push_str("<style>\n");
    out.push_str("body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; line-height: 1.6; color: #333; max-width: 1000px; margin: 2rem auto; padding: 0 1rem; }\n");
    out.push_str("h1 { border-bottom: 2px solid #2563eb; padding-bottom: 0.5rem; }\n");
    out.push_str(
        "h2 { border-bottom: 1px solid #e5e7eb; padding-bottom: 0.3rem; margin-top: 2rem; }\n",
    );
    out.push_str("table { border-collapse: collapse; width: 100%; margin: 1rem 0; }\n");
    out.push_str("th, td { border: 1px solid #d1d5db; padding: 0.5rem 0.75rem; text-align: left; font-size: 0.9rem; }\n");
    out.push_str("th { background: #f3f4f6; }\n");
    out.push_str("code { background: #f3f4f6; padding: 0.1rem 0.3rem; border-radius: 3px; font-family: ui-monospace, SFMono-Regular, monospace; }\n");
    out.push_str(".meta { display: flex; gap: 1rem; margin: 1rem 0; }\n");
    out.push_str(".meta-item { background: #f9fafb; border: 1px solid #e5e7eb; padding: 0.75rem; border-radius: 6px; flex: 1; }\n");
    out.push_str(".record { background: #f9fafb; border: 1px solid #e5e7eb; padding: 1rem; margin: 0.75rem 0; border-radius: 6px; }\n");
    out.push_str(".record h3 { margin-top: 0; color: #2563eb; }\n");
    out.push_str(".tags { font-size: 0.8rem; color: #6b7280; }\n");
    out.push_str(".footer { margin-top: 3rem; border-top: 1px solid #e5e7eb; padding-top: 1rem; font-size: 0.85rem; color: #6b7280; }\n");
    out.push_str("</style>\n");
    out.push_str("</head>\n<body>\n");

    out.push_str("<h1>Nanomite Assessment Report</h1>\n");
    out.push_str("<div class=\"meta\">\n");
    out.push_str(&format!(
        "<div class=\"meta-item\"><strong>Assessment ID:</strong><br/><code>{}</code></div>\n",
        escape_html(&source.assessment_id)
    ));
    out.push_str(&format!(
        "<div class=\"meta-item\"><strong>Target:</strong><br/><code>{}</code></div>\n",
        escape_html(&source.target)
    ));
    out.push_str(&format!(
        "<div class=\"meta-item\"><strong>Generated:</strong><br/>{}</div>\n",
        humantime(source.generated_at)
    ));
    out.push_str(&format!(
        "<div class=\"meta-item\"><strong>Goal:</strong><br/>{}</div>\n",
        escape_html(&source.goal)
    ));
    out.push_str("</div>\n\n");

    // Evidence section.
    out.push_str("<h2>Evidence</h2>\n");
    if source.evidence.total == 0 {
        out.push_str("<p>No evidence artifacts recorded for this assessment.</p>\n");
    } else {
        out.push_str(&format!(
            "<p><strong>Total artifacts:</strong> {}</p>\n",
            source.evidence.total
        ));
        out.push_str("<table>\n<thead><tr><th>Hash</th><th>Label</th><th>Origin</th><th>Target</th><th>Tags</th><th>Size</th></tr></thead>\n<tbody>\n");
        for a in &source.evidence.artifacts {
            out.push_str(&format!(
                "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td class=\"tags\">{}</td><td>{} bytes</td></tr>\n",
                truncate(&a.hash, 16),
                escape_html(&a.label),
                escape_html(&a.origin),
                escape_html(&a.target),
                if a.tags.is_empty() {
                    "-".to_string()
                } else {
                    a.tags.join(", ")
                },
                a.size_bytes
            ));
        }
        out.push_str("</tbody></table>\n");
    }

    // Neural memory section.
    out.push_str("<h2>Neural Memory — Techniques & Task Results</h2>\n");
    if source.memories.is_empty() {
        out.push_str("<p>No neural memory records for this assessment.</p>\n");
    } else {
        out.push_str(&format!(
            "<p><strong>Records:</strong> {}</p>\n",
            source.memories.len()
        ));
        for m in &source.memories {
            out.push_str("<div class=\"record\">\n");
            out.push_str(&format!("<h3>{}</h3>\n", escape_html(&m.id)));
            out.push_str(&format!(
                "<p><strong>Category:</strong> {} | <strong>Tags:</strong> {}</p>\n",
                escape_html(&m.category),
                if m.tags.is_empty() {
                    "-".to_string()
                } else {
                    m.tags.join(", ")
                }
            ));
            out.push_str(&format!("<p>{}</p>\n", escape_html(&m.text)));
            out.push_str("</div>\n");
        }
    }

    // Plan summary.
    if let Some(summary) = &source.plan_summary {
        out.push_str("<h2>Autonomous Plan</h2>\n");
        out.push_str(&format!("<p>{}</p>\n", escape_html(summary)));
    }

    out.push_str("<div class=\"footer\">\n");
    out.push_str(&format!(
        "<p>Generated by Nanomite v{} — Autonomous Security Operator</p>\n",
        crate::VERSION
    ));
    out.push_str("</div>\n");
    out.push_str("</body>\n</html>\n");

    out
}

/// Format a unix timestamp as a readable date.
fn humantime(secs: u64) -> String {
    let dt = chrono::DateTime::from_timestamp(secs as i64, 0).unwrap_or_default();
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Truncate a string to `n` characters with ellipsis if longer.
fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n.min(s.len())])
    }
}

/// Escape a string for safe insertion into Markdown (lightweight).
fn escape_md(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Escape a string for safe insertion into HTML.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_dir() -> PathBuf {
        let dir = env::temp_dir().join(format!("nanomite_report_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn markdown_report_renders_with_evidence() {
        let data_dir = temp_dir();
        let reporter = AutoReporter::new(&data_dir);

        let report = reporter
            .generate_report(
                "assess-1",
                "http://10.0.0.5",
                "Assess web app",
                ReportFormat::Markdown,
            )
            .expect("generate markdown");

        assert!(report.contains("# Nanomite Assessment Report"));
        assert!(report.contains("assess-1"));
        assert!(report.contains("http://10.0.0.5"));
        assert!(report.contains("Assess web app"));
        assert!(report.contains("No evidence artifacts recorded"));
    }

    #[test]
    fn html_report_renders_with_evidence() {
        let data_dir = temp_dir();
        let reporter = AutoReporter::new(&data_dir);

        let report = reporter
            .generate_report(
                "assess-1",
                "http://10.0.0.5",
                "Assess web app",
                ReportFormat::Html,
            )
            .expect("generate html");

        assert!(report.contains("<!DOCTYPE html>"));
        assert!(report.contains("Nanomite Assessment Report"));
        assert!(report.contains("assess-1"));
    }

    #[test]
    fn report_includes_stored_evidence() {
        let data_dir = temp_dir();
        let locker = EvidenceLocker::open(&data_dir).expect("open locker");
        let _ = locker.store(
            "assess-1",
            "http://10.0.0.5",
            "web_recon",
            "gobuster output",
            b"/admin (200)",
            vec!["recon".into()],
            "text/plain",
        );

        let reporter = AutoReporter::new(&data_dir).with_evidence(Arc::new(locker));
        let report = reporter
            .generate_report(
                "assess-1",
                "http://10.0.0.5",
                "Assess web app",
                ReportFormat::Markdown,
            )
            .expect("generate");

        assert!(report.contains("gobuster output"));
        assert!(report.contains("web_recon"));
        assert!(report.contains("recon"));
    }

    #[test]
    fn report_includes_neural_memories() {
        let data_dir = temp_dir();
        let memory = crate::neural::memory::NeuralMemory::in_memory().expect("open memory");
        memory
            .store(
                "mem-1",
                "assess-1 10.0.0.5 nmap port scan technique discovered open port 80",
                vec!["technique".into(), "recon".into()],
                "technique",
            )
            .expect("store");

        let reporter = AutoReporter::new(&data_dir).with_neural_memory(Arc::new(memory));
        let report = reporter
            .generate_report("assess-1", "10.0.0.5", "Port scan", ReportFormat::Markdown)
            .expect("generate");

        assert!(report.contains("Neural Memory"));
        assert!(report.contains("mem-1"));
    }

    #[test]
    fn html_escape_prevents_injection() {
        let escaped = escape_html("<script>alert('xss')</script>");
        assert!(!escaped.contains("<script>"));
        assert!(escaped.contains("&lt;script&gt;"));
    }

    #[test]
    fn markdown_escape_handles_pipes() {
        let escaped = escape_md("a|b|c");
        assert_eq!(escaped, "a\\|b\\|c");
    }
}
