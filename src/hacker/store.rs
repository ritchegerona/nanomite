//! SQLite store for findings and evidence (HACKER MODE Phase 3).
//!
//! Follows the conventions of `board::store` and `memory::store`: a synchronous
//! connection behind a `Mutex` (single-process TUI, one writer), `bundled`
//! rusqlite, typed errors, and an `open_in_memory()` used by tests. The UI is
//! expected to drive it from `spawn_blocking`, never on the event-loop thread.
//!
//! **Untrusted data.** Findings carry text taken from HTTP headers, tool output
//! and CVE metadata. Every stored string is scrubbed ([`scrub_secrets`]) and
//! bounded, and each evidence blob is recorded with a SHA-256 so a later reader
//! can tell whether what they are looking at is what was captured.

use rusqlite::{params, Connection, OptionalExtension, Row};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::SystemTime;

use super::audit::{hacker_dir, scrub_secrets};
use super::context::{AssessmentContext, AssessmentStatus};
use super::findings::{Finding, FindingStatus};
use super::modules::ModuleReport;

/// Database filename under the hacker directory.
pub const STORE_FILE: &str = "findings.db";
/// Absolute ceiling on any single listing.
pub const HARD_FINDING_LIMIT: usize = 1_000;
/// Maximum characters of evidence excerpt kept per finding.
pub const MAX_EVIDENCE_CHARS: usize = 4_000;

/// Every persisted error class for the findings store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingStoreError {
    Invalid(String),
    Database(String),
    Io(String),
}

impl std::fmt::Display for FindingStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FindingStoreError::Invalid(msg) => write!(f, "invalid finding store input: {msg}"),
            FindingStoreError::Database(msg) => write!(f, "findings database error: {msg}"),
            FindingStoreError::Io(msg) => write!(f, "findings store io error: {msg}"),
        }
    }
}

impl std::error::Error for FindingStoreError {}

impl From<rusqlite::Error> for FindingStoreError {
    fn from(err: rusqlite::Error) -> Self {
        FindingStoreError::Database(err.to_string())
    }
}

/// A persisted assessment header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAssessment {
    pub id: String,
    pub name: String,
    pub authorization_ref: Option<String>,
    pub status: String,
    pub started_at: String,
    pub expiry_at: String,
    pub finding_count: i64,
}

/// A persisted assessment together with everything needed to resume it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAssessmentSpec {
    pub id: String,
    pub name: String,
    pub authorization_ref: Option<String>,
    pub status: String,
    pub start: SystemTime,
    pub expiry: SystemTime,
    pub cidrs: Vec<String>,
    pub domains: Vec<String>,
}

/// Counts for one scope (all assessments, or a single one).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReportSummary {
    pub total: usize,
    /// `(severity label, count)`, ordered by the severity scale.
    pub by_severity: Vec<(String, usize)>,
    /// `(status label, count)`, ordered alphabetically.
    pub by_status: Vec<(String, usize)>,
}

/// Result of re-hashing one evidence row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceCheck {
    pub id: i64,
    pub ok: bool,
}

/// A persisted finding row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFinding {
    pub id: i64,
    pub assessment_id: String,
    pub module: String,
    pub operation: String,
    pub target: String,
    pub title: String,
    pub severity: String,
    pub confidence: String,
    pub status: String,
    pub detail: String,
    pub created_at: String,
}

/// One evidence row bound to a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvidence {
    pub id: i64,
    pub finding_id: i64,
    pub kind: String,
    pub sha256: String,
    pub bytes: i64,
    pub excerpt: String,
    pub created_at: String,
}

/// Something that can persist the findings of a finished operation.
///
/// Injected into the coordinator so tests never touch disk; production uses a
/// [`FindingsStore`], a disabled config uses [`NullSink`].
pub trait FindingSink: Send + Sync {
    /// Persists the assessment header and every finding in `report`.
    /// Returns the number of findings written.
    fn record(&self, ctx: &AssessmentContext, report: &ModuleReport) -> Result<usize, String>;

    /// Whether this sink actually persists anything.
    fn enabled(&self) -> bool {
        true
    }
}

/// Write **and** read access to findings, as the coordinator's `reporting`
/// operations need both. Implemented by [`FindingsStore`]; [`NullSink`]
/// answers reads with an error so a disabled store never fakes a report.
pub trait FindingsBackend: FindingSink {
    fn summary(&self, assessment_id: Option<&str>) -> Result<ReportSummary, String>;
    fn export(
        &self,
        dir: &Path,
        assessment_id: Option<&str>,
        format: ReportFormat,
    ) -> Result<PathBuf, String>;
}

/// A sink that persists nothing. Used when the store is disabled.
pub struct NullSink;

impl FindingSink for NullSink {
    fn record(&self, _ctx: &AssessmentContext, _report: &ModuleReport) -> Result<usize, String> {
        Ok(0)
    }
    fn enabled(&self) -> bool {
        false
    }
}

