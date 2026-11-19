//! # Neural Core — Nanomite's Learned Intelligence Layer
//!
//! This module gives nanomite a **complete neural AI agent** capability:
//! a pure-Rust feedforward neural network with backpropagation for learning
//! tool outcomes, embedding-based neural memory with cosine similarity,
//! a learned planner that predicts the best next tool from past patterns,
//! a tool synthesizer that creates new Rust tools from natural language,
//! and self-evolution that continuously rewrites system prompts and tool
//! schemas based on what works.
//!
//! Everything is dependency-free (pure Rust, no external ML crates) so it
//! compiles everywhere nanomite compiles. State persists to JSON so learning
//! survives restarts.
//!
//! ## Architecture
//!
//! - [`autonomous_planner`] — goal-driven execution loop with pre-tool-call gates
//! - [`network`] — feedforward neural network + backpropagation trainer
//! - [`memory`] — embedding store + cosine-similarity recall
//! - [`planner`] — predicts the best next tool given context + history
//! - [`synthesizer`] — generates new Rust tool source from natural language
//! - [`evolution`] — rewrites prompts/schemas from learned patterns
//! - [`runtime_integration`] — hooks into every agent turn to train + predict

pub mod autonomous_planner;
pub mod evolution;
pub mod memory;
pub mod multi_agent;
pub mod network;
pub mod planner;
pub mod runtime_integration;
pub mod synthesizer;

/// Current unix seconds (shared across neural submodules).
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Deterministic FNV-1a hash, shared across neural submodules.
pub(crate) fn hash_string(s: &str) -> u64 {
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

    #[test]
    fn now_secs_is_monotonic_and_plausible() {
        let a = now_secs();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = now_secs();
        assert!(b >= a, "time should not go backwards");
        assert!(a > 1_700_000_000, "unix seconds should be 2023+");
    }

    #[test]
    fn hash_string_is_deterministic() {
        assert_eq!(hash_string("test"), hash_string("test"));
        assert_ne!(hash_string("test"), hash_string("other"));
    }
}
