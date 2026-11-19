//! # Neural Memory — Embedding-Based Recall with Cosine Similarity
//!
//! Stores text records as dense embedding vectors and retrieves them by
//! semantic similarity. Each record is tagged, so the agent can recall
//! past techniques, tool outcomes, and learned patterns across sessions.
//!
//! Embeddings are produced by a deterministic hash-of-tokens projection
//! (no external model needed): each token contributes a pseudo-random
//! vector, averaged and L2-normalized. This is good enough for nanomite's
//! on-device neural memory while keeping zero dependencies.
//!
//! Persists to JSON (vectors + metadata + tags). Query returns top-k by
//! cosine similarity above a threshold.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Dimension of the embedding space. Fixed so all vectors are comparable.
pub const EMBED_DIM: usize = 128;

/// One embedded memory record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralMemoryRecord {
    pub id: String,
    pub text: String,
    pub embedding: Vec<f64>,
    pub tags: Vec<String>,
    pub category: String,
    pub created_at: u64,
    pub last_accessed: u64,
    pub access_count: u64,
    /// How "salient" this memory is (boosted on each successful recall).
    pub salience: f64,
}

/// Result of a neural memory query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralMemoryQueryResult {
    pub records: Vec<NeuralMemoryRecord>,
    pub similarities: Vec<f64>,
}

/// Neural memory store: in-memory index + optional JSON persistence.
#[derive(Debug, Clone, Default)]
pub struct NeuralMemory {
    records: HashMap<String, NeuralMemoryRecord>,
    path: Option<PathBuf>,
}

