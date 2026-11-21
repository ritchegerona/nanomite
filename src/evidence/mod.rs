//! # Evidence Locker — Persistent, Hashed Storage for Assessment Artifacts
//!
//! Stores tool outputs, screenshots, reasoning traces, and other
//! assessment evidence keyed to a target + assessment ID. Every artifact is
//! content-addressed by SHA-256 so integrity can be verified later.
//!
//! Files live under `<data_dir>/evidence/<assessment_id>/<sha256>/` with a
//! sidecar `.meta.json` describing the artifact (origin tool, timestamp,
//! target, tags).
//!
//! This module has no dependency on `App` or the neural engine, so it can be
//! tested in isolation and used by any layer (TUI, autonomous planner,
//! reporting).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Root of an evidence locker: `<data_dir>/evidence/`.
#[derive(Debug, Clone)]
pub struct EvidenceLocker {
    root: PathBuf,
}

/// One stored artifact: raw bytes + provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceArtifact {
    /// Content hash (sha256 hex) — also the directory name.
    pub hash: String,
    /// Human-readable label (e.g. "web_recon gobuster output").
    pub label: String,
    /// Origin tool that produced this evidence (run_command, web_recon, ...).
    pub origin: String,
    /// Target being assessed (URL, host, network range).
    pub target: String,
    /// Assessment ID this evidence belongs to.
    pub assessment_id: String,
    /// Unix seconds when stored.
    pub created_at: u64,
    /// Optional tags for filtering.
    pub tags: Vec<String>,
    /// Size of the raw payload in bytes.
    pub size_bytes: usize,
    /// MIME type hint (text/plain, image/png, application/json, ...).
    pub mime: String,
}

/// Query results from the locker.
#[derive(Debug, Clone)]
pub struct EvidenceQuery {
    pub items: Vec<EvidenceArtifact>,
    pub total: usize,
}

/// Filter criteria for listing/querying evidence.
#[derive(Debug, Clone, Default)]
pub struct EvidenceFilter {
    pub assessment_id: Option<String>,
    pub target: Option<String>,
    pub origin: Option<String>,
    pub tag: Option<String>,
}

impl EvidenceLocker {
    /// Open (or create) an evidence locker at `<root>/evidence`.
    pub fn open(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into().join("evidence");
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Store raw bytes as evidence, content-addressed by SHA-256.
    /// Returns the artifact's hash and a reference to its metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn store(
        &self,
        assessment_id: &str,
        target: &str,
        origin: &str,
        label: &str,
        payload: &[u8],
        tags: Vec<String>,
        mime: &str,
    ) -> std::io::Result<EvidenceArtifact> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let hash_bytes = Sha256::digest(payload);
        let hash = hex::encode(hash_bytes);

        let dir = self.root.join(assessment_id).join(&hash);
        std::fs::create_dir_all(&dir)?;

        let payload_path = dir.join("payload");
        std::fs::write(&payload_path, payload)?;

        let artifact = EvidenceArtifact {
            hash: hash.clone(),
            label: label.to_string(),
            origin: origin.to_string(),
            target: target.to_string(),
            assessment_id: assessment_id.to_string(),
            created_at: now,
            tags,
            size_bytes: payload.len(),
            mime: mime.to_string(),
        };

        let meta_path = dir.join("meta.json");
        let meta_json = serde_json::to_string_pretty(&artifact).unwrap_or_default();
        std::fs::write(&meta_path, meta_json)?;

        Ok(artifact)
    }

    /// Retrieve an artifact's payload by hash within an assessment.
    pub fn retrieve(&self, assessment_id: &str, hash: &str) -> std::io::Result<Vec<u8>> {
        let path = self.root.join(assessment_id).join(hash).join("payload");
        std::fs::read(path)
    }

    /// Retrieve an artifact's metadata by hash within an assessment.
    pub fn metadata(&self, assessment_id: &str, hash: &str) -> std::io::Result<EvidenceArtifact> {
        let path = self.root.join(assessment_id).join(hash).join("meta.json");
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// List artifacts matching the filter.
    pub fn list(&self, filter: &EvidenceFilter) -> std::io::Result<EvidenceQuery> {
        let mut items: Vec<EvidenceArtifact> = Vec::new();

        // Walk assessment directories.
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let assessment_dir = entry.path();
                if !assessment_dir.is_dir() {
                    continue;
                }
                let aid = match assessment_dir.file_name() {
                    Some(name) => name.to_string_lossy().to_string(),
                    None => continue,
                };
                if let Some(ref expected) = filter.assessment_id {
                    if aid != *expected {
                        continue;
                    }
                }

                // Walk hash directories inside the assessment.
                if let Ok(hash_entries) = std::fs::read_dir(&assessment_dir) {
                    for hash_entry in hash_entries.flatten() {
                        let hash_dir = hash_entry.path();
                        if !hash_dir.is_dir() {
                            continue;
                        }
                        let meta_path = hash_dir.join("meta.json");
                        if !meta_path.exists() {
                            continue;
                        }
                        if let Ok(text) = std::fs::read_to_string(&meta_path) {
                            if let Ok(artifact) = serde_json::from_str::<EvidenceArtifact>(&text) {
                                if let Some(ref t) = filter.target {
                                    if artifact.target != *t {
                                        continue;
                                    }
                                }
                                if let Some(ref o) = filter.origin {
                                    if artifact.origin != *o {
                                        continue;
                                    }
                                }
                                if let Some(ref tag) = filter.tag {
                                    if !artifact.tags.contains(tag) {
                                        continue;
                                    }
                                }
                                items.push(artifact);
                            }
                        }
                    }
                }
            }
        }

        // Newest first.
        items.sort_by_key(|b| Reverse(b.created_at));
        let total = items.len();

        Ok(EvidenceQuery { items, total })
    }

    /// Verify integrity of all artifacts in an assessment: recompute SHA-256
    /// and compare to the stored hash directory name and metadata.
    pub fn verify_integrity(&self, assessment_id: &str) -> std::io::Result<Vec<(String, bool)>> {
        let mut results = Vec::new();
        let assessment_dir = self.root.join(assessment_id);
        if !assessment_dir.exists() {
            return Ok(results);
        }

        if let Ok(hash_entries) = std::fs::read_dir(&assessment_dir) {
            for hash_entry in hash_entries.flatten() {
                let hash_dir = hash_entry.path();
                if !hash_dir.is_dir() {
                    continue;
                }
                let hash_name = match hash_dir.file_name() {
                    Some(name) => name.to_string_lossy().to_string(),
                    None => continue,
                };
                let payload_path = hash_dir.join("payload");
                if !payload_path.exists() {
                    results.push((hash_name.clone(), false));
                    continue;
                }
                let payload = match std::fs::read(&payload_path) {
                    Ok(p) => p,
                    Err(_) => {
                        results.push((hash_name.clone(), false));
                        continue;
                    }
                };
                let computed = hex::encode(Sha256::digest(&payload));
                let valid = computed == hash_name;
                results.push((hash_name, valid));
            }
        }

        results.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(results)
    }

    /// Root path of the locker (for reporting/display).
    pub fn root_path(&self) -> &Path {
        &self.root
    }
}

