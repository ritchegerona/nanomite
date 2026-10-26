# Tools

The model sees tools as OpenAI-function schemas (`agent/openai.rs::
tool_schemas()`). Dispatch is `agent/runtime.rs::run_tool`: parse JSON args →
call the matching `ToolSet` group → return `(payload, summary)`. Every call
runs on the blocking pool with a hard timeout; failures are returned to the
model as errors, never panics.

Status: the full **33-tool** inventory is **L3** (integrated + regression-tested,
409-test suite) as of 2026-10-26. `src/tools/catalog.rs` is the single
inventory: `ToolSpec { name, description, category }`, with tests asserting the
catalog matches the exposed schema list exactly — add a tool to one and the
test fails instead of the two drifting apart.

## The catalog

| Tool | Category | Effect | Limits |
|---|---|---|---|
| `list_files` | Files | list workspace path | — |
| `read_file` | Files | read UTF-8, offset/limit | `NANOMITE_MAX_FILE_BYTES` (default 200k) |
| `write_file` | Files | write/append | same cap |
| `edit_file` | Files | string replace (first/all) | same cap |
| `search` | Files | recursive case-insensitive grep | hit caps |
| `read_document` | Document | pdf/docx/odt/xlsx/ods/rtf/csv/md/json → bounded text | read-only, workspace-confined, size/section caps |
| `run_command` | Exec | `sh -c` in workspace | timeout, output cap, user permissions |
| `ssh_run` | Exec | remote command | key-only, `BatchMode`, timeout |
| `web_fetch` | Web | URL → readable text | http(s) only, size cap, HTML stripped |
| `web_search` | Web | DuckDuckGo Lite → titles/URLs/snippets | result cap |
| `memory_*` (6) | Memory | save/search/get/update/delete/list | `MemoryPolicy` gate, `confirm=true` on delete |
| `propose_self_edit` | Self-improve | stage an edit to config, prompt file or source | **cannot apply** — see `docs/SECURITY.md` |
| `list_self_edits` | Self-improve | list pending proposals | read-only |
| `git_status` | Git | branch, ahead/behind, staged/unstaged/untracked | read-only |
| `git_diff` | Git | working tree or staged diff | per-path, staged/untracked options |
| `git_log` | Git | recent commits with optional diffs | limit 200 |
| `git_commit` | Git | create commit with message | stages files, `allow_empty` option |
| `git_branch` | Git | list/create/delete branches | `force` for unmerged delete |
| `run_tests` | Test | cargo/npm/pytest/jest/go/custom | timeout 300s, output cap 50k |
| `index_symbols` | Index | build/rebuild symbol index | ctags + regex fallback |
| `find_symbol` | Index | search symbol index | wildcard, kind/language filters |
| `find_references` | Index | find references to symbol | cross-file, limit 200 |
| `create_plan` | Plan | create task plan with deps/tags | max 200 tasks, 500 chars/task |
| `get_plan` | Plan | retrieve plan by name or latest | — |
| `update_task` | Plan | update task status/desc/deps/tags | — |
| `list_plans` | Plan | list all plans with summaries | — |
| `delete_plan` | Plan | delete plan by name (confirm) | `confirm=true` required |

Filesystem tools are sandboxed to the workspace root: absolute paths, `..`,
and symlink escapes are refused. `read_document` reuses the same bounded
`src/extract/` pipeline the extractors use (originals are never modified).
`run_command` is **not** sandboxed beyond the working directory — it runs with
your user permissions and is documented as the same trust boundary as typing in
a shell.

## Memory tools

`src/tools/memory_tools.rs` exposes the six `memory_*` tools with `confirm`
gates on destructive calls, backed by `MemoryPolicy::authorize`. They are live:
the engine attaches at startup, so the tools are callable and retrieval and
capture run per policy. Status: L3.

## Enabling and disabling tools

`/tools` renders the catalog grouped by category with ✓ enabled / ✗ disabled and
writes the choice to `disabled_tools` in `config.toml` (comma-separated);
`NANOMITE_DISABLED_TOOLS` overrides the file. Unknown names are ignored rather
than failing the config. Enforcement is deliberately **both** ends:

1. **Before the request** — disabled tools are filtered out of the schema list,
   so the model is never even told they exist.
2. **At dispatch** — `runtime.rs::run_tool` refuses a disabled tool by name, so
   a hallucinated or replayed call cannot reach it.

Removing either half leaves a hole: schema filtering alone loses to a guessed
call, dispatch refusal alone makes the model keep trying.

## Tool contract requirements (for new tools)

Every tool needs: unique id · description · typed JSON schema · structured
result · side-effect classification · timeout + output limits · error mapping
· tests. A new tool must be added to **both** `catalog.rs` and the schema list
(the tests enforce this) and must dispatch through the same `run_tool` path —
never around it. Destructive or remote tools require explicit user confirmation
at the tool layer until the central policy engine (`agent/policy.rs`) exists.