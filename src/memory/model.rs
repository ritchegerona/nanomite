//! Typed memory records: the model-independent data layer.
//!
//! Nothing in this module knows about SQLite, the filesystem or any AI provider.
//! Records are plain serde data, so they survive a change of provider or model.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Longest accepted memory body (characters, not bytes).
pub const MAX_CONTENT_CHARS: usize = 8_000;
/// Longest accepted short summary.
pub const MAX_SUMMARY_CHARS: usize = 400;
/// Most tags one record may carry.
pub const MAX_TAGS: usize = 16;
/// Longest single tag.
pub const MAX_TAG_CHARS: usize = 48;
/// Longest free-text provenance detail.
pub const MAX_SOURCE_DETAIL_CHARS: usize = 200;
/// Longest project key after slugification.
pub const MAX_PROJECT_CHARS: usize = 64;

/// Every persisted error class. Kept small and serializable so tools can
/// return actionable, structured failures instead of opaque strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    /// Input failed validation. Carries a human-readable reason.
    Invalid(String),
    /// No record (or file) matched the requested id/path.
    NotFound(String),
    /// The payload looked like a credential; storing it is refused.
    Sensitive(String),
    /// Rejected by the memory policy layer (authorization / limits / retention).
    Policy(String),
    /// Underlying database failure.
    Database(String),
    /// Filesystem failure.
    Io(String),
    /// The database or an index is structurally damaged.
    Corrupt(String),
    /// A required optional capability (e.g. FTS5) is unavailable.
    Unsupported(String),
}

impl MemoryError {
    /// Stable machine-readable code, echoed to the model in tool payloads.
    pub fn code(&self) -> &'static str {
        match self {
            MemoryError::Invalid(_) => "invalid",
            MemoryError::NotFound(_) => "not_found",
            MemoryError::Sensitive(_) => "sensitive",
            MemoryError::Policy(_) => "policy",
            MemoryError::Database(_) => "database",
            MemoryError::Io(_) => "io",
            MemoryError::Corrupt(_) => "corrupt",
            MemoryError::Unsupported(_) => "unsupported",
        }
    }

    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        MemoryError::Invalid(msg.into())
    }

    pub(crate) fn policy(msg: impl Into<String>) -> Self {
        MemoryError::Policy(msg.into())
    }

    pub(crate) fn io(context: &str, e: std::io::Error) -> Self {
        MemoryError::Io(format!("{context}: {e}"))
    }
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (label, detail) = match self {
            MemoryError::Invalid(d) => ("invalid memory request", d),
            MemoryError::NotFound(d) => ("memory not found", d),
            MemoryError::Sensitive(d) => ("refused: sensitive data", d),
            MemoryError::Policy(d) => ("refused by memory policy", d),
            MemoryError::Database(d) => ("memory database error", d),
            MemoryError::Io(d) => ("memory storage error", d),
            MemoryError::Corrupt(d) => ("memory store is corrupt", d),
            MemoryError::Unsupported(d) => ("unsupported", d),
        };
        write!(f, "{label}: {detail}")
    }
}

impl std::error::Error for MemoryError {}

impl From<rusqlite::Error> for MemoryError {
    fn from(e: rusqlite::Error) -> Self {
        // `SQLITE_CORRUPT` / `NOTADB` are reported apart from ordinary failures so the
        // caller can trigger the documented recovery path.
        match e {
            rusqlite::Error::SqliteFailure(ref err, _)
                if matches!(
                    err.code,
                    rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
                ) =>
            {
                MemoryError::Corrupt(e.to_string())
            }
            other => MemoryError::Database(other.to_string()),
        }
    }
}

/// Where a memory came from. Provenance never grants authority: a record with
/// `source.kind = Agent` is a model-generated claim until a human says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// The user stated it explicitly (typed instruction, directive marker).
    UserInstruction,
    /// A model proposed it; unverified.
    Agent,
    /// Derived from a file in the workspace (e.g. AGENTS.md).
    ProjectFile,
    /// Written when a task finished.
    TaskCompletion,
    /// Derived from a stored session.
    Session,
    /// Came in through `import_memories`.
    Import,
    /// Unknown provenance.
    Unknown,
}

