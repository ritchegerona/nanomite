//! Memory tools: the agent-facing surface of the persistent memory engine.
//!
//! Six tools, mirroring the engine API one-to-one - `memory_search`,
//! `memory_save`, `memory_get`, `memory_update`, `memory_delete` and
//! `memory_list`. All argument parsing, validation, authorization (via
//! [`crate::memory::policy::MemoryPolicy`]) and storage live in the engine;
//! this module only adapts JSON in and compact JSON out, and bounds every
//! payload so a chatty model cannot eat the context window.
//!
//! Sync by design - the runtime dispatches every tool call through
//! `tokio::task::spawn_blocking`, exactly like the file, exec and web tools.

use crate::agent::provider::Tool;
use crate::memory::model::{MemoryCategory, MemoryError, MemoryPatch, SearchOrder, SearchQuery};
use crate::memory::{MarkdownHit, MemoryEngine};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// Longest `content` echoed back per record in search/list payloads.
const PREVIEW_CHARS: usize = 400;
/// Longest content echoed back by `memory_get`.
const MAX_GET_CHARS: usize = 4_000;
/// Default page size for tool-level search and list.
const DEFAULT_LIMIT: usize = 8;
/// Hard page-size ceiling for tool callers, independent of the policy default.
const MAX_TOOL_LIMIT: usize = 50;

