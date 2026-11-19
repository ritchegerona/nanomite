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
//! Persists to SQLite (vectors + metadata + tags) so learning survives
//! restarts. Query returns top-k by cosine similarity above a threshold.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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

/// Neural memory store: SQLite-backed index + cosine-similarity recall.
#[derive(Debug, Clone)]
pub struct NeuralMemory {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl NeuralMemory {
    /// Open (or create) a SQLite database at `path`.
    pub fn open(path: &PathBuf) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path: path.clone(),
        })
    }

    /// Open an in-memory database (no persistence).
    pub fn in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path: PathBuf::new(),
        })
    }

    fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS neural_memories (
                id           TEXT PRIMARY KEY,
                text         TEXT NOT NULL,
                embedding    BLOB NOT NULL,
                tags         TEXT NOT NULL DEFAULT '[]',
                category     TEXT NOT NULL DEFAULT '',
                created_at   INTEGER NOT NULL,
                last_accessed INTEGER NOT NULL,
                access_count INTEGER NOT NULL DEFAULT 0,
                salience     REAL NOT NULL DEFAULT 1.0
            );
            CREATE INDEX IF NOT EXISTS idx_neural_memories_category ON neural_memories(category);
            CREATE INDEX IF NOT EXISTS idx_neural_memories_created_at ON neural_memories(created_at);
        ",
        )?;
        Ok(())
    }

    /// Store a new record, computing its embedding from the text.
    pub fn store(&self, id: &str, text: &str, tags: Vec<String>, category: &str) -> rusqlite::Result<()> {
        let embedding = embed_text(text, EMBED_DIM);
        let now = crate::neural::now_secs();
        let embedding_blob = serde_json::to_string(&embedding).unwrap_or_default();
        let tags_json = serde_json::to_string(&tags).unwrap_or_default();

        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO neural_memories
             (id, text, embedding, tags, category, created_at, last_accessed, access_count, salience)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                text,
                embedding_blob,
                tags_json,
                category,
                now,
                now,
                0,
                1.0f64,
            ],
        )?;
        Ok(())
    }

    /// Query by semantic similarity: return top-k records whose cosine
    /// similarity with the query embedding exceeds `threshold`.
    pub fn query(
        &self,
        query_text: &str,
        top_k: usize,
        threshold: f64,
    ) -> rusqlite::Result<NeuralMemoryQueryResult> {
        let query_embedding = embed_text(query_text, EMBED_DIM);
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, text, embedding, tags, category, created_at, last_accessed, access_count, salience
             FROM neural_memories
             ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let text: String = row.get(1)?;
            let embedding_blob: String = row.get(2)?;
            let tags_json: String = row.get(3)?;
            let category: String = row.get(4)?;
            let created_at: u64 = row.get(5)?;
            let last_accessed: u64 = row.get(6)?;
            let access_count: u64 = row.get(7)?;
            let salience: f64 = row.get(8)?;

            let embedding: Vec<f64> = serde_json::from_str(&embedding_blob).unwrap_or_default();
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

            Ok(NeuralMemoryRecord {
                id,
                text,
                embedding,
                tags,
                category,
                created_at,
                last_accessed,
                access_count,
                salience,
            })
        })?;

        let mut scored: Vec<(f64, NeuralMemoryRecord)> = Vec::new();
        for row in rows.flatten() {
            let sim = cosine_similarity(&query_embedding, &row.embedding);
            if sim >= threshold {
                scored.push((sim, row));
            }
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let taken: Vec<(f64, NeuralMemoryRecord)> = scored.into_iter().take(top_k).collect();

        // Update access metadata on recalled records.
        drop(stmt);
        let now = crate::neural::now_secs();
        for (_, r) in &taken {
            conn.execute(
                "UPDATE neural_memories
                 SET last_accessed = ?1, access_count = ?2, salience = ?3
                 WHERE id = ?4",
                params![
                    now,
                    r.access_count.saturating_add(1),
                    (r.salience * 0.9 + 0.1).min(2.0),
                    r.id,
                ],
            )?;
        }

        Ok(NeuralMemoryQueryResult {
            records: taken.iter().map(|(_, r)| r.clone()).collect(),
            similarities: taken.iter().map(|(s, _)| *s).collect(),
        })
    }

    /// Tag-based search: return records that contain all given tags.
    pub fn by_tags(&self, tags: &[String]) -> rusqlite::Result<Vec<NeuralMemoryRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, text, embedding, tags, category, created_at, last_accessed, access_count, salience
             FROM neural_memories",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let text: String = row.get(1)?;
            let embedding_blob: String = row.get(2)?;
            let tags_json: String = row.get(3)?;
            let category: String = row.get(4)?;
            let created_at: u64 = row.get(5)?;
            let last_accessed: u64 = row.get(6)?;
            let access_count: u64 = row.get(7)?;
            let salience: f64 = row.get(8)?;

            let embedding: Vec<f64> = serde_json::from_str(&embedding_blob).unwrap_or_default();
            let tags_vec: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

            Ok(NeuralMemoryRecord {
                id,
                text,
                embedding,
                tags: tags_vec,
                category,
                created_at,
                last_accessed,
                access_count,
                salience,
            })
        })?;

        let result: Vec<NeuralMemoryRecord> = rows
            .flatten()
            .filter(|r| tags.iter().all(|t| r.tags.contains(t)))
            .collect();

        Ok(result)
    }

    /// Record count.
    pub fn len(&self) -> rusqlite::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: u64 = conn.query_row("SELECT COUNT(*) FROM neural_memories", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    pub fn is_empty(&self) -> rusqlite::Result<bool> {
        Ok(self.len()? == 0)
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
    use std::env;

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
        let path = env::temp_dir().join(format!("neural_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mem = NeuralMemory::open(&path).expect("open db");
        mem.store(
            "m1",
            "nmap port scanning technique",
            vec!["technique".to_string(), "recon".to_string()],
            "technique",
        )
        .expect("store m1");
        mem.store(
            "m2",
            "hashcat password cracking rules",
            vec!["technique".to_string(), "password".to_string()],
            "technique",
        )
        .expect("store m2");
        mem.store(
            "m3",
            "sqlite database backup procedure",
            vec!["procedure".to_string(), "database".to_string()],
            "procedure",
        )
        .expect("store m3");

        let results = mem.query("nmap network reconnaissance", 5, 0.1).expect("query");
        assert!(!results.records.is_empty());
        // The nmap record should rank highest.
        assert_eq!(results.records[0].id, "m1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tag_filter_works() {
        let path = env::temp_dir().join(format!("neural_test_tags_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mem = NeuralMemory::open(&path).expect("open db");
        mem.store(
            "t1",
            "recon technique",
            vec!["technique".into(), "recon".into()],
            "tech",
        )
        .expect("store t1");
        mem.store(
            "t2",
            "password cracking",
            vec!["technique".into(), "password".into()],
            "tech",
        )
        .expect("store t2");
        let by_tech = mem.by_tags(&["technique".to_string()]).expect("by_tags");
        assert_eq!(by_tech.len(), 2);
        let by_recon = mem.by_tags(&["recon".to_string()]).expect("by_tags");
        assert_eq!(by_recon.len(), 1);
        assert_eq!(by_recon[0].id, "t1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn threshold_filters_results() {
        let path = env::temp_dir().join(format!("neural_test_thresh_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mem = NeuralMemory::open(&path).expect("open db");
        mem.store(
            "x1",
            "completely unrelated text about cooking",
            vec![],
            "misc",
        )
        .expect("store x1");
        let results = mem.query("nmap port scan", 5, 0.9).expect("query");
        assert!(
            results.records.is_empty(),
            "high threshold should filter out unrelated text"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn in_memory_database_works() {
        let mem = NeuralMemory::in_memory().expect("open in-memory");
        mem.store(
            "s1",
            "test embedding text",
            vec!["tag1".into()],
            "cat",
        )
        .expect("store");
        assert_eq!(mem.len().expect("len"), 1);
        assert!(!mem.is_empty().expect("is_empty"));
    }
}