impl SourceKind {
    pub const ALL: [SourceKind; 7] = [
        SourceKind::UserInstruction,
        SourceKind::Agent,
        SourceKind::ProjectFile,
        SourceKind::TaskCompletion,
        SourceKind::Session,
        SourceKind::Import,
        SourceKind::Unknown,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::UserInstruction => "user_instruction",
            SourceKind::Agent => "agent",
            SourceKind::ProjectFile => "project_file",
            SourceKind::TaskCompletion => "task_completion",
            SourceKind::Session => "session",
            SourceKind::Import => "import",
            SourceKind::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str().eq_ignore_ascii_case(value.trim()))
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Provenance: what kind of source, plus optional human-readable detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySource {
    pub kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl MemorySource {
    pub fn new(kind: SourceKind) -> Self {
        Self { kind, detail: None }
    }

    pub fn with_detail(kind: SourceKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: Some(detail.into()),
        }
    }

    /// Compact `kind` or `kind (detail)` form used in markdown and tool summaries.
    pub fn label(&self) -> String {
        match &self.detail {
            Some(d) if !d.trim().is_empty() => format!("{} ({})", self.kind, d.trim()),
            _ => self.kind.to_string(),
        }
    }
}

impl Default for MemorySource {
    fn default() -> Self {
        Self::new(SourceKind::Unknown)
    }
}

/// The memory taxonomy. Deliberately fixed: an unknown category is rejected at
/// the API boundary rather than silently widening the taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryCategory {
    /// Stable facts and operating preferences that outlive any task.
    Core,
    /// User-approved preferences and working context.
    User,
    /// Architecture, status, decisions and constraints of one project.
    Project,
    /// Past tasks, outcomes, events and lessons learned.
    Episodic,
    /// A decision with rationale, alternatives and consequences.
    Decision,
    /// Persistent session metadata pointing at stored transcripts.
    Session,
    /// Objectives, progress, blockers, checkpoints and results of one task.
    Task,
}

impl MemoryCategory {
    pub const ALL: [MemoryCategory; 7] = [
        MemoryCategory::Core,
        MemoryCategory::User,
        MemoryCategory::Project,
        MemoryCategory::Episodic,
        MemoryCategory::Decision,
        MemoryCategory::Session,
        MemoryCategory::Task,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MemoryCategory::Core => "core",
            MemoryCategory::User => "user",
            MemoryCategory::Project => "project",
            MemoryCategory::Episodic => "episodic",
            MemoryCategory::Decision => "decision",
            MemoryCategory::Session => "session",
            MemoryCategory::Task => "task",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(value.trim()))
    }

    /// The markdown document a category mirrors into, relative to the memory root.
    pub fn document(self) -> Option<&'static str> {
        match self {
            MemoryCategory::Core => Some("MEMORY.md"),
            MemoryCategory::User => Some("USER.md"),
            MemoryCategory::Project => Some("projects/{project}.md"),
            MemoryCategory::Decision => Some("decisions/{project}.md"),
            // Episodic/session/task are high-volume working memory: SQLite only.
            MemoryCategory::Episodic | MemoryCategory::Session | MemoryCategory::Task => None,
        }
    }

    /// True when the category is mirrored into a human-editable markdown document.
    pub fn mirrored(self) -> bool {
        self.document().is_some()
    }

    pub fn parse_list(value: &str) -> Result<Vec<Self>, MemoryError> {
        let mut out = Vec::new();
        for part in value.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let category = Self::parse(part).ok_or_else(|| {
                MemoryError::invalid(format!(
                    "unknown category {part:?} (expected one of: {})",
                    Self::names().join(", ")
                ))
            })?;
            if !out.contains(&category) {
                out.push(category);
            }
        }
        Ok(out)
    }

    pub fn names() -> Vec<&'static str> {
        Self::ALL.iter().map(|c| c.as_str()).collect()
    }
}

impl Default for MemoryCategory {
    /// Mirrors `#[serde(default)]`: an omitted category is a core memory.
    fn default() -> Self {
        MemoryCategory::Core
    }
}

impl fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Classification used for access and retention controls. Ordering is by rank,
/// so a caller can ask for "everything up to internal".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Sensitivity {
    /// Safe to show anywhere, including in shared exports.
    Public,
    /// Ordinary working knowledge.
    #[default]
    Internal,
    /// Sensitive: withheld from prompt injection unless explicitly requested.
    Confidential,
    /// Highest classification. Never written by the agent; import/CLI only.
    Secret,
}

