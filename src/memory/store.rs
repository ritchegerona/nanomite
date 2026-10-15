//! SQLite-backed store: the authoritative home for structured memory records.
//!
//! SQLite is authoritative for every structured field. The markdown documents are
//! a human-readable projection of the same records (see [`super::markdown`]), never
//! a second source of truth.
//!
//! The connection is synchronous on purpose: the agent runtime already funnels every
//! tool call through `spawn_blocking` (see `tools::exec_tools`), so the same pattern
//! keeps memory work off the async worker threads.

use super::model::*;
use super::policy::{MemoryOp, MemoryPolicy};
use super::security;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Bumped whenever [`MIGRATIONS`] grows an entry.
pub const SCHEMA_VERSION: u32 = 1;

/// Absolute row ceiling, independent of policy, for any single page.
pub const HARD_ROW_CAP: usize = 1_000;

/// How many error strings an import report keeps.
const MAX_IMPORT_ERRORS: usize = 10;

const COLUMNS: &str = "m.id, m.category, m.content, m.summary, m.tags, m.source_kind, \
     m.source_detail, m.project, m.created_at, m.updated_at, m.confidence, m.importance, \
     m.last_accessed_at, m.expires_at, m.sensitivity, m.access_count, m.revision, m.content_hash";

/// Ordered schema migrations. Each entry runs inside one transaction and
/// `PRAGMA user_version` is bumped inside that same transaction, so an
/// interrupted upgrade leaves the database on the previous version rather than
/// half-migrated.
const MIGRATIONS: &[&[&str]] = &[&[
    // -- v1 ----------------------------------------------------------------
    r#"CREATE TABLE IF NOT EXISTS memories (
        seq              INTEGER PRIMARY KEY AUTOINCREMENT,
        id               TEXT    NOT NULL UNIQUE,
        category         TEXT    NOT NULL,
        content          TEXT    NOT NULL,
        summary          TEXT,
        tags             TEXT    NOT NULL DEFAULT '',
        source_kind      TEXT    NOT NULL,
        source_detail    TEXT,
        project          TEXT,
        created_at       INTEGER NOT NULL,
        updated_at       INTEGER NOT NULL,
        confidence       REAL,
        importance       REAL    NOT NULL DEFAULT 0.5,
        last_accessed_at INTEGER,
        expires_at       INTEGER,
        sensitivity      TEXT    NOT NULL DEFAULT 'internal',
        access_count     INTEGER NOT NULL DEFAULT 0,
        revision         INTEGER NOT NULL DEFAULT 1,
        content_hash     TEXT    NOT NULL,
        CHECK (importance >= 0.0 AND importance <= 1.0),
        CHECK (category IN ('core','user','project','episodic','decision','session','task')),
        CHECK (sensitivity IN ('public','internal','confidential','secret'))
    )"#,
    "CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_memories_expires ON memories(expires_at) WHERE expires_at IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS idx_memories_hash ON memories(category, content_hash)",
    r#"CREATE TABLE IF NOT EXISTS markdown_index (
        path         TEXT PRIMARY KEY,
        title        TEXT NOT NULL,
        content_hash TEXT NOT NULL,
        mtime        INTEGER NOT NULL,
        size         INTEGER NOT NULL,
        indexed_at   INTEGER NOT NULL,
        block_ids    TEXT NOT NULL DEFAULT ''
    )"#,
    "CREATE TABLE IF NOT EXISTS memory_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
]];

/// How `import_memories` collides with existing rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// Keep the stored record when the id already exists.
    Insert,
    /// Overwrite the stored record when the id already exists.
    Replace,
}

impl ImportMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "insert" | "skip" => Some(ImportMode::Insert),
            "replace" | "upsert" => Some(ImportMode::Replace),
            _ => None,
        }
    }
}

/// Outcome of an import. On any rejection nothing is written at all.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ImportReport {
    pub inserted: usize,
    pub replaced: usize,
    pub skipped: usize,
    pub duplicates_merged: usize,
    pub rejected: usize,
    /// Human-readable reasons for rejected entries (bounded).
    pub errors: Vec<String>,
}

/// A memory store bound to one database file.
///
/// Sync + `Mutex` rather than a pool: this is a single-process TUI with one writer.
/// WAL mode and a busy timeout cover the second process that a backup or an
/// external `sqlite3` shell may open alongside it.
pub struct MemoryStore {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
    /// Runtime-detected capability, not a compile-time assumption.
    fts: AtomicBool,
    policy: MemoryPolicy,
}