impl FindingsBackend for NullSink {
    fn summary(&self, _assessment_id: Option<&str>) -> Result<ReportSummary, String> {
        Err("findings store is disabled".into())
    }
    fn export(
        &self,
        _dir: &Path,
        _assessment_id: Option<&str>,
        _format: ReportFormat,
    ) -> Result<PathBuf, String> {
        Err("findings store is disabled".into())
    }
}

/// Export format for a findings report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Markdown,
    Json,
    Sarif,
}

impl ReportFormat {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "md" | "markdown" => Some(ReportFormat::Markdown),
            "json" => Some(ReportFormat::Json),
            "sarif" => Some(ReportFormat::Sarif),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ReportFormat::Markdown => "md",
            ReportFormat::Json => "json",
            ReportFormat::Sarif => "sarif",
        }
    }
}

/// Findings and evidence store bound to one database file.
pub struct FindingsStore {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl FindingsStore {
    /// Path of the default database (`<hacker dir>/findings.db`).
    pub fn default_path() -> PathBuf {
        hacker_dir().join(STORE_FILE)
    }

    /// Opens (creating if needed) the database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FindingStoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    FindingStoreError::Io(format!("create {}: {e}", parent.display()))
                })?;
            }
        }
        let conn = Connection::open(&path)
            .map_err(|e| FindingStoreError::Database(format!("open {}: {e}", path.display())))?;
        let store = Self {
            conn: Mutex::new(conn),
            path: Some(path),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Private, non-persistent database. Used by tests and throwaway flows.
    pub fn open_in_memory() -> Result<Self, FindingStoreError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| FindingStoreError::Database(format!("open in-memory: {e}")))?;
        let store = Self {
            conn: Mutex::new(conn),
            path: None,
        };
        store.init_schema()?;
        Ok(store)
    }

    /// A poisoned mutex is recovered rather than propagated: one panicking
    /// caller must not wedge the store for the rest of the session.
    fn lock(&self) -> MutexGuard<'_, Connection> {
        match self.conn.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn init_schema(&self) -> Result<(), FindingStoreError> {
        let conn = self.lock();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS assessments (
                id                TEXT PRIMARY KEY,
                name              TEXT NOT NULL,
                authorization_ref TEXT,
                status            TEXT NOT NULL,
                started_at        TEXT NOT NULL,
                expiry_at         TEXT NOT NULL,
                created_at        TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS findings (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                assessment_id TEXT    NOT NULL,
                module        TEXT    NOT NULL,
                operation     TEXT    NOT NULL,
                target        TEXT    NOT NULL,
                title         TEXT    NOT NULL,
                severity      TEXT    NOT NULL,
                confidence    TEXT    NOT NULL,
                status        TEXT    NOT NULL,
                detail        TEXT    NOT NULL DEFAULT '',
                created_at    TEXT    NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_findings_assessment
                ON findings(assessment_id);
            CREATE TABLE IF NOT EXISTS evidence (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                finding_id INTEGER NOT NULL,
                kind       TEXT    NOT NULL,
                sha256     TEXT    NOT NULL,
                bytes      INTEGER NOT NULL,
                excerpt    TEXT    NOT NULL,
                created_at TEXT    NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_evidence_finding
                ON evidence(finding_id);
            CREATE TABLE IF NOT EXISTS assessment_scopes (
                assessment_id TEXT PRIMARY KEY,
                cidrs         TEXT NOT NULL DEFAULT '[]',
                domains       TEXT NOT NULL DEFAULT '[]',
                updated_at    TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    /// Inserts or refreshes the assessment header for `ctx`.
    pub fn upsert_assessment(&self, ctx: &AssessmentContext) -> Result<(), FindingStoreError> {
        let conn = self.lock();
        upsert_assessment(&conn, ctx)
    }

    /// Persists `ctx`'s authorized scope, so the assessment can be listed with
    /// its scope and resumed later.
    pub fn save_scope(&self, ctx: &AssessmentContext) -> Result<(), FindingStoreError> {
        let conn = self.lock();
        save_scope(&conn, ctx)
    }

    /// Updates an assessment's lifecycle status. Returns `false` for unknown ids.
    pub fn set_assessment_status(
        &self,
        id: &str,
        status: AssessmentStatus,
    ) -> Result<bool, FindingStoreError> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE assessments SET status = ?1 WHERE id = ?2",
            params![status.label(), id],
        )?;
        Ok(changed > 0)
    }

    /// Loads one assessment with its persisted scope, for resuming.
    pub fn load_assessment(
        &self,
        id: &str,
    ) -> Result<Option<StoredAssessmentSpec>, FindingStoreError> {
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT a.id, a.name, a.authorization_ref, a.status, a.started_at, a.expiry_at,
                        COALESCE(s.cidrs, '[]'), COALESCE(s.domains, '[]')
                 FROM assessments a
                 LEFT JOIN assessment_scopes s ON s.assessment_id = a.id
                 WHERE a.id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, name, authorization_ref, status, start, expiry, cidrs, domains)) = row else {
            return Ok(None);
        };
        let start = parse_rfc3339(&start).ok_or_else(|| {
            FindingStoreError::Database(format!("assessment {id}: bad started_at"))
        })?;
        let expiry = parse_rfc3339(&expiry).ok_or_else(|| {
            FindingStoreError::Database(format!("assessment {id}: bad expiry_at"))
        })?;
        Ok(Some(StoredAssessmentSpec {
            id,
            name,
            authorization_ref,
            status,
            start,
            expiry,
            cidrs: parse_json_list(&cidrs),
            domains: parse_json_list(&domains),
        }))
    }

    /// Finding counts by severity and status, optionally scoped.
    pub fn summary(&self, assessment_id: Option<&str>) -> Result<ReportSummary, FindingStoreError> {
        let conn = self.lock();
        let mut out = ReportSummary::default();
        {
            let mut stmt = conn.prepare(
                "SELECT severity, COUNT(*) FROM findings
                 WHERE (?1 IS NULL OR assessment_id = ?1)
                 GROUP BY severity",
            )?;
            let rows = stmt.query_map(params![assessment_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (severity, count) = row?;
                out.by_severity.push((severity, count.max(0) as usize));
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT status, COUNT(*) FROM findings
                 WHERE (?1 IS NULL OR assessment_id = ?1)
                 GROUP BY status",
            )?;
            let rows = stmt.query_map(params![assessment_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (status, count) = row?;
                out.by_status.push((status, count.max(0) as usize));
            }
        }
        out.total = out.by_severity.iter().map(|(_, n)| n).sum();
        // Deterministic order: descending severity, then alphabetical status.
        out.by_severity
            .sort_by_key(|(label, _)| std::cmp::Reverse(severity_rank(label)));
        out.by_status.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Re-hashes each evidence row of a finding and reports whether it matches
    /// the recorded digest (empty when the finding has no evidence).
    pub fn verify_evidence(
        &self,
        finding_id: i64,
    ) -> Result<Vec<EvidenceCheck>, FindingStoreError> {
        let rows = self.evidence(finding_id)?;
        Ok(rows
            .into_iter()
            .map(|e| EvidenceCheck {
                id: e.id,
                ok: sha256_hex(e.excerpt.as_bytes()) == e.sha256,
            })
            .collect())
    }

    /// Persists one finding and its evidence row; returns the finding id.
    pub fn insert_finding(
        &self,
        assessment_id: &str,
        finding: &Finding,
    ) -> Result<i64, FindingStoreError> {
        if assessment_id.trim().is_empty() {
            return Err(FindingStoreError::Invalid(
                "assessment_id must not be empty".into(),
            ));
        }
        let conn = self.lock();
        insert_finding(&conn, assessment_id, finding)
    }

    /// Newest findings first, optionally scoped to one assessment.
    pub fn findings(
        &self,
        assessment_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredFinding>, FindingStoreError> {
        let limit = limit.min(HARD_FINDING_LIMIT) as i64;
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, assessment_id, module, operation, target, title, severity,
                    confidence, status, detail, created_at
             FROM findings
             WHERE (?1 IS NULL OR assessment_id = ?1)
             ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![assessment_id, limit], map_finding)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(FindingStoreError::from)
    }

    /// One finding by id.
    pub fn finding(&self, id: i64) -> Result<Option<StoredFinding>, FindingStoreError> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, assessment_id, module, operation, target, title, severity,
                    confidence, status, detail, created_at
             FROM findings WHERE id = ?1",
        )?;
        stmt.query_row(params![id], map_finding)
            .optional()
            .map_err(FindingStoreError::from)
    }

    /// Updates a finding's triage status. Returns `false` when no such id.
    pub fn set_status(&self, id: i64, status: FindingStatus) -> Result<bool, FindingStoreError> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE findings SET status = ?1 WHERE id = ?2",
            params![status.label(), id],
        )?;
        Ok(changed > 0)
    }

    /// Evidence rows for a finding, oldest first.
    pub fn evidence(&self, finding_id: i64) -> Result<Vec<StoredEvidence>, FindingStoreError> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, finding_id, kind, sha256, bytes, excerpt, created_at
             FROM evidence WHERE finding_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![finding_id], |row| {
            Ok(StoredEvidence {
                id: row.get(0)?,
                finding_id: row.get(1)?,
                kind: row.get(2)?,
                sha256: row.get(3)?,
                bytes: row.get(4)?,
                excerpt: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(FindingStoreError::from)
    }

    /// Assessment headers, most recently created first.
    pub fn assessments(&self, limit: usize) -> Result<Vec<StoredAssessment>, FindingStoreError> {
        let limit = limit.min(HARD_FINDING_LIMIT) as i64;
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT a.id, a.name, a.authorization_ref, a.status, a.started_at, a.expiry_at,
                    COUNT(f.id)
             FROM assessments a
             LEFT JOIN findings f ON f.assessment_id = a.id
             GROUP BY a.id
             ORDER BY a.created_at DESC, a.id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(StoredAssessment {
                id: row.get(0)?,
                name: row.get(1)?,
                authorization_ref: row.get(2)?,
                status: row.get(3)?,
                started_at: row.get(4)?,
                expiry_at: row.get(5)?,
                finding_count: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(FindingStoreError::from)
    }

    /// Number of findings, optionally scoped to one assessment.
    pub fn count_findings(&self, assessment_id: Option<&str>) -> Result<i64, FindingStoreError> {
        let conn = self.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM findings WHERE (?1 IS NULL OR assessment_id = ?1)",
            params![assessment_id],
            |row| row.get(0),
        )
        .map_err(FindingStoreError::from)
    }

    /// Renders a markdown report of the findings, newest first.
    pub fn render_markdown(
        &self,
        assessment_id: Option<&str>,
    ) -> Result<String, FindingStoreError> {
        let assessments = self.assessments(HARD_FINDING_LIMIT)?;
        let header = match assessment_id {
            Some(id) => assessments
                .iter()
                .find(|a| a.id == id)
                .map(|a| {
                    format!(
                        "# Findings — {} (`{}`)\n\nauthorization: {}\nstatus: {} · window: {} → {}",
                        a.name,
                        a.id,
                        a.authorization_ref.as_deref().unwrap_or("(none recorded)"),
                        a.status,
                        a.started_at,
                        a.expiry_at
                    )
                })
                .unwrap_or_else(|| format!("# Findings — `{id}`")),
            None => "# Findings — all assessments".to_string(),
        };
        let findings = self.findings(assessment_id, HARD_FINDING_LIMIT)?;
        let mut out = header;
        out.push_str(&format!("\n\ngenerated: {}\n", now_rfc3339()));
        out.push_str(&format!("\ntotal findings: {}\n", findings.len()));
        for f in &findings {
            out.push_str(&format!(
                "\n## {} · {}\n\n- id: {}\n- assessment: {}\n- module: {}/{}\n- target: {}\n- confidence: {}\n- status: {}\n",
                f.severity, f.title, f.id, f.assessment_id, f.module, f.operation, f.target,
                f.confidence, f.status
            ));
            if !f.detail.is_empty() {
                out.push_str(&format!("\n{}\n", f.detail));
            }
        }
        if findings.is_empty() {
            out.push_str("\n(no findings recorded)\n");
        }
        Ok(out)
    }

    /// Renders a machine-readable JSON report.
    pub fn render_json(&self, assessment_id: Option<&str>) -> Result<String, FindingStoreError> {
        let findings = self.findings(assessment_id, HARD_FINDING_LIMIT)?;
        let entries: Vec<serde_json::Value> = findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "id": f.id,
                    "assessment_id": f.assessment_id,
                    "module": f.module,
                    "operation": f.operation,
                    "target": f.target,
                    "title": f.title,
                    "severity": f.severity,
                    "confidence": f.confidence,
                    "status": f.status,
                    "detail": f.detail,
                    "created_at": f.created_at,
                })
            })
            .collect();
        let doc = serde_json::json!({
            "tool": "nanomite-hacker",
            "version": env!("CARGO_PKG_VERSION"),
            "assessment": assessment_id,
            "generated": now_rfc3339(),
            "finding_count": entries.len(),
            "findings": entries,
        });
        serde_json::to_string_pretty(&doc)
            .map_err(|e| FindingStoreError::Database(format!("json: {e}")))
    }

    /// Renders a SARIF 2.1.0 report (the shape CI/security dashboards ingest).
    pub fn render_sarif(&self, assessment_id: Option<&str>) -> Result<String, FindingStoreError> {
        let findings = self.findings(assessment_id, HARD_FINDING_LIMIT)?;
        let results: Vec<serde_json::Value> = findings
            .iter()
            .map(|f| {
                let text = if f.detail.is_empty() {
                    f.title.clone()
                } else {
                    format!("{}\n{}", f.title, f.detail)
                };
                serde_json::json!({
                    "ruleId": format!("{}/{}", f.module, f.operation),
                    "level": sarif_level(&f.severity),
                    "message": { "text": text },
                    "locations": [{
                        "physicalLocation": {
                            "artifactLocation": { "uri": f.target }
                        }
                    }],
                    "properties": {
                        "findingId": f.id,
                        "severity": f.severity,
                        "confidence": f.confidence,
                        "status": f.status,
                    }
                })
            })
            .collect();
        let doc = serde_json::json!({
            "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
            "version": "2.1.0",
            "runs": [{
                "tool": {
                    "driver": {
                        "name": "nanomite-hacker",
                        "informationUri": "https://github.com/ritchegerona/nanomite",
                        "version": env!("CARGO_PKG_VERSION"),
                    }
                },
                "results": results,
            }]
        });
        serde_json::to_string_pretty(&doc)
            .map_err(|e| FindingStoreError::Database(format!("sarif: {e}")))
    }

    /// Renders a report in `format`.
    pub fn render(
        &self,
        assessment_id: Option<&str>,
        format: ReportFormat,
    ) -> Result<String, FindingStoreError> {
        match format {
            ReportFormat::Markdown => self.render_markdown(assessment_id),
            ReportFormat::Json => self.render_json(assessment_id),
            ReportFormat::Sarif => self.render_sarif(assessment_id),
        }
    }

    /// Writes a report in `format` into `dir` and returns the path.
    pub fn export(
        &self,
        dir: &Path,
        assessment_id: Option<&str>,
        format: ReportFormat,
    ) -> Result<PathBuf, FindingStoreError> {
        let body = self.render(assessment_id, format)?;
        std::fs::create_dir_all(dir)
            .map_err(|e| FindingStoreError::Io(format!("create {}: {e}", dir.display())))?;
        let stem = assessment_id.unwrap_or("all");
        let name = format!(
            "{}-{}.{}",
            sanitize_file_stem(stem),
            now_rfc3339().replace([':', '+'], "-"),
            format.extension()
        );
        let path = dir.join(name);
        std::fs::write(&path, body)
            .map_err(|e| FindingStoreError::Io(format!("write {}: {e}", path.display())))?;
        Ok(path)
    }

    /// Writes a markdown report into `dir` and returns the path.
    pub fn export_markdown(
        &self,
        dir: &Path,
        assessment_id: Option<&str>,
    ) -> Result<PathBuf, FindingStoreError> {
        self.export(dir, assessment_id, ReportFormat::Markdown)
    }
}

