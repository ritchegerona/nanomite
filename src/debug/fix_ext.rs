//! Extended auto-fix patterns beyond the base `fix.rs` module.
//!
//! Handles:
//! - **Lifetime/borrow issues** (`E0499`, `E0502`, `E0507`, `E0597`): suggests
//!   `.clone()`, explicit lifetime annotations, or `&` / `&mut` adjustments.
//! - **Clippy warnings** (`clippy::...`): suggests removing unused imports,
//!   replacing `.unwrap()` with error handling, dead code removal.
//! - **Dead code** (`dead_code` lint): suggests prefixing with `_` or removing.
//! - **Unused imports** (`unused_imports`): suggests removing the import line.
//!
//! All proposals are generated as `FixProposal`s and staged through the
//! improve engine — they remain `Pending` until human approval, and the
//! regression gate (`cargo test --lib`) decides whether they stick.

use super::diagnostics::{DebugDiagnostic, DiagnosticKind, Severity};
use super::{FixConfidence, FixGenerator, FixProposal};
use crate::improve::{Actor, ImproveEngine, Roots, Target};

/// Result of extended fix generation.
#[derive(Debug, Clone, Default)]
pub struct FixExtResult {
    pub proposals: Vec<FixProposal>,
    pub unfixable: Vec<DebugDiagnostic>,
}

/// Extended fix generator for patterns not covered by `fix.rs`.
pub struct FixExtGenerator;

impl FixExtGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Generate `FixProposal`s for all fixable diagnostics in a session,
    /// covering lifetime, clippy, dead-code, and unused-import patterns.
    pub fn generate(&self, session: &super::DebugSession) -> FixExtResult {
        let mut proposals = Vec::new();
        let mut unfixable = Vec::new();

        for diagnostic in &session.diagnostics {
            if !diagnostic.has_known_fix {
                unfixable.push(diagnostic.clone());
                continue;
            }

            match diagnostic.kind {
                DiagnosticKind::Lifetime => {
                    if let Some(p) = self.fix_lifetime(diagnostic) {
                        proposals.push(p);
                    } else {
                        unfixable.push(diagnostic.clone());
                    }
                }
                DiagnosticKind::Compiler => {
                    // Clippy warnings come through as Compiler kind with clippy codes
                    if let Some(p) = self.fix_clippy_warning(diagnostic) {
                        proposals.push(p);
                    } else {
                        unfixable.push(diagnostic.clone());
                    }
                }
                _ => {
                    unfixable.push(diagnostic.clone());
                }
            }
        }

        FixExtResult { proposals, unfixable }
    }

    // --- individual fix patterns -------------------------------------------

    /// Fix lifetime/borrow errors by suggesting `.clone()` or `&` adjustments.
    fn fix_lifetime(&self, d: &DebugDiagnostic) -> Option<FixProposal> {
        let message = &d.message;

        // E0499: cannot borrow `x` as mutable more than once at a time
        if message.contains("cannot borrow") || message.contains("borrow of moved") {
            let old = format!("{}", d.context.first().cloned().unwrap_or_default());
            let new = format!(
                "{}  // .clone() to avoid borrow conflict",
                d.context.first().cloned().unwrap_or_default()
            );
            return Some(FixProposal {
                diagnostic_id: d.id.clone(),
                file_path: d.file.clone(),
                old_content: old.clone(),
                new_content: new.clone(),
                rationale: format!(
                    "Borrow conflict at {}:{} — consider `.clone()` or restructuring \
                     to avoid simultaneous mutable borrows.",
                    d.file, d.line
                ),
                confidence: FixConfidence::Medium,
            });
        }

        // E0597: variable does not live long enough
        if message.contains("does not live long enough") {
            return Some(FixProposal {
                diagnostic_id: d.id.clone(),
                file_path: d.file.clone(),
                old_content: String::new(),
                new_content: "  // consider extending lifetime or using Arc<RwLock<T>>\n".to_string(),
                rationale: format!(
                    "Lifetime error at {}:{} — variable dropped before use.",
                    d.file, d.line
                ),
                confidence: FixConfidence::Medium,
            });
        }

        None
    }

    /// Fix clippy warnings: unused imports, unwrap usage, dead code hints.
    fn fix_clippy_warning(&self, d: &DebugDiagnostic) -> Option<FixProposal> {
        let message = &d.message;

        // unused_imports
        if message.contains("unused import") || message.contains("unused_imports") {
            let symbol = extract_quoted(message)?;
            return Some(FixProposal {
                diagnostic_id: d.id.clone(),
                file_path: d.file.clone(),
                old_content: format!("use {};\n", symbol),
                new_content: String::new(),
                rationale: format!(
                    "Unused import `{symbol}` at {}:{} — removing keeps the build clean.",
                    d.file, d.line
                ),
                confidence: FixConfidence::High,
            });
        }

        // unwrap_used / expect_used
        if message.contains("unwrap()") || message.contains("expect(") {
            return Some(FixProposal {
                diagnostic_id: d.id.clone(),
                file_path: d.file.clone(),
                old_content: String::new(),
                new_content: "  // consider `.map_err()` or `?` instead of `.unwrap()`\n".to_string(),
                rationale: format!(
                    "Clippy suggests avoiding `.unwrap()`/`.expect()` at {}:{} — handle the error explicitly.",
                    d.file, d.line
                ),
                confidence: FixConfidence::Low,
            });
        }

        // dead_code
        if message.contains("dead_code")
            || message.contains("dead code")
            || message.contains("never read")
            || message.contains("never used")
        {
            let symbol = extract_quoted(message).unwrap_or_else(|| "item".to_string());
            return Some(FixProposal {
                diagnostic_id: d.id.clone(),
                file_path: d.file.clone(),
                old_content: format!("{symbol}\n"),
                new_content: format!("_{symbol}\n"),
                rationale: format!(
                    "Dead code `{symbol}` at {}:{} — prefix with `_` to silence.",
                    d.file, d.line
                ),
                confidence: FixConfidence::High,
            });
        }

        // needless_borrow / redundant_clone
        if message.contains("needless borrow") || message.contains("redundant clone") {
            return Some(FixProposal {
                diagnostic_id: d.id.clone(),
                file_path: d.file.clone(),
                old_content: String::new(),
                new_content: "  // remove needless borrow or redundant clone\n".to_string(),
                rationale: format!(
                    "Clippy cleanup at {}:{} — remove unnecessary borrow/clone.",
                    d.file, d.line
                ),
                confidence: FixConfidence::High,
            });
        }

        None
    }
}