impl Sensitivity {
    pub const ALL: [Sensitivity; 4] = [
        Sensitivity::Public,
        Sensitivity::Internal,
        Sensitivity::Confidential,
        Sensitivity::Secret,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Sensitivity::Public => "public",
            Sensitivity::Internal => "internal",
            Sensitivity::Confidential => "confidential",
            Sensitivity::Secret => "secret",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|s| s.as_str().eq_ignore_ascii_case(value.trim()))
    }

    pub fn rank(self) -> u8 {
        match self {
            Sensitivity::Public => 0,
            Sensitivity::Internal => 1,
            Sensitivity::Confidential => 2,
            Sensitivity::Secret => 3,
        }
    }

    /// True when `self` is at or below `ceiling`.
    pub fn within(self, ceiling: Sensitivity) -> bool {
        self.rank() <= ceiling.rank()
    }
}

impl fmt::Display for Sensitivity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stored memory record. The schema is documented in `docs`: every column has
/// one job and nothing provider-specific is stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryRecord {
    /// Stable unique identifier (`mem_<16 hex>`).
    pub id: String,
    pub category: MemoryCategory,
    pub content: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source: MemorySource,
    #[serde(default)]
    pub project: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds.
    pub updated_at: i64,
    /// Documented semantics: how much the *source* vouches for the claim.
    /// `None` means "not stated"; `1.0` means "explicitly instructed by the user".
    #[serde(default)]
    pub confidence: Option<f64>,
    /// Retrieval priority in `0.0..=1.0`. Defaults to `0.5`.
    pub importance: f64,
    #[serde(default)]
    pub last_accessed_at: Option<i64>,
    /// Unix seconds; `None` means "keep until deleted".
    #[serde(default)]
    pub expires_at: Option<i64>,
    pub sensitivity: Sensitivity,
    /// How many times retrieval returned this record.
    #[serde(default)]
    pub access_count: i64,
    /// Bumped on every mutation; used for optimistic concurrency in exports.
    #[serde(default)]
    pub revision: i64,
    /// Digest of the normalized content, used for duplicate detection.
    pub content_hash: String,
}

impl MemoryRecord {
    /// True when the record has an `expires_at` in the past.
    pub fn is_expired(&self, now: i64) -> bool {
        self.expires_at.is_some_and(|at| at <= now)
    }

    /// One-line form for tool payloads and UI rows.
    pub fn headline(&self, max_chars: usize) -> String {
        let base = self
            .summary
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(self.content.trim());
        let flat: String = base.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.chars().count() <= max_chars {
            flat
        } else {
            format!(
                "{}…",
                flat.chars()
                    .take(max_chars.saturating_sub(1))
                    .collect::<String>()
            )
        }
    }

    pub fn tags_display(&self) -> String {
        if self.tags.is_empty() {
            "-".to_string()
        } else {
            self.tags.join(", ")
        }
    }
}

/// Everything needed to create a record. Validation happens once, in
/// [`NewMemory::validate`], before anything touches the database or disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMemory {
    #[serde(default = "default_category")]
    pub category: MemoryCategory,
    pub content: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: MemorySource,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub importance: Option<f64>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub sensitivity: Option<Sensitivity>,
    /// When false the record is stored in SQLite only and not mirrored to markdown.
    #[serde(default = "yes")]
    pub mirror_markdown: bool,
}

fn default_category() -> MemoryCategory {
    MemoryCategory::Core
}

fn yes() -> bool {
    true
}

/// A create request defaults to being mirrored into markdown: the human-readable
/// projection is the point of the categories that map to a document. Callers opt
/// out per record (`mirror_markdown: false`) for high-volume working memory.
impl Default for NewMemory {
    fn default() -> Self {
        Self {
            category: MemoryCategory::Core,
            content: String::new(),
            summary: None,
            tags: Vec::new(),
            source: MemorySource::default(),
            project: None,
            confidence: None,
            importance: None,
            expires_at: None,
            sensitivity: None,
            mirror_markdown: true,
        }
    }
}

impl NewMemory {
    pub fn new(category: MemoryCategory, content: impl Into<String>) -> Self {
        Self {
            category,
            content: content.into(),
            ..Self::default()
        }
    }

