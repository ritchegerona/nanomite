//! Self-heal pipeline: watches build/test/tool output for failures,
//! runs them through the auto-debug parser, generates fix proposals via
//! the improve engine, and (optionally) stores learned fixes as techniques
//! in neural memory.
//!
//! The entry point is [`SelfHeal::check_output`] — call it after any
//! `run_command` / `cargo test` / `cargo build` invocation. It returns a
//! `SelfHealResult` describing what was found and what was staged.
//!
//! Nothing is applied automatically: proposals are staged as `Pending` and
//! must be approved in `/improve`, where the regression gate decides.

use crate::debug::{debug_output, FixConfidence, FixGenerator, FixResult};
use crate::improve::{Actor, ImproveEngine, Roots};
use crate::memory::MemoryEngine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Result of a self-heal check on a command's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfHealResult {
    pub command: String,
    pub exit_code: Option<i32>,
    pub diagnostics_found: usize,
    pub fixable: usize,
    pub unfixable: usize,
    pub proposals_staged: Vec<String>,
    pub techniques_saved: Vec<String>,
}

/// Watches output and drives debug → fix → stage → learn.
pub struct SelfHeal {
    engine: Option<std::sync::Arc<ImproveEngine>>,
    roots: Option<Roots>,
    memory: Option<std::sync::Arc<MemoryEngine>>,
    fix_generator: FixGenerator,
}

impl SelfHeal {
    pub fn new() -> Self {
        Self {
            engine: None,
            roots: None,
            memory: None,
            fix_generator: FixGenerator::new(),
        }
    }

    pub fn with_improve(mut self, engine: std::sync::Arc<ImproveEngine>, roots: Roots) -> Self {
        self.engine = Some(engine);
        self.roots = Some(roots);
        self
    }

    pub fn with_memory(mut self, memory: std::sync::Arc<MemoryEngine>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Check the output of a command that just ran. If it contains
    /// compiler/test/runtime diagnostics, generate fix proposals and stage
    /// them through the improve engine.
    pub fn check_output(&mut self, command: &str, output: &str, exit_code: Option<i32>) -> SelfHealResult {
        let session = debug_output(command, output);
        let diagnostics_count = session.diagnostics.len();

        if diagnostics_count == 0 {
            return SelfHealResult {
                command: command.to_string(),
                exit_code,
                diagnostics_found: 0,
                fixable: 0,
                unfixable: 0,
                proposals_staged: Vec::new(),
                techniques_saved: Vec::new(),
            };
        }

        let fix_result = self.fix_generator.generate(&session);
        let fixable = fix_result.proposals.len();
        let unfixable = fix_result.unfixable.len();

        // Stage proposals through the improve engine.
        let mut proposals_staged = Vec::new();
        if let (Some(engine), Some(roots)) = (&self.engine, &self.roots) {
            let staged = crate::debug::stage_fixes(engine, roots, &fix_result);
            for (id, res) in staged {
                if res.is_ok() {
                    proposals_staged.push(id);
                }
            }
        }

        // Store fixable and unfixable patterns as techniques for neural recall.
        let mut techniques_saved = Vec::new();
        if let Some(memory) = &self.memory {
            for d in &session.diagnostics {
                let tags = vec!["technique".to_string(), "debug".to_string(), "auto-fix".to_string()];
                let content = format!(
                    "Technique: auto-debug detected {} in {}\n\
                     Why: {}\n\
                     File: {}:{}\n\
                     Fix known: {}\n\
                     Source output: {}",
                    d.kind.as_str(),
                    command,
                    d.message,
                    d.file,
                    d.line,
                    d.has_known_fix,
                    output.lines().take(5).collect::<Vec<_>>().join("\\n"),
                );
                let mut draft = crate::memory::model::NewMemory::new(
                    crate::memory::model::MemoryCategory::Core,
                    content,
                );
                draft.tags = tags;
                draft.summary = Some(format!("auto-debug: {} in {}", d.kind.as_str(), command));
                draft.source = crate::memory::model::MemorySource::new(
                    crate::memory::model::SourceKind::Agent,
                );
                let outcome = memory.save(&draft);
                if let Ok(id) = outcome.map(|o| o.record.id.clone()) {
                    techniques_saved.push(id);
                }
            }
        }

        SelfHealResult {
            command: command.to_string(),
            exit_code,
            diagnostics_found: diagnostics_count,
            fixable,
            unfixable,
            proposals_staged,
            techniques_saved,
        }
    }
}

impl Default for SelfHeal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::improve::testing::temp_dir;

    #[test]
    fn no_diagnostics_means_nothing_staged() {
        let mut healer = SelfHeal::new();
        let result = healer.check_output("cargo test --lib", "test result: ok. 5 passed", Some(0));
        assert_eq!(result.diagnostics_found, 0);
        assert!(result.proposals_staged.is_empty());
    }

    #[test]
    fn test_failure_is_detected_and_staged_as_technique() {
        let mut healer = SelfHeal::new();
        let result = healer.check_output(
            "cargo test --lib",
            "test result: FAILED. 3 passed; 1 failed\npanicked at 'assertion failed', src/lib.rs:42:10",
            Some(101),
        );
        assert_eq!(result.diagnostics_found, 2);
        assert_eq!(result.unfixable, 2);
        // Without an engine, nothing is staged.
        assert!(result.proposals_staged.is_empty());
    }
}
