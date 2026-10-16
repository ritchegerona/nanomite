//! Self-improvement engine: propose, review and apply sandboxed edits to the
//! agent's own config, prompt and source files.
//!
//! # Safety model
//!
//! This module is the *safe core* only. It stages proposals, renders diffs,
//! keeps an append-only audit trail, and can apply a proposal with backup,
//! rollback and regression checking. It never decides *when* applying is
//! allowed: that gate lives in the UI layer above, which must require an
//! explicit user action before calling [`ImproveEngine::apply`]. A `Pending`
//! proposal is inert — nothing in this module self-modifies on its own, and
//! the safety/permission layer (`src/memory/security.rs`, `src/memory/policy.rs`,
//! `src/main.rs`) can never be targeted at all.
//!
//! Everything is caller-supplied-path based: [`Roots`] anchors and the engine's
//! data directory come from the caller, so tests never touch real user dirs
//! and no environment variables are consulted.
//!
//! All I/O is synchronous; the UI should call these methods from
//! `spawn_blocking`.

mod audit;
mod diff;
mod proposal;
mod runner;

pub use audit::{AuditAction, AuditEvent, AuditLog};
pub use diff::unified_diff;
pub use proposal::{Actor, Proposal, Roots, Status, Target, MAX_PROPOSAL_BYTES};
pub use runner::{CargoTestRunner, FakeRunner, RegressionOutcome, RegressionRunner};

use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use audit::now_millis;

/// What went wrong during a self-improvement operation.
#[derive(Debug)]
pub enum ImproveError {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// The proposal itself is not acceptable (missing file, size cap, extension,
    /// unchanged content).
    Invalid(String),
    /// A safety rule refused the operation (denylist, path binding, confinement).
    Policy(String),
    /// No such proposal (or a malformed proposal id).
    NotFound(String),
    /// A state transition that is not legal (e.g. applying a rejected proposal).
    Conflict(String),
}

impl ImproveError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }

    pub fn policy(msg: impl Into<String>) -> Self {
        Self::Policy(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }
}

impl fmt::Display for ImproveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "improve i/o: {e}"),
            Self::Json(e) => write!(f, "improve json: {e}"),
            Self::Invalid(msg) => write!(f, "invalid proposal: {msg}"),
            Self::Policy(msg) => write!(f, "refused: {msg}"),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::Conflict(msg) => write!(f, "conflict: {msg}"),
        }
    }
}

impl std::error::Error for ImproveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ImproveError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for ImproveError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// Result of an [`ImproveEngine::apply`] call: the final status plus a
/// human-readable detail (empty on success, failure tail on rollback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub id: String,
    pub status: Status,
    pub detail: String,
}

/// The safe core of agent self-improvement. Proposals live in
/// `<data_dir>/proposals/` (`<id>.json`, `<id>.diff`, `<id>.bak`), the audit
/// trail in `<data_dir>/audit.jsonl`.
#[derive(Debug, Clone)]
pub struct ImproveEngine {
    data_dir: PathBuf,
}

enum Verdict {
    Keep,
    Rollback {
        detail: String,
        action: AuditAction,
        status: Status,
    },
}

impl ImproveEngine {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    fn proposals_dir(&self) -> PathBuf {
        self.data_dir.join("proposals")
    }

    fn audit_log(&self) -> AuditLog {
        AuditLog::new(self.data_dir.join("audit.jsonl"))
    }