impl MemoryStore {
    /// Opens (creating if needed) the database at `path` and applies migrations.
    pub fn open(path: impl AsRef<Path>, policy: MemoryPolicy) -> Result<Self, MemoryError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            security::ensure_dir(parent)?;
        }
        let conn = Connection::open(&path)
            .map_err(|e| MemoryError::Database(format!("open {}: {e}", path.display())))?;
        security::restrict(&path, false);
        let store = Self {
            conn: Mutex::new(conn),
            path: Some(path),
            fts: AtomicBool::new(false),
            policy,
        };
        store.migrate()?;
        Ok(store)
    }

    /// Private, non-persistent database. Used by tests and dry-run flows.
    pub fn open_in_memory(policy: MemoryPolicy) -> Result<Self, MemoryError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| MemoryError::Database(format!("open in-memory: {e}")))?;
        let store = Self {
            conn: Mutex::new(conn),
            path: None,
            fts: AtomicBool::new(false),
            policy,
        };
        store.migrate()?;
        Ok(store)
    }

    /// Locks the connection. A poisoned mutex is recovered rather than propagated:
    /// one panicking tool must not wedge the store for the rest of the session.
    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        match self.conn.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn policy(&self) -> &MemoryPolicy {
        &self.policy
    }

    pub fn set_policy(&mut self, policy: MemoryPolicy) {
        self.policy = policy;
    }

    /// True when the FTS5 index is live. When false the store answers keyword
    /// queries with its `LIKE` fallback and says so in [`MemoryList::index`].
    pub fn has_fts(&self) -> bool {
        self.fts.load(Ordering::Relaxed)
    }

    fn set_fts(&self, value: bool) {
        self.fts.store(value, Ordering::Relaxed);
    }

    /// Applies pending migrations and (idempotently) the optional FTS5 index.
    fn migrate(&self) -> Result<(), MemoryError> {
        let mut conn = self.lock();
        // WAL keeps readers (search) from blocking the writer (save), and makes a
        // concurrent second process wait instead of failing immediately.
        let _: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
            .map_err(|e| MemoryError::Database(format!("journal_mode: {e}")))?;
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|e| MemoryError::Database(format!("pragmas: {e}")))?;

        let current: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| MemoryError::Database(format!("read user_version: {e}")))?;

        for (index, statements) in MIGRATIONS.iter().enumerate() {
            let version = index as u32 + 1;
            if version <= current {
                continue;
            }
            let tx = conn
                .transaction()
                .map_err(|e| MemoryError::Database(format!("begin migration: {e}")))?;
            for statement in *statements {
                tx.execute_batch(statement)
                    .map_err(|e| MemoryError::Database(format!("migration v{version}: {e}")))?;
            }
            tx.pragma_update(None, "user_version", version)
                .map_err(|e| MemoryError::Database(format!("set user_version {version}: {e}")))?;
            tx.commit()
                .map_err(|e| MemoryError::Database(format!("commit migration v{version}: {e}")))?;
        }

        // FTS5 is a *capability*, not a schema version: probe it on every open so a
        // database moved between builds heals itself, and record the answer in
        // `memory_meta` for diagnostics.
        let available = self.setup_fts(&conn);
        self.set_fts(available);
        let _ = conn.execute(
            "INSERT INTO memory_meta(key, value) VALUES('fts5', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![if available { "1" } else { "0" }],
        );
        let _ = conn.execute(
            "INSERT INTO memory_meta(key, value) VALUES('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SCHEMA_VERSION.to_string()],
        );
        Ok(())
    }

    /// Creates the FTS5 shadow table and sync triggers. Returns `false` — leaving
    /// the database untouched — when this SQLite build lacks FTS5.
    fn setup_fts(&self, conn: &Connection) -> bool {
        let present: Option<i64> = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN ('memories_fts','memories_fts_ai')",
                [],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        if present.unwrap_or(0) >= 2 {
            return true;
        }
        const CREATE: &str = r#"
            CREATE VIRTUAL TABLE memories_fts USING fts5(
                content, summary, tags,
                content='memories', content_rowid='seq',
                tokenize = "unicode61 remove_diacritics 2"
            );
            CREATE TRIGGER memories_fts_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, content, summary, tags)
                VALUES (new.seq, new.content, coalesce(new.summary, ''), new.tags);
            END;
            CREATE TRIGGER memories_fts_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content, summary, tags)
                VALUES ('delete', old.seq, old.content, coalesce(old.summary, ''), old.tags);
            END;
            CREATE TRIGGER memories_fts_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content, summary, tags)
                VALUES ('delete', old.seq, old.content, coalesce(old.summary, ''), old.tags);
                INSERT INTO memories_fts(rowid, content, summary, tags)
                VALUES (new.seq, new.content, coalesce(new.summary, ''), new.tags);
            END;
        "#;
        match conn.execute_batch(CREATE) {
            Ok(()) => {
                // Backfill anything inserted before the index existed.
                let _ = conn.execute(
                    "INSERT INTO memories_fts(memories_fts) VALUES('rebuild')",
                    [],
                );
                true
            }
            Err(_) => false,
        }
    }

    /// Rebuilds the FTS index from the canonical `memories` table.
    pub fn rebuild_fts(&self) -> Result<usize, MemoryError> {
        if !self.has_fts() {
            return Err(MemoryError::Unsupported(
                "this SQLite build has no FTS5; keyword search uses the LIKE fallback".into(),
            ));
        }
        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .map_err(|e| MemoryError::Database(format!("begin rebuild: {e}")))?;
        tx.execute(
            "INSERT INTO memories_fts(memories_fts) VALUES('rebuild')",
            [],
        )
        .map_err(|e| MemoryError::Database(format!("fts rebuild: {e}")))?;
        let count: i64 = tx
            .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))
            .unwrap_or(0);
        tx.commit()
            .map_err(|e| MemoryError::Database(format!("commit rebuild: {e}")))?;
        Ok(count as usize)
    }

    // -- writes -------------------------------------------------------------

    /// Stores a new record, or returns the existing one when an identical record
    /// (same category, same normalized content) is already present.
    pub fn save(&self, draft: &NewMemory) -> Result<SaveOutcome, MemoryError> {
        let valid = draft.validate(now())?;
        let content_hash = security::content_hash(&valid.content);
        let mut record = MemoryRecord {
            id: String::new(),
            category: valid.category,
            content: valid.content,
            summary: valid.summary,
            tags: valid.tags,
            source: valid.source,
            project: valid.project,
            created_at: valid.now,
            updated_at: valid.now,
            confidence: valid.confidence,
            importance: valid.importance,
            last_accessed_at: None,
            expires_at: valid.expires_at,
            sensitivity: valid.sensitivity,
            access_count: 0,
            revision: 1,
            content_hash,
        };
        security::assert_record_safe(&record)?;
        self.policy
            .authorize(MemoryOp::Write, 1, record.sensitivity, false)?;

        let conn = self.lock();
        if let Some(existing) = find_duplicate(&conn, &record)? {
            return Ok(SaveOutcome {
                record: existing,
                duplicate: true,
                markdown: None,
            });
        }
        insert_record(&conn, &mut record)?;
        Ok(SaveOutcome {
            record,
            duplicate: false,
            markdown: None,
        })
    }

    /// Applies a sparse patch and returns the updated record.
    pub fn update(&self, id: &str, patch: &MemoryPatch) -> Result<MemoryRecord, MemoryError> {
        let current = self.get(id)?;
        let target_sensitivity = patch
            .sensitivity
            .unwrap_or(current.sensitivity)
            .max(current.sensitivity);
        self.policy
            .authorize(MemoryOp::Update, 1, target_sensitivity, false)?;
        let next = patch.apply(&current, now())?;
        security::assert_record_safe(&next)?;

        let conn = self.lock();
        conn.execute(
            "UPDATE memories SET
                category = ?2, content = ?3, summary = ?4, tags = ?5,
                source_kind = ?6, source_detail = ?7, project = ?8,
                updated_at = ?9, confidence = ?10, importance = ?11,
                expires_at = ?12, sensitivity = ?13, revision = ?14, content_hash = ?15
             WHERE id = ?1",
            params![
                next.id,
                next.category.as_str(),
                next.content,
                next.summary,
                encode_tags(&next.tags),
                next.source.kind.as_str(),
                next.source.detail,
                next.project,
                next.updated_at,
                next.confidence,
                next.importance,
                next.expires_at,
                next.sensitivity.as_str(),
                next.revision,
                next.content_hash,
            ],
        )
        .map_err(|e| MemoryError::Database(format!("update {id}: {e}")))?;
        Ok(next)
    }

    /// Removes one record and returns it, so the caller can mirror the deletion.
    ///
    /// Deleting a record removes its SQLite row, its FTS entries and (via the
    /// caller) its markdown block. It never rewrites unrelated user-authored
    /// markdown prose, and there are no derived summaries or caches to invalidate.
    pub fn delete(&self, id: &str) -> Result<MemoryRecord, MemoryError> {
        let record = self.get(id)?;
        self.policy
            .authorize(MemoryOp::Delete, 1, record.sensitivity, false)?;
        let conn = self.lock();
        conn.execute("DELETE FROM memories WHERE id = ?1", params![id])
            .map_err(|e| MemoryError::Database(format!("delete {id}: {e}")))?;
        Ok(record)
    }

    /// Deletes many records in one transaction. `confirmed` is required past the
    /// policy's bulk threshold; the whole batch rolls back on any error.
    pub fn delete_many(
        &self,
        ids: &[String],
        confirmed: bool,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let mut unique: Vec<String> = Vec::new();
        for id in ids {
            let trimmed = id.trim();
            if !trimmed.is_empty() && !unique.iter().any(|u| u == trimmed) {
                unique.push(trimmed.to_string());
            }
        }
        let ceiling = self.max_sensitivity_of(&unique)?;
        self.policy
            .authorize(MemoryOp::Delete, unique.len(), ceiling, confirmed)?;

        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .map_err(|e| MemoryError::Database(format!("begin delete: {e}")))?;
        let mut removed = Vec::with_capacity(unique.len());
        for id in &unique {
            let row = tx
                .query_row(
                    &format!("SELECT {COLUMNS} FROM memories m WHERE m.id = ?1"),
                    params![id],
                    map_row,
                )
                .optional()?;
            let Some(record) = row else { continue };
            tx.execute("DELETE FROM memories WHERE id = ?1", params![id])
                .map_err(|e| MemoryError::Database(format!("delete {id}: {e}")))?;
            removed.push(record);
        }
        tx.commit()
            .map_err(|e| MemoryError::Database(format!("commit delete: {e}")))?;
        Ok(removed)
    }

    fn max_sensitivity_of(&self, ids: &[String]) -> Result<Sensitivity, MemoryError> {
        let mut ceiling = Sensitivity::Public;
        for id in ids {
            if let Ok(record) = self.get(id) {
                ceiling = ceiling.max(record.sensitivity);
            }
        }
        Ok(ceiling)
    }

    /// Records that retrieval surfaced. Called only for memories actually handed
    /// to a model, so read-only queries never write.
    pub fn touch_access(&self, ids: &[String]) -> Result<usize, MemoryError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let stamp = now();
        let conn = self.lock();
        let mut touched = 0usize;
        for id in ids {
            touched += conn
                .execute(
                    "UPDATE memories
                     SET last_accessed_at = ?2, access_count = access_count + 1
                     WHERE id = ?1",
                    params![id, stamp],
                )
                .map_err(|e| MemoryError::Database(format!("touch {id}: {e}")))?;
        }
        Ok(touched)
    }

    // -- reads --------------------------------------------------------------

    /// Exact id lookup.
    pub fn get(&self, id: &str) -> Result<MemoryRecord, MemoryError> {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return Err(MemoryError::invalid("memory id must not be empty"));
        }
        let conn = self.lock();
        conn.query_row(
            &format!("SELECT {COLUMNS} FROM memories m WHERE m.id = ?1"),
            params![trimmed],
            map_row,
        )
        .optional()?
        .ok_or_else(|| MemoryError::NotFound(format!("no memory with id `{trimmed}`")))
    }

    /// Keyword + filter search. Bounded by the policy even when the caller asks
    /// for more rows than it is allowed to see.
    pub fn search(&self, query: &SearchQuery) -> Result<MemoryList, MemoryError> {
        self.policy
            .authorize(MemoryOp::Read, 1, query.max_sensitivity, false)?;
        let limit = self.policy.clamp_limit(query.limit).min(HARD_ROW_CAP);
        let offset = query.offset.min(HARD_ROW_CAP);
        let text = query.text.as_deref().map(str::trim).unwrap_or("");

        let filters = Filters {
            categories: query.categories.clone(),
            tags: normalize_tags(&query.tags).unwrap_or_default(),
            project: query.project.clone(),
            project_unscoped: query.project_unscoped,
            min_importance: query.min_importance,
            max_sensitivity: query.max_sensitivity,
            include_expired: query.include_expired,
        };

        // A punctuation-only query has no tokens to match; answering "everything"
        // would be worse than answering nothing.
        let use_fts = self.has_fts() && !text.is_empty() && has_token(text);
        if self.has_fts() && !text.is_empty() && !has_token(text) {
            return Ok(MemoryList {
                items: Vec::new(),
                total: 0,
                truncated: false,
                index: if use_fts {
                    SearchIndex::Fts5
                } else {
                    SearchIndex::Fallback
                },
            });
        }

        let mut params: Vec<SqlValue> = Vec::new();
        let mut clauses: Vec<String> = Vec::new();

        // Filters bind first, then the text predicate: the SQL uses explicit
        // ?N indices so the SELECT score expression can sit before the WHERE.
        let filter_sql = filters.clause(&mut params);
        if !filter_sql.is_empty() {
            clauses.push(filter_sql);
        }

        let index = if use_fts {
            clauses.push(format!("memories_fts MATCH ?{}", params.len() + 1));
            params.push(SqlValue::Text(fts_match_expression(text)));
            SearchIndex::Fts5
        } else if !text.is_empty() {
            let pattern = like_pattern(text);
            for column in ["m.content", "coalesce(m.summary, '')", "m.tags"] {
                clauses.push(format!("{column} LIKE ?{} ESCAPE '\\'", params.len() + 1));
                params.push(SqlValue::Text(pattern.clone()));
            }
            SearchIndex::Fallback
        } else {
            SearchIndex::Fallback
        };

        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };

        let order_sql = match (query.order, use_fts) {
            (SearchOrder::Relevance, true) => {
                "ORDER BY lexical DESC, m.importance DESC, m.updated_at DESC, m.id ASC"
            }
            (SearchOrder::Recent, _) => "ORDER BY m.updated_at DESC, m.id ASC",
            (SearchOrder::Important, _) => {
                "ORDER BY m.importance DESC, m.updated_at DESC, m.id ASC"
            }
            (SearchOrder::Relevance, false) => {
                "ORDER BY m.importance DESC, m.updated_at DESC, m.id ASC"
            }
        };

        let limit_index = params.len() + 1;
        let offset_index = params.len() + 2;
        params.push(SqlValue::Integer(limit as i64));
        params.push(SqlValue::Integer(offset as i64));

        let (from_sql, score_sql) = if use_fts {
            (
                "memories_fts JOIN memories m ON m.seq = memories_fts.rowid",
                "(-bm25(memories_fts))",
            )
        } else {
            ("memories m", "0.0")
        };

        let conn = self.lock();
        let total: i64 = conn
            .query_row(
                &format!("SELECT count(*) FROM {from_sql} {where_sql}"),
                params_from_iter(&params[..limit_index - 1]),
                |r| r.get(0),
            )
            .unwrap_or(0);

        let sql = format!(
            "SELECT {COLUMNS}, {score_sql} AS lexical FROM {from_sql} {where_sql} {order_sql} \
             LIMIT ?{limit_index} OFFSET ?{offset_index}"
        );
        let mut stmt = conn
            .prepare_cached(&sql)
            .map_err(|e| MemoryError::Database(format!("prepare search: {e}")))?;
        let rows = stmt
            .query_map(params_from_iter(&params), |row| {
                Ok((map_row(row)?, row.get::<_, f64>(18)?))
            })
            .map_err(|e| MemoryError::Database(format!("run search: {e}")))?;

        let items = collect_hits(rows, text)?;
        let total = total.max(0) as usize;
        Ok(MemoryList {
            truncated: items.len() < total,
            items,
            total,
            index,
        })
    }

    /// Filter-only listing (no lexical scoring).
    pub fn list(&self, query: &ListQuery) -> Result<MemoryList, MemoryError> {
        self.policy
            .authorize(MemoryOp::Read, 1, query.max_sensitivity, false)?;
        let limit = self.policy.clamp_limit(query.limit).min(HARD_ROW_CAP);
        let offset = query.offset.min(HARD_ROW_CAP);

        let mut params: Vec<SqlValue> = Vec::new();
        let mut clauses: Vec<String> = Vec::new();
        if let Some(category) = query.category {
            clauses.push(format!("m.category = ?{}", params.len() + 1));
            params.push(SqlValue::Text(category.as_str().to_string()));
        }
        if let Some(project) = &query.project {
            clauses.push(format!("m.project = ?{}", params.len() + 1));
            params.push(SqlValue::Text(project.clone()));
        }
        if let Some(tag) = &query.tag {
            clauses.push(format!("m.tags LIKE ?{} ESCAPE '\\'", params.len() + 1));
            params.push(SqlValue::Text(format!("%,{},%", escape_like(tag))));
        }
        let filters = Filters {
            categories: Vec::new(),
            tags: Vec::new(),
            project: None,
            project_unscoped: false,
            min_importance: None,
            max_sensitivity: query.max_sensitivity,
            include_expired: query.include_expired,
        };
        let filter_sql = filters.clause(&mut params);
        if !filter_sql.is_empty() {
            clauses.push(filter_sql);
        }
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        let order_sql = match query.order {
            SearchOrder::Important => "m.importance DESC, m.updated_at DESC, m.id ASC",
            _ => "m.updated_at DESC, m.id ASC",
        };

        let limit_index = params.len() + 1;
        let offset_index = params.len() + 2;
        params.push(SqlValue::Integer(limit as i64));
        params.push(SqlValue::Integer(offset as i64));

        let conn = self.lock();
        let total: i64 = conn
            .query_row(
                &format!("SELECT count(*) FROM memories m {where_sql}"),
                params_from_iter(&params[..limit_index - 1]),
                |r| r.get(0),
            )
            .unwrap_or(0);
        let sql = format!(
            "SELECT {COLUMNS}, 0.0 AS lexical FROM memories m {where_sql} ORDER BY {order_sql} \
             LIMIT ?{limit_index} OFFSET ?{offset_index}"
        );
        let mut stmt = conn
            .prepare_cached(&sql)
            .map_err(|e| MemoryError::Database(format!("prepare list: {e}")))?;
        let rows = stmt
            .query_map(params_from_iter(&params), |row| {
                Ok((map_row(row)?, row.get::<_, f64>(18)?))
            })
            .map_err(|e| MemoryError::Database(format!("run list: {e}")))?;
        let items = collect_hits(rows, "")?;
        let total = total.max(0) as usize;
        Ok(MemoryList {
            truncated: items.len() < total,
            items,
            total,
            index: SearchIndex::Fallback,
        })
    }

    /// Newest first, across every category.
    pub fn recent(&self, limit: usize) -> Result<MemoryList, MemoryError> {
        self.list(&ListQuery {
            order: SearchOrder::Recent,
            limit: Some(limit),
            ..ListQuery::default()
        })
    }

    /// Everything scoped to one project, most important first.
    pub fn project_memory(&self, project: &str, limit: usize) -> Result<MemoryList, MemoryError> {
        self.list(&ListQuery {
            project: Some(project.to_string()),
            order: SearchOrder::Important,
            limit: Some(limit),
            ..ListQuery::default()
        })
    }

    // -- maintenance --------------------------------------------------------

    /// Merges exact duplicates (same category + normalized content).
    ///
    /// Deterministic and non-destructive: the survivor is the oldest row, tags are
    /// unioned, importance keeps the maximum, access counts are summed, and every
    /// merged-away id is reported. No summarization, no LLM, no fuzzy matching.
    pub fn consolidate(&self) -> Result<ConsolidationReport, MemoryError> {
        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .map_err(|e| MemoryError::Database(format!("begin consolidate: {e}")))?;

        #[derive(Clone)]
        struct Member {
            seq: i64,
            id: String,
            tags: String,
            importance: f64,
            access: i64,
        }

        let groups: Vec<(String, String, i64)> = {
            let mut stmt = tx
                .prepare(
                    "SELECT category, content_hash, min(seq) FROM memories
                     GROUP BY category, content_hash HAVING count(*) > 1
                     ORDER BY category, content_hash",
                )
                .map_err(|e| MemoryError::Database(format!("prepare consolidate: {e}")))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|e| MemoryError::Database(format!("run consolidate: {e}")))?;
            collect(rows, "duplicate group")?
        };

        let scanned: i64 = tx
            .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))
            .unwrap_or(0);
        let mut report = ConsolidationReport {
            duplicate_groups: groups.len(),
            scanned: scanned as usize,
            ..ConsolidationReport::default()
        };

        for (category, hash, keep_seq) in groups {
            let members: Vec<Member> = {
                let mut stmt = tx
                    .prepare(
                        "SELECT seq, id, tags, importance, access_count FROM memories
                         WHERE category = ?1 AND content_hash = ?2 ORDER BY seq ASC",
                    )
                    .map_err(|e| MemoryError::Database(format!("prepare members: {e}")))?;
                let rows = stmt
                    .query_map(params![category, hash], |row| {
                        Ok(Member {
                            seq: row.get(0)?,
                            id: row.get(1)?,
                            tags: row.get(2)?,
                            importance: row.get(3)?,
                            access: row.get(4)?,
                        })
                    })
                    .map_err(|e| MemoryError::Database(format!("query members: {e}")))?;
                collect(rows, "duplicate member")?
            };
            let Some(survivor) = members.iter().find(|m| m.seq == keep_seq) else {
                continue;
            };
            let survivor_id = survivor.id.clone();
            let mut tags = decode_tags(&survivor.tags);
            let mut importance = survivor.importance;
            let mut access = survivor.access;
            let mut removed = 0usize;
            for member in members.iter().filter(|m| m.seq != keep_seq) {
                for tag in decode_tags(&member.tags) {
                    if !tags.contains(&tag) {
                        tags.push(tag);
                        report.tags_merged += 1;
                    }
                }
                importance = importance.max(member.importance);
                access += member.access;
                tx.execute("DELETE FROM memories WHERE id = ?1", params![member.id])
                    .map_err(|e| MemoryError::Database(format!("prune {}: {e}", member.id)))?;
                removed += 1;
            }
            tags.sort();
            tags.truncate(MAX_TAGS);
            tx.execute(
                "UPDATE memories SET tags = ?2, importance = ?3, access_count = ?4,
                    revision = revision + 1 WHERE id = ?1",
                params![survivor_id, encode_tags(&tags), importance, access],
            )
            .map_err(|e| MemoryError::Database(format!("merge survivor: {e}")))?;
            report.records_removed += removed;
        }

        tx.commit()
            .map_err(|e| MemoryError::Database(format!("commit consolidate: {e}")))?;
        Ok(report)
    }

    /// Deletes records whose `expires_at` has passed.
    ///
    /// This is retention, not capacity enforcement: unexpired user data is never
    /// removed automatically.
    pub fn purge_expired(&self) -> Result<Vec<MemoryRecord>, MemoryError> {
        let stamp = now();
        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .map_err(|e| MemoryError::Database(format!("begin purge: {e}")))?;
        let expired: Vec<MemoryRecord> = {
            let mut stmt = tx
                .prepare(&format!(
                    "SELECT {COLUMNS} FROM memories m
                     WHERE m.expires_at IS NOT NULL AND m.expires_at <= ?1
                     ORDER BY m.expires_at ASC"
                ))
                .map_err(|e| MemoryError::Database(format!("prepare purge: {e}")))?;
            let rows = stmt
                .query_map(params![stamp], map_row)
                .map_err(|e| MemoryError::Database(format!("query expired: {e}")))?;
            collect(rows, "expired record")?
        };
        for record in &expired {
            tx.execute("DELETE FROM memories WHERE id = ?1", params![record.id])
                .map_err(|e| MemoryError::Database(format!("purge {}: {e}", record.id)))?;
        }
        tx.commit()
            .map_err(|e| MemoryError::Database(format!("commit purge: {e}")))?;
        Ok(expired)
    }

    // -- transfer -----------------------------------------------------------

    /// Snapshot of every stored record plus the markdown index.
    pub fn export(&self) -> Result<ExportBundle, MemoryError> {
        // The connection guard must be dropped before `markdown_index()`: it takes
        // the same non-reentrant mutex, so holding it across the call deadlocks.
        let memories: Vec<MemoryRecord> = {
            let conn = self.lock();
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {COLUMNS} FROM memories m ORDER BY m.created_at ASC, m.id ASC"
                ))
                .map_err(|e| MemoryError::Database(format!("prepare export: {e}")))?;
            let rows = stmt
                .query_map([], map_row)
                .map_err(|e| MemoryError::Database(format!("query export: {e}")))?;
            collect(rows, "exported record")?
        };
        let markdown_index = self.markdown_index()?;
        Ok(ExportBundle {
            version: ExportBundle::VERSION,
            exported_at: now(),
            memories,
            markdown_index,
        })
    }

    /// Validates an entire bundle before writing anything: one bad record aborts
    /// the import with a report, leaving existing memory untouched.
    pub fn import(
        &self,
        bundle: &ExportBundle,
        mode: ImportMode,
        confirmed: bool,
    ) -> Result<ImportReport, MemoryError> {
        if bundle.version > ExportBundle::VERSION {
            return Err(MemoryError::Unsupported(format!(
                "export bundle version {} is newer than this build understands ({})",
                bundle.version,
                ExportBundle::VERSION
            )));
        }
        self.policy.authorize(
            MemoryOp::Import,
            bundle.memories.len(),
            Sensitivity::Confidential,
            confirmed,
        )?;

        // Phase 1 - validate everything. No write happens in this phase.
        let stamp = now();
        let mut prepared: Vec<MemoryRecord> = Vec::with_capacity(bundle.memories.len());
        let mut report = ImportReport::default();
        for raw in &bundle.memories {
            match sanitize_imported(raw, stamp) {
                Ok(record) => prepared.push(record),
                Err(e) => {
                    report.rejected += 1;
                    if report.errors.len() < MAX_IMPORT_ERRORS {
                        report.errors.push(format!(
                            "{}: {}",
                            raw.id.chars().take(24).collect::<String>(),
                            e
                        ));
                    }
                }
            }
        }
        if report.rejected > 0 {
            return Err(MemoryError::invalid(format!(
                "import rejected: {} of {} records failed validation, so nothing was written \
                 (first problem: {})",
                report.rejected,
                bundle.memories.len(),
                report.errors.first().cloned().unwrap_or_default()
            )));
        }

        // Phase 2 - one transaction, so a failure mid-way leaves no partial state.
        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .map_err(|e| MemoryError::Database(format!("begin import: {e}")))?;
        for record in &prepared {
            let exists: Option<i64> = tx
                .query_row(
                    "SELECT seq FROM memories WHERE id = ?1",
                    params![record.id],
                    |r| r.get(0),
                )
                .optional()?;
            match (exists, mode) {
                (Some(_), ImportMode::Insert) => report.skipped += 1,
                (Some(_), ImportMode::Replace) => {
                    tx.execute(
                        "UPDATE memories SET category = ?2, content = ?3, summary = ?4, tags = ?5,
                            source_kind = ?6, source_detail = ?7, project = ?8, updated_at = ?9,
                            confidence = ?10, importance = ?11, expires_at = ?12, sensitivity = ?13,
                            revision = revision + 1, content_hash = ?14, created_at = ?15
                         WHERE id = ?1",
                        params![
                            record.id,
                            record.category.as_str(),
                            record.content,
                            record.summary,
                            encode_tags(&record.tags),
                            record.source.kind.as_str(),
                            record.source.detail,
                            record.project,
                            record.updated_at,
                            record.confidence,
                            record.importance,
                            record.expires_at,
                            record.sensitivity.as_str(),
                            record.content_hash,
                            record.created_at,
                        ],
                    )
                    .map_err(|e| MemoryError::Database(format!("replace {}: {e}", record.id)))?;
                    report.replaced += 1;
                }
                (None, _) => {
                    let mut fresh = record.clone();
                    insert_record(&tx, &mut fresh)?;
                    report.inserted += 1;
                }
            }
            if let Some(survivor) = find_duplicate(&tx, record)? {
                if survivor.id != record.id {
                    report.duplicates_merged += 1;
                }
            }
        }
        tx.commit()
            .map_err(|e| MemoryError::Database(format!("commit import: {e}")))?;
        Ok(report)
    }

    /// Writes a consistent copy of the database to `destination` (SQLite 3.27+).
    pub fn backup_to(&self, destination: &Path) -> Result<(), MemoryError> {
        if destination == self.path().unwrap_or(Path::new("")) {
            return Err(MemoryError::invalid(
                "refused: a backup cannot overwrite the live database",
            ));
        }
        if let Some(parent) = destination.parent() {
            security::ensure_dir(parent)?;
        }
        let _ = std::fs::remove_file(destination);
        let conn = self.lock();
        conn.execute("VACUUM INTO ?1", params![destination.to_string_lossy()])
            .map_err(|e| {
                MemoryError::Database(format!("backup to {}: {e}", destination.display()))
            })?;
        security::restrict(destination, false);
        Ok(())
    }

    /// Replaces the live database with `source` after validating it.
    ///
    /// The candidate is integrity-checked, migrated and counted on a scratch
    /// connection first; only then is it swapped in. A bad restore can therefore
    /// never destroy working memory.
    pub fn restore_from(&self, source: &Path) -> Result<usize, MemoryError> {
        if !source.exists() {
            return Err(MemoryError::NotFound(format!(
                "backup {} does not exist",
                source.display()
            )));
        }
        {
            let candidate = Connection::open(source)
                .map_err(|e| MemoryError::Corrupt(format!("open {}: {e}", source.display())))?;
            let check: String = candidate
                .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .map_err(|e| MemoryError::Corrupt(format!("integrity_check: {e}")))?;
            if check != "ok" {
                return Err(MemoryError::Corrupt(format!(
                    "backup {} failed integrity_check: {check}",
                    source.display()
                )));
            }
        }

        let fresh = Connection::open(source)
            .map_err(|e| MemoryError::Database(format!("reopen backup: {e}")))?;
        let staged = Self {
            conn: Mutex::new(fresh),
            path: self.path.clone(),
            fts: AtomicBool::new(false),
            policy: self.policy.clone(),
        };
        staged.migrate()?;
        let count = staged.stats()?.total;

        // Hand the validated connection to the live store. Swapping (rather than
        // replacing) drops the old connection here, which closes it cleanly.
        let restored_fts = staged.has_fts();
        let mut staged_conn = staged.lock();
        let mut live = self.lock();
        std::mem::swap(&mut *live, &mut *staged_conn);
        drop(live);
        drop(staged_conn);
        self.set_fts(restored_fts);
        Ok(count)
    }

    /// `PRAGMA integrity_check` plus row counts.
    pub fn integrity_check(&self) -> Result<MemoryStats, MemoryError> {
        {
            let conn = self.lock();
            let result: String = conn
                .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .map_err(|e| MemoryError::Database(format!("integrity_check: {e}")))?;
            if result != "ok" {
                return Err(MemoryError::Corrupt(result));
            }
        }
        self.stats()
    }

    pub fn stats(&self) -> Result<MemoryStats, MemoryError> {
        let conn = self.lock();
        let total: i64 = conn
            .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))
            .map_err(|e| MemoryError::Database(format!("count: {e}")))?;
        let expired: i64 = conn
            .query_row(
                "SELECT count(*) FROM memories WHERE expires_at IS NOT NULL AND expires_at <= ?1",
                params![now()],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let by_category: Vec<(MemoryCategory, usize)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT category, count(*) FROM memories GROUP BY category ORDER BY category",
                )
                .map_err(|e| MemoryError::Database(format!("prepare stats: {e}")))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|e| MemoryError::Database(format!("query stats: {e}")))?;
            collect(rows, "category count")?
                .into_iter()
                .filter_map(|(name, count)| {
                    MemoryCategory::parse(&name).map(|c| (c, count.max(0) as usize))
                })
                .collect()
        };
        let markdown_documents: i64 = conn
            .query_row("SELECT count(*) FROM markdown_index", [], |r| r.get(0))
            .unwrap_or(0);
        Ok(MemoryStats {
            total: total.max(0) as usize,
            expired: expired.max(0) as usize,
            by_category,
            markdown_documents: markdown_documents.max(0) as usize,
            markdown_stale: 0,
            search_index: if self.has_fts() {
                SearchIndex::Fts5
            } else {
                SearchIndex::Fallback
            },
        })
    }

    // -- markdown index -----------------------------------------------------

    pub fn markdown_index(&self) -> Result<Vec<MarkdownIndexRow>, MemoryError> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT path, title, content_hash, mtime, size, indexed_at, block_ids
                 FROM markdown_index ORDER BY path ASC",
            )
            .map_err(|e| MemoryError::Database(format!("prepare markdown_index: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .map_err(|e| MemoryError::Database(format!("query markdown index: {e}")))?;
        let tuples = collect(rows, "markdown index row")?;
        Ok(tuples
            .into_iter()
            .map(
                |(path, title, content_hash, mtime, size, indexed_at, block_ids)| {
                    MarkdownIndexRow {
                        path,
                        title,
                        content_hash,
                        mtime,
                        size: size.max(0) as u64,
                        indexed_at,
                        block_ids: decode_tags(&block_ids),
                    }
                },
            )
            .collect())
    }

    pub fn upsert_markdown_index(&self, row: &MarkdownIndexRow) -> Result<(), MemoryError> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO markdown_index(path, title, content_hash, mtime, size, indexed_at, block_ids)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(path) DO UPDATE SET
                title = excluded.title, content_hash = excluded.content_hash,
                mtime = excluded.mtime, size = excluded.size,
                indexed_at = excluded.indexed_at, block_ids = excluded.block_ids",
            params![
                row.path,
                row.title,
                row.content_hash,
                row.mtime,
                row.size as i64,
                row.indexed_at,
                encode_tags(&row.block_ids),
            ],
        )
        .map_err(|e| MemoryError::Database(format!("markdown_index {}: {e}", row.path)))?;
        Ok(())
    }

    pub fn remove_markdown_index(&self, path: &str) -> Result<(), MemoryError> {
        let conn = self.lock();
        conn.execute("DELETE FROM markdown_index WHERE path = ?1", params![path])
            .map_err(|e| MemoryError::Database(format!("markdown_index delete: {e}")))?;
        Ok(())
    }
}

