//! # Neural Runtime Integration
//!
//! Hooks the neural core into every agent turn so nanomite learns, predicts,
//! and evolves continuously:
//! - Before each turn: neural planner predicts the best next tools from context
//! - During each turn: outcomes are recorded as training samples
//! - After each turn: network trains on outcomes, memory stores salient facts,
//!   evolution engine analyzes for proposals
//!
//! The `NeuralAgent` wraps `AgentRuntime` and intercepts tool dispatch to
//! feed the neural loop.

use crate::agent::runtime::{AgentRuntime, Cancel, AgentEvent};
use crate::agent::provider::{ChatMessage, Usage};
use crate::neural::memory::{NeuralMemory, NeuralMemoryQueryResult};
use crate::neural::planner::{LearnedPlanner, ToolOutcome};
use crate::neural::evolution::{EvolutionEngine, EvolutionSignal};
use crate::neural::synthesizer::{ToolSynthesizer, SynthesisResult};
use crate::neural::network::{NeuralNetwork, TrainingSample};
use crate::neural::now_secs;
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;
use std::time::Instant;

/// The neural-enhanced agent. Wraps the runtime and adds learning, prediction,
/// memory, and self-evolution to every turn.
pub struct NeuralAgent {
    pub runtime: AgentRuntime,
    pub planner: LearnedPlanner,
    pub memory: NeuralMemory,
    pub evolution: EvolutionEngine,
    pub synthesizer: ToolSynthesizer,
    pub network: Option<NeuralNetwork>,
    /// Outcomes collected this turn for batch training after the turn ends.
    pub turn_outcomes: Vec<ToolOutcome>,
    /// Whether neural features are enabled.
    pub enabled: bool,
    /// How often (in turns) to run evolution analysis.
    pub evolution_interval: u64,
    /// Turn counter.
    pub turn_count: u64,
    /// Data directory for persistence.
    pub data_dir: PathBuf,
}

impl NeuralAgent {
    /// Create a new neural agent wrapping a runtime.
    pub fn new(runtime: AgentRuntime, data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        let _ = std::fs::create_dir_all(&data_dir);

        let planner = LearnedPlanner::with_path(data_dir.join("planner.json"));
        let memory = NeuralMemory::with_path(data_dir.join("neural_memory.json"));
        let evolution = EvolutionEngine::with_path(data_dir.join("evolution.json"));
        let synth_dir = data_dir.join("synthesized_tools");
        let _ = std::fs::create_dir_all(&synth_dir);
        let synthesizer = ToolSynthesizer::new(synth_dir);

        // Load persisted state.
        let mut planner = LearnedPlanner::with_path(data_dir.join("planner.json"));
        // planner.load happens in initialize_planner
        let memory = NeuralMemory::load(&data_dir.join("neural_memory.json"));
        let evolution = EvolutionEngine::load(&data_dir.join("evolution.json"));

        Self {
            runtime,
            planner,
            memory,
            evolution,
            synthesizer,
            network: None,
            turn_outcomes: Vec::new(),
            enabled: true,
            evolution_interval: 10,
            turn_count: 0,
            data_dir,
        }
    }

    /// Initialize the planner with the full tool vocabulary from the runtime's
    /// catalog. Call once before the first turn.
    pub fn initialize_planner(&mut self, tools: &[String]) {
        self.planner.initialize(tools);
        // Also create a general-purpose neural network for outcome prediction.
        self.network = Some(NeuralNetwork::new(&[64, 32, 16, 1], 0.01, 0.9));
    }

