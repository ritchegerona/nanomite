//! # Self-Evolution — Neural Core Rewrites Itself
//!
//! Uses patterns learned by the planner, memory, and synthesizer to
//! continuously evolve nanomite's system prompt, tool schemas, and
//! behavior. Every turn produces evidence (what tools were used, what
//! succeeded, what failed); the evolution module distills that into
//! concrete, reviewable proposals (via `propose_self_edit`) and direct
//! prompt/schema tweaks that take effect immediately.
//!
//! Evolution is conservative: it only changes things when there is
//! statistically meaningful evidence (N observations, confidence threshold),
//! and every structural change is staged for user approval.

use crate::neural::now_secs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// One observed evolution signal: a tool outcome with context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionSignal {
    pub tool: String,
    pub success: bool,
    pub context: String,
    pub timestamp: u64,
}

/// A proposed change to nanomite's behavior (staged for `/improve`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionProposal {
    pub id: String,
    pub kind: ProposalKind,
    pub description: String,
    pub rationale: String,
    pub evidence_count: u64,
    pub confidence: f64,
    pub created_at: u64,
    pub applied: bool,
}

/// Kinds of evolutionary changes the system can propose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalKind {
    /// Add a new tool to the system prompt's recommended set.
    AddToolRecommendation,
    /// Remove a tool that never succeeds.
    RemoveToolRecommendation,
    /// Adjust the system prompt's instructions for a tool.
    AdjustPrompt,
    /// Change default parameters for a tool.
    AdjustDefaults,
    /// Create a new synthesized tool.
    SynthesizeTool,
}

/// The evolution engine: accumulates signals and produces proposals.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvolutionEngine {
    pub signals: Vec<EvolutionSignal>,
    pub proposals: Vec<EvolutionProposal>,
    /// Minimum observations before a proposal is generated.
    pub min_evidence: u64,
    /// Confidence threshold (0-1) for auto-applying prompt tweaks.
    pub confidence_threshold: f64,
    /// Tool success rates: tool -> (successes, total).
    pub tool_stats: HashMap<String, (u64, u64)>,
    /// Persistence path.
    pub path: Option<PathBuf>,
}

impl EvolutionEngine {
    pub fn new() -> Self {
        Self {
            min_evidence: 10,
            confidence_threshold: 0.8,
            ..Default::default()
        }
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Default::default()
        }
    }

    /// Record a tool outcome as an evolution signal.
    pub fn observe(&mut self, signal: EvolutionSignal) {
        let success = signal.success;
        let tool = signal.tool.clone();
        self.signals.push(signal);

        // Update running stats.
        let entry = self
            .tool_stats
            .entry(tool)
            .or_insert((0, 0));
        if success {
            entry.0 += 1;
        }
        entry.1 += 1;

        // Keep signal history bounded.
        if self.signals.len() > 50_000 {
            self.signals.drain(0..self.signals.len() / 2);
        }

        self.save();
    }

    /// Analyze accumulated signals and generate evolution proposals.
    /// Call this periodically (e.g. every N turns).
    pub fn analyze(&mut self) -> Vec<EvolutionProposal> {
        let mut new_proposals = Vec::new();

        for (tool, (successes, total)) in &self.tool_stats {
            if *total < self.min_evidence {
                continue;
            }
            let rate = *successes as f64 / *total as f64;

            if rate < 0.2 && *total >= self.min_evidence {
                // Tool rarely succeeds: propose removing from recommendations.
                new_proposals.push(EvolutionProposal {
                    id: format!("evolve-{}-{}", tool, now_secs()),
                    kind: ProposalKind::RemoveToolRecommendation,
                    description: format!("Remove `{tool}` from recommended tools (success rate {rate:.0}%)"),
                    rationale: format!(
                        "`{tool}` has been used {total} times with only {successes} successes ({rate:.0}%). It rarely works in this environment.",
                        tool = tool, total = total, successes = successes, rate = rate * 100.0
                    ),
                    evidence_count: *total,
                    confidence: 0.9,
                    created_at: now_secs(),
                    applied: false,
                });
            } else if rate > 0.8 && *total >= self.min_evidence * 2 {
                // Tool reliably succeeds: no action needed (keep using it).
                // Could propose making it a default.
            }
        }

        // Check for tool gaps: contexts where no tool succeeded.
        let failed_contexts: HashMap<String, u64> = self
            .signals
            .iter()
            .filter(|s| !s.success)
            .map(|s| s.context.clone())
            .fold(HashMap::new(), |mut acc, ctx| {
                *acc.entry(ctx).or_insert(0) += 1;
                acc
        });
        for (ctx, count) in failed_contexts {
            if count >= self.min_evidence {
                new_proposals.push(EvolutionProposal {
                    id: format!("evolve-synth-{}-{}", slug(&ctx), now_secs()),
                    kind: ProposalKind::SynthesizeTool,
                    description: format!("Synthesize a new tool for failed context: {ctx}"),
                    rationale: format!(
                        "No existing tool succeeded in context '{ctx}' after {count} attempts. A new synthesized tool may fill this gap.",
                        ctx = ctx, count = count
                    ),
                    evidence_count: count,
                    confidence: 0.7,
                    created_at: now_secs(),
                    applied: false,
                });
            }
        }

        self.proposals.extend(new_proposals.clone());
        self.save();
        new_proposals
    }

    /// Mark a proposal as applied (by the user in /improve, or automatically).
    pub fn apply_proposal(&mut self, id: &str) {
        if let Some(p) = self.proposals.iter_mut().find(|p| p.id == id) {
            p.applied = true;
            self.save();
        }
    }

    /// Get all pending (unapplied) proposals.
    pub fn pending(&self) -> Vec<&EvolutionProposal> {
        self.proposals.iter().filter(|p| !p.applied).collect()
    }

    /// Persist to disk.
    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(self).unwrap_or_default();
        if std::fs::write(&tmp, &body).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Load from disk.
    pub fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.signals.len()
    }
}

