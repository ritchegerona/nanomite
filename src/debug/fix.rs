//! Auto-fix generator: analyzes parsed diagnostics and proposes code patches
//! via the improve engine (`src/improve/`).
//!
//! For each `DebugDiagnostic` with a known fix pattern, this module generates
//! a `FixProposal` containing old/new content, rationale, and confidence.
//! Proposals are staged through `ImproveEngine::propose()` — they remain
//! `Pending` until a human approves them in `/improve`, after which the
//! regression gate (`cargo test --lib`) decides whether they stick.
//!
//! ## Fix patterns supported
//!
//! - **Missing import** (`E0433`/`E0601`): suggests adding `use` for the
//!   unresolved symbol if we can infer the likely path.
//! - **Type mismatch** (`E0308`): suggests adding `.to_string()` / `.into()`
//!   or explicit type annotation when the expected type is visible.
//! - **Missing dependency** (`E0463`): suggests adding to Cargo.toml.
//! - **Panic / test failure**: logs as a technique for the neural memory.

use super::diagnostics::{DebugDiagnostic, DiagnosticKind, Severity};
use super::FixProposal;
use super::FixConfidence;
use crate::improve::{Actor, ImproveEngine, Proposal, Roots, Status, Target};

/// Result of auto-fix generation: proposals plus non-fixable diagnostics.
#[derive(Debug, Clone, Default)]
pub struct FixResult {
    pub proposals: Vec<FixProposal>,
    pub unfixable: Vec<DebugDiagnostic>,
}

/// Analyzes diagnostics and generates fix proposals.
pub struct FixGenerator;

impl FixGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Generate `FixProposal`s for all fixable diagnostics in a session.
    pub fn generate(&self, session: &super::DebugSession) -> FixResult {
        let mut proposals = Vec::new();
        let mut unfixable = Vec::new();

        for diagnostic in &session.diagnostics {
            if !diagnostic.has_known_fix {
                unfixable.push(diagnostic.clone());
                continue;
            }

            match diagnostic.kind {
                DiagnosticKind::UnresolvedImport => {
                    if let Some(proposal) = self.fix_unresolved_import(diagnostic) {
                        proposals.push(proposal);
                    } else {
                        unfixable.push(diagnostic.clone());
                    }
                }
                DiagnosticKind::TypeMismatch => {
                    if let Some(proposal) = self.fix_type_mismatch(diagnostic) {
                        proposals.push(proposal);
                    } else {
                        unfixable.push(diagnostic.clone());
                    }
                }
                DiagnosticKind::MissingDependency => {
                    if let Some(proposal) = self.fix_missing_dependency(diagnostic) {
                        proposals.push(proposal);
                    } else {
                        unfixable.push(diagnostic.clone());
                    }
                }
                _ => {
                    unfixable.push(diagnostic.clone());
                }
            }
        }

        FixResult { proposals, unfixable }
    }

    // --- individual fix patterns -------------------------------------------

    fn fix_unresolved_import(&self, d: &DebugDiagnostic) -> Option<FixProposal> {
        // Extract the missing symbol from messages like:
        // "could not find `Foo` in scope" or "cannot find `Bar` in this scope"
        let symbol = extract_quoted(&d.message)?;
        // Heuristic: suggest `use crate::{symbol};` as a common pattern.
        let old = "use crate::;\n";
        let new = format!("use crate::{};\n", symbol);
        Some(FixProposal {
            diagnostic_id: d.id.clone(),
            file_path: d.file.clone(),
            old_content: old.to_string(),
            new_content: new.clone(),
            rationale: format!(
                "Unresolved import `{symbol}` in {}:{} — adding `use crate::{symbol};` \
                 is a common fix pattern.",
                d.file, d.line
            ),
            confidence: FixConfidence::Medium,
        })
    }

    fn fix_type_mismatch(&self, d: &DebugDiagnostic) -> Option<FixProposal> {
        // Messages like "expected `String`, found `&str`" — suggest `.to_string()`
        let expected = extract_expected_type(&d.message)?;
        let found = extract_found_type(&d.message)?;
        let new_content = format!(".to_string()  // expected {expected}, found {found}");
        Some(FixProposal {
            diagnostic_id: d.id.clone(),
            file_path: d.file.clone(),
            old_content: String::new(),
            new_content,
            rationale: format!(
                "Type mismatch at {}:{} — expected {expected}, found {found}. \
                 Consider converting with .to_string() or an explicit cast.",
                d.file, d.line
            ),
            confidence: FixConfidence::Medium,
        })
    }

    fn fix_missing_dependency(&self, d: &DebugDiagnostic) -> Option<FixProposal> {
        let crate_name = extract_quoted(&d.message)?;
        let new_content = format!("{crate_name} = \"*\"\n");
        Some(FixProposal {
            diagnostic_id: d.id.clone(),
            file_path: "Cargo.toml".to_string(),
            old_content: String::new(),
            new_content,
            rationale: format!(
                "Missing dependency `{crate_name}` — add to [dependencies] in Cargo.toml."
            ),
            confidence: FixConfidence::Low,
        })
    }
}