impl NeuralMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Default::default()
        }
    }

    /// Load from JSON file, or start empty if the file doesn't exist.
    pub fn load(path: &PathBuf) -> Self {
        let records = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => HashMap::new(),
        };
        Self {
            records,
            path: Some(path.clone()),
        }
    }

    /// Persist to disk (atomic write). No-op when no path is set.
    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(&self.records).unwrap_or_default();
        if std::fs::write(&tmp, &body).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Store a new record, computing its embedding from the text.
    pub fn store(&mut self, id: &str, text: &str, tags: Vec<String>, category: &str) {
        let embedding = embed_text(text, EMBED_DIM);
        let now = crate::neural::now_secs();
        self.records.insert(
            id.to_string(),
            NeuralMemoryRecord {
                id: id.to_string(),
                text: text.to_string(),
                embedding,
                tags,
                category: category.to_string(),
                created_at: now,
                last_accessed: now,
                access_count: 0,
                salience: 1.0,
            },
        );
        self.save();
    }

    /// Query by semantic similarity: return top-k records whose cosine
    /// similarity with the query embedding exceeds `threshold`.
    pub fn query(&mut self, query_text: &str, top_k: usize, threshold: f64) -> NeuralMemoryQueryResult {
        let query_embedding = embed_text(query_text, EMBED_DIM);
        let mut scored: Vec<(f64, NeuralMemoryRecord)> = self
            .records
            .values()
            .filter_map(|r| {
                let sim = cosine_similarity(&query_embedding, &r.embedding);
                if sim >= threshold {
                    Some((sim, r.clone()))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let taken: Vec<(f64, NeuralMemoryRecord)> = scored.into_iter().take(top_k).collect();
        // Update access metadata on recalled records.
        let now = crate::neural::now_secs();
        for (_, r) in &taken {
            if let Some(rec) = self.records.get_mut(&r.id) {
                rec.last_accessed = now;
                rec.access_count = rec.access_count.saturating_add(1);
                rec.salience = (rec.salience * 0.9 + 0.1).min(2.0);
            }
        }
        self.save();
        NeuralMemoryQueryResult {
            records: taken.iter().map(|(_, r)| r.clone()).collect(),
            similarities: taken.iter().map(|(s, _)| *s).collect(),
        }
    }

    /// Tag-based search: return records that contain all given tags.
    pub fn by_tags(&self, tags: &[String]) -> Vec<&NeuralMemoryRecord> {
        self.records
            .values()
            .filter(|r| tags.iter().all(|t| r.tags.contains(t)))
            .collect()
    }

    /// Record count.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Deterministic text embedding: hash tokens into a fixed-dimension vector,
/// average, and L2-normalize. Same text always -> same embedding.
fn embed_text(text: &str, dim: usize) -> Vec<f64> {
    let tokens: Vec<&str> = text
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return vec![0.0; dim];
    }
    let mut sum = vec![0.0f64; dim];
    for token in &tokens {
        let vec = token_embedding(token, dim);
        for (i, v) in vec.iter().enumerate() {
            sum[i] += v;
        }
    }
    let n = tokens.len() as f64;
    let mut out: Vec<f64> = sum.iter().map(|s| s / n).collect();
    // L2 normalize.
    let norm: f64 = out.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm > 0.0 {
        out.iter_mut().for_each(|v| *v /= norm);
    }
    out
}

/// Hash a single token into a pseudo-random unit vector in `dim` dimensions.
fn token_embedding(token: &str, dim: usize) -> Vec<f64> {
    let mut h = hash_string(token);
    let mut out = Vec::with_capacity(dim);
    for i in 0..dim {
        h = splitmix64(h.wrapping_add((i as u64).wrapping_mul(0x9e3779b97f4a7c15)));
        let v = (h as f64 / u64::MAX as f64) * 2.0 - 1.0;
        out.push(v);
    }
    // Normalize the token vector itself.
    let norm: f64 = out.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm > 0.0 {
        out.iter_mut().for_each(|v| *v /= norm);
    }
    out
}

fn hash_string(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325; // FNV offset
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

/// Cosine similarity between two vectors (assumes same length).
fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    let mut dot: f64 = 0.0;
    let mut na: f64 = 0.0;
    let mut nb: f64 = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = (na * nb).sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeddings_are_deterministic_and_unit_length() {
        let a = embed_text("nmap port scan", EMBED_DIM);
        let b = embed_text("nmap port scan", EMBED_DIM);
        assert_eq!(a, b);
        let norm: f64 = a.iter().map(|v| v * v).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn similar_texts_have_high_cosine() {
        let a = embed_text("nmap network scan", EMBED_DIM);
        let b = embed_text("nmap network scanning", EMBED_DIM);
        let sim = cosine_similarity(&a, &b);
        assert!(sim > 0.6, "similar texts should have high similarity: {sim}");
    }

    #[test]
    fn dissimilar_texts_have_low_cosine() {
        let a = embed_text("password cracking hashcat", EMBED_DIM);
        let b = embed_text("web vulnerability scanning nikto", EMBED_DIM);
        let sim = cosine_similarity(&a, &b);
        assert!(sim < 0.6, "dissimilar texts should be low: {sim}");
    }

    #[test]
    fn store_and_query_returns_top_k() {
        let mut mem = NeuralMemory::new();
        mem.store(
            "m1",
            "nmap port scanning technique",
            vec!["technique".to_string(), "recon".to_string()],
            "technique",
        );
        mem.store(
            "m2",
            "hashcat password cracking rules",
            vec!["technique".to_string(), "password".to_string()],
            "technique",
        );
        mem.store(
            "m3",
            "sqlite database backup procedure",
            vec!["procedure".to_string(), "database".to_string()],
            "procedure",
        );

        let results = mem.query("nmap network reconnaissance", 5, 0.1);
        assert!(!results.records.is_empty());
        // The nmap record should rank highest.
        assert_eq!(results.records[0].id, "m1");
    }

    #[test]
    fn tag_filter_works() {
        let mut mem = NeuralMemory::new();
        mem.store("t1", "recon technique", vec!["technique".into(), "recon".into()], "tech");
        mem.store("t2", "password cracking", vec!["technique".into(), "password".into()], "tech");
        let by_tech: Vec<&NeuralMemoryRecord> = mem.by_tags(&["technique".to_string()]);
        assert_eq!(by_tech.len(), 2);
        let by_recon: Vec<&NeuralMemoryRecord> = mem.by_tags(&["recon".to_string()]);
        assert_eq!(by_recon.len(), 1);
        assert_eq!(by_recon[0].id, "t1");
    }

    #[test]
    fn threshold_filters_results() {
        let mut mem = NeuralMemory::new();
        mem.store("x1", "completely unrelated text about cooking", vec![], "misc");
        let results = mem.query("nmap port scan", 5, 0.9);
        assert!(
            results.records.is_empty(),
            "high threshold should filter out unrelated text"
        );
    }

    #[test]
    fn serialize_roundtrip() {
        let mut mem = NeuralMemory::new();
        mem.store("s1", "test embedding text", vec!["tag1".into()], "cat");
        let _json = serde_json::to_string(&mem.records).unwrap();
        let restored: HashMap<String, NeuralMemoryRecord> = serde_json::from_str(&_json).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored["s1"].text, "test embedding text");
    }
}