/// Stage all extended fixable proposals into the improve engine.
pub fn stage_ext_fixes(
    engine: &ImproveEngine,
    roots: &Roots,
    result: &FixExtResult,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::{DebugDiagnostic, DebugSession, DiagnosticKind, Severity};

    fn make_diag(kind: DiagnosticKind, message: &str, has_known_fix: bool) -> DebugDiagnostic {
        DebugDiagnostic::new(
            "test-id".into(),
            kind,
            Severity::Error,
            "src/lib.rs".into(),
            1,
            1,
            message.into(),
            vec![],
        )
    }

    #[test]
    fn unused_import_proposal() {
        let mut d = make_diag(DiagnosticKind::Compiler, "unused import `foo`", true);
        d.has_known_fix = true;
        let session = DebugSession {
            source: "test".into(),
            diagnostics: vec![d],
            proposed_fixes: Vec::new(),
        };
        let gen = FixExtGenerator::new();
        let result = gen.generate(&session);
        assert_eq!(result.proposals.len(), 1);
        assert_eq!(result.proposals[0].confidence, FixConfidence::High);
    }

    #[test]
    fn dead_code_proposal() {
        let mut d = make_diag(DiagnosticKind::Compiler, "dead code `bar`", true);
        d.has_known_fix = true;
        let session = DebugSession {
            source: "test".into(),
            diagnostics: vec![d],
            proposed_fixes: Vec::new(),
        };
        let gen = FixExtGenerator::new();
        let result = gen.generate(&session);
        assert_eq!(result.proposals.len(), 1);
        assert!(result.proposals[0].new_content.contains("_bar"));
    }

    #[test]
    fn unfixable_without_known_fix() {
        let d = make_diag(DiagnosticKind::Lifetime, "lifetime may not live long enough", false);
        let session = DebugSession {
            source: "test".into(),
            diagnostics: vec![d],
            proposed_fixes: Vec::new(),
        };
        let gen = FixExtGenerator::new();
        let result = gen.generate(&session);
        assert!(result.proposals.is_empty());
        assert_eq!(result.unfixable.len(), 1);
    }

    #[test]
    fn borrow_conflict_proposal() {
        let mut d = make_diag(
            DiagnosticKind::Lifetime,
            "cannot borrow `x` as mutable more than once at a time",
            true,
        );
        d.has_known_fix = true;
        d.context = vec!["  let x = &mut y;".to_string()];
        let session = DebugSession {
            source: "test".into(),
            diagnostics: vec![d],
            proposed_fixes: Vec::new(),
        };
        let gen = FixExtGenerator::new();
        let result = gen.generate(&session);
        assert_eq!(result.proposals.len(), 1);
        assert!(result.proposals[0].rationale.contains("Borrow conflict"));
    }
}
