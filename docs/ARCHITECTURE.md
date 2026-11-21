# Architecture

nanomite is a single-crate Rust binary (`nanomite-ai`, edition 2021). The
library crate (`src/lib.rs`) contains all testable logic; `src/main.rs` owns
the terminal, the tokio event loop, and the panic hook.

## Data flow

```
src/main.rs ── tokio event loop ──▶ App (src/app/mod.rs) ──▶ ui::render
        │  (input: sync event::read() on a dedicated OS thread)
        │                                │
        │                          AgentRuntime ──▶ AgentProvider ──▶ HTTPS
        │                                │              (any OpenAI-compatible /v1)
        │                          ToolSet ──▶ files / document / exec / web
        │                                            ├─▶ memory*   (policy-gated)
        │                                            ├─▶ improve*  (stage only)
        │                                            ├─▶ git*       (workspace-confined)
        │                                            ├─▶ test*      (run_tests)
        │                                            ├─▶ index*     (symbol index)
        │                                            └─▶ plan*      (task plans)
        │                          Swarm ──▶ BoardEngine (src/board/)
        └─ Config (defaults ← config.toml ← env)   Session (JSON v1)
```

## Layers

### UI (`src/ui/`)
ratatui immediate-mode rendering. `chat.rs` virtualizes the transcript (only
visible entries materialize; render trees are cached). `composer.rs` provides
multi-line input. `panels.rs` handles headers, footers, overlays, and pickers.
`theme.rs` defines four palettes. `text.rs` handles wrapping and minimal
markdown rendering. All state lives in `App`; rendering is a pure function of
`&App` per frame, redrawn only when dirty.

### App (`src/app/mod.rs`)
The state machine: key handling, overlay stack, provider/model pickers, command
router (`src/commands.rs`, parsed locally), session persist/restore, and the
engine pump. Agent turns run as background tasks feeding `AgentEvent`s; the UI
thread never blocks.

### Agent (`src/agent/`)
`provider.rs` defines the `AgentProvider` trait (`stream_chat`, `list_models`)
and the streaming event model (`Delta` / `ToolCall` / `Usage` / `Done`) with a
fragment accumulator for out-of-order tool-call chunks. `runtime.rs` runs one
turn: system prompt + history + input → stream → dispatch tools via
`spawn_blocking` → append results → repeat, bounded by `max_steps` and
`max_tool_calls` (overridable by env), with cooperative cancellation. `policy.rs`
enforces `ToolPolicy` (Plan / Coder / Build / Hacker) at dispatch. `swarm.rs`
runs several providers in parallel for the multi-agent board.

### Tools (`src/tools/`)
`catalog.rs` holds the inventory as `ToolSpec { name, description, category }`;
tests pin it to the exposed schema list. File tools resolve every path against
the workspace root and refuse escapes. `document_tools.rs` (`read_document`)
reuses the bounded `src/extract/` pipeline. Exec tools run `sh -c` in the
workspace with a hard timeout and capped output; SSH is key-only with
`BatchMode=yes`. Web tools speak http(s) only and truncate before returning text
to the model. Git tools run `git` directly in the workspace. Test runner
supports cargo, npm, pytest, jest, go, and custom commands. Symbol index uses
ctags with regex fallback. Plan tools create/update/list/delete task plans with
dependencies. Dispatch is a `match` in `runtime.rs::run_tool` with a central
policy engine (`agent/policy.rs`): `ToolPolicy` is checked before dispatch; a
disabled tool is filtered from the schema list **and** refused at dispatch.

### HACKER MODE (`src/hacker/`)
An authorized cybersecurity operations environment, separate from
`ToolPolicy::Hacker`. It layers a capability-session state machine, an
assessment scope, an independent authorization gate, an append-only audit trail,
and a module registry on top of the nanomite agent runtime. Disabled by default.
Requires explicit opt-in configuration and an authorized scope before any
operation can run.

### Board (`src/board/`)
`personas.rs` defines Architect, Engineer, Scientist, and Critic personas plus
a Chair. `BoardEngine::convene` runs parallel discussion rounds through `Swarm`
and then a chair synthesis, checking for cancellation between rounds.
`store.rs` persists board messages in SQLite under `~/.local/share/nanomite/board/`
(`NANOMITE_BOARD_DIR`). Opened with `/panel <topic>`, streamed live into an
overlay.

### Self-improvement (`src/improve/`)
`proposal.rs` validates staged proposals. `diff.rs` builds unified diff
artifacts. `runner.rs` writes `.bak` backups, runs `cargo test --lib` as a
regression gate, and rolls back byte-exactly on failure. `audit.rs` appends
every decision to a JSONL log. The agent can only **stage** a proposal
(`propose_self_edit`); applying requires the user pressing `a` in `/improve`.

### Learning (`src/learn/`)
The daily learning loop is pure logic: cadence state (`state.json`), topic
rotation, slugging, and `learning_prompt`, the instruction text for a normal
agent turn. `/learn <topic>` starts a research turn that saves lessons to memory
(tags `learning` + `technique`) and stages improvements with `propose_self_edit`.
`/learn daily on` opts into one automatic session per local calendar day (off
by default). No network or model calls inside the module.

### Providers and config (`src/config.rs` + `src/agent/openai.rs`)
Setup persists to `~/.config/nanomite/config.toml` (atomic write, `0600`);
precedence is built-in defaults < file < environment (`NANOMITE_*`). Per-provider
API keys live in `[providers]`, so `/model` can query every keyed provider in
parallel and group results under provider headers. The file also carries
`theme`, `disabled_tools`, `tool_policy`, and `prompt_file`.

### Sessions (`src/session.rs`)
`StoredSession` v1 JSON: provider, model, cwd, transcript entries. Markdown
export for `/export` and `/share`.

### Memory (`src/memory/`)
SQLite (bundled rusqlite) is authoritative; markdown documents under the memory
root are a one-way, block-scoped human-readable projection. FTS5 is probed at
runtime with a LIKE fallback. Attached in production at startup; a failed attach
degrades to a footer hint instead of an error. Details in `docs/MEMORY.md`.

## Invariants

1. The UI thread never does blocking I/O (tools use `spawn_blocking`; memory
   uses a synchronous `Mutex<Connection>`).
2. File tools cannot escape the workspace root (`..`, absolute paths, and
   symlinks are contained; memory has its own root confinement).
3. The API key is never logged and is stored `0600`.
4. Model output, memory records, board messages, files, and tool results are
   **untrusted data**; none of them can authorize privileged operations.
   Enforcement: central `ToolPolicy` at dispatch plus per-subsystem gates
   (`MemoryPolicy`, `disabled_tools`, the `/improve` proposal gate).
5. A self-edit is staged for review and never applied by a tool: applying is
   reachable only from the user's `/improve` approval. The hard self-edit
   denylist was removed by owner decision (2026-10-30); user approval and the
   regression gate remain the controls.
6. HACKER MODE capability sessions use typestate: `authorize_operation()` only
   exists on `Active` sessions, and invalid state transitions are unrepresentable
   (Rust) or runtime-guarded (Python).