/// Slugify a context string for proposal IDs.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "context".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_updates_tool_stats() {
        let mut engine = EvolutionEngine::new();
        engine.observe(EvolutionSignal {
            tool: "nmap".to_string(),
            success: true,
            context: "recon".to_string(),
            timestamp: now_secs(),
        });
        engine.observe(EvolutionSignal {
            tool: "nmap".to_string(),
            success: false,
            context: "recon".to_string(),
            timestamp: now_secs(),
        });
        let stats = engine.tool_stats.get("nmap").unwrap();
        assert_eq!(stats, &(1, 2));
    }

    #[test]
    fn analyze_proposes_removal_for_low_success() {
        let mut engine = EvolutionEngine::new();
        engine.min_evidence = 3;
        // Record 10 failures for a tool.
        for _ in 0..10 {
            engine.observe(EvolutionSignal {
                tool: "broken_tool".to_string(),
                success: false,
                context: "always fails".to_string(),
                timestamp: now_secs(),
            });
        }
        let proposals = engine.analyze();
        assert!(
            proposals.iter().any(|p| p.kind == ProposalKind::RemoveToolRecommendation && p.description.contains("broken_tool")),
            "should propose removing broken_tool: {proposals:?}"
        );
    }

    #[test]
    fn analyze_proposes_synthesis_for_repeated_failures() {
        let mut engine = EvolutionEngine::new();
        engine.min_evidence = 3;
        for _ in 0..5 {
            engine.observe(EvolutionSignal {
                tool: "existing_tool".to_string(),
                success: false,
                context: "stealth syn scan".to_string(),
                timestamp: now_secs(),
            });
        }
        let proposals = engine.analyze();
        assert!(
            proposals.iter().any(|p| p.kind == ProposalKind::SynthesizeTool),
            "should propose synthesizing a tool for repeated failures: {proposals:?}"
        );
    }

    #[test]
    fn pending_and_apply_work() {
        let mut engine = EvolutionEngine::new();
        engine.observe(EvolutionSignal {
            tool: "x".to_string(),
            success: false,
            context: "ctx".to_string(),
            timestamp: now_secs(),
        });
        engine.min_evidence = 1;
        let props = engine.analyze();
        assert!(!props.is_empty());
        let id = props[0].id.clone();
        assert!(engine.pending().iter().any(|p| p.id == id));
        engine.apply_proposal(&id);
        assert!(!engine.pending().iter().any(|p| p.id == id));
    }

    #[test]
    fn high_success_tool_not_proposed_for_removal() {
        let mut engine = EvolutionEngine::new();
        engine.min_evidence = 3;
        for _ in 0..10 {
            engine.observe(EvolutionSignal {
                tool: "great_tool".to_string(),
                success: true,
                context: "always works".to_string(),
                timestamp: now_secs(),
            });
        }
        let proposals = engine.analyze();
        assert!(
            !proposals.iter().any(|p| p.description.contains("great_tool")),
            "great_tool should not be proposed for removal: {proposals:?}"
        );
    }
}
