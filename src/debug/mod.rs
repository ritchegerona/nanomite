//! Advanced auto-debug & auto-fix engine for nanomite code writing.
//!
//! This module parses compiler errors, test failures, and runtime panics
//! from build/test/output text, classifies them into structured diagnostics,
//! and feeds them to the auto-fix generator (see [`fix`]) which proposes
//! patches via the improve engine.
//!
//! All parsing is pure Rust, zero dependencies — diagnostics are extracted
//! from plain text using regex so it works on any `cargo test --lib`,
//! `cargo build`, or `run_command` output.
//!
//! ## Safety
//!
//! This module only *reads* output and *stages* proposals. It never applies
//! changes itself — the improve engine's regression gate (`cargo test --lib`)
//! must pass before any fix is kept, and a human approval gate sits above
//! that. No code is ever auto-applied without explicit user action.

mod auto_apply;
mod clippy;
mod diagnostics;
mod fix;
mod fix_ext;
mod history;
mod parser;
mod self_heal;

pub use auto_apply::{AutoApplyConfig, AutoApplyGate, AutoApplyPolicy, AutoApplyResult};
pub use clippy::ClippyParser;
pub use diagnostics::{DebugDiagnostic, DiagnosticKind, Severity};
pub use fix::{stage_fixes, FixGenerator, FixResult};
pub use fix_ext::{stage_ext_fixes, FixExtGenerator, FixExtResult};
pub use history::{FixDecision, FixHistory, FixHistoryRecord};
pub use self_heal::SelfHeal;
pub use parser::DiagnosticsParser;

/// How confident the auto-fix generator is in a proposed fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixConfidence {
    /// The fix is a well-known pattern (e.g. missing import, type mismatch
    /// with obvious import). Safe to auto-propose.
    High,
    /// The fix needs human review (e.g. logic error, lifetime issue).
    Medium,
    /// The fix requires significant refactor; only log as a technique.
    Low,
}

impl FixConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// Result of a debug session: parsed diagnostics plus generated fix proposals.
#[derive(Debug, Clone)]
pub struct DebugSession {
    pub source: String,
    pub diagnostics: Vec<DebugDiagnostic>,
    pub proposed_fixes: Vec<FixProposal>,
}

/// A proposed fix tied to a specific diagnostic.
#[derive(Debug, Clone)]
pub struct FixProposal {
    pub diagnostic_id: String,
    pub file_path: String,
    pub old_content: String,
    pub new_content: String,
    pub rationale: String,
    pub confidence: FixConfidence,
}

/// Parse raw tool/build output and return structured diagnostics.
pub fn debug_output(source: &str, output: &str) -> DebugSession {
    let parser = DiagnosticsParser::new();
    let diagnostics = parser.parse(output);
    DebugSession {
        source: source.to_string(),
        diagnostics,
        proposed_fixes: Vec::new(), // populated by fix() after analysis
    }
}