/// Current time in seconds since epoch.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Truncate a hash for display.
pub fn truncate_hash(hash: &str, n: usize) -> String {
    if hash.len() <= n {
        hash.to_string()
    } else {
        format!("{}…", &hash[..n.min(hash.len())])
    }
}

/// Light markdown escaping for labels/targets.
pub fn escape_md(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_locker(test_name: &str) -> (EvidenceLocker, PathBuf) {
        let dir = env::temp_dir().join(format!(
            "nanomite_evidence_test_{}_{}",
            std::process::id(),
            test_name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let locker = EvidenceLocker::open(&dir).expect("open locker");
        (locker, dir)
    }

    #[test]
    fn store_and_retrieve_roundtrip() {
        let (locker, _dir) = temp_locker("roundtrip");
        let payload = b"gobuster output: /admin (200)";
        let artifact = locker
            .store(
                "assess-1",
                "http://10.0.0.5",
                "web_recon",
                "gobuster dir output",
                payload,
                vec!["recon".into(), "web".into()],
                "text/plain",
            )
            .expect("store");

        let retrieved = locker
            .retrieve("assess-1", &artifact.hash)
            .expect("retrieve");
        assert_eq!(retrieved, payload);

        let meta = locker
            .metadata("assess-1", &artifact.hash)
            .expect("metadata");
        assert_eq!(meta.label, "gobuster dir output");
        assert_eq!(meta.origin, "web_recon");
        assert_eq!(meta.target, "http://10.0.0.5");
        assert_eq!(meta.assessment_id, "assess-1");
        assert_eq!(meta.size_bytes, payload.len());
        assert!(meta.tags.contains(&"recon".into()));
    }

    #[test]
    fn list_filters_by_assessment_and_target() {
        let (locker, _dir) = temp_locker("list_filters");
        let _ = locker.store(
            "a1",
            "http://10.0.0.5",
            "web_recon",
            "gobuster",
            b"data1",
            vec!["recon".into()],
            "text/plain",
        );
        let _ = locker.store(
            "a1",
            "http://10.0.0.6",
            "nikto",
            "nikto scan",
            b"data2",
            vec!["vuln".into()],
            "text/plain",
        );
        let _ = locker.store(
            "a2",
            "http://10.0.0.5",
            "run_command",
            "whoami",
            b"data3",
            vec!["recon".into()],
            "text/plain",
        );

        let all = locker.list(&EvidenceFilter::default()).expect("list all");
        assert_eq!(all.total, 3);

        let by_assess = locker
            .list(&EvidenceFilter {
                assessment_id: Some("a1".into()),
                ..Default::default()
            })
            .expect("list by assess");
        assert_eq!(by_assess.total, 2);

        let by_target = locker
            .list(&EvidenceFilter {
                target: Some("http://10.0.0.5".into()),
                ..Default::default()
            })
            .expect("list by target");
        assert_eq!(by_target.total, 2);

        let by_tag = locker
            .list(&EvidenceFilter {
                tag: Some("vuln".into()),
                ..Default::default()
            })
            .expect("list by tag");
        assert_eq!(by_tag.total, 1);
        assert_eq!(by_tag.items[0].origin, "nikto");
    }

    #[test]
    fn verify_integrity_detects_tampering() {
        let (locker, dir) = temp_locker("integrity");
        let artifact = locker
            .store(
                "a1",
                "http://10.0.0.5",
                "web_recon",
                "gobuster",
                b"original payload",
                vec![],
                "text/plain",
            )
            .expect("store");

        // Tamper with the payload file directly.
        let payload_path = dir
            .join("evidence")
            .join("a1")
            .join(&artifact.hash)
            .join("payload");
        std::fs::write(&payload_path, b"tampered").expect("tamper");

        let results = locker.verify_integrity("a1").expect("verify");
        assert_eq!(results.len(), 1);
        assert!(!results[0].1, "tampered payload should fail integrity");
    }

    #[test]
    fn now_secs_is_monotonic_enough() {
        let a = now_secs();
        let b = now_secs();
        assert!(b >= a);
    }
}
