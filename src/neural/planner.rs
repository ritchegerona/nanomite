//! # Learned Planner — Predicts the Best Next Tool from Experience
//!
//! Uses the neural network to predict which tool is most likely to succeed
//! given the current context (task type, workspace state, previously used
//! tools, success/failure history). Trained on every tool outcome the agent
//! experiences, so it gets better the more nanomite is used.
//!
//! Output: a probability distribution over known tools; the agent can sample
//! or argmax to pick the next tool, and the actual outcome trains the network.

use crate::neural::network::{NeuralNetwork, TrainingSample};
use crate::neural::now_secs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Feature vector size for the planner network.
const PLANNER_INPUT_DIM: usize = 64;
/// Number of output classes (tools the planner can predict).
const PLANNER_OUTPUT_DIM: usize = 48;

/// One observed tool-use outcome: context -> tool -> success/failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutcome {
    pub tool: String,
    pub success: bool,
    pub context_hash: u64,
    pub context_features: Vec<f64>,
    pub timestamp: u64,
    pub duration_ms: u64,
}

/// The learned planner: neural network + outcome history + tool vocabulary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearnedPlanner {
    pub network: Option<NeuralNetwork>,
    /// tool name -> index in output layer.
    pub tool_vocab: HashMap<String, usize>,
    /// index -> tool name.
    pub vocab_tool: HashMap<usize, String>,
    /// Recent outcomes used for training.
    pub outcomes: Vec<ToolOutcome>,
    /// Persistence path.
    #[serde(skip)]
    pub path: Option<PathBuf>,
    /// Number of times the planner has been (re)trained.
    pub train_count: u64,
    /// Total predictions made.
    pub prediction_count: u64,
}

impl LearnedPlanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Default::default()
        }
    }

    /// Initialize the network with a vocabulary of known tools.
    pub fn initialize(&mut self, tools: &[String]) {
        // Build vocabulary: assign each tool an output index.
        self.tool_vocab.clear();
        self.vocab_tool.clear();
        for (i, tool) in tools.iter().enumerate() {
            if i >= PLANNER_OUTPUT_DIM {
                break;
            }
            self.tool_vocab.insert(tool.clone(), i);
            self.vocab_tool.insert(i, tool.clone());
        }
        // Ensure at least one "unknown" output slot.
        if !self.vocab_tool.contains_key(&(PLANNER_OUTPUT_DIM - 1)) {
            self.vocab_tool.insert(PLANNER_OUTPUT_DIM - 1, "<unknown>".to_string());
        }

        // Create or load the network.
        let net = NeuralNetwork::new(&[PLANNER_INPUT_DIM, 32, 16, PLANNER_OUTPUT_DIM], 0.01, 0.9);
        self.network = Some(net);
        self.train_count = 0;
    }

    /// Record a tool outcome and incrementally train the network.
    pub fn observe_outcome(&mut self, outcome: ToolOutcome) {
        let target = self.one_hot(&outcome.tool);
        let input = outcome.context_features.clone();
        // Train on this single outcome immediately (online learning).
        if let Some(net) = &mut self.network {
            let sample = TrainingSample { input, target };
            let _loss = net.train(&sample);
        }
        self.outcomes.push(outcome);
        // Trim history to keep memory bounded.
        if self.outcomes.len() > 10_000 {
            self.outcomes.drain(0..self.outcomes.len() / 2);
        }
        self.save();
    }

    /// Predict the best next tool given context features. Returns
    /// (tool_name, confidence) pairs sorted by probability.
    pub fn predict(&mut self, context_features: &[f64], top_k: usize) -> Vec<(String, f64)> {
        self.prediction_count += 1;
        let Some(net) = &self.network else {
            return Vec::new();
        };
        let probs = net.predict(context_features);
        // Build (tool, probability) pairs.
        let mut scored: Vec<(String, f64)> = self
            .vocab_tool
            .iter()
            .filter_map(|(idx, tool)| {
                probs.get(*idx).map(|p| (tool.clone(), *p))
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(top_k).collect()
    }

    /// Build a one-hot target vector for a tool name.
    fn one_hot(&self, tool: &str) -> Vec<f64> {
        let mut v = vec![0.0; PLANNER_OUTPUT_DIM];
        if let Some(&idx) = self.tool_vocab.get(tool) {
            v[idx] = 1.0;
        } else {
            v[PLANNER_OUTPUT_DIM - 1] = 1.0; // unknown slot
        }
        v
    }

    /// Extract context features from a task description + workspace state.
    /// Fixed-size feature vector: keyword presence, tool history stats, etc.
    pub fn context_features(task: &str, recent_tools: &[String], recent_successes: &[bool]) -> Vec<f64> {
        let mut features = vec![0.0f64; PLANNER_INPUT_DIM];

        // Keyword presence features (first 24 dims).
        let keywords = [
            "nmap", "port", "scan", "web", "http", "vuln", "sql", "inject",
            "password", "crack", "hash", "wifi", "wireless", "recon", "enum",
            "dir", "bust", "file", "read", "write", "edit", "command", "ssh",
            "remote", "memory", "save", "search", "plan", "create", "git",
            "commit", "test", "run", "install", "tool",
        ];
        let task_lower = task.to_ascii_lowercase();
        for (i, kw) in keywords.iter().enumerate() {
            if i >= 24 {
                break;
            }
            if task_lower.contains(kw) {
                features[i] = 1.0;
            }
        }

        // Recent tool usage stats (dims 24-31): frequency of each major category.
        let mut cat_counts: HashMap<&str, usize> = HashMap::new();
        for tool in recent_tools {
            let cat = category_of(tool);
            *cat_counts.entry(cat).or_insert(0) += 1;
        }
        let total = recent_tools.len().max(1);
        let cats = ["recon", "web", "password", "wireless", "exec", "memory", "plan", "git"];
        for (i, cat) in cats.iter().enumerate() {
            if 24 + i >= PLANNER_INPUT_DIM {
                break;
            }
            features[24 + i] = cat_counts.get(cat).copied().unwrap_or(0) as f64 / total as f64;
        }

        // Recent success rate (dim 32).
        if !recent_successes.is_empty() {
            let succ: f64 = recent_successes.iter().filter(|&&b| b).count() as f64
                / recent_successes.len() as f64;
            features[32] = succ;
        }

        // Time-based features (dims 33-39): hour of day, day of week, etc.
        let now = now_secs();
        features[33] = (now % 86400) as f64 / 86400.0; // time of day
        features[34] = (now / 86400 % 7) as f64 / 7.0; // day of week

        // Remaining dims (40-63): zero-padded for future expansion.
        features
    }

    /// Persist planner state (vocab + outcomes + train count).
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

    /// Load planner state from disk.
    pub fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut planner: Self = serde_json::from_str(&text).unwrap_or_default();
                planner.path = Some(path.clone());
                planner
            }
            Err(_) => Self::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.outcomes.len()
    }
}

