# Architecture

Single-crate Rust binary (`nanomite`, edition 2021) over a library crate:
`src/main.rs` owns the terminal, everything testable lives in `src/lib.rs`,
which declares `agent`, `app`, `board`, `commands`, `config`, `extract`,
`improve`, `memory` (behind a deliberate `#[allow(dead_code)]` for its API
superset), `session`, `tools`, `ui`. Updated 2026-10-26.

```
src/main.rs ── tokio event loop ──▶ App (src/app.rs) ──▶ ui::render
       │  (input: sync event::read() on a dedicated OS thread —
       │   never `EventStream`, whose polls are cancelled by the
       │   90ms tick and drop partial SGR mouse sequences)
       │                                │
       │                          AgentRuntime ──▶ AgentProvider ──▶ HTTPS
       │                                │              (openai.rs: any /v1 endpoint)
       │                          ToolSet ──▶ files / document / exec / web
       │                                            ├─▶ memory*   (policy-gated)
       │                                            ├─▶ improve*  (stage only)
       │                                            ├─▶ git*       (workspace-confined)
       │                                            ├─▶ test*      (run_tests)
       │                                            ├─▶ index*     (symbol index)
       │                                            └─▶ plan*      (task plans)
       │                          Swarm ──▶ BoardEngine (src/board/)
       └─ Config (defaults <- config.toml <- env)   Session (JSON v1)
```

## Layers

- **UI (`src/ui/`)** — ratatui immediate-mode rendering. `chat.rs` virtualizes
  the transcript (only visible entries materialize; render trees cached).
  `composer.rs` multi-line input, `panels.rs` header/footer/overlays/pickers,
  `theme.rs` 4 palettes (default `umbrella`: red on near-black),
  `text.rs` wrapping + minimal markdown. All state lives in `App`; rendering is
  a pure function of `&App` per frame, redrawn only when dirty.
- **App (`src/app.rs`)** — the state machine: key handling, overlay stack,
  provider/model pickers, command router (`src/commands.rs`, 28 commands
  including `/memory`, `/copy`, `/tools`, `/panel`, `/improve`, `/policy`,
  `/models`, `/prev`, `/next`, all parsed locally), session persist/restore,
  engine pump. Agent turns run as background tasks feeding `AgentEvent`s;
  the UI never blocks.
- **Agent (`src/agent/`)** — `provider.rs` defines the `AgentProvider` trait
  (`stream_chat`, `list_models`, `chat_completion` helper) and the streaming
  event model (`Delta` / `ToolCall` / `Usage` / `Done`) with a fragment
  accumulator for out-of-order tool-call chunks. `runtime.rs` runs one turn:
  system prompt (+ the user's `prompt_file` appendix) + history + input →
  stream → dispatch tools via `spawn_blocking` → append results → repeat,
  bounded by `max_steps` and `max_tool_calls` (both unlimited by default,
  overridable by env), a per-step timeout, and cooperative `Cancel`.
  `policy.rs` enforces `ToolPolicy` (Auto / Plan / Readonly) at dispatch.
  `swarm.rs` runs several providers in parallel and is what the board convenes
  on.
- **Tools (`src/tools/`)** — `catalog.rs` holds the inventory as data
  (`ToolSpec { name, description, category }`); tests pin it to the exposed
  schema list. File tools resolve every path against the workspace root and
  refuse escapes; `document_tools.rs` (`read_document`) reuses the bounded
  `extract/` pipeline. Exec tools run `sh -c` in the workspace with a hard
  timeout and capped output; SSH is key-only with `BatchMode=yes`. Web tools
  speak http(s) only and truncate before returning text to the model. Git tools
  run `git` directly in the workspace. Test runner supports cargo, npm, pytest,
  jest, go, and custom commands. Symbol index uses ctags with regex fallback.
  Plan tools create/update/list/delete task plans with dependencies. Dispatch
  is a `match` in `runtime.rs::run_tool` with a **central policy engine**
  (`agent/policy.rs`): `ToolPolicy` (Auto / Plan / Readonly) is checked before
  dispatch; a disabled tool is filtered from the schema list *and* refused at
  dispatch.
- **Board (`src/board/`)** — `personas.rs` defines Architect, Engineer,
  Scientist and Critic plus a Chair; `BoardEngine::convene` runs two rounds of
  parallel discussion through `Swarm` and then a chair synthesis, checking for
  cancellation between rounds; `store.rs` persists `board_messages` in SQLite
  under `~/.local/share/nanomite/board/` (`NANOMITE_BOARD_DIR`). Opened with
  `/panel <topic>`, streamed live into an overlay, `esc` cancels.
- **Self-improvement (`src/improve/`)** — `proposal.rs` validates staged
  proposals (size, extension, content-difference, root confinement) and
  enforces the hard self-edit denylist (`src/memory/security.rs`,
  `src/memory/policy.rs`, `src/main.rs`); `diff.rs` builds unified diff
  artifacts; `runner.rs` writes `.bak` backups, runs `cargo test --lib` and
  rolls back byte-exactly on failure; `audit.rs` appends every decision to a
  JSONL log. The agent can only stage (`propose_self_edit`); applying requires
  the user pressing `a` in `/improve`.
- **Providers (`src/config.rs` + `src/agent/openai.rs`)** — one HTTP client
  serves every preset (17 cloud + ollama/lmstudio/vllm/llamacpp + `custom`).
  Setup persists to `~/.config/nanomite/config.toml` (atomic write, `0600`);
  precedence is built-in defaults < file < environment (`NANOMITE_*`). The
  file also carries `theme`, `disabled_tools`, `tool_policy` and `prompt_file`.
- **Sessions (`src/session.rs`)** — `StoredSession` v1 JSON: provider, model,
  cwd, transcript entries. Markdown export for `/export` and `/share`.
- **Memory (`src/memory/`)** — SQLite (bundled) is authoritative; markdown
  documents under the memory root are a one-way, block-scoped human-readable
  projection. FTS5 is probed at runtime with a LIKE fallback. **Attached in
  production**: `MemoryEngine` is created at startup, retrieval runs before each
  request and capture after each turn, all through `MemoryPolicy`; a failed
  attach degrades to a footer hint instead of an error. Details in
  `docs/MEMORY.md`.

## Invariants

1. The UI thread never does blocking I/O (tool calls go through
   `spawn_blocking`; memory uses a synchronous `Mutex<Connection>` for the
   same reason — see `docs/PERFORMANCE.md`).
2. File tools cannot escape the workspace root (`..`, absolute paths, and
   symlinks are contained; memory has its own root confinement).
3. The API key is never logged and is stored `0600`.
4. Model output, memory records, board messages, files, and tool results are
   **untrusted data**; none of them can authorize privileged operations.
   Enforcement: central `ToolPolicy` at dispatch + per-subsystem gates
   (`MemoryPolicy`, `disabled_tools`, the `/improve` proposal gate).
5. A self-edit may never target the safety layer: `src/memory/security.rs`,
   `src/memory/policy.rs`, `src/main.rs`. And apply is reachable only from the
   user's `/improve` approval, never from a tool call.