// -- row mapping and helpers -------------------------------------------------

type RowResult<T> = Result<T, MemoryError>;

/// Drains a rusqlite row iterator into a `Vec`, mapping SQLite errors to
/// [`MemoryError`] with the query that produced them.
fn collect<T>(rows: impl Iterator<Item = rusqlite::Result<T>>, what: &str) -> RowResult<Vec<T>> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| MemoryError::Database(format!("read {what}: {e}")))?);
    }
    Ok(out)
}

fn map_row(row: &Row<'_>) -> rusqlite::Result<MemoryRecord> {
    let category: String = row.get(1)?;
    let sensitivity: String = row.get(14)?;
    let source_kind: String = row.get(5)?;
    Ok(MemoryRecord {
        id: row.get(0)?,
        category: MemoryCategory::parse(&category).unwrap_or(MemoryCategory::Core),
        content: row.get(2)?,
        summary: row.get(3)?,
        tags: decode_tags(&row.get::<_, String>(4)?),
        source: MemorySource {
            kind: SourceKind::parse(&source_kind).unwrap_or(SourceKind::Unknown),
            detail: row.get(6)?,
        },
        project: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        confidence: row.get(10)?,
        importance: row.get(11)?,
        last_accessed_at: row.get(12)?,
        expires_at: row.get(13)?,
        sensitivity: Sensitivity::parse(&sensitivity).unwrap_or_default(),
        access_count: row.get(15)?,
        revision: row.get(16)?,
        content_hash: row.get(17)?,
    })
}

