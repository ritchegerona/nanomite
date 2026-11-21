//! Configurable auto-apply gate for high-confidence auto-fix proposals.
//!
//! When `auto_apply` is enabled, `High`-confidence fixes (e.g. removing an
//! unused import, dead-code cleanup) can be applied automatically without
//! waiting for `/improve` review — but only after passing the regression
//! gate (`cargo test --lib`). `Medium` and `Low` confidence fixes always
//! require explicit human approval via `/improve`.
//!
//! Nothing is ever applied silently without the user first opting in with
//! `/debug auto-apply on`.

use crate::debug::{FixConfidence, FixProposal};
use crate::improve::{Actor, CargoTestRunner, ImproveEngine, Roots, Target};

/// Auto-apply policy: which confidence levels may auto-apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoApplyPolicy {
    /// Never auto-apply — every fix needs `/improve` approval.
    Off,
    /// Auto-apply `High` confidence fixes after regression gate passes.
    HighOnly,
}

/// Result of an auto-apply attempt.
#[derive(Debug, Clone)]
pub enum AutoApplyResult {
    /// Proposal applied successfully after regression gate passed.
    Applied { id: String, proposal: FixProposal },
    /// Proposal was staged for `/improve` review (not auto-applied).
    Staged { id: String },
    /// Regression gate failed — proposal rolled back.
    RolledBack { id: String, detail: String },
    /// Proposal could not be applied (engine error).
    Error { id: String, error: String },
}

/// Config for the auto-apply subsystem.
#[derive(Debug, Clone)]
pub struct AutoApplyConfig {
    pub policy: AutoApplyPolicy,
    pub regression_command: String,
}

impl Default for AutoApplyConfig {
    fn default() -> Self {
        Self {
            policy: AutoApplyPolicy::Off,
            regression_command: "cargo test --lib".to_string(),
        }
    }
}

/// The auto-apply gate: evaluates proposals against policy and applies
/// high-confidence ones through the improve engine's regression gate.
pub struct AutoApplyGate {
    config: AutoApplyConfig,
}

impl AutoApplyGate {
    pub fn new(config: AutoApplyConfig) -> Self {
        Self { config }
    }

    pub fn policy(&self) -> AutoApplyPolicy {
        self.config.policy
    }

    pub fn set_policy(&mut self, policy: AutoApplyPolicy) {
        self.config.policy = policy;
    }

    /// Evaluate a fix proposal: auto-apply if policy allows and confidence
    /// is high enough; otherwise stage it for `/improve` review.
    pub fn evaluate(
        &self,
        engine: &ImproveEngine,
        roots: &Roots,
        proposal: &FixProposal,
    ) -> AutoApplyResult {
        let path = std::path::PathBuf::from(&proposal.file_path);
        let target = if proposal.file_path == "Cargo.toml" {
            Target::Config
        } else {
            Target::Source
        };
        let rationale = proposal.rationale.clone();

        let id = match engine.propose(
            Actor::Agent,
            target,
            path,
            proposal.new_content.clone(),
            rationale,
            roots,
        ) {
            Ok(p) => p.id.clone(),
            Err(e) => {
                return AutoApplyResult::Error {
                    id: proposal.diagnostic_id.clone(),
                    error: e.to_string(),
                }
            }
        };

        // Auto-apply only high-confidence fixes when policy is HighOnly.
        let should_auto = matches!(self.config.policy, AutoApplyPolicy::HighOnly)
            && proposal.confidence == FixConfidence::High;

        if !should_auto {
            return AutoApplyResult::Staged { id };
        }

        // Run the regression gate.
        let runner = CargoTestRunner;
        match engine.apply(&id, &runner, roots) {
            Ok(outcome) => match outcome.status {
                crate::improve::Status::Applied => AutoApplyResult::Applied {
                    id,
                    proposal: proposal.clone(),
                },
                crate::improve::Status::RolledBack => AutoApplyResult::RolledBack {
                    id,
                    detail: outcome.detail,
                },
                other => AutoApplyResult::Error {
                    id,
                    error: format!("unexpected status: {:?}", other),
                },
            },
            Err(e) => AutoApplyResult::Error {
                id,
                error: e.to_string(),
            },
        }
    }

    /// Evaluate a batch of proposals; returns per-proposal results.
    pub fn evaluate_batch(
        &self,
        engine: &ImproveEngine,
        roots: &Roots,
        proposals: &[FixProposal],
    ) -> Vec<AutoApplyResult> {
        proposals
            .iter()
            .map(|p| self.evaluate(engine, roots, p))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::FixProposal;
    use crate::improve::testing::temp_dir;

    fn make_proposal(file: &str, confidence: FixConfidence) -> FixProposal {
        FixProposal {
            diagnostic_id: "test-diag".into(),
            file_path: file.to_string(),
            old_content: String::new(),
            new_content: "// test fix\n".to_string(),
            rationale: "test rationale".to_string(),
            confidence,
        }
    }

    #[test]
    fn off_policy_never_auto_applies() {
        let temp = temp_dir("auto-off");
        let root = &temp.path;
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src").join("widget.rs");
        std::fs::write(&file, "pub fn widget() -> u32 { 1 }\n").unwrap();

        let roots = Roots {
            config_path: root.join("config.toml"),
            prompt_path: root.join("prompt.md"),
            repo_root: root.clone(),
        };
        let engine = ImproveEngine::new(root.join(".improve"));
        let gate = AutoApplyGate::new(AutoApplyConfig {
            policy: AutoApplyPolicy::Off,
            ..Default::default()
        });

        let proposal = make_proposal(file.to_str().unwrap(), FixConfidence::High);
        let result = gate.evaluate(&engine, &roots, &proposal);
        assert!(matches!(result, AutoApplyResult::Staged { .. }));
    }

    #[test]
    fn high_only_applies_high_confidence() {
        let temp = temp_dir("auto-high");
        let root = &temp.path;
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src").join("widget.rs");
        std::fs::write(&file, "pub fn widget() -> u32 { 1 }\n").unwrap();

        let roots = Roots {
            config_path: root.join("config.toml"),
            prompt_path: root.join("prompt.md"),
            repo_root: root.clone(),
        };
        let engine = ImproveEngine::new(root.join(".improve"));
        let gate = AutoApplyGate::new(AutoApplyConfig {
            policy: AutoApplyPolicy::HighOnly,
            ..Default::default()
        });

        let proposal = make_proposal(file.to_str().unwrap(), FixConfidence::High);
        let result = gate.evaluate(&engine, &roots, &proposal);
        // Regression gate runs cargo test --lib which will fail in temp dir
        // (no Cargo.toml), so we expect RolledBack or Error — but the key
        // point is it attempted auto-apply, not Staged.
        assert!(!matches!(result, AutoApplyResult::Staged { .. }));
    }

    #[test]
    fn medium_confidence_never_auto_applies() {
        let temp = temp_dir("auto-med");
        let root = &temp.path;
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src").join("widget.rs");
        std::fs::write(&file, "pub fn widget() -> u32 { 1 }\n").unwrap();

        let roots = Roots {
            config_path: root.join("config.toml"),
            prompt_path: root.join("prompt.md"),
            repo_root: root.clone(),
        };
        let engine = ImproveEngine::new(root.join(".improve"));
        let gate = AutoApplyGate::new(AutoApplyConfig {
            policy: AutoApplyPolicy::HighOnly,
            ..Default::default()
        });

        let proposal = make_proposal(file.to_str().unwrap(), FixConfidence::Medium);
        let result = gate.evaluate(&engine, &roots, &proposal);
        assert!(matches!(result, AutoApplyResult::Staged { .. }));
    }
}
