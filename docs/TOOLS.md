# Tools

The model sees tools as OpenAI-function schemas (`agent/openai.rs::
tool_schemas()`). Dispatch is `agent/runtime.rs::run_tool`: parse JSON args →
call the matching `ToolSet` group → return `(payload, summary)`. Every call
runs on the blocking pool with a hard timeout; failures are returned to the
model as errors, never panics.

Status: core set **L3** (integrated + regression-tested, 55-test suite);
memory tools **L1** (landing, concurrent workstream — verify before use).

## Core tools (stable contracts — do not change without migration)

| Tool | Group | Effect | Limits |
|---|---|---|---|
| `list_files` | files | list workspace path | — |
| `read_file` | files | read UTF-8, offset/limit | `NANOMITE_MAX_FILE_BYTES` (default 200k) |
| `write_file` | files | write/append | same cap |
| `edit_file` | files | string replace (first/all) | same cap |
| `search` | files | recursive case-insensitive grep | hit caps |
| `run_command` | exec | `sh -c` in workspace | timeout, output cap, user permissions |
| `ssh_run` | exec | remote command | key-only, `BatchMode`, timeout |
| `web_fetch` | web | URL → readable text | http(s) only, size cap, HTML stripped |
| `web_search` | web | DuckDuckGo Lite → titles/URLs/snippets | result cap |

Filesystem tools are sandboxed to the workspace root: absolute paths, `..`,
and symlink escapes are refused. `run_command` is **not** sandboxed beyond the
working directory — it runs with your user permissions and is documented as
the same trust boundary as typing in a shell.

## Memory tools (in progress, unverified)

`src/tools/memory_tools.rs` (concurrent workstream) adds `memory_*` tools
(save/search/get/update/delete/…) with `confirm` gates on destructive calls,
backed by `MemoryPolicy::authorize`. Not audited; treat as L1 until the suite
is green and the schemas are reviewed.

## Tool contract requirements (for new tools)

Every tool needs: unique id · description · typed JSON schema · structured
result · side-effect classification · timeout + output limits · error mapping
· tests. New tools must dispatch through the same `run_tool` path — never
around it. Destructive or remote tools require explicit user confirmation at
the tool layer until the central policy engine (Milestone 2) exists.
