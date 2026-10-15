//! Persistent, model-independent memory for nanomite.
//!
//! # Layout
//!
//! ```text
//! $XDG_CONFIG_HOME/nanomite/memory/     (override: NANOMITE_MEMORY_DIR)
//!   MEMORY.md            core facts
//!   USER.md              user-approved preferences
//!   projects/<slug>.md   per-project architecture, status, constraints
//!   decisions/<slug>.md  decisions with rationale
//!
//! $XDG_DATA_HOME/nanomite/memory/        (override: NANOMITE_MEMORY_DB)
//!   nanomite.db          structured records, metadata, search indexes
//!   backups/             `backup()` output
//! ```
//!
//! The config root is used for markdown because that is where nanomite already
//! keeps hand-editable state (`config.toml`), and the data root for the database
//! because that is where it keeps generated state (`sessions/`).
//!
//! # Trust
//!
//! Retrieved memory is *untrusted data*. It is never treated as an instruction,
//! never widens tool permissions, and credentials are refused on the way in.
//! Every operation - tool call, API call or agent-loop injection - passes through
//! [`policy::MemoryPolicy`], which is this subsystem's authorization boundary.

pub mod context;
pub mod markdown;
pub mod model;
pub mod policy;
pub mod security;
pub mod store;

use markdown::KnowledgeStore;
use model::*;
use policy::MemoryPolicy;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use store::MemoryStore;

pub use context::{CaptureReport, MemoryContext, TurnObservation, TurnOutcome, CONTEXT_OPEN};
pub use store::{ImportMode, ImportReport};

/// Resolved storage locations.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryPaths {
    /// Markdown root.
    pub root: PathBuf,
    /// SQLite file.
    pub database: PathBuf,
    /// Where [`MemoryEngine::backup`] writes.
    pub backups: PathBuf,
}

impl MemoryPaths {
    /// Platform-resolved defaults, honouring the `NANOMITE_MEMORY_*` overrides.
    ///
    /// Nothing here hardcodes a home directory: `dirs` does the resolution, which
    /// is the same convention `config.rs` and `session.rs` already use.
    pub fn resolve() -> Self {
        let root = std::env::var("NANOMITE_MEMORY_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("nanomite")
                    .join("memory")
            });

        let data_root = std::env::var("NANOMITE_MEMORY_DB")
            .ok()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| {
                dirs::data_dir()
                    .or_else(dirs::home_dir)
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("nanomite")
                    .join("memory")
                    .join("nanomite.db")
            });
        let (database, backups) = if data_root.is_dir() || data_root.extension().is_some() {
            let parent = data_root
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            (data_root.clone(), parent.join("backups"))
        } else {
            (data_root.join("nanomite.db"), data_root.join("backups"))
        };
        Self {
            root,
            database,
            backups,
        }
    }

    /// Paths rooted at `base`, used by tests so the real memory directory is
    /// never touched.
    pub fn under(base: impl AsRef<Path>) -> Self {
        let base = base.as_ref().to_path_buf();
        Self {
            root: base.join("memory"),
            database: base.join("data").join("nanomite.db"),
            backups: base.join("data").join("backups"),
        }
    }
}

/// Bounded markdown scan limits shared with the retrieval path.
const MARKDOWN_SCAN_FILES: usize = 50;
const MARKDOWN_SCAN_RESULTS: usize = 10;

/// Everything the agent runtime, the tools and the UI need, in one handle.
pub struct MemoryEngine {
    store: Arc<MemoryStore>,
    knowledge: KnowledgeStore,
    paths: MemoryPaths,
    policy: MemoryPolicy,
}

/// A markdown hit, returned next to structured search results.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MarkdownHit {
    pub path: String,
    /// `mem_*` id when the line sits inside a nanomite-managed block.
    pub block_id: String,
    pub line: String,
}