/// Finds an *exact* duplicate of `record`: same category, normalized content,
/// tags, importance, summary, project and sensitivity.
///
/// The match is deliberately strict. A record whose metadata differs is a new
/// record, not a duplicate - consolidation (see [`MemoryStore::consolidate`]) is
/// the explicit step that merges rows which are content-identical but
/// metadata-divergent, and it needs those rows to exist.
fn find_duplicate(
    conn: &Connection,
    record: &MemoryRecord,
) -> Result<Option<MemoryRecord>, MemoryError> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM memories m
             WHERE m.category = ?1 AND m.content_hash = ?2
               AND m.tags = ?3 AND m.importance = ?4
               AND coalesce(m.summary, '') = coalesce(?5, '')
               AND coalesce(m.project, '') = coalesce(?6, '')
               AND m.sensitivity = ?7
               AND (m.expires_at IS NULL OR m.expires_at > ?8)
             ORDER BY m.created_at ASC, m.id ASC LIMIT 1"
        ),
        params![
            record.category.as_str(),
            record.content_hash,
            encode_tags(&record.tags),
            record.importance,
            record.summary.as_deref().unwrap_or(""),
            record.project.as_deref().unwrap_or(""),
            record.sensitivity.as_str(),
            now(),
        ],
        map_row,
    )
    .optional()
    .map_err(|e| MemoryError::Database(format!("find duplicate: {e}")))
}

