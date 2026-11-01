//! Append-only security audit log.
//!
//! Every HACKER MODE operation records one JSONL line. It is separate from
//! conversational memory and from the self-improvement audit log. Records
//! deliberately carry metadata only - never arguments, previews, or any value
//! that could contain a credential - and free-text fields are additionally
//! scrubbed as a belt-and-braces measure.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde::Serialize;

static EVENT_SEQ: AtomicU64 = AtomicU64::new(0);

/// One audit record. Optional fields are omitted (`serde(skip_serializing_if)`)
/// so the JSONL stays compact and never carries a `null` that could be
/// mistaken for a real value.
#[derive(Debug, Clone, Serialize)]
pub struct AuditRecord {
    pub event_id: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assessment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    /// `allowed` or `denied`.
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<String>,
    /// `ok`, `denied`, `error`, or `pending`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AuditRecord {
    pub fn new(decision: &str, status: &str) -> Self {
        Self {
            event_id: next_event_id(),
            timestamp: chrono::Local::now().to_rfc3339(),
            assessment_id: None,
            module: None,
            tool: None,
            operation: None,
            target: None,
            risk: None,
            decision: decision.to_string(),
            code: None,
            approval_ref: None,
            status: status.to_string(),
            duration_ms: None,
            detail: None,
        }
    }

    pub fn with_assessment(mut self, id: impl Into<String>) -> Self {
        self.assessment_id = Some(id.into());
        self
    }

    pub fn with_module(mut self, id: impl Into<String>) -> Self {
        self.module = Some(id.into());
        self
    }

    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self
    }

    pub fn with_operation(mut self, op: impl Into<String>) -> Self {
        self.operation = Some(op.into());
        self
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(scrub_secrets(&target.into()));
        self
    }

    pub fn with_risk(mut self, risk: impl Into<String>) -> Self {
        self.risk = Some(risk.into());
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_approval_ref(mut self, r: impl Into<String>) -> Self {
        self.approval_ref = Some(r.into());
        self
    }

    pub fn with_duration(mut self, d: Duration) -> Self {
        self.duration_ms = Some(d.as_millis().min(u64::MAX as u128) as u64);
        self
    }

    /// Free-text detail; scrubbed before it is stored.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(scrub_secrets(&detail.into()));
        self
    }
}

fn next_event_id() -> String {
    let seq = EVENT_SEQ.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{millis:x}-{seq:x}")
}

/// Redacts the value after common secret keys. Best-effort by design: it
/// cannot prove absence, but it removes the obvious `password=hunter2` cases
/// that must never reach disk.
pub fn scrub_secrets(text: &str) -> String {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)\b(api[_-]?key|apikey|token|password|passwd|secret|authorization)\b\s*[:=]?\s*(?:bearer\s+)?\S+",
        )
        .expect("valid secret-scan regex")
    });
    re.replace_all(text, "$1=***").into_owned()
}

/// Append-only writer bound to one file. `disabled()` is a no-op sink.
#[derive(Debug, Clone, Default)]
pub struct AuditLog {
    path: Option<PathBuf>,
}

impl AuditLog {
    /// Opens (or lazily creates) `dir/audit.jsonl`. Directory creation is
    /// best-effort: a failure leaves audit disabled rather than aborting the
    /// caller.
    pub fn open(dir: &Path) -> Self {
        if std::fs::create_dir_all(dir).is_err() {
            return Self::disabled();
        }
        Self {
            path: Some(dir.join("audit.jsonl")),
        }
    }

    pub fn disabled() -> Self {
        Self { path: None }
    }

    pub fn is_enabled(&self) -> bool {
        self.path.is_some()
    }

    /// Appends one record. Best-effort: an unwritable log never panics or
    /// blocks an operation, matching the memory subsystem's "quiet degrade".
    pub fn append(&self, record: &AuditRecord) {
        let Some(path) = &self.path else {
            return;
        };
        let Ok(line) = serde_json::to_string(record) else {
            return;
        };
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// HACKER MODE data directory: `NANOMITE_HACKER_DIR`, else
/// `~/.local/share/nanomite/hacker`.
pub fn hacker_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("NANOMITE_HACKER_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nanomite")
        .join("hacker")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_values_are_scrubbed() {
        assert_eq!(scrub_secrets("password=hunter2"), "password=***");
        assert_eq!(scrub_secrets("api_key: sk-abc123"), "api_key=***");
        assert_eq!(
            scrub_secrets("Authorization: Bearer abc.def"),
            "Authorization=***"
        );
        // Non-secret prose is untouched.
        assert_eq!(scrub_secrets("target is 10.0.0.1"), "target is 10.0.0.1");
    }

    #[test]
    fn append_writes_one_json_line_and_never_secrets() {
        let dir = std::env::temp_dir().join(format!("nanomite-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = AuditLog::open(&dir);
        assert!(log.is_enabled());

        let rec = AuditRecord::new("denied", "denied")
            .with_assessment("asm-1")
            .with_module("recon")
            .with_tool("ssh_run")
            .with_target("10.0.0.1")
            .with_risk("high")
            .with_code("hacker_target_out_of_scope")
            .with_detail("password=hunter2 rejected");
        log.append(&rec);
        log.append(&AuditRecord::new("allowed", "ok").with_assessment("asm-1"));

        let body = std::fs::read_to_string(dir.join("audit.jsonl")).unwrap();
        let lines: Vec<_> = body.lines().collect();
        assert_eq!(lines.len(), 2, "append-only, one line per record");
        assert!(lines[0].contains("\"code\":\"hacker_target_out_of_scope\""));
        assert!(lines[1].contains("\"decision\":\"allowed\""));
        // The credential must never reach disk, even inside a detail string.
        assert!(!body.contains("hunter2"), "secret leaked: {body}");
        assert!(body.contains("password=***"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disabled_log_is_a_noop() {
        let log = AuditLog::disabled();
        assert!(!log.is_enabled());
        log.append(&AuditRecord::new("allowed", "ok"));
    }
}