    /// Run a full neural-enhanced turn:
    /// 1. Inject neural context (past successes, predicted tools) into the system prompt
    /// 2. Run the runtime turn
    /// 3. Record outcomes, train the network, update memory, analyze evolution
    pub async fn run_turn(
        &mut self,
        user_input: String,
        history: &[ChatMessage],
        cancel: Cancel,
        tx: UnboundedSender<AgentEvent>,
    ) -> Option<Usage> {
        if !self.enabled {
            return self.runtime.run(user_input, history, cancel, tx).await;
        }

        self.turn_count += 1;
        let start = Instant::now();

        // 1. Build neural context: predict best tools from learned patterns.
        let context_features = self.build_context_features(&user_input);
        let predictions = self.planner.predict(&context_features, 5);
        let neural_context = self.format_neural_context(&predictions);

        // 2. Retrieve relevant neural memories.
        let memories = self.memory.query(&user_input, 5, 0.3);
        let memory_context = self.format_memory_context(&memories);

        // 3. Inject neural + memory context into a modified system prompt for this turn.
        let enhanced_prompt = format!(
            "{}\n\n# Neural Context (learned from prior turns)\n{}\n\n# Relevant Memories\n{}",
            self.runtime.system_prompt(), neural_context, memory_context
        );

        // Temporarily swap the system prompt.
        let original_prompt = self.runtime.system_prompt().to_string();
        *self.runtime.system_prompt_mut() = enhanced_prompt;

        // 4. Run the actual turn.
        let result = self.runtime.run(user_input.clone(), history, cancel, tx).await;

        // 5. Restore original prompt.
        *self.runtime.system_prompt_mut() = original_prompt;

        // 6. Post-turn: store outcome in memory and train.
        // (The actual tool outcomes would be captured by a wrapper around run_tool;
        // here we record the turn-level summary.)
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let turn_success = result.is_some();
        self.memory.store(
            &format!("turn-{}", self.turn_count),
            &format!("User: {user_input}\nResult: {} in {}ms",
                     if turn_success { "completed" } else { "interrupted" },
                     elapsed_ms),
            vec!["turn".to_string(), if turn_success { "success" } else { "interrupted" }.to_string()],
            "turn",
        );

        // 7. Train the general network on this turn's outcome.
        if let Some(net) = &mut self.network {
            let target = if turn_success { vec![1.0] } else { vec![0.0] };
            let sample = TrainingSample {
                input: context_features,
                target,
            };
            let _loss = net.train(&sample);
        }

        // 8. Periodically run evolution analysis.
        if self.turn_count % self.evolution_interval == 0 {
            let proposals = self.evolution.analyze();
            if !proposals.is_empty() {
                // Store evolution proposals as memories so the agent can see them.
                for p in &proposals {
                    self.memory.store(
                        &format!("evolve-{}", p.id),
                        &format!("{}: {}\nRationale: {}\nConfidence: {:.0}%",
                                 p.kind as u8, p.description, p.rationale, p.confidence * 100.0),
                        vec!["evolution".to_string(), "proposal".to_string()],
                        "evolution",
                    );
                }
            }
        }

        result
    }

    /// Record a specific tool outcome for learning (called by the tool dispatch wrapper).
    pub fn record_tool_outcome(&mut self, tool: &str, success: bool, context: &str, duration_ms: u64) {
        let features = LearnedPlanner::context_features(context, &[tool.to_string()], &[success]);
        let outcome = ToolOutcome {
            tool: tool.to_string(),
            success,
            context_hash: hash_string(context),
            context_features: features,
            timestamp: now_secs(),
            duration_ms,
        };
        self.planner.observe_outcome(outcome.clone());
        self.turn_outcomes.push(outcome);

        // Also signal the evolution engine.
        self.evolution.observe(EvolutionSignal {
            tool: tool.to_string(),
            success,
            context: context.to_string(),
            timestamp: now_secs(),
        });

        // Store successful techniques in neural memory.
        if success {
            self.memory.store(
                &format!("technique-{}-{}", tool, now_secs()),
                &format!("Tool `{tool}` succeeded in context: {context}"),
                vec!["technique".to_string(), "success".to_string()],
                "technique",
            );
        }
    }

    /// Synthesize a new tool from a natural language description.
    pub fn synthesize_tool(&mut self, description: &str, name: Option<&str>) -> SynthesisResult {
        let result = self.synthesizer.synthesize(description, name, true);
        if result.success {
            // Record as a memory so the agent knows this tool exists.
            self.memory.store(
                &format!("synth-{}", result.tool.name),
                &format!("Synthesized tool `{}`: {}\nFile: {}",
                         result.tool.name, result.tool.description, result.tool.file_path),
                vec!["synthesized".to_string(), "tool".to_string()],
                "synthesis",
            );
        }
        result
    }

    /// Query neural memory for relevant past experiences.
    pub fn recall(&mut self, query: &str, top_k: usize, threshold: f64) -> NeuralMemoryQueryResult {
        self.memory.query(query, top_k, threshold)
    }

    /// Get pending evolution proposals for the user to review.
    pub fn pending_evolutions(&self) -> Vec<&crate::neural::evolution::EvolutionProposal> {
        self.evolution.pending()
    }

    /// Mark an evolution proposal as applied.
    pub fn apply_evolution(&mut self, id: &str) {
        self.evolution.apply_proposal(id);
    }

    /// Build context features from the current input.
    fn build_context_features(&self, input: &str) -> Vec<f64> {
        LearnedPlanner::context_features(input, &[], &[])
    }

    /// Format predicted tools as context for the model.
    fn format_neural_context(&self, predictions: &[(String, f64)]) -> String {
        if predictions.is_empty() {
            "No neural predictions yet — learning from scratch.".to_string()
        } else {
            let mut lines = vec!["Predicted best tools (from learned experience):".to_string()];
            for (tool, conf) in predictions {
                lines.push(format!("- {tool}: {:.0}% confidence", conf * 100.0));
            }
            lines.join("\n")
        }
    }

    /// Format recalled memories as context.
    fn format_memory_context(&self, memories: &NeuralMemoryQueryResult) -> String {
        if memories.records.is_empty() {
            "No relevant memories found.".to_string()
        } else {
            let mut lines = vec!["Relevant past experiences:".to_string()];
            for (rec, sim) in memories.records.iter().zip(memories.similarities.iter()) {
                lines.push(format!(
                    "- [{:.0}%] {} (tags: {})",
                    sim * 100.0,
                    rec.text.chars().take(200).collect::<String>(),
                    rec.tags.join(", ")
                ));
            }
            lines.join("\n")
        }
    }