/// Inserts `record`, assigning a stable id. Retries on id collision rather than
/// failing the save.
fn insert_record(conn: &Connection, record: &mut MemoryRecord) -> Result<(), MemoryError> {
    let stamp = now();
    for attempt in 0..8u32 {
        let candidate = security::new_id(stamp, u64::from(attempt), &record.content);
        let inserted = conn.execute(
            "INSERT INTO memories
                (id, category, content, summary, tags, source_kind, source_detail, project,
                 created_at, updated_at, confidence, importance, last_accessed_at, expires_at,
                 sensitivity, access_count, revision, content_hash)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,?13,?14,0,?15,?16)",
            params![
                candidate,
                record.category.as_str(),
                record.content,
                record.summary,
                encode_tags(&record.tags),
                record.source.kind.as_str(),
                record.source.detail,
                record.project,
                record.created_at,
                record.updated_at,
                record.confidence,
                record.importance,
                record.expires_at,
                record.sensitivity.as_str(),
                record.revision,
                record.content_hash,
            ],
        );
        match inserted {
            Ok(_) => {
                record.id = candidate;
                return Ok(());
            }
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                continue;
            }
            Err(e) => return Err(MemoryError::Database(format!("insert: {e}"))),
        }
    }
    Err(MemoryError::Database(
        "could not allocate a unique memory id".into(),
    ))
}