// -- arguments ----------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemorySearchArgs {
    /// Free-text query. Empty/omitted means "filters only".
    #[serde(default)]
    pub query: Option<String>,
    /// Filter: category names, e.g. ["decision", "project"].
    #[serde(default)]
    pub categories: Option<Vec<String>>,
    /// Filter: tags (AND-ed).
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Filter: project key.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    /// Also scan the markdown knowledge files when the record index is empty.
    #[serde(default)]
    pub include_markdown: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySaveArgs {
    /// One of: core, user, project, episodic, decision, session, task.
    pub category: String,
    /// The durable fact, in plain prose. Never credentials.
    pub content: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub project: Option<String>,
    /// 0.0-1.0 retrieval priority. Defaults to 0.5.
    #[serde(default)]
    pub importance: Option<f64>,
    /// Retention hint: record becomes purgeable after this many days.
    #[serde(default)]
    pub expires_in_days: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGetArgs {
    pub id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryUpdateArgs {
    pub id: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub importance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDeleteArgs {
    pub id: String,
    /// Required. Deleting memory is destructive and never implicit.
    pub confirm: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryListArgs {
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    /// `recent` (default) or `important`.
    #[serde(default)]
    pub order: Option<String>,
}

// -- results ------------------------------------------------------------------

/// One record as the model sees it: compact, bounded, no internal bookkeeping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryToolHit {
    pub id: String,
    pub category: String,
    /// Short preview (search) or bounded full content (get).
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub importance: f64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub results: Vec<MemoryToolHit>,
    pub total: usize,
    pub truncated: bool,
    /// `fts5` or `fallback`, so retrieval quality is observable.
    pub index: String,
    /// Hits from the human-edited markdown files (only when requested and needed).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub markdown: Vec<MarkdownHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySaveResult {
    pub id: String,
    /// True when an identical record already existed and was returned instead.
    pub duplicate: bool,
    /// Markdown document the record was mirrored into, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGetResult {
    pub record: MemoryToolHit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUpdateResult {
    pub record: MemoryToolHit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDeleteResult {
    pub deleted: String,
    /// Where the record's markdown copy was removed from, when it had one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryListResult {
    pub results: Vec<MemoryToolHit>,
    pub total: usize,
    pub truncated: bool,
}

// -- tool struct --------------------------------------------------------------

pub struct MemoryTools {
    engine: Arc<MemoryEngine>,
}

impl MemoryTools {
    pub fn new(engine: Arc<MemoryEngine>) -> Self {
        Self { engine }
    }

    /// `memory_search` - bounded keyword + filter search.
    pub fn search(&self, args: MemorySearchArgs) -> Result<MemorySearchResult, MemoryError> {
        let mut query = SearchQuery {
            project: normalize_project_arg(args.project.as_deref())?,
            ..SearchQuery::default()
        };
        if let Some(categories) = &args.categories {
            let mut parsed = Vec::new();
            for raw in categories {
                let category = MemoryCategory::parse(raw).ok_or_else(|| {
                    MemoryError::invalid(format!(
                        "unknown category {raw:?} (expected one of: {})",
                        MemoryCategory::names().join(", ")
                    ))
                })?;
                if !parsed.contains(&category) {
                    parsed.push(category);
                }
            }
            query.categories = parsed;
        }
        if let Some(tags) = &args.tags {
            query.tags = tags.iter().map(|t| t.trim().to_lowercase()).collect();
        }
        if let Some(text) = args.query.as_deref() {
            query.text = (!text.trim().is_empty()).then(|| text.trim().to_string());
        }
        query.order = SearchOrder::Relevance;
        query.limit = Some(tool_limit(args.limit));

        let (list, markdown) = self
            .engine
            .search(&query, args.include_markdown.unwrap_or(false))?;
        Ok(MemorySearchResult {
            results: list
                .items
                .iter()
                .map(|hit| tool_hit(&hit.record, Some(hit.score), Some(hit.snippet.clone())))
                .collect(),
            total: list.total,
            truncated: list.truncated,
            index: list.index.as_str().to_string(),
            markdown,
        })
    }

    /// `memory_save` - store a durable fact (and mirror it into markdown when the
    /// category maps to a document).
    pub fn save(&self, args: MemorySaveArgs) -> Result<MemorySaveResult, MemoryError> {
        let category = MemoryCategory::parse(&args.category).ok_or_else(|| {
            MemoryError::invalid(format!(
                "unknown category {:?} (expected one of: {})",
                args.category,
                MemoryCategory::names().join(", ")
            ))
        })?;
        let mut draft = crate::memory::model::NewMemory::new(category, args.content);
        draft.summary = args.summary;
        draft.tags = args.tags.unwrap_or_default();
        draft.project = normalize_project_arg(args.project.as_deref())?;
        draft.importance = args.importance;
        draft.expires_at = args
            .expires_in_days
            .map(|days| crate::memory::model::now() + (days as i64).saturating_mul(86_400));
        draft.source =
            crate::memory::model::MemorySource::new(crate::memory::model::SourceKind::Agent);
        let outcome = self.engine.save(&draft)?;
        Ok(MemorySaveResult {
            id: outcome.record.id,
            duplicate: outcome.duplicate,
            markdown: outcome.markdown,
        })
    }

    /// `memory_get` - exact id lookup with a bounded payload.
    pub fn get(&self, args: MemoryGetArgs) -> Result<MemoryGetResult, MemoryError> {
        let record = self.engine.get(args.id.trim())?;
        Ok(MemoryGetResult {
            record: tool_hit(&record, None, None),
        })
    }

    /// `memory_update` - sparse patch; omitted fields are left alone.
    pub fn update(&self, args: MemoryUpdateArgs) -> Result<MemoryUpdateResult, MemoryError> {
        let patch = MemoryPatch {
            content: args.content,
            summary: args.summary,
            tags: args.tags,
            category: parse_optional_category(args.category.as_deref())?,
            project: Some(normalize_project_arg(args.project.as_deref())?),
            importance: args.importance,
            ..MemoryPatch::default()
        };
        let updated = self.engine.update(args.id.trim(), &patch)?;
        Ok(MemoryUpdateResult {
            record: tool_hit(&updated, None, None),
        })
    }

    /// `memory_delete` - removes the record and its markdown block. Requires an
    /// explicit confirmation at the tool layer, on top of the policy gate.
    pub fn delete(&self, args: MemoryDeleteArgs) -> Result<MemoryDeleteResult, MemoryError> {
        if !args.confirm {
            return Err(MemoryError::policy(
                "deleting memory needs confirm=true - repeat the call with confirm once \
                 the user has approved the deletion",
            ));
        }
        let record = self.engine.delete(args.id.trim())?;
        let markdown = self
            .engine
            .knowledge()
            .document_for(record.category, record.project.as_deref());
        Ok(MemoryDeleteResult {
            deleted: record.id,
            markdown,
        })
    }

    /// `memory_list` - filter-only listing, newest or most important first.
    pub fn list(&self, args: MemoryListArgs) -> Result<MemoryListResult, MemoryError> {
        let order = match args.order.as_deref().map(str::trim) {
            Some(o) if !o.is_empty() => SearchOrder::parse(o).ok_or_else(|| {
                MemoryError::invalid(format!("unknown order {o:?} (recent or important)"))
            })?,
            _ => SearchOrder::Recent,
        };
        let query = crate::memory::model::ListQuery {
            category: parse_optional_category(args.category.as_deref())?,
            project: normalize_project_arg(args.project.as_deref())?,
            tag: args
                .tag
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(|t| t.to_lowercase()),
            order,
            limit: Some(tool_limit(args.limit)),
            ..crate::memory::model::ListQuery::default()
        };
        let list = self.engine.list(&query)?;
        Ok(MemoryListResult {
            results: list
                .items
                .iter()
                .map(|hit| tool_hit(&hit.record, None, None))
                .collect(),
            total: list.total,
            truncated: list.truncated,
        })
    }
}

// -- helpers ------------------------------------------------------------------

fn tool_limit(requested: Option<usize>) -> usize {
    requested.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_TOOL_LIMIT)
}

fn parse_optional_category(raw: Option<&str>) -> Result<Option<MemoryCategory>, MemoryError> {
    match raw.map(str::trim).filter(|c| !c.is_empty()) {
        None => Ok(None),
        Some(value) => MemoryCategory::parse(value).map(Some).ok_or_else(|| {
            MemoryError::invalid(format!(
                "unknown category {value:?} (expected one of: {})",
                MemoryCategory::names().join(", ")
            ))
        }),
    }
}

fn normalize_project_arg(raw: Option<&str>) -> Result<Option<String>, MemoryError> {
    match raw.map(str::trim).filter(|p| !p.is_empty()) {
        None => Ok(None),
        Some(value) => Ok(crate::memory::model::normalize_project(Some(value))?),
    }
}

/// Projects a record into the bounded tool form. `get` keeps more than search.
fn tool_hit(
    record: &crate::memory::model::MemoryRecord,
    score: Option<f64>,
    snippet: Option<String>,
) -> MemoryToolHit {
    let budget = if snippet.is_some() || score.is_some() {
        PREVIEW_CHARS
    } else {
        MAX_GET_CHARS
    };
    let content = match &record.summary {
        Some(summary) if !summary.trim().is_empty() && budget <= PREVIEW_CHARS => summary.clone(),
        _ => bounded(&record.content, budget),
    };
    MemoryToolHit {
        id: record.id.clone(),
        category: record.category.as_str().to_string(),
        content,
        summary: (budget <= PREVIEW_CHARS)
            .then(|| record.summary.clone())
            .flatten(),
        tags: record.tags.clone(),
        project: record.project.clone(),
        importance: record.importance,
        updated_at: record.updated_at,
        score,
        snippet,
    }
}

fn bounded(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!(
            "{}…",
            flat.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

// -- schemas ------------------------------------------------------------------

/// JSON schemas for the six memory tools, appended to the runtime's tool list
/// only when the memory engine is available.
pub fn memory_tool_schemas() -> Vec<Tool> {
    let category_note = format!("One of: {}.", MemoryCategory::names().join(", "));
    vec![
        Tool::new(
            "memory_search",
            "Search persistent memory for relevant facts, decisions, preferences and past work. Use before asking the user to repeat context.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Free-text keyword query (omit for filter-only)" },
                    "categories": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by categories, e.g. [\"decision\", \"project\"]"
                    },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Filter by tags (all must match)" },
                    "project": { "type": "string", "description": "Filter by project key" },
                    "limit": { "type": "integer", "description": "Max results (default 8, max 50)" },
                    "include_markdown": { "type": "boolean", "description": "Also scan human-edited markdown knowledge files when the record index finds nothing" }
                },
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "memory_save",
            "Store a durable fact, decision, preference or lesson in persistent memory. Never store credentials or secrets.",
            json!({
                "type": "object",
                "properties": {
                    "category": { "type": "string", "description": category_note },
                    "content": { "type": "string", "description": "The durable fact in plain prose" },
                    "summary": { "type": "string", "description": "Optional short summary used as the title" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Optional searchable tags" },
                    "project": { "type": "string", "description": "Optional project key" },
                    "importance": { "type": "number", "description": "0.0-1.0 retrieval priority (default 0.5)" },
                    "expires_in_days": { "type": "integer", "description": "Optional retention window in days" }
                },
                "required": ["category", "content"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "memory_get",
            "Fetch one stored memory by its exact id.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Memory id (mem_...)" }
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "memory_update",
            "Update fields of an existing memory. Omitted fields stay unchanged.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Memory id (mem_...)" },
                    "content": { "type": "string", "description": "Replacement content" },
                    "summary": { "type": "string", "description": "Replacement summary" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Replacement tag set" },
                    "category": { "type": "string", "description": category_note },
                    "project": { "type": "string", "description": "Replacement project key" },
                    "importance": { "type": "number", "description": "0.0-1.0 retrieval priority" }
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "memory_delete",
            "Delete one memory by id. Destructive: requires confirm=true, which you should only set after the user asked for the deletion.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Memory id (mem_...)" },
                    "confirm": { "type": "boolean", "description": "Must be true to delete" }
                },
                "required": ["id", "confirm"],
                "additionalProperties": false
            }),
        ),
        Tool::new(
            "memory_list",
            "List stored memories, newest or most important first, with optional filters.",
            json!({
                "type": "object",
                "properties": {
                    "category": { "type": "string", "description": category_note },
                    "project": { "type": "string", "description": "Filter by project key" },
                    "tag": { "type": "string", "description": "Filter by one tag" },
                    "limit": { "type": "integer", "description": "Max results (default 8, max 50)" },
                    "order": { "type": "string", "description": "\"recent\" (default) or \"important\"" }
                },
                "additionalProperties": false
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::policy::MemoryPolicy;

    fn engine() -> Arc<MemoryEngine> {
        let policy = MemoryPolicy {
            mirror_markdown: false,
            ..MemoryPolicy::default()
        };
        Arc::new(MemoryEngine::open_in_memory(policy).expect("engine"))
    }

    fn tools() -> MemoryTools {
        MemoryTools::new(engine())
    }

    #[test]
    fn save_get_search_and_list_round_trip() {
        let tools = tools();
        let saved = tools
            .save(MemorySaveArgs {
                category: "decision".into(),
                content: "memory lives in sqlite, not json".into(),
                summary: Some("storage choice".into()),
                tags: Some(vec!["Storage".into()]),
                project: Some("Nanomite!".into()),
                importance: Some(0.9),
                expires_in_days: None,
            })
            .expect("save");
        assert!(saved.id.starts_with("mem_"));
        assert!(!saved.duplicate);

        let got = tools
            .get(MemoryGetArgs {
                id: saved.id.clone(),
            })
            .expect("get");
        assert_eq!(got.record.category, "decision");
        assert_eq!(got.record.project.as_deref(), Some("nanomite"));
        assert_eq!(got.record.tags, vec!["storage"]);

        let found = tools
            .search(MemorySearchArgs {
                query: Some("sqlite".into()),
                ..MemorySearchArgs::default()
            })
            .expect("search");
        assert_eq!(found.total, 1);
        assert_eq!(found.results[0].id, saved.id);
        assert!(found.results[0].snippet.is_some());

        let listed = tools
            .list(MemoryListArgs {
                category: Some("Decision".into()),
                ..MemoryListArgs::default()
            })
            .expect("list");
        assert_eq!(listed.total, 1);
    }

    #[test]
    fn update_changes_only_given_fields() {
        let tools = tools();
        let saved = tools
            .save(MemorySaveArgs {
                category: "core".into(),
                content: "original".into(),
                ..MemorySaveArgs::for_test()
            })
            .unwrap();
        let updated = tools
            .update(MemoryUpdateArgs {
                id: saved.id.clone(),
                importance: Some(0.8),
                ..MemoryUpdateArgs::default()
            })
            .expect("update");
        assert_eq!(updated.record.importance, 0.8);
        assert!(updated.record.content.contains("original"));

        let again = tools.update(MemoryUpdateArgs {
            id: "mem_missing".into(),
            importance: Some(0.1),
            ..MemoryUpdateArgs::default()
        });
        assert!(matches!(again, Err(MemoryError::NotFound(_))));
    }

    #[test]
    fn delete_requires_confirmation() {
        let tools = tools();
        let saved = tools
            .save(MemorySaveArgs {
                category: "core".into(),
                content: "doomed".into(),
                ..MemorySaveArgs::for_test()
            })
            .unwrap();

        let refused = tools.delete(MemoryDeleteArgs {
            id: saved.id.clone(),
            confirm: false,
        });
        assert!(matches!(refused, Err(MemoryError::Policy(_))));
        assert!(tools
            .get(MemoryGetArgs {
                id: saved.id.clone()
            })
            .is_ok());

        tools
            .delete(MemoryDeleteArgs {
                id: saved.id.clone(),
                confirm: true,
            })
            .expect("confirmed delete");
        assert!(matches!(
            tools.get(MemoryGetArgs { id: saved.id }),
            Err(MemoryError::NotFound(_))
        ));
    }

    #[test]
    fn save_refuses_credentials_and_unknown_categories() {
        let tools = tools();
        let secret = tools.save(MemorySaveArgs {
            category: "core".into(),
            content: "the key is sk-abcdefghijklmnopqrstuvwxyz0123".into(),
            ..MemorySaveArgs::for_test()
        });
        assert!(matches!(secret, Err(MemoryError::Sensitive(_))));

        let category = tools.save(MemorySaveArgs {
            category: "random".into(),
            content: "fine".into(),
            ..MemorySaveArgs::for_test()
        });
        assert!(matches!(category, Err(MemoryError::Invalid(_))));
    }

    #[test]
    fn read_only_engine_refuses_writes_but_serves_reads() {
        let policy = MemoryPolicy {
            writable: false,
            mirror_markdown: false,
            ..MemoryPolicy::default()
        };
        let tools = MemoryTools::new(Arc::new(MemoryEngine::open_in_memory(policy).unwrap()));
        assert!(tools
            .save(MemorySaveArgs {
                category: "core".into(),
                content: "nope".into(),
                ..MemorySaveArgs::for_test()
            })
            .is_err());
        assert!(tools.list(MemoryListArgs::default()).is_ok());
    }

    #[test]
    fn search_payloads_stay_bounded() {
        let tools = tools();
        for i in 0..30 {
            tools
                .save(MemorySaveArgs {
                    category: "episodic".into(),
                    content: format!("event {i}: {}", "detail ".repeat(200)),
                    ..MemorySaveArgs::for_test()
                })
                .unwrap();
        }
        let found = tools
            .search(MemorySearchArgs {
                query: Some("event".into()),
                ..MemorySearchArgs::default()
            })
            .unwrap();
        assert_eq!(found.results.len(), DEFAULT_LIMIT);
        for hit in &found.results {
            assert!(hit.content.chars().count() <= PREVIEW_CHARS + 1);
        }
    }

    #[test]
    fn schemas_cover_the_six_tools() {
        let names: Vec<String> = memory_tool_schemas()
            .into_iter()
            .map(|t| t.function.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "memory_search",
                "memory_save",
                "memory_get",
                "memory_update",
                "memory_delete",
                "memory_list",
            ]
        );
    }

    impl MemorySaveArgs {
        fn for_test() -> Self {
            Self {
                category: "core".into(),
                content: String::new(),
                summary: None,
                tags: None,
                project: None,
                importance: None,
                expires_in_days: None,
            }
        }
    }
}
