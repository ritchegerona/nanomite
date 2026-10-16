//! Append-only JSONL audit trail for every self-improvement state change.
//!
//! One JSON object per line; entries are fsynced so a crash can lose at most
//! the entry being written, never corrupt earlier ones. Reading tolerates
//! malformed lines (truncated writes, manual edits) by skipping them.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::proposal::{Actor, Target};
use super::ImproveError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Proposed,
    Rejected,
    Applied,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// RFC 3339 UTC timestamp, millisecond precision.
    pub ts: String,
    pub id: String,
    pub actor: Actor,
    pub action: AuditAction,
    pub target: Target,
    pub path: String,
    pub detail: String,
}

impl AuditEvent {
    pub fn new(
        id: &str,
        actor: Actor,
        action: AuditAction,
        target: Target,
        path: &Path,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            ts: timestamp_utc(now_millis()),
            id: id.to_string(),
            actor,
            action,
            target,
            path: path.to_string_lossy().into_owned(),
            detail: detail.into(),
        }
    }
}

/// Append-only JSONL log at a caller-supplied path.
#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Appends one event as a single JSON line and fsyncs it.
    pub fn append(&self, event: &AuditEvent) -> Result<(), ImproveError> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = serde_json::to_string(event)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    /// Reads every well-formed event; malformed lines are skipped.
    pub fn read_all(&self) -> Result<Vec<AuditEvent>, ImproveError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        Ok(text
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }
}

/// Milliseconds since the Unix epoch (0 when the clock is before the epoch).
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// RFC 3339 UTC timestamp with millisecond precision, no external deps.
pub fn timestamp_utc(millis: u64) -> String {
    let secs = (millis / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z",
        h = sod / 3600,
        m = (sod % 3600) / 60,
        s = sod % 60,
        ms = millis % 1000
    )
}

/// Days since 1970-01-01 to (year, month, day) — Howard Hinnant's civil
/// calendar algorithm, integer-only and proleptic-Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::{timestamp_utc, AuditAction, AuditEvent, AuditLog};
    use crate::improve::proposal::{Actor, Target};
    use crate::improve::testing::temp_dir;

    fn event(id: &str, action: AuditAction) -> AuditEvent {
        AuditEvent::new(
            id,
            Actor::Agent,
            action,
            Target::Source,
            std::path::Path::new("src/x.rs"),
            "detail",
        )
    }

    #[test]
    fn append_read_roundtrip() {
        let temp = temp_dir("audit-roundtrip");
        let log = AuditLog::new(temp.path.join("audit.jsonl"));
        log.append(&event("imp-1", AuditAction::Proposed)).unwrap();
        log.append(&event("imp-1", AuditAction::Rejected)).unwrap();
        log.append(&event("imp-2", AuditAction::Applied)).unwrap();
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].id, "imp-1");
        assert_eq!(events[0].action, AuditAction::Proposed);
        assert_eq!(events[0].actor, Actor::Agent);
        assert_eq!(events[0].target, Target::Source);
        assert_eq!(events[1].action, AuditAction::Rejected);
        assert_eq!(events[2].id, "imp-2");
        // Timestamps look like RFC 3339, not raw epoch counters.
        assert_eq!(events[0].ts.len(), 24);
        assert!(events[0].ts.ends_with('Z'));
    }

    #[test]
    fn malformed_lines_are_skipped() {
        use std::io::Write as _;
        let temp = temp_dir("audit-malformed");
        let log = AuditLog::new(temp.path.join("audit.jsonl"));
        log.append(&event("imp-1", AuditAction::Proposed)).unwrap();
        log.append(&event("imp-1", AuditAction::Applied)).unwrap();
        // Corrupt the log the way a truncated write or manual edit would.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(log_path(&temp))
            .unwrap();
        writeln!(file, "{{not json at all").unwrap();
        writeln!(file, "{{\"id\": \"imp-x\", \"ts\": \"trunc").unwrap();
        writeln!(file).unwrap();
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].action, AuditAction::Applied);
    }

    fn log_path(temp: &crate::improve::testing::TempDir) -> std::path::PathBuf {
        temp.path.join("audit.jsonl")
    }

    #[test]
    fn missing_log_reads_as_empty() {
        let temp = temp_dir("audit-missing");
        let log = AuditLog::new(temp.path.join("nested").join("audit.jsonl"));
        assert_eq!(log.read_all().unwrap(), Vec::new());
    }

    #[test]
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(timestamp_utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(timestamp_utc(1_767_225_600_000), "2026-01-01T00:00:00.000Z");
        assert_eq!(timestamp_utc(951_782_400_123), "2000-02-29T00:00:00.123Z");
    }
}