/// Re-validates an imported record and rebuilds its derived fields. Imported
/// hashes, counters and timestamps are never trusted.
fn sanitize_imported(raw: &MemoryRecord, stamp: i64) -> Result<MemoryRecord, MemoryError> {
    let draft = NewMemory {
        category: raw.category,
        content: raw.content.clone(),
        summary: raw.summary.clone(),
        tags: raw.tags.clone(),
        source: raw.source.clone(),
        project: raw.project.clone(),
        confidence: raw.confidence,
        importance: Some(raw.importance),
        expires_at: raw.expires_at,
        sensitivity: Some(raw.sensitivity),
        mirror_markdown: false,
    };
    let valid = draft.validate(stamp)?;
    let mut record = raw.clone();
    record.category = valid.category;
    record.content = valid.content;
    record.summary = valid.summary;
    record.tags = valid.tags;
    record.source = valid.source;
    record.project = valid.project;
    record.confidence = valid.confidence;
    record.importance = valid.importance;
    record.sensitivity = valid.sensitivity;
    record.expires_at = valid.expires_at;
    record.created_at = raw.created_at.clamp(0, stamp);
    record.updated_at = raw.updated_at.clamp(record.created_at, stamp);
    record.access_count = raw.access_count.max(0);
    record.revision = raw.revision.max(1);
    record.last_accessed_at = None;
    record.content_hash = security::content_hash(&record.content);
    if record.id.trim().is_empty() {
        record.id = security::new_id(stamp, 0, &record.content_hash);
    }
    security::assert_record_safe(&record)?;
    Ok(record)
}

// -- filters -----------------------------------------------------------------

struct Filters {
    categories: Vec<MemoryCategory>,
    tags: Vec<String>,
    project: Option<String>,
    project_unscoped: bool,
    min_importance: Option<f64>,
    max_sensitivity: Sensitivity,
    include_expired: bool,
}

impl Filters {
    /// Appends the shared WHERE fragments, binding with explicit `?N` indices so
    /// call sites can place the score expression in the SELECT list.
    fn clause(&self, params: &mut Vec<SqlValue>) -> String {
        let mut parts: Vec<String> = Vec::new();

        if !self.categories.is_empty() {
            let placeholders = self
                .categories
                .iter()
                .map(|category| {
                    params.push(SqlValue::Text(category.as_str().to_string()));
                    format!("?{}", params.len())
                })
                .collect::<Vec<_>>();
            parts.push(format!("m.category IN ({})", placeholders.join(", ")));
        }

        for tag in &self.tags {
            parts.push(format!("m.tags LIKE ?{} ESCAPE '\\'", params.len() + 1));
            params.push(SqlValue::Text(format!("%,{},%", escape_like(tag))));
        }

        if let Some(project) = &self.project {
            // A scoped query also matches project-neutral records: something saved
            // without a project applies everywhere, this project included.
            if self.project_unscoped {
                parts.push(format!(
                    "(m.project = ?{} OR m.project IS NULL)",
                    params.len() + 1
                ));
            } else {
                parts.push(format!("m.project = ?{}", params.len() + 1));
            }
            params.push(SqlValue::Text(project.clone()));
        }

        if let Some(min) = self.min_importance {
            parts.push(format!("m.importance >= ?{}", params.len() + 1));
            params.push(SqlValue::Real(min));
        }

        let ranks = Sensitivity::ALL
            .iter()
            .take_while(|s| s.within(self.max_sensitivity))
            .map(|s| {
                params.push(SqlValue::Text(s.as_str().to_string()));
                format!("?{}", params.len())
            })
            .collect::<Vec<_>>();
        parts.push(format!("m.sensitivity IN ({})", ranks.join(", ")));

        if !self.include_expired {
            parts.push(format!(
                "(m.expires_at IS NULL OR m.expires_at > ?{})",
                params.len() + 1
            ));
            params.push(SqlValue::Integer(now()));
        }

        parts.join(" AND ")
    }
}

// -- encoding and text helpers ----------------------------------------------

fn encode_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        String::new()
    } else {
        format!(",{},", tags.join(","))
    }
}

fn decode_tags(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn like_pattern(value: &str) -> String {
    format!("%{}%", escape_like(value))
}

/// True when the query contains at least one alphanumeric token.
fn has_token(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|token| !token.is_empty())
}

