//! The agent's tool inventory as data.
//!
//! One entry per tool the runtime can dispatch, matching the schemas the
//! model actually sees (`openai::tool_schemas()` plus the memory schemas).
//! The unit tests below pin the catalog and the schema list together, so a
//! tool added to one place without the other fails a test instead of
//! drifting. UI surfaces (e.g. a tool-library overlay) read this list
//! instead of hard-coding names.

use super::ToolSet;

/// Coarse grouping used by UI listings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Files,
    Exec,
    Web,
    Memory,
    Document,
    SelfImprove,
}

/// One tool in the inventory: stable name, one-line description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub category: ToolCategory,
}

impl ToolSet {
    /// Every tool the runtime knows: the 16 dispatched today plus
    /// `read_document`. Names are unique and match the schema list exactly
    /// (enforced by tests).
    pub fn catalog() -> Vec<ToolSpec> {
        use ToolCategory::{Document, Exec, Files, Memory, SelfImprove, Web};
        vec![
            ToolSpec {
                name: "list_files",
                description: "List files and directories at a path inside the workspace.",
                category: Files,
            },
            ToolSpec {
                name: "read_file",
                description: "Read a bounded window of a UTF-8 text file from the workspace.",
                category: Files,
            },
            ToolSpec {
                name: "search",
                description: "Case-insensitive substring search over workspace text files, returning path and line for each hit.",
                category: Files,
            },
            ToolSpec {
                name: "write_file",
                description: "Write (or append) UTF-8 content to a file inside the workspace.",
                category: Files,
            },
            ToolSpec {
                name: "edit_file",
                description: "Replace occurrences of an exact string in a workspace file.",
                category: Files,
            },
            ToolSpec {
                name: "read_document",
                description: "Extract bounded text and structure from a workspace document (pdf, docx, xlsx, ods, odt, rtf, csv and the text family).",
                category: Document,
            },
            ToolSpec {
                name: "run_command",
                description: "Run a shell command inside the workspace (sh -c) with a hard timeout.",
                category: Exec,
            },
            ToolSpec {
                name: "ssh_run",
                description: "Run a command on a remote host over key-based SSH (BatchMode).",
                category: Exec,
            },
            ToolSpec {
                name: "web_fetch",
                description: "Fetch an http(s) URL and return its readable text content (HTML stripped).",
                category: Web,
            },
            ToolSpec {
                name: "web_search",
                description: "Search the web (DuckDuckGo) and return titles, URLs and snippets.",
                category: Web,
            },
            ToolSpec {
                name: "memory_search",
                description: "Search persistent memory for facts, decisions and preferences from earlier sessions.",
                category: Memory,
            },
            ToolSpec {
                name: "memory_save",
                description: "Store a durable fact, decision, preference or lesson in persistent memory.",
                category: Memory,
            },
            ToolSpec {
                name: "memory_get",
                description: "Fetch one stored memory by its exact id.",
                category: Memory,
            },
            ToolSpec {
                name: "memory_update",
                description: "Update fields of an existing memory by id; omitted fields stay unchanged.",
                category: Memory,
            },
            ToolSpec {
                name: "memory_delete",
                description: "Delete one memory by id (requires confirm=true).",
                category: Memory,
            },
            ToolSpec {
                name: "memory_list",
                description: "List stored memories with optional category, project and tag filters.",
                category: Memory,
            },
            ToolSpec {
                name: "propose_self_edit",
                description: "Stage a proposed edit to your config, prompt file or a source file for the user to review in /improve.",
                category: SelfImprove,
            },
            ToolSpec {
                name: "list_self_edits",
                description: "List self-edit proposals still waiting for the user's decision.",
                category: SelfImprove,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::openai::tool_schemas;
    use crate::tools::improve_tools::improve_tool_schemas;
    use crate::tools::memory_tools::memory_tool_schemas;

    /// The catalog is the single inventory: names must be unique.
    #[test]
    fn catalog_names_are_unique() {
        let mut names: Vec<&'static str> = ToolSet::catalog().into_iter().map(|s| s.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate tool names in catalog");
        assert_eq!(count, 18, "expected the full 18-tool inventory");
    }

    /// Catalog and schemas must stay in step: the exact name set the model
    /// can call (`tool_schemas()` + memory schemas) equals the catalog.
    #[test]
    fn catalog_matches_exposed_schemas() {
        let mut schema_names: Vec<String> = tool_schemas()
            .into_iter()
            .chain(memory_tool_schemas())
            .chain(improve_tool_schemas())
            .map(|t| t.function.name)
            .collect();
        let mut catalog_names: Vec<String> = ToolSet::catalog()
            .into_iter()
            .map(|s| s.name.to_string())
            .collect();
        schema_names.sort();
        catalog_names.sort();
        assert_eq!(
            catalog_names, schema_names,
            "catalog and schema list diverge"
        );
    }
}