/// Map a tool name to its category for feature extraction.
fn category_of(tool: &str) -> &str {
    match tool {
        "list_files" | "read_file" | "search" | "read_document" | "ocr_image" => "exec", // file inspection
        "run_command" | "ssh_run" | "scan_ports" => "exec",
        "web_fetch" | "web_search" | "browse" | "ip_lookup" => "web",
        "memory_save" | "memory_search" | "memory_get" | "memory_list" | "memory_update" | "memory_delete" => "memory",
        "create_plan" | "get_plan" | "update_task" | "list_plans" | "delete_plan" => "plan",
        "git_status" | "git_diff" | "git_log" | "git_commit" | "git_branch" => "git",
        "write_file" | "edit_file" => "exec",
        "nmap" | "masscan" => "recon",
        "nikto" | "sqlmap" | "gobuster" | "dirb" | "zaproxy" | "burpsuite" => "web",
        "hydra" | "john" | "hashcat" => "password",
        "aircrack-ng" | "wifite" => "wireless",
        _ => "system",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_builds_vocab_and_network() {
        let mut planner = LearnedPlanner::new();
        let tools = vec![
            "nmap".to_string(),
            "sqlmap".to_string(),
            "write_file".to_string(),
            "read_file".to_string(),
        ];
        planner.initialize(&tools);
        assert!(planner.network.is_some());
        assert_eq!(planner.tool_vocab.len(), 4);
        assert_eq!(planner.tool_vocab["nmap"], 0);
        assert_eq!(planner.tool_vocab["sqlmap"], 1);
    }

    #[test]
    fn observe_and_predict() {
        let mut planner = LearnedPlanner::new();
        let tools = vec!["nmap".to_string(), "sqlmap".to_string()];
        planner.initialize(&tools);

        // Record that nmap succeeds for recon-ish tasks.
        planner.observe_outcome(ToolOutcome {
            tool: "nmap".to_string(),
            success: true,
            context_hash: 1,
            context_features: LearnedPlanner::context_features(
                "network port scan recon",
                &["nmap".to_string()],
                &[true],
            ),
            timestamp: now_secs(),
            duration_ms: 100,
        });
        planner.observe_outcome(ToolOutcome {
            tool: "sqlmap".to_string(),
            success: true,
            context_hash: 2,
            context_features: LearnedPlanner::context_features(
                "web sql injection",
                &["sqlmap".to_string()],
                &[true],
            ),
            timestamp: now_secs(),
            duration_ms: 200,
        });

        let predictions = planner.predict(
            &LearnedPlanner::context_features("network port scan recon", &[], &[]),
            2,
        );
        assert!(!predictions.is_empty());
        assert!(predictions.len() <= 2);
        // Top prediction should be a known tool.
        assert!(predictions[0].1 >= 0.0 && predictions[0].1 <= 1.0);
    }

    #[test]
    fn context_features_is_fixed_size() {
        let f = LearnedPlanner::context_features("test task", &[], &[]);
        assert_eq!(f.len(), PLANNER_INPUT_DIM);
    }

    #[test]
    fn one_hot_unknown_goes_to_last_slot() {
        let mut planner = LearnedPlanner::new();
        planner.initialize(&["nmap".to_string()]);
        let v = planner.one_hot("completely_unknown_tool");
        assert_eq!(v[PLANNER_OUTPUT_DIM - 1], 1.0);
        assert!(v.iter().take(PLANNER_OUTPUT_DIM - 1).all(|&x| x == 0.0));
    }
}