/// Tokenizes a user query into safe FTS5 terms. Every term is quoted, so no
/// operator in the input (`AND`, `NEAR`, `*`, `-`, `:`) can reach the query
/// planner, and no input can be interpreted as syntax.
///
/// Terms are combined with `OR`: retrieval answers "what might be relevant to
/// this input", and bm25 ranks records that match more (or rarer) terms above
/// records that match one. `AND` semantics would return nothing the moment the
/// caller paraphrases, which makes memory useless exactly when it is needed.
fn fts_match_expression(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn collect_hits<I>(rows: I, text: &str) -> RowResult<Vec<MemoryHit>>
where
    I: Iterator<Item = rusqlite::Result<(MemoryRecord, f64)>>,
{
    let mut items = Vec::new();
    for row in rows {
        let (record, score) =
            row.map_err(|e| MemoryError::Database(format!("read search row: {e}")))?;
        items.push(MemoryHit {
            snippet: snippet(&record, text),
            record,
            score,
        });
    }
    Ok(items)
}

/// Excerpt around the first matching term, or the headline when there is no text.
fn snippet(record: &MemoryRecord, text: &str) -> String {
    const WINDOW: usize = 160;
    if text.is_empty() {
        return record.headline(WINDOW);
    }
    let lower = record.content.to_lowercase();
    let anchor = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.chars().count() > 2)
        .find_map(|token| lower.find(&token.to_lowercase()));

    let raw_start = anchor.unwrap_or(0);
    let start = lower[..raw_start]
        .char_indices()
        .rev()
        .take(24)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(raw_start);
    let excerpt: String = record.content[start..].chars().take(WINDOW).collect();
    let joined = excerpt.split_whitespace().collect::<Vec<_>>().join(" ");
    if start > 0 {
        format!("…{joined}")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemoryStore {
        MemoryStore::open_in_memory(MemoryPolicy::default()).expect("open")
    }

    fn draft(category: MemoryCategory, content: impl Into<String>) -> NewMemory {
        NewMemory::new(category, content)
    }

    #[test]
    fn save_and_get_round_trip() {
        let store = store();
        let mut request = draft(MemoryCategory::Project, "nanomite uses rusqlite for memory");
        request.tags = vec!["storage".into()];
        request.project = Some("nanomite".into());
        request.summary = Some("rusqlite".into());
        request.source = MemorySource::with_detail(SourceKind::UserInstruction, "user");
        let saved = store.save(&request).expect("save");
        assert!(!saved.duplicate);
        assert!(saved.record.id.starts_with("mem_"));

        let fetched = store.get(&saved.record.id).expect("get");
        assert_eq!(fetched.content, saved.record.content);
        assert_eq!(fetched.tags, vec!["storage"]);
        assert_eq!(fetched.project.as_deref(), Some("nanomite"));
        assert_eq!(fetched.source.kind, SourceKind::UserInstruction);
        assert_eq!(fetched.summary.as_deref(), Some("rusqlite"));
    }

    #[test]
    fn records_survive_a_reopen() {
        let dir = std::env::temp_dir().join(format!("nanomite-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = dir.join("memory.db");
        let id = {
            let store = MemoryStore::open(&db, MemoryPolicy::default()).expect("open");
            store
                .save(&draft(MemoryCategory::Core, "persist me"))
                .expect("save")
                .record
                .id
        };
        let reopened = MemoryStore::open(&db, MemoryPolicy::default()).expect("reopen");
        assert_eq!(reopened.get(&id).expect("get").content, "persist me");
        drop(reopened);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn identical_save_is_reported_as_duplicate() {
        let store = store();
        let first = store
            .save(&draft(MemoryCategory::Core, "always use tabs"))
            .expect("save");
        let second = store
            .save(&draft(MemoryCategory::Core, "  always   use TABS "))
            .expect("save");
        assert!(second.duplicate);
        assert_eq!(first.record.id, second.record.id);
        assert_eq!(store.stats().unwrap().total, 1);
    }

    #[test]
    fn unknown_id_is_not_found() {
        let store = store();
        assert!(matches!(
            store.get("mem_missing"),
            Err(MemoryError::NotFound(_))
        ));
        assert!(matches!(store.get("   "), Err(MemoryError::Invalid(_))));
    }

    #[test]
    fn keyword_search_finds_saved_records() {
        let store = store();
        store
            .save(&draft(MemoryCategory::Core, "the cache is warm after boot"))
            .unwrap();
        store
            .save(&draft(MemoryCategory::Core, "unrelated prose"))
            .unwrap();
        let hits = store.search(&SearchQuery::text("cache")).expect("search");
        assert_eq!(hits.total, 1, "{hits:?}");
        assert!(hits.items[0].snippet.to_lowercase().contains("cache"));
    }

    #[test]
    fn search_filters_by_category_tag_and_project() {
        let store = store();
        let mut a = draft(MemoryCategory::Decision, "use sqlite not a vector db");
        a.tags = vec!["storage".into()];
        a.project = Some("nanomite".into());
        a.importance = Some(0.9);
        store.save(&a).unwrap();
        let mut b = draft(MemoryCategory::Episodic, "fixed a flaky test");
        b.project = Some("other".into());
        store.save(&b).unwrap();

        let decisions = store
            .search(&SearchQuery::text("sqlite").with_categories(vec![MemoryCategory::Decision]))
            .unwrap();
        assert_eq!(decisions.total, 1);

        let tagged = store
            .search(&SearchQuery {
                tags: vec!["storage".into()],
                ..SearchQuery::default()
            })
            .unwrap();
        assert_eq!(tagged.total, 1);

        let scoped = store.project_memory("nanomite", 10).unwrap();
        assert_eq!(scoped.total, 1);
        assert_eq!(scoped.items[0].record.project.as_deref(), Some("nanomite"));
    }

    #[test]
    fn injection_of_sql_metacharacters_is_inert() {
        let store = store();
        store
            .save(&draft(MemoryCategory::Core, "safe content"))
            .unwrap();
        for hostile in [
            "'; DROP TABLE memories; --",
            "a\" OR 1=1 --",
            "%_%",
            "mem_1",
        ] {
            let hits = store.search(&SearchQuery::text(hostile)).expect("search");
            assert!(hits.items.is_empty(), "unexpected hit for {hostile:?}");
        }
        assert_eq!(store.stats().unwrap().total, 1, "the table survived");
    }

    #[test]
    fn result_limits_are_clamped_and_paged() {
        let store = store();
        for i in 0..60 {
            store
                .save(&draft(
                    MemoryCategory::Episodic,
                    format!("event number {i}"),
                ))
                .unwrap();
        }
        let listed = store
            .list(&ListQuery {
                limit: Some(10_000),
                ..ListQuery::default()
            })
            .unwrap();
        assert_eq!(listed.items.len(), store.policy().max_result_rows);
        assert!(listed.truncated);
        assert_eq!(listed.total, 60);

        let first = store
            .list(&ListQuery {
                limit: Some(10),
                ..ListQuery::default()
            })
            .unwrap();
        let second = store
            .list(&ListQuery {
                limit: Some(10),
                offset: 10,
                ..ListQuery::default()
            })
            .unwrap();
        assert_eq!(first.items.len(), 10);
        assert!(first.items.iter().all(|hit| second
            .items
            .iter()
            .all(|other| other.record.id != hit.record.id)));
    }

    #[test]
    fn update_changes_only_named_fields_and_bumps_revision() {
        let store = store();
        let saved = store
            .save(&draft(MemoryCategory::Core, "original text"))
            .unwrap();
        let updated = store
            .update(
                &saved.record.id,
                &MemoryPatch {
                    importance: Some(0.75),
                    tags: Some(vec!["ranked".into()]),
                    ..MemoryPatch::default()
                },
            )
            .expect("update");
        assert_eq!(updated.content, "original text");
        assert_eq!(updated.tags, vec!["ranked"]);
        assert_eq!(updated.importance, 0.75);
        assert_eq!(updated.revision, 2);
        assert_eq!(store.get(&updated.id).unwrap().revision, 2);
        assert!(store
            .update(
                "mem_nope",
                &MemoryPatch {
                    importance: Some(0.5),
                    ..MemoryPatch::default()
                }
            )
            .is_err());
    }

    #[test]
    fn delete_removes_record_and_its_search_entry() {
        let store = store();
        let saved = store
            .save(&draft(MemoryCategory::Core, "ephemeral kubernetes note"))
            .unwrap();
        store.delete(&saved.record.id).expect("delete");
        assert!(matches!(
            store.get(&saved.record.id),
            Err(MemoryError::NotFound(_))
        ));
        assert!(store
            .search(&SearchQuery::text("kubernetes"))
            .unwrap()
            .items
            .is_empty());
        assert!(
            store.delete(&saved.record.id).is_err(),
            "second delete is a clean error"
        );
    }

    #[test]
    fn bulk_delete_requires_confirmation_and_is_all_or_nothing() {
        let store = MemoryStore::open_in_memory(MemoryPolicy {
            bulk_confirm_threshold: 5,
            ..MemoryPolicy::default()
        })
        .unwrap();
        let mut ids = Vec::new();
        for i in 0..12 {
            ids.push(
                store
                    .save(&draft(MemoryCategory::Episodic, format!("bulk item {i}")))
                    .unwrap()
                    .record
                    .id,
            );
        }
        let err = store.delete_many(&ids, false).unwrap_err();
        assert!(err.to_string().contains("confirm=true"), "{err}");
        assert_eq!(store.stats().unwrap().total, 12, "nothing deleted yet");

        assert_eq!(store.delete_many(&ids, true).expect("confirmed").len(), 12);
        assert_eq!(store.stats().unwrap().total, 0);
    }

    #[test]
    fn expired_records_are_hidden_and_purgeable() {
        let store = store();
        let saved = store
            .save(&draft(MemoryCategory::Episodic, "short lived note"))
            .unwrap();
        {
            let conn = store.lock();
            conn.execute(
                "UPDATE memories SET expires_at = ?2 WHERE id = ?1",
                params![saved.record.id, now() - 5],
            )
            .unwrap();
        }
        assert!(store
            .search(&SearchQuery::text("short"))
            .unwrap()
            .items
            .is_empty());
        let with_expired = store
            .search(&SearchQuery {
                text: Some("short".into()),
                include_expired: true,
                ..SearchQuery::default()
            })
            .unwrap();
        assert_eq!(with_expired.total, 1);
        assert_eq!(store.purge_expired().unwrap().len(), 1);
        assert_eq!(store.stats().unwrap().total, 0);
    }

    #[test]
    fn consolidation_merges_exact_duplicates_only() {
        let store = store();
        store
            .save(&draft(MemoryCategory::Core, "port is 8080"))
            .unwrap();
        let mut with_tag = draft(MemoryCategory::Core, "PORT   IS 8080");
        with_tag.tags = vec!["network".into()];
        with_tag.importance = Some(0.9);
        store.save(&with_tag).unwrap();
        store
            .save(&draft(MemoryCategory::Core, "port is 9090"))
            .unwrap();

        let report = store.consolidate().expect("consolidate");
        assert_eq!(report.duplicate_groups, 1);
        assert_eq!(report.records_removed, 1);
        assert_eq!(store.stats().unwrap().total, 2);

        let hits = store
            .search(&SearchQuery {
                tags: vec!["network".into()],
                ..SearchQuery::default()
            })
            .unwrap();
        assert_eq!(hits.total, 1, "tags must survive consolidation");
        assert!((hits.items[0].record.importance - 0.9).abs() < 1e-9);
    }

    #[test]
    fn export_and_import_round_trip() {
        let source = store();
        let saved = source
            .save(&draft(
                MemoryCategory::Decision,
                "prefer rusqlite over sqlx",
            ))
            .unwrap();
        let bundle = source.export().expect("export");
        assert_eq!(bundle.memories.len(), 1);
        assert_eq!(bundle.version, ExportBundle::VERSION);

        let other = store();
        let report = other
            .import(&bundle, ImportMode::Insert, false)
            .expect("import");
        assert_eq!(report.inserted, 1);
        assert_eq!(
            other
                .get(&saved.record.id)
                .expect("get after import")
                .content,
            saved.record.content
        );

        let again = other
            .import(&bundle, ImportMode::Insert, false)
            .expect("reimport");
        assert_eq!(again.skipped, 1);
        assert_eq!(other.stats().unwrap().total, 1);
    }

    #[test]
    fn malformed_import_is_rejected_without_touching_memory() {
        let store = store();
        store.save(&draft(MemoryCategory::Core, "keeper")).unwrap();
        let saved = store.save(&draft(MemoryCategory::Core, "second")).unwrap();

        let mut bundle = store.export().unwrap();
        let mut poisoned = saved.record.clone();
        poisoned.id = "mem_poison".into();
        poisoned.content = "carries sk-abcdefghijklmnopqrstuvwxyz0123456789".into();
        bundle.memories.push(poisoned);

        let err = store
            .import(&bundle, ImportMode::Replace, true)
            .unwrap_err();
        assert!(matches!(err, MemoryError::Invalid(_)), "{err}");
        assert_eq!(store.stats().unwrap().total, 2, "nothing may be written");
        assert!(store.get("mem_poison").is_err());
    }

    #[test]
    fn out_of_range_imported_values_are_rejected() {
        let store = store();
        let mut bundle = store.export().unwrap();
        let record = MemoryRecord {
            id: "mem_bad".into(),
            category: MemoryCategory::Core,
            content: "imported with a nonsense importance".into(),
            summary: None,
            tags: Vec::new(),
            source: MemorySource::new(SourceKind::Import),
            project: None,
            created_at: now(),
            updated_at: now(),
            confidence: None,
            importance: 42.0,
            last_accessed_at: None,
            expires_at: None,
            sensitivity: Sensitivity::Internal,
            access_count: 0,
            revision: 1,
            content_hash: String::new(),
        };
        bundle.memories.push(record);
        assert!(store.import(&bundle, ImportMode::Insert, false).is_err());
    }

    #[test]
    fn import_from_a_newer_bundle_version_is_refused() {
        let store = store();
        let mut bundle = store.export().unwrap();
        bundle.version = ExportBundle::VERSION + 1;
        assert!(matches!(
            store.import(&bundle, ImportMode::Insert, true),
            Err(MemoryError::Unsupported(_))
        ));
    }

    #[test]
    fn backup_and_restore_round_trip() {
        let dir = std::env::temp_dir().join(format!("nanomite-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = dir.join("memory.db");
        let backup = dir.join("backup.db");

        let store = MemoryStore::open(&db, MemoryPolicy::default()).expect("open");
        store
            .save(&draft(MemoryCategory::Core, "before backup"))
            .unwrap();
        store.backup_to(&backup).expect("backup");
        store
            .save(&draft(MemoryCategory::Core, "after backup"))
            .unwrap();
        assert_eq!(store.stats().unwrap().total, 2);
        assert!(
            store.backup_to(&db).is_err(),
            "refuses to overwrite the live db"
        );

        store.restore_from(&backup).expect("restore");
        assert_eq!(store.stats().unwrap().total, 1, "restore must roll back");
        assert_eq!(
            store.search(&SearchQuery::text("before")).unwrap().items[0]
                .record
                .content,
            "before backup"
        );
        assert!(store.integrity_check().is_ok());
        assert!(store.restore_from(&dir.join("missing.db")).is_err());

        drop(store);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn restore_rejects_a_corrupt_backup_and_keeps_memory() {
        let dir = std::env::temp_dir().join(format!("nanomite-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store =
            MemoryStore::open(dir.join("memory.db"), MemoryPolicy::default()).expect("open");
        store
            .save(&draft(MemoryCategory::Core, "real data"))
            .unwrap();

        let junk = dir.join("junk.db");
        std::fs::write(&junk, b"this is definitely not a sqlite database").unwrap();
        assert!(store.restore_from(&junk).is_err());
        assert_eq!(store.stats().unwrap().total, 1, "memory must survive");

        drop(store);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn touch_access_records_retrieval_without_a_search_write() {
        let store = store();
        let saved = store
            .save(&draft(MemoryCategory::Core, "retrieved often"))
            .unwrap();
        assert!(store
            .get(&saved.record.id)
            .unwrap()
            .last_accessed_at
            .is_none());

        store.search(&SearchQuery::text("retrieved")).unwrap();
        assert!(
            store
                .get(&saved.record.id)
                .unwrap()
                .last_accessed_at
                .is_none(),
            "a plain search must not write"
        );

        assert_eq!(
            store
                .touch_access(std::slice::from_ref(&saved.record.id))
                .unwrap(),
            1
        );
        let after = store.get(&saved.record.id).unwrap();
        assert!(after.last_accessed_at.is_some());
        assert_eq!(after.access_count, 1);
    }

    #[test]
    fn policy_ceiling_blocks_confidential_injection() {
        let store = MemoryStore::open_in_memory(MemoryPolicy {
            inject_ceiling: Sensitivity::Public,
            ..MemoryPolicy::default()
        })
        .unwrap();
        let mut confidential = draft(MemoryCategory::Core, "salary band for engineers");
        confidential.sensitivity = Some(Sensitivity::Confidential);
        store.save(&confidential).unwrap();

        assert_eq!(store.stats().unwrap().total, 1, "still readable");
        assert!(store
            .policy()
            .authorize(MemoryOp::Inject, 1, Sensitivity::Confidential, false)
            .is_err());
    }

    #[test]
    fn read_only_policy_blocks_writes() {
        let store = MemoryStore::open_in_memory(MemoryPolicy {
            writable: false,
            ..MemoryPolicy::default()
        })
        .unwrap();
        assert!(store.save(&draft(MemoryCategory::Core, "nope")).is_err());
        store
            .policy()
            .authorize(MemoryOp::Read, 1, Sensitivity::Public, false)
            .unwrap();
    }

    #[test]
    fn migrations_are_idempotent_across_reopens() {
        let dir = std::env::temp_dir().join(format!("nanomite-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = dir.join("memory.db");
        {
            let store = MemoryStore::open(&db, MemoryPolicy::default()).expect("open");
            store
                .save(&draft(MemoryCategory::Core, "persisted across restarts"))
                .unwrap();
        }
        let store = MemoryStore::open(&db, MemoryPolicy::default()).expect("reopen");
        assert_eq!(store.stats().unwrap().total, 1);
        assert!(store
            .search(&SearchQuery::text("persisted"))
            .unwrap()
            .items
            .first()
            .map(|hit| hit.record.id.starts_with("mem_"))
            .unwrap_or(false));
        drop(store);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fts5_is_detected_and_reported_either_way() {
        let store = store();
        store
            .save(&draft(MemoryCategory::Core, "capability detection probe"))
            .unwrap();
        let hits = store.search(&SearchQuery::text("detection")).unwrap();
        let expected = if store.has_fts() {
            SearchIndex::Fts5
        } else {
            SearchIndex::Fallback
        };
        assert_eq!(
            hits.index, expected,
            "the index used must be reported honestly"
        );
        assert_eq!(store.stats().unwrap().search_index, expected);
        if store.has_fts() {
            assert!(store.rebuild_fts().unwrap() >= 1);
        } else {
            assert!(matches!(
                store.rebuild_fts(),
                Err(MemoryError::Unsupported(_))
            ));
        }
    }

    #[test]
    fn punctuation_only_queries_match_nothing() {
        let store = store();
        store
            .save(&draft(MemoryCategory::Core, "content one"))
            .unwrap();
        store
            .save(&draft(MemoryCategory::Core, "content two"))
            .unwrap();
        let hits = store.search(&SearchQuery::text("!!! ???")).unwrap();
        assert!(hits.items.is_empty(), "got {} rows", hits.items.len());
    }

    #[test]
    fn markdown_index_rows_round_trip() {
        let store = store();
        let row = MarkdownIndexRow {
            path: "projects/nanomite.md".into(),
            title: "nanomite".into(),
            content_hash: "abc".into(),
            mtime: 10,
            size: 42,
            indexed_at: 11,
            block_ids: vec!["mem_a".into(), "mem_b".into()],
        };
        store.upsert_markdown_index(&row).unwrap();
        let rows = store.markdown_index().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].block_ids, vec!["mem_a", "mem_b"]);
        store.remove_markdown_index("projects/nanomite.md").unwrap();
        assert!(store.markdown_index().unwrap().is_empty());
    }
}