    /// Normalizes and checks every field. Returns the cleaned record or the first
    /// reason the request is unusable.
    pub fn validate(&self, now: i64) -> Result<ValidatedMemory, MemoryError> {
        let content = self.content.trim().to_string();
        if content.is_empty() {
            return Err(MemoryError::invalid("content must not be empty"));
        }
        if content.chars().count() > MAX_CONTENT_CHARS {
            return Err(MemoryError::invalid(format!(
                "content is {} characters, the maximum is {MAX_CONTENT_CHARS}",
                content.chars().count()
            )));
        }

        let summary = match self.summary.as_deref().map(str::trim) {
            Some(s) if !s.is_empty() => {
                if s.chars().count() > MAX_SUMMARY_CHARS {
                    return Err(MemoryError::invalid(format!(
                        "summary is {} characters, the maximum is {MAX_SUMMARY_CHARS}",
                        s.chars().count()
                    )));
                }
                Some(s.to_string())
            }
            _ => None,
        };

        let tags = normalize_tags(&self.tags)?;
        let source = normalize_source(&self.source)?;
        let project = normalize_project(self.project.as_deref())?;

        if let Some(confidence) = self.confidence {
            if !(0.0..=1.0).contains(&confidence) || confidence.is_nan() {
                return Err(MemoryError::invalid("confidence must be within 0.0..=1.0"));
            }
        }
        let importance = self.importance.unwrap_or(0.5);
        if !(0.0..=1.0).contains(&importance) || importance.is_nan() {
            return Err(MemoryError::invalid("importance must be within 0.0..=1.0"));
        }
        if let Some(expires) = self.expires_at {
            if expires <= now {
                return Err(MemoryError::invalid("expires_at must be in the future"));
            }
        }

        Ok(ValidatedMemory {
            category: self.category,
            content,
            summary,
            tags,
            source,
            project,
            confidence: self.confidence,
            importance,
            expires_at: self.expires_at,
            sensitivity: self.sensitivity.unwrap_or_default(),
            mirror_markdown: self.mirror_markdown,
            now,
        })
    }
}

/// A validated, normalized create request. Cheap to clone, safe to persist.
#[derive(Debug, Clone)]
pub struct ValidatedMemory {
    pub category: MemoryCategory,
    pub content: String,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub source: MemorySource,
    pub project: Option<String>,
    pub confidence: Option<f64>,
    pub importance: f64,
    pub expires_at: Option<i64>,
    pub sensitivity: Sensitivity,
    pub mirror_markdown: bool,
    pub now: i64,
}

/// Sparse update. `None` means "leave alone", so an update never blanks a field
/// by accident.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryPatch {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub category: Option<MemoryCategory>,
    #[serde(default)]
    pub project: Option<Option<String>>,
    #[serde(default)]
    pub importance: Option<f64>,
    #[serde(default)]
    pub confidence: Option<Option<f64>>,
    #[serde(default)]
    pub expires_at: Option<Option<i64>>,
    #[serde(default)]
    pub sensitivity: Option<Sensitivity>,
    #[serde(default)]
    pub source: Option<MemorySource>,
}

impl MemoryPatch {
    pub fn is_empty(&self) -> bool {
        self.content.is_none()
            && self.summary.is_none()
            && self.tags.is_none()
            && self.category.is_none()
            && self.project.is_none()
            && self.importance.is_none()
            && self.confidence.is_none()
            && self.expires_at.is_none()
            && self.sensitivity.is_none()
            && self.source.is_none()
    }

