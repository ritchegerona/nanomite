//! Phase 2 security modules: reconnaissance, network, web and vulnerability
//! analysis.
//!
//! Every module splits its work into **pure, testable logic** and a small
//! injected I/O trait (resolver / connector / fetcher), so the suite runs
//! fully offline and no module reaches the network during tests. The
//! [`super::coordinator`] authorizes and audits; modules never bypass it.

use super::findings::Finding;

pub mod advanced_cloud;
pub mod advanced_evasion;
pub mod advanced_network;
pub mod advanced_recon;
pub mod advanced_web;
pub mod attack_path;
pub mod c2_simulation;
pub mod exploitation;
pub mod kali;
pub mod network;
pub mod post_exploitation;
pub mod recon;
pub mod reporting;
pub mod supply_chain;
pub mod vuln;
pub mod web;

/// A bounded report from one module operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleReport {
    pub module: &'static str,
    pub operation: &'static str,
    pub target: String,
    pub summary: String,
    /// Body lines for display; always bounded by the module.
    pub lines: Vec<String>,
    /// Structured findings this operation produced. The coordinator persists
    /// these (via the injected `FindingSink`); the display `lines` are derived
    /// and are not a source of truth.
    pub findings: Vec<Finding>,
}

impl ModuleReport {
    pub fn new(
        module: &'static str,
        operation: &'static str,
        target: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            module,
            operation,
            target: target.into(),
            summary: summary.into(),
            lines: Vec::new(),
            findings: Vec::new(),
        }
    }

    pub fn line(mut self, line: impl Into<String>) -> Self {
        self.lines.push(line.into());
        self
    }

    /// Attaches one structured finding.
    pub fn finding(mut self, finding: Finding) -> Self {
        self.findings.push(finding);
        self
    }

    /// Renders the report for the transcript: a title line, the summary and
    /// the body.
    pub fn render(&self) -> Vec<String> {
        let mut out = vec![format!(
            "[hacker:{}] {} · {}",
            self.module, self.operation, self.target
        )];
        out.push(format!("  {}", self.summary));
        for line in &self.lines {
            out.push(format!("  {line}"));
        }
        out
    }
}
