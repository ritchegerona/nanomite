//! Human-readable markdown knowledge files.
//!
//! # What lives where
//!
//! | Information | Home |
//! |---|---|
//! | Durable prose a human maintains (architecture, conventions, decisions) | these `.md` files |
//! | Structured record fields, metadata, retrieval statistics | SQLite |
//! | Working memory (episodic, session, task) | SQLite only |
//!
//! # One-way projection, not a sync loop
//!
//! A record whose category maps to a document is *mirrored* into one block of
//! that document. Updating a single memory rewrites exactly its own block; the
//! rest of the file — including anything you typed by hand — is preserved
//! byte for byte. Edits made outside nanomite are detected by `mtime`/`size` in
//! the SQLite markdown index and re-read on demand.
//!
//! Conflict rule: when a block exists both in SQLite and in markdown and the two
//! disagree, the copy with the newer `updated` timestamp wins, and the newer one
//! is written back to the *lesser* side on the next explicit save. nanomite never
//! rewrites markdown on the strength of a background read alone.

use super::model::{
    MarkdownIndexRow, MemoryCategory, MemoryError, MemoryRecord, Sensitivity, SourceKind,
};
use super::security;
use std::path::{Path, PathBuf};

/// Marker that opens a nanomite-managed block.
pub const BLOCK_MARKER: &str = "<!-- nanomite:mem ";
/// Largest markdown file the indexer will read.
pub const MAX_DOCUMENT_BYTES: u64 = 1_000_000;
/// Largest number of documents one scan touches.
pub const MAX_DOCUMENTS: usize = 500;

/// Documents that always exist, created on first run.
pub const CORE_DOCUMENT: &str = "MEMORY.md";
/// User-level knowledge document.
pub const USER_DOCUMENT: &str = "USER.md";

const HEADER: &str = "<!-- nanomite knowledge base. Safe to edit: nanomite rewrites only the\n     blocks it owns, each marked with `nanomite:mem id=...`. Everything else here is\n     yours and is never removed. -->";

const CORE_SEED: &str = "# Core memory

<!-- Stable facts and operating preferences that apply everywhere. -->

## Facts

- _nothing recorded yet_
";

const USER_SEED: &str = "# User memory

<!-- Preferences and working context you have approved. Never stores credentials. -->

## Preferences

- _nothing recorded yet_
";

const PROJECT_SEED: &str = "# Project memory

<!-- Architecture, current status, constraints and known issues. Add free prose here;
     nanomite-managed entries are appended below as individual blocks. -->

## Notes

- _nothing recorded yet_
";

const DECISION_SEED: &str = "# Decisions

<!-- Decision records: what was decided, why, what was considered, what it costs. -->
";

/// One nanomite-owned block inside a document.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownBlock {
    pub id: String,
    pub category: MemoryCategory,
    pub tags: Vec<String>,
    pub source: SourceKind,
    pub sensitivity: Sensitivity,
    pub importance: f64,
    /// Unix seconds the block claims to have been written.
    pub updated: i64,
    /// Byte offset of the marker line.
    pub start: usize,
    /// Byte offset just past the block (start of the next marker, or EOF).
    pub end: usize,
    /// Block body with the marker line removed.
    pub text: String,
}

/// Index of a document: which files exist and which blocks they carry.
#[derive(Debug, Clone, Default)]
pub struct IndexReport {
    pub scanned: usize,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub blocks: usize,
    /// Set when a document was unreadable; the scan continued past it.
    pub warnings: Vec<String>,
}

/// Filesystem view of the markdown knowledge base.
pub struct KnowledgeStore {
    /// Canonicalized memory root.
    root: PathBuf,
    policy_mirror: bool,
}

