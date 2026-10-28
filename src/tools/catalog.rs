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
    Git,
    Test,
    Index,
    Plan,
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
        use ToolCategory::{
            Document, Exec, Files, Git, Index, Memory, Plan, SelfImprove, Test, Web,
        };
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
                name: "browse",
                description: "Open a URL in a headless browser (JavaScript rendered) and return the page's readable text.",
                category: Web,
            },
            ToolSpec {
                name: "browse_screenshot",
                description: "Open a URL in a headless browser and save a full-page screenshot PNG in the workspace.",
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
            ToolSpec {
                name: "git_status",
                description: "Show git status: branch, ahead/behind, staged/unstaged/untracked files.",
                category: Git,
            },
            ToolSpec {
                name: "git_diff",
                description: "Show git diff of working tree or staged changes (optionally for a specific path).",
                category: Git,
            },
            ToolSpec {
                name: "git_log",
                description: "Show recent git commit history (with optional diffs).",
                category: Git,
            },
            ToolSpec {
                name: "git_commit",
                description: "Create a git commit with the given message, optionally staging specific files.",
                category: Git,
            },
            ToolSpec {
                name: "git_branch",
                description: "List, create, or delete git branches.",
                category: Git,
            },
            ToolSpec {
                name: "run_tests",
                description: "Run project tests (cargo, npm, pytest, jest, go, custom) and return structured results.",
                category: Test,
            },
            ToolSpec {
                name: "index_symbols",
                description: "Build or rebuild the project symbol index for fast code navigation.",
                category: Index,
            },
            ToolSpec {
                name: "find_symbol",
                description: "Search the symbol index for definitions matching a pattern.",
                category: Index,
            },
            ToolSpec {
                name: "find_references",
                description: "Find all references to a symbol across the project.",
                category: Index,
            },
            ToolSpec {
                name: "create_plan",
                description: "Create a new task plan with structured tasks, dependencies, and tags.",
                category: Plan,
            },
            ToolSpec {
                name: "get_plan",
                description: "Retrieve a plan by name, or the most recently updated plan.",
                category: Plan,
            },
            ToolSpec {
                name: "update_task",
                description: "Update a task's status, description, dependencies, or tags.",
                category: Plan,
            },
            ToolSpec {
                name: "list_plans",
                description: "List all saved plans with completion summaries.",
                category: Plan,
            },
            ToolSpec {
                name: "delete_plan",
                description: "Delete a plan by name (requires confirmation).",
                category: Plan,
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
        assert_eq!(count, 34, "expected the full 34-tool inventory");
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
