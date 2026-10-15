# Architecture

Single-crate Rust binary (`nanomite`, edition 2021). The binary owns the
terminal; a concurrent workstream is extracting a library crate (`src/lib.rs`,
incomplete at last audit — modules `context`, `extract`, `html`, `index`,
`telemetry` declared but absent).

```
src/main.rs ── tokio event loop ──▶ App (src/app.rs) ──▶ ui::render
       │                                │
       │                          AgentRuntime ──▶ AgentProvider ──▶ HTTPS
       │                                │              (openai.rs: any /v1 endpoint)
       │                          ToolSet ──▶ files / exec / web
       │                                            └─▶ memory* (landing)
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
  provider/model pickers, command router (`src/commands.rs`, 20 commands
  including the newly added `/memory`, all parsed locally), session
  persist/restore, engine pump. Agent turns run as
  background tasks feeding `AgentEvent`s; the UI never blocks.
- **Agent (`src/agent/`)** — `provider.rs` defines the `AgentProvider` trait
  (`stream_chat`, `list_models`, `chat_completion` helper) and the streaming
  event model (`Delta` / `ToolCall` / `Usage` / `Done`) with a fragment
  accumulator for out-of-order tool-call chunks. `runtime.rs` runs one turn:
  system prompt + history + input → stream → dispatch tools via
  `spawn_blocking` → append results → repeat, bounded by `max_steps` (12),
  `max_tool_calls` (24), per-step timeout, and cooperative `Cancel`.
- **Tools (`src/tools/`)** — three groups behind `ToolSet`. File tools resolve
  every path against the workspace root and refuse escapes. Exec tools run
  `sh -c` in the workspace with a hard timeout and capped output; SSH is
  key-only with `BatchMode=yes`. Web tools speak http(s) only and truncate
  before returning text to the model. Dispatch is a `match` in
  `runtime.rs::run_tool` — there is **no central policy engine** (see
  `docs/SECURITY.md`).
- **Providers (`src/config.rs` + `src/agent/openai.rs`)** — one HTTP client
  serves every preset (17 cloud + ollama/lmstudio/vllm/llamacpp + `custom`).
  Setup persists to `~/.config/nanomite/config.toml` (atomic write, `0600`);
  precedence is built-in defaults < file < environment (`NANOMITE_*`).
- **Sessions (`src/session.rs`)** — `StoredSession` v1 JSON: provider, model,
  cwd, transcript entries. Markdown export for `/export` and `/share`.
- **Memory (`src/memory/`)** — SQLite (bundled) is authoritative; markdown
  documents under the memory root are a one-way, block-scoped human-readable
  projection. FTS5 is probed at runtime with a LIKE fallback. Details in
  `docs/MEMORY.md`. Integration into the agent loop/UI is in progress.

## Invariants

1. The UI thread never does blocking I/O (tool calls go through
   `spawn_blocking`; memory uses a synchronous `Mutex<Connection>` for the
   same reason — see `docs/PERFORMANCE.md`).
2. File tools cannot escape the workspace root (`..`, absolute paths, and
   symlinks are contained; memory has its own root confinement).
3. The API key is never logged and is stored `0600`.
4. Model output, memory records, files, and tool results are **untrusted
   data**; none of them can authorize privileged operations (enforced by
   code structure today, not by a policy engine — closing that gap is
   Milestone 2 work).

## What is deliberately absent (2026-10-14)

Central authorization, capability registry, evolution/update machinery,
telemetry, benchmarks, file index — see `docs/AUDIT-2026-10-14.md` §5 and
`docs/EVOLUTION.md`.