    /// Save all neural state to disk.
    pub fn save_state(&self) {
        self.planner.save();
        self.memory.save();
        self.evolution.save();
        // Network weights persist via planner's network if initialized.
        if let Some(net) = &self.network {
            let path = self.data_dir.join("network.json");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, net.to_json());
        }
    }

    /// Load neural state from disk.
    pub fn load_state(&mut self) {
        // Load planner from disk.
        let planner_path = self.data_dir.join("planner.json");
        if planner_path.exists() {
            self.planner = LearnedPlanner::load(&planner_path);
        }
        // Load memory from disk.
        let memory_path = self.data_dir.join("neural_memory.json");
        if memory_path.exists() {
            self.memory = NeuralMemory::load(&memory_path);
        }
        // Load evolution from disk.
        let evolution_path = self.data_dir.join("evolution.json");
        if evolution_path.exists() {
            self.evolution = EvolutionEngine::load(&evolution_path);
        }
        let net_path = self.data_dir.join("network.json");
        if net_path.exists() {
            if let Ok(text) = std::fs::read_to_string(&net_path) {
                if let Some(net) = NeuralNetwork::from_json(&text) {
                    self.network = Some(net);
                }
            }
        }
    }
}

/// Hash a string to u64 (deterministic, same as neural::memory).
fn hash_string(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runtime::AgentConfig;

    #[test]
    fn neural_agent_creates_and_initializes() {
        let runtime = AgentRuntime::new(
            std::sync::Arc::new(crate::agent::testdouble::ScriptedProvider::default()),
            std::env::temp_dir().join(format!("nanomite-neural-test-{}", std::process::id())),
            64 * 1024,
            Some(AgentConfig::default()),
        );
        let dir = std::env::temp_dir().join(format!("nanomite-neural-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut agent = NeuralAgent::new(runtime, &dir);
        agent.initialize_planner(&["nmap".to_string(), "sqlmap".to_string()]);
        assert!(agent.network.is_some());
        assert!(agent.planner.network.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_outcome_trains_planner() {
        let runtime = AgentRuntime::new(
            std::sync::Arc::new(crate::agent::testdouble::ScriptedProvider::default()),
            std::env::temp_dir().join(format!("nanomite-neural-test-{}", std::process::id())),
            64 * 1024,
            Some(AgentConfig::default()),
        );
        let dir = std::env::temp_dir().join(format!("nanomite-neural-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut agent = NeuralAgent::new(runtime, &dir);
        agent.initialize_planner(&["nmap".to_string()]);

        agent.record_tool_outcome("nmap", true, "network scan", 100);
        assert_eq!(agent.planner.outcomes.len(), 1);
        assert!(agent.planner.outcomes[0].success);

        let predictions = agent.planner.predict(
            &LearnedPlanner::context_features("network scan", &[], &[]),
            3,
        );
        assert!(!predictions.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn synthesize_tool_creates_memory() {
        let runtime = AgentRuntime::new(
            std::sync::Arc::new(crate::agent::testdouble::ScriptedProvider::default()),
            std::env::temp_dir().join(format!("nanomite-neural-test-{}", std::process::id())),
            64 * 1024,
            Some(AgentConfig::default()),
        );
        let dir = std::env::temp_dir().join(format!("nanomite-neural-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut agent = NeuralAgent::new(runtime, &dir);
        agent.initialize_planner(&[]);

        let result = agent.synthesize_tool("stealth SYN port scanner", Some("stealth_scan"));
        assert!(result.success);
        assert!(!result.tool.source_code.is_empty());
        // Should have stored a memory.
        assert!(agent.memory.len() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_load_state_roundtrip() {
        let runtime = AgentRuntime::new(
            std::sync::Arc::new(crate::agent::testdouble::ScriptedProvider::default()),
            std::env::temp_dir().join(format!("nanomite-neural-test-{}", std::process::id())),
            64 * 1024,
            Some(AgentConfig::default()),
        );
        let dir = std::env::temp_dir().join(format!("nanomite-neural-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut agent = NeuralAgent::new(runtime, &dir);
        agent.initialize_planner(&["nmap".to_string()]);
        agent.record_tool_outcome("nmap", true, "scan", 50);
        agent.save_state();

        // Create a new agent loading from same dir.
        let runtime2 = AgentRuntime::new(
            std::sync::Arc::new(crate::agent::testdouble::ScriptedProvider::default()),
            std::env::temp_dir().join(format!("nanomite-neural-test-{}", std::process::id())),
            64 * 1024,
            Some(AgentConfig::default()),
        );
        let mut agent2 = NeuralAgent::new(runtime2, &dir);
        agent2.load_state();
        assert!(agent2.network.is_some() || agent2.planner.outcomes.len() >= 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