    /// Filesystem path for a proposal id; refuses ids that could traverse out
    /// of the proposals directory.
    fn proposal_path(&self, id: &str) -> Result<PathBuf, ImproveError> {
        if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
            return Err(ImproveError::not_found(format!(
                "malformed proposal id `{id}`"
            )));
        }
        Ok(self.proposals_dir().join(format!("{id}.json")))
    }

    fn diff_path(&self, id: &str) -> PathBuf {
        self.proposals_dir().join(format!("{id}.diff"))
    }

    fn backup_path(&self, id: &str) -> PathBuf {
        self.proposals_dir().join(format!("{id}.bak"))
    }

    /// Timestamp+counter id, unique within the proposals directory.
    fn next_id(&self) -> String {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let millis = now_millis();
        loop {
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let id = format!("imp-{millis:013}-{seq:04}");
            if !self.proposals_dir().join(format!("{id}.json")).exists() {
                return id;
            }
        }
    }

    /// Stages a new proposal: validate → assign id → compute diff → persist
    /// JSON → audit `proposed` → return it. The proposal stays `Pending` and
    /// changes nothing on disk outside the engine's data dir.
    pub fn propose(
        &self,
        actor: Actor,
        target: Target,
        path: PathBuf,
        new_content: String,
        rationale: String,
        roots: &Roots,
    ) -> Result<Proposal, ImproveError> {
        // Source paths are anchored to the repo root up front so the stored
        // path is unambiguous regardless of the caller's working directory.
        let path = match target {
            Target::Source if path.is_relative() => roots.repo_root.join(&path),
            _ => path,
        };
        let mut proposal = Proposal {
            id: String::new(),
            created_at: audit::timestamp_utc(now_millis()),
            actor,
            target,
            path,
            new_content,
            rationale,
            status: Status::Pending,
        };
        proposal.validate(roots)?;

        proposal.id = self.next_id();
        let current = fs::read_to_string(&proposal.path).map_err(|e| {
            ImproveError::invalid(format!(
                "cannot read current `{}`: {e}",
                proposal.path.display()
            ))
        })?;
        let diff_text = unified_diff(
            &current,
            &proposal.new_content,
            &proposal.path.to_string_lossy(),
        );
        let (added, removed) = count_changes(&diff_text);

        fs::create_dir_all(self.proposals_dir())?;
        self.persist(&proposal)?;
        fs::write(self.diff_path(&proposal.id), diff_text)?;
        self.audit_log().append(&AuditEvent::new(
            &proposal.id,
            actor,
            AuditAction::Proposed,
            proposal.target,
            &proposal.path,
            format!("+{added} -{removed}"),
        ))?;
        Ok(proposal)
    }

    /// Loads one proposal by id.
    pub fn get(&self, id: &str) -> Result<Proposal, ImproveError> {
        let path = self.proposal_path(id)?;
        let text = fs::read_to_string(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ImproveError::not_found(format!("no proposal `{id}`")),
            _ => ImproveError::Io(e),
        })?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Lists proposals (oldest first), optionally filtered by status.
    /// Unreadable or corrupt entries are skipped.
    pub fn list(&self, status: Option<Status>) -> Result<Vec<Proposal>, ImproveError> {
        let mut proposals = Vec::new();
        let entries = match fs::read_dir(self.proposals_dir()) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(proposals),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(proposal) = serde_json::from_str::<Proposal>(&text) {
                    let wanted = match status {
                        Some(want) => proposal.status == want,
                        None => true,
                    };
                    if wanted {
                        proposals.push(proposal);
                    }
                }
            }
        }
        proposals.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(proposals)
    }

    /// Rejects a pending proposal. Only `Pending` proposals can be rejected;
    /// the decision is audited.
    pub fn reject(&self, id: &str, actor: Actor) -> Result<Proposal, ImproveError> {
        let mut proposal = self.get(id)?;
        if proposal.status != Status::Pending {
            return Err(ImproveError::conflict(format!(
                "proposal `{id}` is `{}`; only `pending` proposals can be rejected",
                proposal.status.as_str()
            )));
        }
        proposal.status = Status::Rejected;
        self.persist(&proposal)?;
        self.audit_log().append(&AuditEvent::new(
            id,
            actor,
            AuditAction::Rejected,
            proposal.target,
            &proposal.path,
            format!("rejected by {}", actor.as_str()),
        ))?;
        Ok(proposal)
    }

    /// Applies a pending proposal: back up the original bytes, write the new
    /// content, then verify —
    ///
    /// * `Target::Source`: the supplied [`RegressionRunner`] must pass
    ///   (`cargo test --lib` in production); on failure the backup is restored
    ///   byte-exact and the proposal ends `RolledBack`;
    /// * `Target::Config`: the new text is parsed through the public
    ///   `Config::load_from` API via a temp file; a config that does not
    ///   survive the round trip restores the backup and ends `Failed`;
    /// * `Target::Prompt`: the new content must be non-empty, else the backup
    ///   is restored and the proposal ends `Failed`.
    ///
    /// Success ends the proposal `Applied` (the `.bak` is kept as the rollback
    /// reference). Every state change is audited.
    ///
    /// # Safety
    ///
    /// The approval gate lives in the UI layer above: the caller must have
    /// obtained explicit user approval *before* invoking this method. Applying
    /// is only legal on `Pending` proposals — anything else is a `Conflict`.
    pub fn apply(
        &self,
        id: &str,
        runner: &dyn RegressionRunner,
        roots: &Roots,
    ) -> Result<ApplyOutcome, ImproveError> {
        let proposal = self.get(id)?;
        if proposal.status != Status::Pending {
            return Err(ImproveError::conflict(format!(
                "proposal `{id}` is `{}`; apply is only legal on `pending` proposals",
                proposal.status.as_str()
            )));
        }
        // Byte-exact snapshot taken before anything is written.
        let original = fs::read(&proposal.path)?;
        fs::create_dir_all(self.proposals_dir())?;
        fs::write(self.backup_path(id), &original)?;
        fs::write(&proposal.path, proposal.new_content.as_bytes())?;

        let verdict = match proposal.target {
            Target::Source => {
                let outcome = runner.run_regression(&roots.repo_root);
                if outcome.passed {
                    Verdict::Keep
                } else {
                    Verdict::Rollback {
                        detail: outcome.detail,
                        action: AuditAction::RolledBack,
                        status: Status::RolledBack,
                    }
                }
            }
            Target::Config => match self.validate_config_target(&proposal.new_content) {
                Ok(()) => Verdict::Keep,
                Err(detail) => Verdict::Rollback {
                    detail,
                    action: AuditAction::Failed,
                    status: Status::Failed,
                },
            },
            Target::Prompt => {
                if proposal.new_content.trim().is_empty() {
                    Verdict::Rollback {
                        detail: "prompt content must not be empty".to_string(),
                        action: AuditAction::Failed,
                        status: Status::Failed,
                    }
                } else {
                    Verdict::Keep
                }
            }
        };

        match verdict {
            Verdict::Keep => {
                let mut proposal = proposal;
                proposal.status = Status::Applied;
                self.persist(&proposal)?;
                self.audit_log().append(&AuditEvent::new(
                    id,
                    proposal.actor,
                    AuditAction::Applied,
                    proposal.target,
                    &proposal.path,
                    "applied after checks passed",
                ))?;
                Ok(ApplyOutcome {
                    id: id.to_string(),
                    status: Status::Applied,
                    detail: String::new(),
                })
            }
            Verdict::Rollback {
                detail,
                action,
                status,
            } => {
                // Restore the original bytes exactly; the written content is discarded.
                fs::write(&proposal.path, &original)?;
                let tail = failure_tail(&detail);
                let mut proposal = proposal;
                proposal.status = status;
                self.persist(&proposal)?;
                self.audit_log().append(&AuditEvent::new(
                    id,
                    proposal.actor,
                    action,
                    proposal.target,
                    &proposal.path,
                    tail.clone(),
                ))?;
                Ok(ApplyOutcome {
                    id: id.to_string(),
                    status,
                    detail: tail,
                })
            }
        }
    }

    /// Validates replacement config text by round-tripping it through the
    /// public `Config::load_from` parser. The text is staged in a temp file
    /// inside the engine's own data dir; because the app's parser silently
    /// skips broken lines, every recognized key must survive the parse — a
    /// silent drop means this engine may not install the file.
    fn validate_config_target(&self, text: &str) -> Result<(), String> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let tmp = self.data_dir.join(format!(
            ".config-check-{}-{}.toml",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<(), String> {
            fs::write(&tmp, text)
                .map_err(|e| format!("cannot stage config for validation: {e}"))?;
            let cfg = crate::config::Config::load_from(&tmp);
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((key, raw)) = line.split_once('=') else {
                    return Err(format!(
                        "invalid config line {line:?}: expected `key = \"value\"`"
                    ));
                };
                let value = unquote_stored_value(raw.trim());
                if value.is_empty() {
                    return Err(format!("empty value for config key `{}`", key.trim()));
                }
                check_config_key(&cfg, key.trim(), &value)?;
            }
            Ok(())
        })();
        let _ = fs::remove_file(&tmp);
        result
    }

    fn persist(&self, proposal: &Proposal) -> Result<(), ImproveError> {
        fs::create_dir_all(self.proposals_dir())?;
        let path = self.proposal_path(&proposal.id)?;
        fs::write(path, serde_json::to_string_pretty(proposal)?)?;
        Ok(())
    }
}