    /// Applies the patch to a copy of `record`, enforcing the same limits as a create.
    pub fn apply(&self, record: &MemoryRecord, now: i64) -> Result<MemoryRecord, MemoryError> {
        if self.is_empty() {
            return Err(MemoryError::invalid("update changes nothing"));
        }
        let mut next = record.clone();

        if let Some(content) = &self.content {
            let content = content.trim().to_string();
            if content.is_empty() {
                return Err(MemoryError::invalid("content must not be empty"));
            }
            if content.chars().count() > MAX_CONTENT_CHARS {
                return Err(MemoryError::invalid(format!(
                    "content is {} characters, the maximum is {MAX_CONTENT_CHARS}",
                    content.chars().count()
                )));
            }
            next.content = content;
        }
        if let Some(summary) = &self.summary {
            let summary = summary.trim();
            if summary.chars().count() > MAX_SUMMARY_CHARS {
                return Err(MemoryError::invalid(format!(
                    "summary is {} characters, the maximum is {MAX_SUMMARY_CHARS}",
                    summary.chars().count()
                )));
            }
            next.summary = (!summary.is_empty()).then(|| summary.to_string());
        }
        if let Some(tags) = &self.tags {
            next.tags = normalize_tags(tags)?;
        }
        if let Some(category) = self.category {
            next.category = category;
        }
        if let Some(project) = &self.project {
            next.project = normalize_project(project.as_deref())?;
        }
        if let Some(importance) = self.importance {
            if !(0.0..=1.0).contains(&importance) || importance.is_nan() {
                return Err(MemoryError::invalid("importance must be within 0.0..=1.0"));
            }
            next.importance = importance;
        }
        if let Some(confidence) = self.confidence {
            if let Some(value) = confidence {
                if !(0.0..=1.0).contains(&value) || value.is_nan() {
                    return Err(MemoryError::invalid("confidence must be within 0.0..=1.0"));
                }
            }
            next.confidence = confidence;
        }
        if let Some(expires) = self.expires_at {
            if let Some(at) = expires {
                if at <= now {
                    return Err(MemoryError::invalid("expires_at must be in the future"));
                }
            }
            next.expires_at = expires;
        }
        if let Some(sensitivity) = self.sensitivity {
            next.sensitivity = sensitivity;
        }
        if let Some(source) = &self.source {
            next.source = normalize_source(source)?;
        }

        next.updated_at = now;
        next.revision += 1;
        next.content_hash = super::security::content_hash(&next.content);
        Ok(next)
    }
}

/// Result ordering for search and list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchOrder {
    /// Lexical score, then importance, then recency.
    #[default]
    Relevance,
    /// Newest first.
    Recent,
    /// Highest importance first.
    Important,
}

impl SearchOrder {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "relevance" | "score" => Some(SearchOrder::Relevance),
            "recent" | "newest" => Some(SearchOrder::Recent),
            "important" | "importance" => Some(SearchOrder::Important),
            _ => None,
        }
    }
}

/// Bounded keyword + filter search. Every field is optional; an empty query
/// still returns a deterministic, limited page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub categories: Vec<MemoryCategory>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub project: Option<String>,
    /// When true (with `project` set), records with **no** project also match:
    /// project-neutral decisions and preferences apply to every project.
    #[serde(default)]
    pub project_unscoped: bool,
    #[serde(default)]
    pub min_importance: Option<f64>,
    /// Highest sensitivity to return. Defaults to [`Sensitivity::Confidential`]:
    /// `secret` records never reach the model unless a caller opts in.
    #[serde(default = "default_ceiling")]
    pub max_sensitivity: Sensitivity,
    #[serde(default)]
    pub include_expired: bool,
    #[serde(default)]
    pub order: SearchOrder,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: usize,
}

fn default_ceiling() -> Sensitivity {
    Sensitivity::Confidential
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: None,
            categories: Vec::new(),
            tags: Vec::new(),
            project: None,
            project_unscoped: false,
            min_importance: None,
            max_sensitivity: default_ceiling(),
            include_expired: false,
            order: SearchOrder::default(),
            limit: None,
            offset: 0,
        }
    }
}

impl SearchQuery {
    pub fn text(query: impl Into<String>) -> Self {
        Self {
            text: Some(query.into()),
            ..Self::default()
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_categories(mut self, categories: Vec<MemoryCategory>) -> Self {
        self.categories = categories;
        self
    }

    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// True when the query is only a filter with no free text.
    pub fn is_filter_only(&self) -> bool {
        self.text.as_deref().map(str::trim).unwrap_or("").is_empty()
    }
}

/// Filter-only listing (no lexical scoring involved).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub category: Option<MemoryCategory>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default = "default_ceiling")]
    pub max_sensitivity: Sensitivity,
    #[serde(default)]
    pub include_expired: bool,
    #[serde(default)]
    pub order: SearchOrder,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: usize,
}

/// A record plus the lexical score and a preview snippet used for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub record: MemoryRecord,
    pub score: f64,
    /// Short excerpt around the match; falls back to the headline.
    pub snippet: String,
}

/// A bounded page of results. `total` is the full match count before paging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryList {
    pub items: Vec<MemoryHit>,
    pub total: usize,
    pub truncated: bool,
    /// Lexical index in use, for diagnostics.
    pub index: SearchIndex,
}

/// Which retrieval path produced a result set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SearchIndex {
    Fts5,
    #[default]
    Fallback,
}

impl SearchIndex {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchIndex::Fts5 => "fts5",
            SearchIndex::Fallback => "fallback",
        }
    }
}