/// Maps a stored severity label onto a SARIF result level.
fn sarif_level(severity: &str) -> &'static str {
    match severity {
        "medium" => "warning",
        "high" | "critical" => "error",
        _ => "note",
    }
}

impl FindingsBackend for FindingsStore {
    fn summary(&self, assessment_id: Option<&str>) -> Result<ReportSummary, String> {
        FindingsStore::summary(self, assessment_id).map_err(|e| e.to_string())
    }

    fn export(
        &self,
        dir: &Path,
        assessment_id: Option<&str>,
        format: ReportFormat,
    ) -> Result<PathBuf, String> {
        FindingsStore::export(self, dir, assessment_id, format).map_err(|e| e.to_string())
    }
}

impl FindingSink for FindingsStore {
    /// Persists the assessment header, scope, every finding and one evidence row
    /// per finding (the finding's detail, hashed). Done under a single lock so a
    /// partially written operation is never observable.
    fn record(&self, ctx: &AssessmentContext, report: &ModuleReport) -> Result<usize, String> {
        let conn = self.lock();
        upsert_assessment(&conn, ctx).map_err(|e| e.to_string())?;
        save_scope(&conn, ctx).map_err(|e| e.to_string())?;
        for finding in &report.findings {
            let id = insert_finding(&conn, &ctx.id, finding).map_err(|e| e.to_string())?;
            let excerpt = bounded_scrub(&finding.detail);
            insert_evidence(&conn, id, "operation-output", &excerpt).map_err(|e| e.to_string())?;
        }
        Ok(report.findings.len())
    }
}