/// Round-trip check for one recognized config key against the parsed config.
/// Unknown keys are allowed (the app's parser ignores them too).
fn check_config_key(cfg: &crate::config::Config, key: &str, value: &str) -> Result<(), String> {
    let ok = match key {
        // The parser canonicalizes ids and labels; the round trip must keep them.
        "provider" => cfg.preset.eq_ignore_ascii_case(value),
        "base_url" => cfg.base_url == value.trim_end_matches('/'),
        "api_key" => cfg.api_key.as_deref() == Some(value),
        "model" => cfg.model == value,
        "reasoning" => cfg.reasoning.as_str().eq_ignore_ascii_case(value),
        "theme" => cfg.theme == value,
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "config key `{key}` was dropped or altered by the config parser; value {value:?} is not accepted"
        ))
    }
}

/// Mirrors `config::unquote`: strips surrounding quotes and resolves escapes
/// so the round-trip comparison sees the value the parser sees.
fn unquote_stored_value(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Counts added/removed lines in a rendered diff (file headers excluded).
fn count_changes(diff_text: &str) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for line in diff_text.lines() {
        if line.starts_with("+++ ") || line.starts_with("--- ") {
            continue; // file header, not a content line
        }
        if line.starts_with('+') {
            added += 1;
        } else if line.starts_with('-') {
            removed += 1;
        }
    }
    (added, removed)
}