/// Result of a save: the stored record plus whether an identical record already existed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveOutcome {
    pub record: MemoryRecord,
    /// True when an existing record with the same category + content was returned instead.
    pub duplicate: bool,
    /// Markdown document touched by this save, when the category is mirrored.
    pub markdown: Option<String>,
}

/// Consolidation report. Deterministic and auditable: no summarization happens here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsolidationReport {
    pub duplicate_groups: usize,
    pub records_removed: usize,
    pub tags_merged: usize,
    pub scanned: usize,
}

/// Portable snapshot for `export_memories` / `import_memories`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportBundle {
    /// Bundle format version, currently `1`.
    pub version: u32,
    /// Unix seconds.
    pub exported_at: i64,
    pub memories: Vec<MemoryRecord>,
    /// Markdown index rows, so a restore knows which documents existed.
    #[serde(default)]
    pub markdown_index: Vec<MarkdownIndexRow>,
}

impl ExportBundle {
    pub const VERSION: u32 = 1;
}

/// One row of the markdown -> SQLite index. Lets retrieval find human-edited
/// documents without reading every file on every model request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownIndexRow {
    /// Path relative to the memory root, e.g. `projects/nanomite.md`.
    pub path: String,
    pub title: String,
    /// Digest of the indexed body; a mismatch marks the document stale.
    pub content_hash: String,
    /// `mtime` seconds at index time, used as a cheap change probe.
    pub mtime: i64,
    pub size: u64,
    pub indexed_at: i64,
    /// Block ids currently present in the document.
    #[serde(default)]
    pub block_ids: Vec<String>,
}

/// Counters for `/memory` and the header footer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total: usize,
    pub expired: usize,
    pub by_category: Vec<(MemoryCategory, usize)>,
    pub markdown_documents: usize,
    pub markdown_stale: usize,
    pub search_index: SearchIndex,
}

// -- normalization helpers ---------------------------------------------------

/// Lowercases, trims, de-duplicates and sorts tags; rejects oversized input.
///
/// The *supplied* list may not exceed [`MAX_TAGS`] even when de-duplication
/// would shrink it: seventeen copies of one tag is a caller bug, not a tag set.
pub fn normalize_tags(tags: &[String]) -> Result<Vec<String>, MemoryError> {
    let supplied = tags.iter().filter(|t| !t.trim().is_empty()).count();
    if supplied > MAX_TAGS {
        return Err(MemoryError::invalid(format!(
            "{supplied} tags supplied, the maximum is {MAX_TAGS}"
        )));
    }
    let mut out: Vec<String> = Vec::new();
    for raw in tags {
        let tag = raw.trim().to_lowercase();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > MAX_TAG_CHARS {
            return Err(MemoryError::invalid(format!(
                "tag {tag:?} is longer than {MAX_TAG_CHARS} characters"
            )));
        }
        if tag.chars().any(|c| c.is_control()) {
            return Err(MemoryError::invalid(
                "tags must not contain control characters",
            ));
        }
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(MemoryError::invalid(format!(
            "{} tags supplied, the maximum is {MAX_TAGS}",
            out.len()
        )));
    }
    out.sort();
    Ok(out)
}

/// Splits a caller-supplied comma/space separated tag list.
pub fn parse_tags(value: &str) -> Vec<String> {
    value
        .split([',', ' ', '\n', '\t'])
        .filter(|s| !s.trim().is_empty())
        .map(str::trim)
        .map(|s| s.to_lowercase())
        .collect()
}

fn normalize_source(source: &MemorySource) -> Result<MemorySource, MemoryError> {
    let detail = match source.detail.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => {
            if d.chars().count() > MAX_SOURCE_DETAIL_CHARS {
                return Err(MemoryError::invalid(format!(
                    "source detail is longer than {MAX_SOURCE_DETAIL_CHARS} characters"
                )));
            }
            Some(d.to_string())
        }
        _ => None,
    };
    Ok(MemorySource {
        kind: source.kind,
        detail,
    })
}

/// Project keys are filesystem-safe slugs: lowercase alphanumerics plus `-` and `_`.
pub fn normalize_project(project: Option<&str>) -> Result<Option<String>, MemoryError> {
    let Some(raw) = project.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let slug = slugify(raw);
    if slug.is_empty() || slug.chars().count() > MAX_PROJECT_CHARS {
        return Err(MemoryError::invalid(format!(
            "project key {raw:?} does not slugify to a usable name"
        )));
    }
    Ok(Some(slug))
}