/// Stage all fixable proposals into the improve engine for human review.
pub fn stage_fixes(
    engine: &ImproveEngine,
    roots: &Roots,
    result: &FixResult,
) -> Vec<(String, Result<String, String>)> {
    let mut staged = Vec::new();
    for proposal in &result.proposals {
        let target = if proposal.file_path == "Cargo.toml" {
            Target::Config
        } else {
            Target::Source
        };
        let path = std::path::PathBuf::from(&proposal.file_path);
        let actor = Actor::Agent;
        let rationale = proposal.rationale.clone();

        match engine.propose(
            actor,
            target,
            path,
            proposal.new_content.clone(),
            rationale,
            roots,
        ) {
            Ok(p) => {
                let id = p.id.clone();
                staged.push((id.clone(), Ok(id)));
            }
            Err(e) => staged.push((proposal.diagnostic_id.clone(), Err(e.to_string()))),
        }
    }
    staged
}

/// Extract a quoted symbol like `Foo` from an error message.
fn extract_quoted(message: &str) -> Option<String> {
    let start = message.find('`')?;
    let after = &message[start + 1..];
    let end = after.find('`')?;
    Some(after[..end].to_string())
}

/// Extract the expected type from "expected `X`, found `Y`".
fn extract_expected_type(message: &str) -> Option<String> {
    let marker = "expected `";
    let start = message.find(marker)?;
    let after = &message[start + marker.len()..];
    let end = after.find('`')?;
    Some(after[..end].to_string())
}

/// Extract the found type from "expected `X`, found `Y`".
fn extract_found_type(message: &str) -> Option<String> {
    let marker = "found `";
    let start = message.find(marker)?;
    let after = &message[start + marker.len()..];
    let end = after.find('`')?;
    Some(after[..end].to_string())
}

#[cfg(test)]
mod tests {
    use crate::debug::{
        DebugDiagnostic, DebugSession, DiagnosticKind, Severity,
    };
    use super::{extract_expected_type, extract_found_type, extract_quoted, FixGenerator};

    #[test]
    fn generates_proposal_for_unresolved_import() {
        let diag = DebugDiagnostic::new(
            "E0433-src/lib.rs:3:5".into(),
            DiagnosticKind::UnresolvedImport,
            Severity::Error,
            "src/lib.rs".into(),
            3,
            5,
            "could not find `Foo` in scope".into(),
            vec![],
        );
        let session = crate::debug::DebugSession {
            source: "test".into(),
            diagnostics: vec![diag],
            proposed_fixes: Vec::new(),
        };
        let gen = FixGenerator::new();
        let result = gen.generate(&session);
        assert_eq!(result.proposals.len(), 1);
        assert!(result.unfixable.is_empty());
        assert!(result.proposals[0].new_content.contains("use crate::Foo"));
    }

    #[test]
    fn unfixable_diagnostics_are_collected() {
        let diag = DebugDiagnostic::new(
            "E0308-src/lib.rs:1:1".into(),
            DiagnosticKind::Lifetime,
            Severity::Error,
            "src/lib.rs".into(),
            1,
            1,
            "lifetime may not live long enough".into(),
            vec![],
        );
        let session = crate::debug::DebugSession {
            source: "test".into(),
            diagnostics: vec![diag],
            proposed_fixes: Vec::new(),
        };
        let gen = FixGenerator::new();
        let result = gen.generate(&session);
        assert!(result.proposals.is_empty());
        assert_eq!(result.unfixable.len(), 1);
    }

    #[test]
    fn extracts_quoted_symbol() {
        assert_eq!(extract_quoted("could not find `Foo` in scope"), Some("Foo".into()));
        assert_eq!(extract_quoted("no backticks here"), None);
    }

    #[test]
    fn extract_expected_and_found_types() {
        let msg = "expected `String`, found `&str`";
        assert_eq!(extract_expected_type(msg), Some("String".into()));
        assert_eq!(extract_found_type(msg), Some("&str".into()));
    }
}
