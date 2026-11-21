//! Fix history tracking: records every fix proposal decision
//! (staged, applied, rejected, rolled back) so the operator can review
//! what changed and why across sessions.
//!
//! Stored as JSONL in `<data_dir>/debug/fix-history.jsonl`. Each record
//! carries the diagnostic id, confidence, decision, file path, rationale,
//! and timestamp.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// One recorded fix decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixHistoryRecord {
    pub id: String,
    pub timestamp: u64,
    pub diagnostic_id: String,
    pub file_path: String,
    pub confidence: String,
    pub decision: FixDecision,
    pub rationale: String,
    pub applied_content_preview: String,
}

/// What happened to a fix proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixDecision {
    Staged,
    Applied,
    Rejected,
    RolledBack,
    Failed,
    Error,
}

impl FixDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::RolledBack => "rolled_back",
            Self::Failed => "failed",
            Self::Error => "error",
        }
    }
}

/// Tracks fix decisions, appending to a JSONL file.
pub struct FixHistory {
    path: PathBuf,
    seq: AtomicU64,
}

impl FixHistory {
    pub fn open(data_dir: impl Into<PathBuf>) -> Self {
        let dir = data_dir.into();
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("fix-history.jsonl");
        let seq = Self::count_existing(&path);
        Self {
            path,
            seq: AtomicU64::new(seq),
        }
    }

    fn count_existing(path: &PathBuf) -> u64 {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return 0,
        };
        BufReader::new(file).lines().count() as u64
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Record a fix decision. Returns the record id.
    pub fn record(
        &mut self,
        diagnostic_id: &str,
        file_path: &str,
        confidence: &str,
        decision: FixDecision,
        rationale: &str,
        applied_content_preview: &str,
    ) -> String {
        let id = format!(
            "fix-{}-{:04}",
            Self::now(),
            self.seq.fetch_add(1, Ordering::Relaxed)
        );
        let record = FixHistoryRecord {
            id: id.clone(),
            timestamp: Self::now(),
            diagnostic_id: diagnostic_id.to_string(),
            file_path: file_path.to_string(),
            confidence: confidence.to_string(),
            decision,
            rationale: rationale.to_string(),
            applied_content_preview: applied_content_preview.to_string(),
        };
        let line = serde_json::to_string(&record).unwrap_or_default();
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(f, "{line}");
        }
        id
    }

    /// Read all records, newest first.
    pub fn list(&self) -> Vec<FixHistoryRecord> {
        let file = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mut records: Vec<FixHistoryRecord> = BufReader::new(file)
            .lines()
            .map_while(|l| l.ok())
            .map_while(|l| serde_json::from_str(&l).ok())
            .collect();
        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.id.cmp(&a.id)));
        records
    }

    /// Filter records by decision.
    pub fn by_decision(&self, decision: FixDecision) -> Vec<FixHistoryRecord> {
        self.list()
            .into_iter()
            .filter(|r| r.decision == decision)
            .collect()
    }

    /// Clear all history (destructive).
    pub fn clear(&mut self) {
        let _ = fs::remove_file(&self.path);
        self.seq.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::improve::testing::temp_dir;

    #[test]
    fn record_and_list_roundtrip() {
        let temp = temp_dir("fix-history");
        let mut history = FixHistory::open(temp.path.join("debug"));
        let id1 = history.record(
            "diag-1",
            "src/lib.rs",
            "high",
            FixDecision::Applied,
            "unused import removed",
            "// removed use foo\n",
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
        let id2 = history.record(
            "diag-2",
            "src/main.rs",
            "medium",
            FixDecision::Staged,
            "type mismatch",
            ".to_string()\n",
        );
        assert_ne!(id1, id2);

        let records = history.list();
        assert_eq!(records.len(), 2);
        // Newest first
        assert_eq!(records[0].id, id2);
        assert_eq!(records[1].id, id1);
        assert_eq!(records[0].decision, FixDecision::Staged);
        assert_eq!(records[1].decision, FixDecision::Applied);
    }

    #[test]
    fn by_decision_filter() {
        let temp = temp_dir("fix-history-filter");
        let mut history = FixHistory::open(temp.path.join("debug"));
        history.record("d1", "a.rs", "high", FixDecision::Applied, "r1", "p1");
        history.record("d2", "b.rs", "medium", FixDecision::Rejected, "r2", "p2");
        history.record("d3", "c.rs", "low", FixDecision::Applied, "r3", "p3");

        let applied = history.by_decision(FixDecision::Applied);
        assert_eq!(applied.len(), 2);
        let rejected = history.by_decision(FixDecision::Rejected);
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].diagnostic_id, "d2");
    }

    #[test]
    fn clear_removes_all() {
        let temp = temp_dir("fix-history-clear");
        let mut history = FixHistory::open(temp.path.join("debug"));
        history.record("d1", "a.rs", "high", FixDecision::Applied, "r1", "p1");
        assert_eq!(history.list().len(), 1);
        history.clear();
        assert!(history.list().is_empty());
    }
}