/// Conservative slug: keeps word characters, folds everything else to `-`.
pub fn slugify(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_PROJECT_CHARS));
    let mut pending_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() {
            pending_dash = true;
        }
        if out.chars().count() >= MAX_PROJECT_CHARS {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Current unix time in seconds; 0 only if the clock is before the epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_and_sensitivity_parse_case_insensitively() {
        assert_eq!(
            MemoryCategory::parse("Project"),
            Some(MemoryCategory::Project)
        );
        assert_eq!(MemoryCategory::parse("nope"), None);
        assert_eq!(
            Sensitivity::parse("CONFIDENTIAL"),
            Some(Sensitivity::Confidential)
        );
        assert!(Sensitivity::Internal.within(Sensitivity::Confidential));
        assert!(!Sensitivity::Secret.within(Sensitivity::Confidential));
    }

    #[test]
    fn validate_normalizes_tags_and_project() {
        let draft = NewMemory {
            content: "  Deploys on fridays  ".into(),
            tags: vec![" Ops ".into(), "ops".into(), "Release".into()],
            project: Some("My Project!".into()),
            ..NewMemory::new(MemoryCategory::Project, "x")
        };
        let v = draft.validate(now()).expect("valid");
        assert_eq!(v.content, "Deploys on fridays");
        assert_eq!(v.tags, vec!["ops", "release"]);
        assert_eq!(v.project.as_deref(), Some("my-project"));
        assert_eq!(v.importance, 0.5);
    }

    #[test]
    fn validate_rejects_bad_input() {
        let now = now();
        assert!(NewMemory::new(MemoryCategory::Core, "   ")
            .validate(now)
            .is_err());
        assert!(
            NewMemory::new(MemoryCategory::Core, "x".repeat(MAX_CONTENT_CHARS + 1))
                .validate(now)
                .is_err()
        );
        assert!(NewMemory {
            importance: Some(1.5),
            ..NewMemory::new(MemoryCategory::Core, "ok")
        }
        .validate(now)
        .is_err());
        assert!(NewMemory {
            confidence: Some(f64::NAN),
            ..NewMemory::new(MemoryCategory::Core, "ok")
        }
        .validate(now)
        .is_err());
        assert!(NewMemory {
            expires_at: Some(now - 10),
            ..NewMemory::new(MemoryCategory::Core, "ok")
        }
        .validate(now)
        .is_err());
        assert!(NewMemory {
            tags: vec!["t".into(); MAX_TAGS + 1],
            ..NewMemory::new(MemoryCategory::Core, "ok")
        }
        .validate(now)
        .is_err());
    }

    #[test]
    fn patch_updates_only_supplied_fields() {
        let now = now();
        let draft = NewMemory::new(MemoryCategory::Core, "original")
            .validate(now)
            .unwrap();
        let mut record = MemoryRecord {
            id: "mem_x".into(),
            category: draft.category,
            content: draft.content,
            summary: draft.summary,
            tags: draft.tags.clone(),
            source: draft.source,
            project: None,
            created_at: now,
            updated_at: now,
            confidence: None,
            importance: draft.importance,
            last_accessed_at: None,
            expires_at: None,
            sensitivity: draft.sensitivity,
            access_count: 0,
            revision: 1,
            content_hash: super::super::security::content_hash("original"),
        };

        let patch = MemoryPatch {
            importance: Some(0.9),
            ..MemoryPatch::default()
        };
        let updated = patch.apply(&record, now).expect("patch");
        assert_eq!(updated.content, "original");
        assert_eq!(updated.importance, 0.9);
        assert_eq!(updated.revision, 2);

        assert!(MemoryPatch::default().apply(&record, now).is_err());
        let bad = MemoryPatch {
            content: Some("".into()),
            ..MemoryPatch::default()
        };
        assert!(bad.apply(&record, now).is_err());
        record.revision = 1;
    }

    #[test]
    fn slugify_and_parse_tags_are_conservative() {
        assert_eq!(slugify("My Project!"), "my-project");
        assert_eq!(slugify("///"), "");
        assert_eq!(parse_tags("a, B  c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(MemoryError::invalid("x").code(), "invalid");
        assert_eq!(MemoryError::NotFound("x".into()).code(), "not_found");
    }
}