/// Audit entries carry a compact failure tail, not the whole log dump.
fn failure_tail(detail: &str) -> String {
    let lines: Vec<&str> = detail.trim_end().lines().collect();
    let start = lines.len().saturating_sub(20);
    lines[start..].join("\n")
}

#[cfg(test)]
pub(crate) mod testing {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Unique temp directory, removed on drop. Every test in this module
    /// operates strictly inside one of these — never real user dirs.
    pub(crate) struct TempDir {
        pub(crate) path: PathBuf,
    }

    impl TempDir {
        pub(crate) fn new(tag: &str) -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("nanomite-improve-{tag}-{}-{n}", std::process::id()));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    pub(crate) fn temp_dir(tag: &str) -> TempDir {
        TempDir::new(tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::improve::testing::temp_dir;
    use std::path::Path;

    /// A fake repo root with one editable source file, roots and an engine
    /// whose data dir lives inside the temp root.
    fn setup(
        tag: &str,
    ) -> (
        crate::improve::testing::TempDir,
        Roots,
        ImproveEngine,
        PathBuf,
    ) {
        let temp = temp_dir(tag);
        let root = temp.path.clone();
        fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src").join("widget.rs");
        fs::write(&file, "pub fn widget() -> u32 { 1 }\n").unwrap();
        let roots = Roots {
            config_path: root.join("config.toml"),
            prompt_path: root.join("prompt.md"),
            repo_root: root.clone(),
        };
        let engine = ImproveEngine::new(root.join(".improve"));
        (temp, roots, engine, file)
    }

    fn audit_events(temp: &crate::improve::testing::TempDir) -> Vec<AuditEvent> {
        AuditLog::new(temp.path.join(".improve").join("audit.jsonl"))
            .read_all()
            .unwrap()
    }

    #[test]
    fn propose_apply_keeps_change_when_regression_passes() {
        let (temp, roots, engine, file) = setup("apply-pass");
        let proposal = engine
            .propose(
                Actor::Agent,
                Target::Source,
                file.clone(),
                "pub fn widget() -> u32 { 2 }\n".to_string(),
                "bump widget to 2".to_string(),
                &roots,
            )
            .unwrap();
        assert_eq!(proposal.status, Status::Pending);
        assert!(!proposal.id.is_empty());

        let runner = FakeRunner {
            passed: true,
            detail: "test result: ok. 42 passed".to_string(),
        };
        let outcome = engine.apply(&proposal.id, &runner, &roots).unwrap();
        assert_eq!(outcome.status, Status::Applied);

        // The file now holds the new content; the backup holds the original.
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "pub fn widget() -> u32 { 2 }\n"
        );
        let backup = temp
            .path
            .join(".improve")
            .join("proposals")
            .join(format!("{}.bak", proposal.id));
        assert_eq!(
            fs::read(&backup).unwrap(),
            b"pub fn widget() -> u32 { 1 }\n".to_vec()
        );

        // The review UI can read the diff artifact.
        assert!(temp
            .path
            .join(".improve")
            .join("proposals")
            .join(format!("{}.diff", proposal.id))
            .exists());

        // Audit trail: proposed, then applied.
        let events = audit_events(&temp);
        let actions: Vec<AuditAction> = events.iter().map(|e| e.action).collect();
        assert_eq!(actions, vec![AuditAction::Proposed, AuditAction::Applied]);
        assert_eq!(events[0].detail, "+1 -1");

        assert_eq!(engine.get(&proposal.id).unwrap().status, Status::Applied);
    }

    #[test]
    fn apply_restores_bytes_when_regression_fails() {
        let (temp, roots, engine, file) = setup("apply-fail");
        let original = b"pub fn widget() -> u32 { 1 }\n".to_vec();
        let proposal = engine
            .propose(
                Actor::Agent,
                Target::Source,
                file.clone(),
                "pub fn widget() -> u32 { 3 }\n".to_string(),
                "breaks the build".to_string(),
                &roots,
            )
            .unwrap();
        let runner = FakeRunner {
            passed: false,
            detail: "running 12 tests\ntest widget::tests::basic ... FAILED\n\nfailures: 1"
                .to_string(),
        };
        let outcome = engine.apply(&proposal.id, &runner, &roots).unwrap();
        assert_eq!(outcome.status, Status::RolledBack);
        assert!(
            outcome.detail.contains("FAILED"),
            "failure tail in detail: {}",
            outcome.detail
        );

        // Byte-identical restore.
        assert_eq!(fs::read(&file).unwrap(), original);
        assert_eq!(engine.get(&proposal.id).unwrap().status, Status::RolledBack);

        let events = audit_events(&temp);
        let actions: Vec<AuditAction> = events.iter().map(|e| e.action).collect();
        assert_eq!(
            actions,
            vec![AuditAction::Proposed, AuditAction::RolledBack]
        );
        assert!(events[1].detail.contains("FAILED"));
    }

    #[test]
    fn config_apply_with_invalid_toml_fails_and_restores() {
        let (temp, roots, engine, _file) = setup("config-invalid");
        let original = "provider = \"openai\"\nmodel = \"gpt-4o-mini\"\n";
        fs::write(&roots.config_path, original).unwrap();
        let proposal = engine
            .propose(
                Actor::User,
                Target::Config,
                roots.config_path.clone(),
                "provider = \"not-a-real-preset\"\nmodel = \"gpt-4o-mini\"\n".to_string(),
                "unknown provider".to_string(),
                &roots,
            )
            .unwrap();
        // The runner must not even be consulted for a Config target.
        let runner = FakeRunner {
            passed: false,
            detail: "should not run".to_string(),
        };
        let outcome = engine.apply(&proposal.id, &runner, &roots).unwrap();
        assert_eq!(outcome.status, Status::Failed);
        assert_eq!(fs::read_to_string(&roots.config_path).unwrap(), original);
        assert_eq!(engine.get(&proposal.id).unwrap().status, Status::Failed);
        let events = audit_events(&temp);
        let actions: Vec<AuditAction> = events.iter().map(|e| e.action).collect();
        assert_eq!(actions, vec![AuditAction::Proposed, AuditAction::Failed]);

        // Shape-level garbage is rejected by the same validation path at
        // apply time (propose only checks the structural rules).
        let garbage = engine
            .propose(
                Actor::User,
                Target::Config,
                roots.config_path.clone(),
                "this line has no equals sign\n".to_string(),
                "garbage".to_string(),
                &roots,
            )
            .unwrap();
        let outcome = engine.apply(&garbage.id, &runner, &roots).unwrap();
        assert_eq!(outcome.status, Status::Failed);
        assert_eq!(fs::read_to_string(&roots.config_path).unwrap(), original);
        // Two proposed/failed pairs audited in total.
        let events = audit_events(&temp);
        assert_eq!(events.len(), 4);
        assert_eq!(events[3].action, AuditAction::Failed);
        assert!(events[3].detail.contains("no equals sign"));
    }

    #[test]
    fn config_apply_with_valid_toml_succeeds() {
        let (_temp, roots, engine, _file) = setup("config-valid");
        fs::write(
            &roots.config_path,
            "provider = \"openai\"\nmodel = \"gpt-4o-mini\"\n",
        )
        .unwrap();
        let proposal = engine
            .propose(
                Actor::User,
                Target::Config,
                roots.config_path.clone(),
                "provider = \"openai\"\nmodel = \"gpt-4o-mini-2024\"\n".to_string(),
                "pin a newer model".to_string(),
                &roots,
            )
            .unwrap();
        let runner = FakeRunner {
            passed: false,
            detail: "never consulted for config targets".to_string(),
        };
        let outcome = engine.apply(&proposal.id, &runner, &roots).unwrap();
        assert_eq!(outcome.status, Status::Applied);
        assert_eq!(
            fs::read_to_string(&roots.config_path).unwrap(),
            "provider = \"openai\"\nmodel = \"gpt-4o-mini-2024\"\n"
        );
    }

    #[test]
    fn prompt_apply_rejects_empty_and_accepts_real_edits() {
        let (_temp, roots, engine, _file) = setup("prompt");
        fs::write(&roots.prompt_path, "You are nanomite.\n").unwrap();

        // Empty replacement → Failed + restored.
        let proposal = engine
            .propose(
                Actor::Agent,
                Target::Prompt,
                roots.prompt_path.clone(),
                String::new(),
                "wipe the prompt (bad idea)".to_string(),
                &roots,
            )
            .unwrap();
        let runner = FakeRunner {
            passed: true,
            detail: String::new(),
        };
        let outcome = engine.apply(&proposal.id, &runner, &roots).unwrap();
        assert_eq!(outcome.status, Status::Failed);
        assert_eq!(
            fs::read_to_string(&roots.prompt_path).unwrap(),
            "You are nanomite.\n"
        );

        // A real edit → Applied.
        let proposal = engine
            .propose(
                Actor::Agent,
                Target::Prompt,
                roots.prompt_path.clone(),
                "You are nanomite, a careful coding assistant.\n".to_string(),
                "clarify the persona".to_string(),
                &roots,
            )
            .unwrap();
        let outcome = engine.apply(&proposal.id, &runner, &roots).unwrap();
        assert_eq!(outcome.status, Status::Applied);
        assert_eq!(
            fs::read_to_string(&roots.prompt_path).unwrap(),
            "You are nanomite, a careful coding assistant.\n"
        );
    }

    #[test]
    fn denylist_proposals_never_persist() {
        let (temp, roots, engine, _file) = setup("denylist");
        fs::create_dir_all(roots.repo_root.join("src/memory")).unwrap();
        let guarded = roots.repo_root.join("src/memory/security.rs");
        fs::write(&guarded, "pub fn never() {}\n").unwrap();

        let err = engine
            .propose(
                Actor::Agent,
                Target::Source,
                guarded,
                "pub fn hacked() {}\n".to_string(),
                "nope".to_string(),
                &roots,
            )
            .unwrap_err();
        assert!(matches!(err, ImproveError::Policy(_)), "{err:?}");

        // Nothing was staged and nothing was audited.
        assert!(engine.list(None).unwrap().is_empty());
        assert!(audit_events(&temp).is_empty());
    }

    #[test]
    fn reject_flow_and_apply_conflict() {
        let (temp, roots, engine, file) = setup("reject");
        let proposal = engine
            .propose(
                Actor::Agent,
                Target::Source,
                file,
                "pub fn widget() -> u32 { 9 }\n".to_string(),
                "rejected idea".to_string(),
                &roots,
            )
            .unwrap();

        let rejected = engine.reject(&proposal.id, Actor::User).unwrap();
        assert_eq!(rejected.status, Status::Rejected);
        assert_eq!(engine.get(&proposal.id).unwrap().status, Status::Rejected);

        // Applying a rejected proposal is a conflict.
        let runner = FakeRunner {
            passed: true,
            detail: String::new(),
        };
        let err = engine.apply(&proposal.id, &runner, &roots).unwrap_err();
        assert!(matches!(err, ImproveError::Conflict(_)), "{err:?}");

        // Rejecting twice is a conflict too.
        let err = engine.reject(&proposal.id, Actor::User).unwrap_err();
        assert!(matches!(err, ImproveError::Conflict(_)), "{err:?}");

        let events = audit_events(&temp);
        let actions: Vec<AuditAction> = events.iter().map(|e| e.action).collect();
        assert_eq!(actions, vec![AuditAction::Proposed, AuditAction::Rejected]);
        assert_eq!(events[1].actor, Actor::User);
        assert_eq!(events[1].detail, "rejected by user");
    }

    #[test]
    fn list_and_get_roundtrip_through_json() {
        let (_temp, roots, engine, file) = setup("list");
        let other = roots.repo_root.join("src").join("other.rs");
        fs::write(&other, "pub fn other() {}\n").unwrap();

        let first = engine
            .propose(
                Actor::Agent,
                Target::Source,
                file,
                "pub fn a() {}\n".to_string(),
                "one".to_string(),
                &roots,
            )
            .unwrap();
        let second = engine
            .propose(
                Actor::User,
                Target::Source,
                other,
                "pub fn b() {}\n".to_string(),
                "two".to_string(),
                &roots,
            )
            .unwrap();

        assert_eq!(engine.list(None).unwrap().len(), 2);
        assert_eq!(engine.list(Some(Status::Pending)).unwrap().len(), 2);
        assert_eq!(engine.list(Some(Status::Rejected)).unwrap().len(), 0);

        engine.reject(&first.id, Actor::User).unwrap();
        assert_eq!(engine.list(Some(Status::Rejected)).unwrap().len(), 1);
        assert_eq!(engine.list(Some(Status::Pending)).unwrap().len(), 1);
        assert_eq!(engine.list(Some(Status::Rejected)).unwrap()[0].id, first.id);

        // Full-field round trip through JSON.
        let loaded = engine.get(&second.id).unwrap();
        assert_eq!(loaded.actor, Actor::User);
        assert_eq!(loaded.target, Target::Source);
        assert_eq!(loaded.rationale, "two");
        assert_eq!(loaded.new_content, "pub fn b() {}\n");
        assert_eq!(loaded.status, Status::Pending);

        // Unknown and malformed ids.
        assert!(matches!(
            engine.get("imp-does-not-exist"),
            Err(ImproveError::NotFound(_))
        ));
        assert!(matches!(
            engine.get("../escape"),
            Err(ImproveError::NotFound(_))
        ));
        assert!(matches!(
            engine.apply(
                "imp-does-not-exist",
                &FakeRunner {
                    passed: true,
                    detail: String::new()
                },
                &roots
            ),
            Err(ImproveError::NotFound(_))
        ));
    }

    #[test]
    fn apply_twice_conflicts() {
        let (_temp, roots, engine, file) = setup("apply-twice");
        let proposal = engine
            .propose(
                Actor::Agent,
                Target::Source,
                file,
                "pub fn c() {}\n".to_string(),
                "once".to_string(),
                &roots,
            )
            .unwrap();
        let runner = FakeRunner {
            passed: true,
            detail: String::new(),
        };
        assert_eq!(
            engine.apply(&proposal.id, &runner, &roots).unwrap().status,
            Status::Applied
        );
        let err = engine.apply(&proposal.id, &runner, &roots).unwrap_err();
        assert!(matches!(err, ImproveError::Conflict(_)), "{err:?}");
    }

    #[test]
    fn relative_source_paths_are_anchored_to_the_repo_root() {
        let (temp, roots, engine, file) = setup("relative");
        let proposal = engine
            .propose(
                Actor::Agent,
                Target::Source,
                Path::new("src/widget.rs").to_path_buf(),
                "pub fn widget() -> u32 { 7 }\n".to_string(),
                "relative path proposal".to_string(),
                &roots,
            )
            .unwrap();
        assert_eq!(proposal.path, file);
        let runner = FakeRunner {
            passed: true,
            detail: String::new(),
        };
        assert_eq!(
            engine.apply(&proposal.id, &runner, &roots).unwrap().status,
            Status::Applied
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "pub fn widget() -> u32 { 7 }\n"
        );
        // And nothing was written next to the cwd.
        assert!(
            !std::env::current_dir()
                .unwrap()
                .join("src/widget.rs")
                .exists()
                || file.is_absolute()
        );
        let _ = temp;
    }
}