impl MemoryEngine {
    /// Opens the engine at the platform default location.
    pub fn open() -> Result<Self, MemoryError> {
        Self::open_at(MemoryPaths::resolve(), MemoryPolicy::from_env(true))
    }

    /// Opens the engine at explicit paths. Never overwrites existing files.
    pub fn open_at(paths: MemoryPaths, policy: MemoryPolicy) -> Result<Self, MemoryError> {
        let knowledge = KnowledgeStore::open(&paths.root, policy.mirror_markdown)?;
        let store = MemoryStore::open(&paths.database, policy.clone())?;
        let engine = Self {
            store: Arc::new(store),
            knowledge,
            paths,
            policy,
        };
        // First run (or an externally edited tree) gets its index refreshed.
        engine.reindex_if_stale()?;
        Ok(engine)
    }

    /// In-memory engine with no filesystem footprint. Used by tests and dry-run flows.
    /// The markdown half still needs a directory; it gets a private one under the
    /// system temp path and is never the user's real memory tree.
    pub fn open_in_memory(policy: MemoryPolicy) -> Result<Self, MemoryError> {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("nanomite-inmem-{}-{}", std::process::id(), unique));
        let paths = MemoryPaths::under(&base);
        let _ = std::fs::remove_dir_all(&paths.root);
        let knowledge = KnowledgeStore::open(&paths.root, policy.mirror_markdown)?;
        let store = MemoryStore::open_in_memory(policy.clone())?;
        let engine = Self {
            store: Arc::new(store),
            knowledge,
            paths,
            policy,
        };
        let _ = engine.reindex_if_stale();
        Ok(engine)
    }

    /// In-memory engine with no filesystem footprint. Used by tests and dry-run flows.
    pub fn ephemeral(policy: MemoryPolicy) -> Result<Self, MemoryError> {
        let base = std::env::temp_dir().join(format!("nanomite-ephemeral-{}", std::process::id()));
        let paths = MemoryPaths::under(&base);
        let _ = std::fs::remove_dir_all(&paths.root);
        let knowledge = KnowledgeStore::open(&paths.root, policy.mirror_markdown)?;
        let store = MemoryStore::open_in_memory(policy.clone())?;
        let engine = Self {
            store: Arc::new(store),
            knowledge,
            paths,
            policy,
        };
        let _ = engine.reindex_if_stale();
        Ok(engine)
    }

    pub fn store(&self) -> &Arc<MemoryStore> {
        &self.store
    }

    pub fn knowledge(&self) -> &KnowledgeStore {
        &self.knowledge
    }

    pub fn paths(&self) -> &MemoryPaths {
        &self.paths
    }

    pub fn policy(&self) -> &MemoryPolicy {
        &self.policy
    }

    // -- writes -------------------------------------------------------------

    /// Stores a record and mirrors it into its markdown document when the
    /// category is mirrored and the record asks for it.
    pub fn save(&self, draft: &NewMemory) -> Result<SaveOutcome, MemoryError> {
        let valid = draft.validate(now())?;
        let mut outcome = self.store.save(draft)?;
        if outcome.duplicate || !valid.mirror_markdown {
            return Ok(outcome);
        }
        if let Some(relative) = self
            .knowledge
            .document_for(valid.category, valid.project.as_deref())
        {
            match self.knowledge.upsert_block(&relative, &outcome.record) {
                Ok(()) => {
                    self.refresh_index_for(&relative)?;
                    outcome.markdown = Some(relative);
                }
                Err(e) => {
                    // The record is safe in SQLite; a markdown problem must not
                    // lose it. Report the record and remember the failure.
                    outcome.markdown = Some(format!("{relative} (not written: {e})"));
                }
            }
        }
        Ok(outcome)
    }

    /// Updates a record and re-renders exactly its markdown block.
    pub fn update(&self, id: &str, patch: &MemoryPatch) -> Result<MemoryRecord, MemoryError> {
        let updated = self.store.update(id, patch)?;
        if let Some(relative) = self
            .knowledge
            .document_for(updated.category, updated.project.as_deref())
        {
            let _ = self.knowledge.upsert_block(&relative, &updated);
            let _ = self.refresh_index_for(&relative);
        }
        Ok(updated)
    }

    /// Deletes a record everywhere: SQLite row, FTS entry and markdown block.
    ///
    /// The markdown document, its heading and any hand-written prose are left in
    /// place. Returns the record that was removed.
    pub fn delete(&self, id: &str) -> Result<MemoryRecord, MemoryError> {
        let record = self.store.delete(id)?;
        if let Some(relative) = self
            .knowledge
            .document_for(record.category, record.project.as_deref())
        {
            if self.knowledge.remove_block(&relative, id)? {
                let _ = self.refresh_index_for(&relative);
            }
        }
        Ok(record)
    }

    pub fn delete_many(
        &self,
        ids: &[String],
        confirmed: bool,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let removed = self.store.delete_many(ids, confirmed)?;
        for record in &removed {
            if let Some(relative) = self
                .knowledge
                .document_for(record.category, record.project.as_deref())
            {
                if self
                    .knowledge
                    .remove_block(&relative, &record.id)
                    .unwrap_or(false)
                {
                    let _ = self.refresh_index_for(&relative);
                }
            }
        }
        Ok(removed)
    }

    // -- reads --------------------------------------------------------------

    pub fn get(&self, id: &str) -> Result<MemoryRecord, MemoryError> {
        self.store.get(id)
    }

    /// Structured keyword search, optionally widened with a bounded markdown scan.
    pub fn search(
        &self,
        query: &SearchQuery,
        include_markdown: bool,
    ) -> Result<(MemoryList, Vec<MarkdownHit>), MemoryError> {
        let list = self.store.search(query)?;
        let mut markdown = Vec::new();
        // The markdown scan is a *fallback*: it only runs when the structured
        // index came up empty, so mirrored records are never reported twice.
        if include_markdown && list.total == 0 {
            if let Some(text) = query.text.as_deref().filter(|t| !t.trim().is_empty()) {
                markdown = self
                    .knowledge
                    .search_text(text, MARKDOWN_SCAN_RESULTS, MARKDOWN_SCAN_FILES)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(path, block_id, line)| MarkdownHit {
                        path,
                        block_id,
                        line,
                    })
                    .collect();
            }
        }
        Ok((list, markdown))
    }

    pub fn list(&self, query: &ListQuery) -> Result<MemoryList, MemoryError> {
        self.store.list(query)
    }

    pub fn recent(&self, limit: usize) -> Result<MemoryList, MemoryError> {
        self.store.recent(limit)
    }

    pub fn project_memory(&self, project: &str, limit: usize) -> Result<MemoryList, MemoryError> {
        self.store.project_memory(project, limit)
    }

    pub fn stats(&self) -> Result<MemoryStats, MemoryError> {
        self.store.stats()
    }

    // -- maintenance --------------------------------------------------------

    pub fn consolidate(&self) -> Result<ConsolidationReport, MemoryError> {
        self.store.consolidate()
    }

    pub fn purge_expired(&self) -> Result<Vec<MemoryRecord>, MemoryError> {
        let expired = self.store.purge_expired()?;
        for record in &expired {
            if let Some(relative) = self
                .knowledge
                .document_for(record.category, record.project.as_deref())
            {
                let _ = self.knowledge.remove_block(&relative, &record.id);
            }
        }
        Ok(expired)
    }

    /// Re-reads every markdown document and refreshes the SQLite index.
    ///
    /// Only documents whose digest actually moved are reported as changed, so a
    /// routine reindex stays quiet.
    pub fn reindex(&self) -> Result<markdown::IndexReport, MemoryError> {
        let (rows, mut report) = self.knowledge.index_documents()?;
        let previous = self.store.markdown_index()?;
        let known: Vec<String> = previous.iter().map(|row| row.path.clone()).collect();

        for row in rows {
            let moved = previous
                .iter()
                .find(|old| old.path == row.path)
                .is_none_or(|old| old.content_hash != row.content_hash);
            if moved {
                report.changed.push(row.path.clone());
            }
            self.store.upsert_markdown_index(&row)?;
        }
        let live: Vec<String> = report
            .changed
            .iter()
            .cloned()
            .chain(previous.iter().map(|row| row.path.clone()))
            .collect();
        for path in known {
            if !live.contains(&path) {
                self.store.remove_markdown_index(&path)?;
                report.removed.push(path);
            }
        }
        Ok(report)
    }

    /// Cheap staleness probe: reindexes only when a document changed on disk.
    pub fn reindex_if_stale(&self) -> Result<usize, MemoryError> {
        let indexed = self.store.markdown_index()?;
        let stale = self.knowledge.stale_documents(&indexed)?;
        if stale.is_empty() {
            return Ok(0);
        }
        self.reindex()?;
        Ok(stale.len())
    }

    /// Rebuilds the FTS index from the canonical table.
    pub fn rebuild_search_index(&self) -> Result<usize, MemoryError> {
        self.store.rebuild_fts()
    }

    /// `PRAGMA integrity_check` plus counts.
    pub fn verify(&self) -> Result<MemoryStats, MemoryError> {
        self.store.integrity_check()
    }

    // -- transfer -----------------------------------------------------------

    /// Writes a timestamped backup into the managed backups directory.
    pub fn backup(&self) -> Result<PathBuf, MemoryError> {
        security::ensure_dir(&self.paths.backups)?;
        let path = self
            .paths
            .backups
            .join(format!("nanomite-memory-{}.db", now()));
        self.store.backup_to(&path)?;
        Ok(path)
    }

    pub fn backup_to(&self, destination: &Path) -> Result<(), MemoryError> {
        self.store.backup_to(destination)
    }

    /// Restores from a backup file after validating it. A bad file is refused and
    /// the live memory is untouched.
    pub fn restore(&self, source: &Path) -> Result<usize, MemoryError> {
        let count = self.store.restore_from(source)?;
        self.reindex()?;
        Ok(count)
    }

    pub fn export_bundle(&self) -> Result<ExportBundle, MemoryError> {
        self.store.export()
    }

    /// Serializes every record to `destination` as a portable JSON bundle.
    pub fn export_to(&self, destination: &Path) -> Result<ExportBundle, MemoryError> {
        let bundle = self.store.export()?;
        let bytes = serde_json::to_vec_pretty(&bundle)
            .map_err(|e| MemoryError::Database(format!("serialize bundle: {e}")))?;
        if let Some(parent) = destination.parent() {
            security::ensure_dir(parent)?;
        }
        security::write_atomic(destination, &bytes)?;
        Ok(bundle)
    }

    /// Reads and validates a JSON bundle, then imports it.
    ///
    /// Malformed input fails before a single row is written.
    pub fn import_from(
        &self,
        source: &Path,
        mode: ImportMode,
        confirmed: bool,
    ) -> Result<ImportReport, MemoryError> {
        let raw = std::fs::read(source)
            .map_err(|e| MemoryError::io(&format!("read {}", source.display()), e))?;
        if raw.len() > 64 * 1024 * 1024 {
            return Err(MemoryError::invalid(
                "import file is larger than 64 MiB; refusing to parse",
            ));
        }
        let bundle: ExportBundle = serde_json::from_slice(&raw).map_err(|e| {
            MemoryError::invalid(format!(
                "{} is not a nanomite memory bundle: {e}",
                source.display()
            ))
        })?;
        let report = self.store.import(&bundle, mode, confirmed)?;
        self.reindex_if_stale()?;
        Ok(report)
    }

    // -- retrieval ----------------------------------------------------------

    /// Bounded memory context for one model request. Never fails: a retrieval
    /// problem yields an empty context and a note, so memory can never break a turn.
    pub fn context(&self, input: &str, project: Option<&str>) -> MemoryContext {
        context::retrieve(self.store.as_ref(), Some(&self.knowledge), input, project)
    }

    /// Applies the post-task persistence policy. Never fails the caller.
    pub fn observe(&self, observation: &TurnObservation) -> CaptureReport {
        context::capture(self.store.as_ref(), &self.policy, observation)
    }

    fn refresh_index_for(&self, relative: &str) -> Result<(), MemoryError> {
        let path = self.knowledge.resolve(relative)?;
        let Ok(meta) = std::fs::metadata(&path) else {
            self.store.remove_markdown_index(relative)?;
            return Ok(());
        };
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        let blocks = markdown::parse_blocks(&body);
        self.store.upsert_markdown_index(&MarkdownIndexRow {
            path: relative.to_string(),
            title: relative
                .trim_end_matches(".md")
                .rsplit('/')
                .next()
                .unwrap_or(relative)
                .to_string(),
            content_hash: security::content_hash(&body),
            mtime: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            size: meta.len(),
            indexed_at: now(),
            block_ids: blocks.into_iter().map(|b| b.id).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("nanomite-engine-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }

        fn engine(&self) -> MemoryEngine {
            MemoryEngine::open_at(
                MemoryPaths::under(&self.0),
                MemoryPolicy {
                    mirror_markdown: true,
                    ..MemoryPolicy::default()
                },
            )
            .expect("open engine")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn first_run_creates_both_stores_without_touching_real_data() {
        let scratch = Scratch::new("firstrun");
        let engine = scratch.engine();
        assert!(scratch.0.join("memory").join("MEMORY.md").exists());
        assert!(scratch.0.join("memory").join("USER.md").exists());
        assert!(scratch.0.join("memory").join("projects").is_dir());
        assert!(scratch.0.join("data").join("nanomite.db").exists());
        assert_eq!(engine.stats().unwrap().total, 0);
        assert!(engine.stats().unwrap().markdown_documents >= 2);
    }

    #[test]
    fn paths_respect_the_platform_convention() {
        let paths = MemoryPaths::resolve();
        assert!(paths.root.ends_with("memory"));
        assert!(paths.database.ends_with("nanomite.db"));
        assert!(paths.backups.ends_with("backups"));
    }

    #[test]
    fn save_mirrors_into_markdown_and_get_round_trips() {
        let scratch = Scratch::new("mirror");
        let engine = scratch.engine();
        let mut draft = NewMemory::new(MemoryCategory::Core, "cargo test must pass before pushing");
        draft.tags = vec!["workflow".into()];
        let outcome = engine.save(&draft).expect("save");
        assert_eq!(outcome.markdown.as_deref(), Some("MEMORY.md"));

        let body = std::fs::read_to_string(scratch.0.join("memory").join("MEMORY.md")).unwrap();
        assert!(body.contains("cargo test must pass before pushing"));
        assert!(body.contains(outcome.record.id.as_str()));

        let fetched = engine.get(&outcome.record.id).expect("get");
        assert_eq!(fetched.content, draft.content);
    }

    #[test]
    fn project_memories_land_in_their_own_document() {
        let scratch = Scratch::new("project");
        let engine = scratch.engine();
        let mut draft = NewMemory::new(MemoryCategory::Decision, "store memory in sqlite");
        draft.project = Some("nanomite".into());
        let outcome = engine.save(&draft).expect("save");
        assert_eq!(outcome.markdown.as_deref(), Some("decisions/nanomite.md"));
        assert!(scratch
            .0
            .join("memory")
            .join("decisions")
            .join("nanomite.md")
            .exists());
    }

    #[test]
    fn markdown_edits_survive_working_memory_deletes() {
        let scratch = Scratch::new("preserve");
        let engine = scratch.engine();
        engine
            .save(&NewMemory::new(MemoryCategory::Core, "temporary note"))
            .unwrap();
        let doc = scratch.0.join("memory").join("MEMORY.md");
        std::fs::write(&doc, "# Core memory\n\n## My own notes\n\nthis is mine\n").unwrap();

        let temporary = engine
            .search(&SearchQuery::text("temporary"), false)
            .unwrap()
            .0
            .items[0]
            .record
            .id
            .clone();
        engine.delete(&temporary).expect("delete");

        let body = std::fs::read_to_string(&doc).unwrap();
        assert!(
            body.contains("this is mine"),
            "user prose must survive:\n{body}"
        );
        assert!(!body.contains("temporary note"));
        assert!(body.contains("## My own notes"));
    }

    #[test]
    fn memories_survive_a_restart() {
        let scratch = Scratch::new("restart");
        let id = {
            let engine = scratch.engine();
            engine
                .save(&NewMemory::new(
                    MemoryCategory::Core,
                    "durable across restarts",
                ))
                .unwrap()
                .record
                .id
        };
        let reopened = scratch.engine();
        assert_eq!(
            reopened.get(&id).unwrap().content,
            "durable across restarts"
        );
    }

    #[test]
    fn reindex_detects_external_edits_without_reading_on_every_call() {
        let scratch = Scratch::new("reindex");
        let engine = scratch.engine();
        assert_eq!(engine.reindex_if_stale().unwrap(), 0, "fresh tree");

        let doc = scratch.0.join("memory").join("MEMORY.md");
        std::thread::sleep(std::time::Duration::from_millis(1_100));
        std::fs::write(&doc, "# Core memory\n\n## Facts\n\n- hand written line\n").unwrap();
        assert_eq!(engine.reindex_if_stale().unwrap(), 1);
        assert_eq!(engine.reindex_if_stale().unwrap(), 0);
    }

    #[test]
    fn export_import_backup_restore_round_trip() {
        let scratch = Scratch::new("transfer");
        let engine = scratch.engine();
        engine
            .save(&NewMemory::new(MemoryCategory::Core, "transfer me"))
            .unwrap();

        let bundle_path = scratch.0.join("bundle.json");
        let bundle = engine.export_to(&bundle_path).expect("export");
        assert_eq!(bundle.memories.len(), 1);

        let backup = engine.backup().expect("backup");
        assert!(backup.exists());

        engine
            .save(&NewMemory::new(MemoryCategory::Core, "after the backup"))
            .unwrap();
        assert_eq!(engine.stats().unwrap().total, 2);

        let report = engine
            .import_from(&bundle_path, ImportMode::Insert, false)
            .expect("import");
        assert!(report.skipped >= 1, "{report:?}");
        assert_eq!(engine.stats().unwrap().total, 2);

        engine.restore(&backup).expect("restore");
        assert_eq!(engine.stats().unwrap().total, 1);
    }

    #[test]
    fn bad_import_file_is_rejected_without_side_effects() {
        let scratch = Scratch::new("badimport");
        let engine = scratch.engine();
        engine
            .save(&NewMemory::new(MemoryCategory::Core, "untouched"))
            .unwrap();

        let junk = scratch.0.join("junk.json");
        std::fs::write(&junk, b"{ this is not json").unwrap();
        assert!(engine
            .import_from(&junk, ImportMode::Replace, true)
            .is_err());

        let empty = scratch.0.join("empty.json");
        std::fs::write(&empty, b"{}").unwrap();
        assert!(engine
            .import_from(&empty, ImportMode::Replace, true)
            .is_err());

        assert_eq!(engine.stats().unwrap().total, 1);
    }

    #[test]
    fn secrets_never_reach_markdown_or_sqlite() {
        let scratch = Scratch::new("secrets");
        let engine = scratch.engine();
        let draft = NewMemory::new(
            MemoryCategory::Core,
            "the deploy key is AKIAIOSFODNN7EXAMPLE and must not be stored",
        );
        assert!(engine.save(&draft).is_err());
        assert_eq!(engine.stats().unwrap().total, 0);
        let body = std::fs::read_to_string(scratch.0.join("memory").join("MEMORY.md")).unwrap();
        assert!(!body.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn context_is_bounded_and_provider_independent() {
        let scratch = Scratch::new("context");
        let engine = scratch.engine();
        engine
            .save(&NewMemory::new(
                MemoryCategory::Decision,
                "nanomite uses rusqlite, never sqlx",
            ))
            .unwrap();
        for i in 0..30 {
            engine
                .save(&NewMemory::new(
                    MemoryCategory::Core,
                    format!("fact {i} about the parser"),
                ))
                .unwrap();
        }

        // Nothing about the provider, model or endpoint leaks into retrieval.
        let context = engine.context("which database does nanomite use", Some("nanomite"));
        assert!(context.text.contains("rusqlite"));
        assert!(context.records.len() <= engine.policy().max_inject_records);
        assert!(!context.text.to_lowercase().contains("openai"));
        assert!(!context.text.to_lowercase().contains("api_key"));
    }

    #[test]
    fn observe_never_fails_the_turn_and_respects_read_only() {
        let scratch = Scratch::new("observe");
        let read_only = MemoryEngine::open_at(
            MemoryPaths::under(&scratch.0),
            MemoryPolicy {
                writable: false,
                ..MemoryPolicy::default()
            },
        )
        .expect("open");
        let report = read_only.observe(&TurnObservation {
            user_input: "Remember that x is y".into(),
            outcome: TurnOutcome::Completed,
            tools: vec!["run_command".into()],
            workspace: "/tmp".into(),
            project: None,
            provider: "p".into(),
            model: "m".into(),
            session: "s".into(),
        });
        assert!(report.skipped.is_some());
        assert_eq!(read_only.stats().unwrap().total, 0);
    }

    #[test]
    fn policy_gates_writes_across_the_engine() {
        let scratch = Scratch::new("policy");
        let engine = MemoryEngine::open_at(
            MemoryPaths::under(&scratch.0),
            MemoryPolicy {
                writable: false,
                ..MemoryPolicy::default()
            },
        )
        .expect("open");
        assert!(engine
            .save(&NewMemory::new(MemoryCategory::Core, "nope"))
            .is_err());
        assert!(engine.delete_many(&["mem_x".to_string()], true).is_err());
    }

    #[test]
    fn markdown_search_is_bounded_and_optional() {
        let scratch = Scratch::new("mdsearch");
        let engine = scratch.engine();
        engine
            .save(&NewMemory::new(MemoryCategory::Core, "alpha document note"))
            .unwrap();
        let (records, markdown) = engine
            .search(&SearchQuery::text("alpha"), true)
            .expect("search");
        assert_eq!(records.total, 1);
        assert!(markdown.is_empty(), "already covered by the record index");

        std::fs::write(
            scratch.0.join("memory").join("MEMORY.md"),
            "# Core memory\n\nhand written: alpha appears here\n",
        )
        .unwrap();
        let (_, markdown) = engine
            .search(
                &SearchQuery {
                    text: Some("hand written".into()),
                    ..SearchQuery::default()
                },
                true,
            )
            .unwrap();
        assert_eq!(markdown.len(), 1);
        assert!(markdown[0].line.contains("alpha appears here"));
    }

    #[test]
    fn stats_report_the_active_search_index() {
        let scratch = Scratch::new("stats");
        let engine = scratch.engine();
        let stats = engine.stats().unwrap();
        assert_eq!(
            stats.search_index,
            if engine.store().has_fts() {
                SearchIndex::Fts5
            } else {
                SearchIndex::Fallback
            }
        );
        assert!(engine.verify().is_ok());
    }
}