fn save_scope(conn: &Connection, ctx: &AssessmentContext) -> Result<(), FindingStoreError> {
    let cidrs = serde_json::to_string(&ctx.allowed_cidrs).unwrap_or_else(|_| "[]".into());
    let domains = serde_json::to_string(&ctx.allowed_domains).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO assessment_scopes (assessment_id, cidrs, domains, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(assessment_id) DO UPDATE SET
            cidrs = excluded.cidrs,
            domains = excluded.domains,
            updated_at = excluded.updated_at",
        params![ctx.id, cidrs, domains, now_rfc3339()],
    )?;
    Ok(())
}

fn parse_json_list(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn severity_rank(label: &str) -> u8 {
    match label {
        "informational" => 0,
        "low" => 1,
        "medium" => 2,
        "high" => 3,
        "critical" => 4,
        _ => 5,
    }
}

fn parse_rfc3339(raw: &str) -> Option<SystemTime> {
    let dt = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
    Some(dt.with_timezone(&chrono::Utc).into())
}

fn upsert_assessment(conn: &Connection, ctx: &AssessmentContext) -> Result<(), FindingStoreError> {
    conn.execute(
        "INSERT INTO assessments (id, name, authorization_ref, status, started_at, expiry_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            authorization_ref = excluded.authorization_ref,
            status = excluded.status,
            started_at = excluded.started_at,
            expiry_at = excluded.expiry_at",
        params![
            ctx.id,
            ctx.name,
            ctx.authorization_ref,
            ctx.status.label(),
            sys_to_rfc3339(ctx.start),
            sys_to_rfc3339(ctx.expiry),
            now_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_finding(
    conn: &Connection,
    assessment_id: &str,
    finding: &Finding,
) -> Result<i64, FindingStoreError> {
    conn.execute(
        "INSERT INTO findings
            (assessment_id, module, operation, target, title, severity, confidence, status, detail, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            assessment_id,
            finding.module,
            finding.operation,
            bounded_scrub(&finding.target),
            bounded_scrub(&finding.title),
            finding.severity.label(),
            finding.confidence.label(),
            finding.status.label(),
            bounded_scrub(&finding.detail),
            now_rfc3339(),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn insert_evidence(
    conn: &Connection,
    finding_id: i64,
    kind: &str,
    excerpt: &str,
) -> Result<i64, FindingStoreError> {
    let bytes = excerpt.as_bytes();
    conn.execute(
        "INSERT INTO evidence (finding_id, kind, sha256, bytes, excerpt, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            finding_id,
            kind,
            sha256_hex(bytes),
            bytes.len() as i64,
            excerpt,
            now_rfc3339(),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn map_finding(row: &Row<'_>) -> rusqlite::Result<StoredFinding> {
    Ok(StoredFinding {
        id: row.get(0)?,
        assessment_id: row.get(1)?,
        module: row.get(2)?,
        operation: row.get(3)?,
        target: row.get(4)?,
        title: row.get(5)?,
        severity: row.get(6)?,
        confidence: row.get(7)?,
        status: row.get(8)?,
        detail: row.get(9)?,
        created_at: row.get(10)?,
    })
}

/// SHA-256 of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Scrubs secrets and bounds an evidence excerpt on a char boundary.
fn bounded_scrub(text: &str) -> String {
    let scrubbed = scrub_secrets(text);
    if scrubbed.chars().count() <= MAX_EVIDENCE_CHARS {
        return scrubbed;
    }
    let mut out: String = scrubbed.chars().take(MAX_EVIDENCE_CHARS - 1).collect();
    out.push('…');
    out
}

fn sanitize_file_stem(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "assessment".into()
    } else {
        cleaned.chars().take(60).collect()
    }
}

fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339()
}

fn sys_to_rfc3339(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).to_rfc3339()
}

/// Where exported reports are written (under the hacker directory).
pub fn reports_dir() -> PathBuf {
    hacker_dir().join("reports")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hacker::context::AssessmentStatus;
    use crate::hacker::findings::Confidence;
    use crate::hacker::risk::RiskLevel;
    use std::time::Duration;

    fn ctx() -> AssessmentContext {
        let now = SystemTime::now();
        let mut ctx = AssessmentContext::new("asm-1", "Lab");
        ctx.allowed_cidrs = vec!["10.0.0.0/8".into()];
        ctx.permitted_modules = vec!["web".into()];
        ctx.authorization_ref = Some("LAB-1".into());
        ctx.status = AssessmentStatus::Authorized;
        ctx.start = now;
        ctx.expiry = now + Duration::from_secs(3600);
        ctx
    }

    fn report() -> ModuleReport {
        ModuleReport::new("web", "headers", "https://example.com", "1 finding").finding(
            Finding::new(
                "web",
                "headers",
                "https://example.com",
                "missing content-security-policy",
                RiskLevel::Medium,
            )
            .confidence(Confidence::Confirmed)
            .detail("body: no CSP header · password=hunter2"),
        )
    }

    #[test]
    fn record_persists_assessment_and_findings_with_evidence() {
        let store = FindingsStore::open_in_memory().unwrap();
        let n = store.record(&ctx(), &report()).unwrap();
        assert_eq!(n, 1);

        let assessments = store.assessments(10).unwrap();
        assert_eq!(assessments.len(), 1);
        assert_eq!(assessments[0].id, "asm-1");
        assert_eq!(assessments[0].finding_count, 1);
        assert_eq!(assessments[0].authorization_ref.as_deref(), Some("LAB-1"));

        let findings = store.findings(Some("asm-1"), 10).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "medium");
        assert_eq!(findings[0].confidence, "confirmed");
        assert_eq!(findings[0].status, "open");

        let evidence = store.evidence(findings[0].id).unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].kind, "operation-output");
        assert_eq!(evidence[0].bytes as usize, evidence[0].excerpt.len());
        assert_eq!(
            evidence[0].sha256,
            sha256_hex(evidence[0].excerpt.as_bytes())
        );
        // The secret must not survive to disk.
        assert!(
            !evidence[0].excerpt.contains("hunter2"),
            "{}",
            evidence[0].excerpt
        );
        assert!(evidence[0].excerpt.contains("password=***"));
    }

    #[test]
    fn upsert_refreshes_the_assessment_header() {
        let store = FindingsStore::open_in_memory().unwrap();
        store.upsert_assessment(&ctx()).unwrap();
        let mut second = ctx();
        second.name = "Renamed".into();
        second.status = AssessmentStatus::Completed;
        store.upsert_assessment(&second).unwrap();
        let all = store.assessments(10).unwrap();
        assert_eq!(all.len(), 1, "the same id must not duplicate");
        assert_eq!(all[0].name, "Renamed");
        assert_eq!(all[0].status, "completed");
    }

    #[test]
    fn listings_are_scoped_and_bounded() {
        let store = FindingsStore::open_in_memory().unwrap();
        store.upsert_assessment(&ctx()).unwrap();
        store
            .insert_finding(
                "asm-1",
                &Finding::new("web", "headers", "t1", "one", RiskLevel::Low),
            )
            .unwrap();
        store
            .insert_finding(
                "asm-1",
                &Finding::new("web", "headers", "t2", "two", RiskLevel::High),
            )
            .unwrap();
        assert_eq!(store.count_findings(Some("asm-1")).unwrap(), 2);
        assert_eq!(store.count_findings(None).unwrap(), 2);
        assert_eq!(store.count_findings(Some("other")).unwrap(), 0);
        // Newest first.
        assert_eq!(store.findings(Some("asm-1"), 10).unwrap()[0].title, "two");
        assert!(store.findings(None, 0).unwrap().is_empty());
        assert!(store.findings(Some("nope"), 10).unwrap().is_empty());
        assert!(store.finding(999).unwrap().is_none());
        assert!(store.finding(1).unwrap().is_some());
    }

    #[test]
    fn empty_assessment_id_is_refused() {
        let store = FindingsStore::open_in_memory().unwrap();
        assert!(matches!(
            store.insert_finding(
                "  ",
                &Finding::new("web", "headers", "t", "x", RiskLevel::Low)
            ),
            Err(FindingStoreError::Invalid(_))
        ));
    }

    #[test]
    fn file_backed_store_round_trips_and_reports_path() {
        let dir = std::env::temp_dir().join(format!("nanomite-findings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(STORE_FILE);
        {
            let store = FindingsStore::open(&path).unwrap();
            assert_eq!(store.path(), Some(path.as_path()));
            store.record(&ctx(), &report()).unwrap();
        }
        let reopened = FindingsStore::open(&path).unwrap();
        assert_eq!(reopened.count_findings(Some("asm-1")).unwrap(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn markdown_render_and_export() {
        let store = FindingsStore::open_in_memory().unwrap();
        store.record(&ctx(), &report()).unwrap();
        let md = store.render_markdown(Some("asm-1")).unwrap();
        assert!(md.contains("# Findings — Lab"));
        assert!(md.contains("missing content-security-policy"));
        assert!(md.contains("confidence: confirmed"));

        let dir = std::env::temp_dir().join(format!("nanomite-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = store.export_markdown(&dir, Some("asm-1")).unwrap();
        assert!(path.starts_with(&dir));
        assert!(path.extension().is_some_and(|e| e == "md"));
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("missing content-security-policy"));
        let _ = std::fs::remove_dir_all(&dir);

        // All-assessments rendering works too.
        assert!(store
            .render_markdown(None)
            .unwrap()
            .contains("all assessments"));
    }

    #[test]
    fn status_can_be_triaged_and_round_trips() {
        let store = FindingsStore::open_in_memory().unwrap();
        let id = store
            .insert_finding(
                "asm-1",
                &Finding::new("web", "headers", "t", "x", RiskLevel::Low),
            )
            .unwrap();
        assert!(store.set_status(id, FindingStatus::Triaged).unwrap());
        assert_eq!(store.finding(id).unwrap().unwrap().status, "triaged");
        assert!(store.set_status(id, FindingStatus::Closed).unwrap());
        assert_eq!(store.finding(id).unwrap().unwrap().status, "closed");
        // Unknown id: no change, no error.
        assert!(!store.set_status(9999, FindingStatus::Closed).unwrap());
    }

    #[test]
    fn report_formats_parse_and_emit_valid_documents() {
        assert_eq!(ReportFormat::parse(""), Some(ReportFormat::Markdown));
        assert_eq!(ReportFormat::parse("MD"), Some(ReportFormat::Markdown));
        assert_eq!(ReportFormat::parse("json"), Some(ReportFormat::Json));
        assert_eq!(ReportFormat::parse("SARIF"), Some(ReportFormat::Sarif));
        assert_eq!(ReportFormat::parse("xml"), None);
        assert_eq!(ReportFormat::Sarif.extension(), "sarif");

        let store = FindingsStore::open_in_memory().unwrap();
        store.record(&ctx(), &report()).unwrap();

        let json: serde_json::Value =
            serde_json::from_str(&store.render_json(Some("asm-1")).unwrap()).unwrap();
        assert_eq!(json["finding_count"], 1);
        assert_eq!(json["findings"][0]["severity"], "medium");

        let sarif: serde_json::Value =
            serde_json::from_str(&store.render_sarif(Some("asm-1")).unwrap()).unwrap();
        assert_eq!(sarif["version"], "2.1.0");
        assert_eq!(
            sarif["runs"][0]["tool"]["driver"]["name"],
            "nanomite-hacker"
        );
        assert_eq!(sarif["runs"][0]["results"][0]["level"], "warning");
        assert_eq!(sarif["runs"][0]["results"][0]["ruleId"], "web/headers");
    }

    #[test]
    fn export_writes_each_format_with_its_extension() {
        let store = FindingsStore::open_in_memory().unwrap();
        store.record(&ctx(), &report()).unwrap();
        let dir = std::env::temp_dir().join(format!("nanomite-report-fmt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (format, ext) in [
            (ReportFormat::Markdown, "md"),
            (ReportFormat::Json, "json"),
            (ReportFormat::Sarif, "sarif"),
        ] {
            let path = store.export(&dir, Some("asm-1"), format).unwrap();
            assert_eq!(path.extension().unwrap(), ext);
            assert!(path.is_file());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scope_is_persisted_and_an_assessment_can_be_loaded_for_resume() {
        let store = FindingsStore::open_in_memory().unwrap();
        store.record(&ctx(), &report()).unwrap();

        let spec = store.load_assessment("asm-1").unwrap().expect("loaded");
        assert_eq!(spec.id, "asm-1");
        assert_eq!(spec.name, "Lab");
        assert_eq!(spec.authorization_ref.as_deref(), Some("LAB-1"));
        assert_eq!(spec.status, "authorized");
        assert_eq!(spec.cidrs, vec!["10.0.0.0/8".to_string()]);
        assert!(spec.domains.is_empty());
        assert!(spec.expiry > spec.start);

        assert!(store.load_assessment("nope").unwrap().is_none());

        // Scope is refreshed on the next save.
        let mut renamed = ctx();
        renamed.allowed_domains = vec!["example.com".into()];
        store.save_scope(&renamed).unwrap();
        let spec = store.load_assessment("asm-1").unwrap().unwrap();
        assert_eq!(spec.domains, vec!["example.com".to_string()]);
    }

    #[test]
    fn assessment_status_can_be_completed() {
        let store = FindingsStore::open_in_memory().unwrap();
        store.record(&ctx(), &report()).unwrap();
        assert!(store
            .set_assessment_status("asm-1", AssessmentStatus::Completed)
            .unwrap());
        assert_eq!(
            store.load_assessment("asm-1").unwrap().unwrap().status,
            "completed"
        );
        assert!(!store
            .set_assessment_status("missing", AssessmentStatus::Completed)
            .unwrap());
    }

    #[test]
    fn summary_counts_by_severity_and_status() {
        let store = FindingsStore::open_in_memory().unwrap();
        store.upsert_assessment(&ctx()).unwrap();
        for (sev, status) in [
            (RiskLevel::Critical, FindingStatus::Open),
            (RiskLevel::High, FindingStatus::Open),
            (RiskLevel::Low, FindingStatus::Closed),
        ] {
            let id = store
                .insert_finding("asm-1", &Finding::new("web", "headers", "t", "x", sev))
                .unwrap();
            store.set_status(id, status).unwrap();
        }
        let s = store.summary(Some("asm-1")).unwrap();
        assert_eq!(s.total, 3);
        // Descending severity: critical, high, low.
        assert_eq!(
            s.by_severity,
            vec![
                ("critical".to_string(), 1),
                ("high".to_string(), 1),
                ("low".to_string(), 1),
            ]
        );
        assert_eq!(
            s.by_status,
            vec![("closed".to_string(), 1), ("open".to_string(), 2)]
        );
        assert!(store.summary(Some("other")).unwrap().total == 0);
        assert_eq!(store.summary(None).unwrap().total, 3);
    }

    #[test]
    fn evidence_verification_detects_tampering() {
        let dir = std::env::temp_dir().join(format!("nanomite-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(STORE_FILE);
        let (finding_id, id);
        {
            let store = FindingsStore::open(&path).unwrap();
            // `record` writes the evidence row that `verify_evidence` checks.
            store.record(&ctx(), &report()).unwrap();
            finding_id = store.findings(Some("asm-1"), 10).unwrap()[0].id;
            id = store.evidence(finding_id).unwrap()[0].id;
            let checks = store.verify_evidence(finding_id).unwrap();
            assert_eq!(checks, vec![EvidenceCheck { id, ok: true }]);
        }
        // Simulate an external writer tampering with the stored excerpt.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute(
                "UPDATE evidence SET excerpt = 'tampered' WHERE id = ?1",
                rusqlite::params![id],
            )
            .unwrap();
        }
        let store = FindingsStore::open(&path).unwrap();
        let checks = store.verify_evidence(finding_id).unwrap();
        assert_eq!(checks, vec![EvidenceCheck { id, ok: false }]);
        // A finding with no evidence yields an empty check list.
        assert!(store.verify_evidence(999).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn findings_store_implements_the_backend_reads() {
        let store = FindingsStore::open_in_memory().unwrap();
        store.record(&ctx(), &report()).unwrap();
        let backend: &dyn FindingsBackend = &store;
        assert_eq!(backend.summary(Some("asm-1")).unwrap().total, 1);
        let dir = std::env::temp_dir().join(format!("nanomite-backend-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = backend
            .export(&dir, Some("asm-1"), ReportFormat::Json)
            .unwrap();
        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn null_sink_is_disabled_and_writes_nothing() {
        let sink = NullSink;
        assert!(!sink.enabled());
        assert_eq!(sink.record(&ctx(), &report()).unwrap(), 0);
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn findings_confidence_defaults_to_suspected_in_storage() {
        // A producer cannot smuggle "confirmed" through a version-only match.
        let store = FindingsStore::open_in_memory().unwrap();
        let f = Finding::new("vuln", "correlate", "widget 1.0", "CVE-X", RiskLevel::High);
        assert_eq!(f.confidence, Confidence::Suspected);
        store.insert_finding("asm-1", &f).unwrap();
        assert_eq!(store.findings(None, 1).unwrap()[0].confidence, "suspected");
    }
}