impl KnowledgeStore {
    /// Resolves (and canonicalizes) the root, then creates the layout if missing.
    ///
    /// Nothing that already exists is ever overwritten: an existing `MEMORY.md`
    /// keeps its contents, even if it was written by an older version or by hand.
    pub fn open(root: impl AsRef<Path>, mirror: bool) -> Result<Self, MemoryError> {
        let requested = root.as_ref().to_path_buf();
        security::ensure_dir(&requested)?;
        let root = requested.canonicalize().unwrap_or(requested);
        security::ensure_dir(&root)?;
        for sub in ["projects", "decisions"] {
            security::ensure_dir(&root.join(sub))?;
        }
        seed_if_absent(&root.join(CORE_DOCUMENT), CORE_SEED)?;
        seed_if_absent(&root.join(USER_DOCUMENT), USER_SEED)?;
        Ok(Self {
            root,
            policy_mirror: mirror,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn mirrors(&self) -> bool {
        self.policy_mirror
    }

    /// Relative path of the document a category mirrors into.
    ///
    /// Project-scoped categories need a project key; the high-volume working
    /// categories (episodic, session, task) are never mirrored.
    pub fn document_for(&self, category: MemoryCategory, project: Option<&str>) -> Option<String> {
        let project = || project.map(str::trim).filter(|p| !p.is_empty());
        match category {
            MemoryCategory::Core => Some(CORE_DOCUMENT.to_string()),
            MemoryCategory::User => Some(USER_DOCUMENT.to_string()),
            MemoryCategory::Project => Some(format!("projects/{}.md", project()?)),
            MemoryCategory::Decision => Some(format!("decisions/{}.md", project()?)),
            MemoryCategory::Episodic | MemoryCategory::Session | MemoryCategory::Task => None,
        }
    }

    /// Absolute path of a document, with confinement enforced.
    pub fn resolve(&self, relative: &str) -> Result<PathBuf, MemoryError> {
        security::resolve_within(&self.root, relative)
    }

    /// Every `.md` document under the root, as `(relative path, absolute path)`.
    pub fn documents(&self) -> Result<Vec<(String, PathBuf)>, MemoryError> {
        let mut out = Vec::new();
        let mut stack = vec![(self.root.clone(), String::new(), 0usize)];
        while let Some((dir, prefix, depth)) = stack.pop() {
            if out.len() >= MAX_DOCUMENTS {
                break;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let path = entry.path();
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let relative = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                // Symlinked entries are skipped: the memory root never follows links.
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    if depth < 2 {
                        stack.push((path, relative, depth + 1));
                    }
                    continue;
                }
                if name.ends_with(".md") {
                    out.push((relative, path));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.truncate(MAX_DOCUMENTS);
        Ok(out)
    }

    /// Reads a document, refusing traversal and symlinked paths.
    pub fn read(&self, relative: &str) -> Result<String, MemoryError> {
        let path = self.resolve(relative)?;
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| MemoryError::io(&format!("stat {}", relative), e))?;
        if meta.len() > MAX_DOCUMENT_BYTES {
            return Err(MemoryError::Policy(format!(
                "refused: {relative} is {} bytes, the indexing limit is {MAX_DOCUMENT_BYTES}",
                meta.len()
            )));
        }
        std::fs::read_to_string(&path).map_err(|e| MemoryError::io(relative, e))
    }

    /// Writes a document atomically. The caller must already have validated content.
    pub fn write(&self, relative: &str, text: &str) -> Result<(), MemoryError> {
        let path = self.resolve(relative)?;
        if !relative.ends_with(".md") {
            return Err(MemoryError::invalid("documents must end in .md"));
        }
        if text.len() as u64 > MAX_DOCUMENT_BYTES {
            return Err(MemoryError::invalid("document would exceed the size limit"));
        }
        security::write_atomic(&path, text.as_bytes())
    }

    /// Creates a document with a starter heading if it does not exist yet.
    /// Project and decision documents get their dedicated seeds; everything
    /// else gets a generic one.
    pub fn ensure_document(&self, relative: &str) -> Result<PathBuf, MemoryError> {
        let path = self.resolve(relative)?;
        if path.exists() {
            return Ok(path);
        }
        let seed = match () {
            _ if relative.starts_with("projects/") => PROJECT_SEED.to_string(),
            _ if relative.starts_with("decisions/") => DECISION_SEED.to_string(),
            _ => {
                let title = Path::new(relative)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Notes".to_string());
                format!("# {title}\n\n{HEADER}\n\n## Notes\n\n- _nothing recorded yet_\n")
            }
        };
        self.write(relative, &seed)?;
        Ok(path)
    }

    // -- block level ---------------------------------------------------------

    /// Adds or replaces exactly one block, leaving every other byte alone.
    pub fn upsert_block(&self, relative: &str, record: &MemoryRecord) -> Result<(), MemoryError> {
        security::assert_record_safe(record)?;
        let document = if self.resolve(relative)?.exists() {
            self.read(relative)?
        } else {
            self.ensure_document(relative)?;
            self.read(relative)?
        };
        let rendered = render_block(record);
        let blocks = parse_blocks(&document);

        let updated = match blocks.iter().find(|b| b.id == record.id) {
            Some(existing) => {
                let mut next = String::with_capacity(document.len() + rendered.len());
                next.push_str(&document[..existing.start]);
                next.push_str(&rendered);
                next.push_str(&document[existing.end..]);
                next
            }
            None => {
                let mut next = document;
                if !next.ends_with('\n') {
                    next.push('\n');
                }
                if !next.ends_with("\n\n") {
                    next.push('\n');
                }
                next.push_str(&rendered);
                next
            }
        };
        self.write(relative, &updated)
    }

    /// Removes one block. Returns whether anything was removed. Everything else
    /// in the document — including the heading and any hand-written prose — stays.
    pub fn remove_block(&self, relative: &str, id: &str) -> Result<bool, MemoryError> {
        let Ok(document) = self.read(relative) else {
            return Ok(false);
        };
        let blocks = parse_blocks(&document);
        let Some(existing) = blocks.iter().find(|b| b.id == id) else {
            return Ok(false);
        };
        let mut next = String::with_capacity(document.len());
        next.push_str(&document[..existing.start]);
        next.push_str(&document[existing.end..]);
        self.write(relative, &next)?;
        Ok(true)
    }

    /// Removes every block belonging to a category (used when a document is reset).
    pub fn remove_category_blocks(
        &self,
        relative: &str,
        category: MemoryCategory,
    ) -> Result<usize, MemoryError> {
        let Ok(document) = self.read(relative) else {
            return Ok(0);
        };
        let blocks = parse_blocks(&document);
        let doomed: Vec<&MarkdownBlock> =
            blocks.iter().filter(|b| b.category == category).collect();
        if doomed.is_empty() {
            return Ok(0);
        }
        let mut next = String::new();
        let mut cursor = 0usize;
        for block in &doomed {
            next.push_str(&document[cursor..block.start]);
            cursor = block.end;
        }
        next.push_str(&document[cursor..]);
        self.write(relative, &next)?;
        Ok(doomed.len())
    }

    pub fn blocks(&self, relative: &str) -> Result<Vec<MarkdownBlock>, MemoryError> {
        Ok(parse_blocks(&self.read(relative)?))
    }

    // -- indexing ------------------------------------------------------------

    /// One index row per document.
    pub fn index_documents(&self) -> Result<(Vec<MarkdownIndexRow>, IndexReport), MemoryError> {
        let mut rows = Vec::new();
        let mut report = IndexReport::default();
        for (relative, path) in self.documents()? {
            report.scanned += 1;
            let Ok(meta) = std::fs::metadata(&path) else {
                report.warnings.push(format!("{relative}: unreadable"));
                continue;
            };
            let body = match std::fs::read_to_string(&path) {
                Ok(body) => body,
                Err(e) => {
                    report.warnings.push(format!("{relative}: {e}"));
                    continue;
                }
            };
            let blocks = parse_blocks(&body);
            report.blocks += blocks.len();
            rows.push(MarkdownIndexRow {
                path: relative,
                title: document_title(&body, &path),
                content_hash: security::content_hash(&body),
                mtime: mtime_secs(&meta),
                size: meta.len(),
                indexed_at: super::model::now(),
                block_ids: blocks.into_iter().map(|b| b.id).collect(),
            });
        }
        Ok((rows, report))
    }

    /// Documents that need reindexing: the union of
    ///
    /// * indexed documents whose `mtime`/`size` moved on disk (edited outside
    ///   nanomite), and
    /// * documents on disk that the index has never seen (first run, or a file
    ///   added by hand).
    ///
    /// Only directory entries and stat metadata are read - no file contents - so
    /// the probe stays cheap enough for the retrieval hot path.
    pub fn stale_documents(
        &self,
        indexed: &[MarkdownIndexRow],
    ) -> Result<Vec<String>, MemoryError> {
        let mut stale = Vec::new();
        for (relative, _) in self.documents()? {
            if !indexed.iter().any(|row| row.path == relative) {
                stale.push(relative);
            }
        }
        for row in indexed {
            let path = self.resolve(&row.path)?;
            let Ok(meta) = std::fs::metadata(&path) else {
                stale.push(row.path.clone());
                continue;
            };
            if mtime_secs(&meta) != row.mtime || meta.len() != row.size {
                stale.push(row.path.clone());
            }
        }
        Ok(stale)
    }

    /// Bounded keyword scan over the indexed documents. Used by `memory_search`
    /// to answer "what did I write in the markdown files?" without adding a second
    /// authoritative store.
    pub fn search_text(
        &self,
        query: &str,
        max_results: usize,
        max_files: usize,
    ) -> Result<Vec<(String, String, String)>, MemoryError> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() || max_results == 0 {
            return Ok(Vec::new());
        }
        let mut hits = Vec::new();
        for (relative, _) in self.documents()?.into_iter().take(max_files.max(1)) {
            let Ok(body) = self.read(&relative) else {
                continue;
            };
            let lower = body.to_lowercase();
            // Parsed once per document: block membership does not change mid-scan.
            let blocks = parse_blocks(&body);
            // One hit per block (a block is one memory; its title and body often
            // both contain the term) and one per prose line outside blocks.
            let mut seen: std::collections::HashSet<(String, String, usize)> =
                std::collections::HashSet::new();
            let mut cursor = 0usize;
            while let Some(rel) = lower[cursor..].find(&needle) {
                // `find` reports an offset into the *slice*; convert to an
                // absolute offset before slicing or advancing.
                let pos = cursor + rel;
                let block = blocks
                    .iter()
                    .find(|b| pos >= b.start && pos < b.end)
                    .map(|b| b.id.clone());
                let line = body[..pos].matches('\n').count() + 1;
                let dedupe_key = (
                    relative.clone(),
                    block.clone().unwrap_or_default(),
                    if block.is_some() { 0 } else { line },
                );
                if !seen.insert(dedupe_key) {
                    cursor = pos + needle.len();
                    continue;
                }
                let start = body[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let end = body[pos..]
                    .find('\n')
                    .map(|i| pos + i)
                    .unwrap_or(body.len());
                hits.push((
                    relative.clone(),
                    block.unwrap_or_default(),
                    format!(
                        "{}:{} {}",
                        relative,
                        line,
                        security::redact(body[start..end].trim())
                    ),
                ));
                if hits.len() >= max_results {
                    return Ok(hits);
                }
                cursor = pos + needle.len();
            }
        }
        Ok(hits)
    }
}

/// Splits a document into its nanomite-owned blocks plus surrounding user prose.
pub fn parse_blocks(document: &str) -> Vec<MarkdownBlock> {
    let mut blocks: Vec<MarkdownBlock> = Vec::new();
    let mut offset = 0usize;
    let mut current: Option<MarkdownBlock> = None;
    let mut body = String::new();

    for line in document.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if let Some(rest) = trimmed.trim().strip_prefix(BLOCK_MARKER) {
            if let Some(block) = current.take() {
                blocks.push(MarkdownBlock {
                    end: offset,
                    text: body.trim_end().to_string(),
                    ..block
                });
                body.clear();
            }
            current = parse_marker(rest.trim_end_matches("-->").trim());
            if let Some(block) = current.as_mut() {
                block.start = offset;
            }
            offset += line.len();
            continue;
        }
        if current.is_some() {
            body.push_str(line);
        }
        offset += line.len();
    }
    if let Some(block) = current.take() {
        blocks.push(MarkdownBlock {
            end: offset,
            text: body.trim_end().to_string(),
            ..block
        });
    }
    blocks
}

fn parse_marker(payload: &str) -> Option<MarkdownBlock> {
    let mut id = None;
    let mut category = MemoryCategory::Core;
    let mut tags = Vec::new();
    let mut source = SourceKind::Unknown;
    let mut sensitivity = Sensitivity::Internal;
    let mut importance = 0.5f64;
    let mut updated = 0i64;

    for field in payload.split_whitespace() {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        let value = value.trim_matches('"');
        match key {
            "id" => id = Some(value.to_string()),
            "category" => category = MemoryCategory::parse(value).unwrap_or(MemoryCategory::Core),
            "tags" => tags = decode_csv(value),
            "source" => source = SourceKind::parse(value).unwrap_or(SourceKind::Unknown),
            "sensitivity" => sensitivity = Sensitivity::parse(value).unwrap_or_default(),
            "importance" => importance = value.parse().unwrap_or(0.5),
            "updated" => updated = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    let id = id?;
    Some(MarkdownBlock {
        id,
        category,
        tags,
        source,
        sensitivity,
        importance,
        updated,
        start: 0,
        end: 0,
        text: String::new(),
    })
}

/// Renders one record as a self-describing block.
pub fn render_block(record: &MemoryRecord) -> String {
    let title = record
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| record.headline(72));
    let tags = if record.tags.is_empty() {
        "-".to_string()
    } else {
        record.tags.join(",")
    };
    let body = record.content.trim();
    format!(
        "{BLOCK_MARKER}id={} category={} tags={} source={} importance={:.2} sensitivity={} updated={} -->\n\
         ### {title}\n\n{body}\n\n",
        record.id,
        record.category,
        tags.replace(',', "+"),
        record.source.kind,
        record.importance,
        record.sensitivity,
        record.updated_at,
    )
}

fn document_title(body: &str, path: &Path) -> String {
    body.lines()
        .find_map(|line| line.strip_prefix("# ").map(|t| t.trim().to_string()))
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "untitled".to_string())
        })
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn decode_csv(raw: &str) -> Vec<String> {
    if raw == "-" {
        return Vec::new();
    }
    raw.split([',', '+'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Creates a document only when it is missing. Never overwrites user content.
fn seed_if_absent(path: &Path, seed: &str) -> Result<(), MemoryError> {
    if path.exists() {
        return Ok(());
    }
    security::write_atomic(path, seed.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::model::MemorySource;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nanomite-md-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn record(id: &str, content: &str, summary: Option<&str>) -> MemoryRecord {
        MemoryRecord {
            id: id.into(),
            category: MemoryCategory::Core,
            content: content.into(),
            summary: summary.map(|s| s.into()),
            tags: vec!["alpha".into(), "beta".into()],
            source: MemorySource::new(SourceKind::UserInstruction),
            project: None,
            created_at: 1,
            updated_at: 1_700_000_000,
            confidence: Some(1.0),
            importance: 0.8,
            last_accessed_at: None,
            expires_at: None,
            sensitivity: Sensitivity::Internal,
            access_count: 0,
            revision: 1,
            content_hash: "hash".into(),
        }
    }

    #[test]
    fn first_run_seeds_documents_without_overwriting() {
        let root = temp_root("seed");
        let store = KnowledgeStore::open(&root, true).expect("open");
        assert!(root.join(CORE_DOCUMENT).exists());
        assert!(root.join(USER_DOCUMENT).exists());
        assert!(root.join("projects").exists());
        assert!(root.join("decisions").exists());

        // A hand-edited document survives a second open.
        std::fs::write(root.join(CORE_DOCUMENT), "# my own file\n\nkeep me\n").unwrap();
        let again = KnowledgeStore::open(&root, true).expect("reopen");
        again
            .upsert_block(CORE_DOCUMENT, &record("mem_a", "a fact", None))
            .unwrap();
        let body = again.read(CORE_DOCUMENT).unwrap();
        assert!(
            body.contains("keep me"),
            "user prose must be preserved:\n{body}"
        );
        drop(store);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn upsert_replaces_only_its_own_block() {
        let root = temp_root("upsert");
        let store = KnowledgeStore::open(&root, true).expect("open");
        store
            .upsert_block(CORE_DOCUMENT, &record("mem_a", "first fact", None))
            .unwrap();
        store
            .upsert_block(CORE_DOCUMENT, &record("mem_b", "second fact", None))
            .unwrap();

        let mut updated = record("mem_a", "revised fact", Some("Revised"));
        updated.updated_at = 1_700_000_100;
        store.upsert_block(CORE_DOCUMENT, &updated).unwrap();

        let body = store.read(CORE_DOCUMENT).unwrap();
        assert_eq!(body.matches("nanomite:mem").count(), 2, "{body}");
        assert!(body.contains("revised fact"));
        assert!(!body.contains("first fact"));
        assert!(body.contains("second fact"), "sibling block survived");
        assert!(body.contains("### Revised"));

        let blocks = store.blocks(CORE_DOCUMENT).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].id, "mem_a");
        assert_eq!(blocks[0].tags, vec!["alpha", "beta"]);
        assert_eq!(blocks[0].source, SourceKind::UserInstruction);
        assert_eq!(blocks[1].id, "mem_b");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remove_block_keeps_hand_written_sections() {
        let root = temp_root("remove");
        let store = KnowledgeStore::open(&root, true).expect("open");
        store
            .upsert_block(CORE_DOCUMENT, &record("mem_a", "doomed", None))
            .unwrap();
        store
            .upsert_block(CORE_DOCUMENT, &record("mem_b", "kept", None))
            .unwrap();

        assert!(store.remove_block(CORE_DOCUMENT, "mem_a").expect("remove"));
        assert!(!store
            .remove_block(CORE_DOCUMENT, "mem_a")
            .expect("idempotent"));
        let body = store.read(CORE_DOCUMENT).unwrap();
        assert!(!body.contains("doomed"));
        assert!(body.contains("kept"));
        assert!(body.contains("## Facts"), "heading preserved:\n{body}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn blocks_round_trip_every_metadata_field() {
        let mut rec = record("mem_meta", "content", Some("Title"));
        rec.category = MemoryCategory::Decision;
        rec.importance = 0.42;
        rec.sensitivity = Sensitivity::Confidential;
        rec.source = MemorySource::with_detail(SourceKind::TaskCompletion, "run 7");
        let rendered = render_block(&rec);
        let parsed = parse_blocks(&rendered);
        assert_eq!(parsed.len(), 1);
        let block = &parsed[0];
        assert_eq!(block.id, "mem_meta");
        assert_eq!(block.category, MemoryCategory::Decision);
        assert_eq!(block.sensitivity, Sensitivity::Confidential);
        assert_eq!(block.source, SourceKind::TaskCompletion);
        assert!((block.importance - 0.42).abs() < 0.01);
        assert!(block.text.contains("content"));
    }

    #[test]
    fn read_refuses_traversal_and_symlinks() {
        let root = temp_root("escape");
        let store = KnowledgeStore::open(&root, true).expect("open");
        assert!(store.read("../../etc/passwd").is_err());
        assert!(store.read("/etc/passwd").is_err());
        assert!(store.resolve("projects/../../escape.md").is_err());

        #[cfg(unix)]
        {
            let outside = root.parent().unwrap().join("nanomite-outside.md");
            std::fs::write(&outside, "secret").unwrap();
            let link = root.join("link.md");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(
                store.read("link.md").is_err(),
                "symlink escape must be refused"
            );
            // Linked documents are skipped by the scanner as well.
            assert!(store
                .documents()
                .unwrap()
                .iter()
                .all(|(rel, _)| rel != "link.md"));
            std::fs::remove_file(&outside).ok();
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn writes_refuse_credentials() {
        let root = temp_root("secret");
        let store = KnowledgeStore::open(&root, true).expect("open");
        let bad = record(
            "mem_secret",
            "token is sk-abcdefghijklmnopqrstuvwxyz0123",
            None,
        );
        assert!(store.upsert_block(CORE_DOCUMENT, &bad).is_err());
        let body = store.read(CORE_DOCUMENT).unwrap();
        assert!(!body.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn stale_documents_detects_external_edits() {
        let root = temp_root("stale");
        let store = KnowledgeStore::open(&root, true).expect("open");
        let (rows, report) = store.index_documents().expect("index");
        assert_eq!(report.scanned, 2);
        assert!(rows.iter().any(|r| r.path == CORE_DOCUMENT));
        assert!(store.stale_documents(&rows).unwrap().is_empty());

        std::thread::sleep(std::time::Duration::from_millis(1_100));
        std::fs::write(
            root.join(CORE_DOCUMENT),
            "# Core memory\n\n## Facts\n\n- edited by hand\n",
        )
        .unwrap();
        let stale = store.stale_documents(&rows).unwrap();
        assert_eq!(stale, vec![CORE_DOCUMENT.to_string()]);

        let (refreshed, _) = store.index_documents().expect("reindex");
        assert!(store.stale_documents(&refreshed).unwrap().is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn search_text_is_bounded_and_redacts() {
        let root = temp_root("search");
        let store = KnowledgeStore::open(&root, true).expect("open");
        store
            .upsert_block(
                CORE_DOCUMENT,
                &record("mem_a", "deployment uses blue-green", None),
            )
            .unwrap();
        store
            .upsert_block(USER_DOCUMENT, &record("mem_b", "prefers dark mode", None))
            .unwrap();

        let hits = store.search_text("deployment", 5, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, CORE_DOCUMENT);
        assert_eq!(hits[0].1, "mem_a");
        assert!(store.search_text("deployment", 0, 10).unwrap().is_empty());
        assert!(store
            .search_text("nothing matches", 5, 10)
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn document_for_maps_categories_to_paths() {
        let root = temp_root("mapping");
        let store = KnowledgeStore::open(&root, true).expect("open");
        assert_eq!(
            store.document_for(MemoryCategory::Core, None).as_deref(),
            Some(CORE_DOCUMENT)
        );
        assert_eq!(
            store.document_for(MemoryCategory::User, None).as_deref(),
            Some(USER_DOCUMENT)
        );
        assert_eq!(
            store
                .document_for(MemoryCategory::Project, Some("nanomite"))
                .as_deref(),
            Some("projects/nanomite.md")
        );
        assert_eq!(
            store
                .document_for(MemoryCategory::Decision, Some("nanomite"))
                .as_deref(),
            Some("decisions/nanomite.md")
        );
        assert_eq!(
            store.document_for(MemoryCategory::Episodic, Some("x")),
            None
        );
        assert!(!MemoryCategory::Episodic.mirrored());
        std::fs::remove_dir_all(&root).ok();
    }
}
