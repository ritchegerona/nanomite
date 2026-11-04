# Architecture

Single-crate Rust binary (`nanomite`, edition 2021) over a library crate:
`src/main.rs` owns the terminal, everything testable lives in `src/lib.rs`,
which declares `app`, `agent`, `board`, `commands`, `config`, `extract`,
`hacker`, `improve`, `learn`, `memory` (behind a deliberate `#[allow(dead_code)]` for its
API superset), `session`, `tools`, `ui`. Updated 2026-11-04.

```
src/main.rs ── tokio event loop ──▶ App (src/app/mod.rs) ──▶ ui::render
       │  (input: sync event::read() on a dedicated OS thread —
       │   never EventStream, whose polls are cancelled by the
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

- **App (`src/app/mod.rs`)** — the state machine: key handling, overlay stack,
  provider/model pickers, command router (`src/commands.rs`, 42 commands
  including `/memory`, `/copy`, `/tools`, `/panel`, `/improve`, `/policy`,
  `/models`, `/learn`, `/prev`, `/next`, all parsed locally), session
  persist/restore, engine pump. Agent turns run as background tasks feeding
  `AgentEvent`s; the UI never blocks.

  **`src/app/hacker.rs`** — extracted HACKER MODE slash-command dispatch:
  `/hacker`, `/hacker run`, `/hacker kali`, `/hacker findings`,
  `/hacker report`, `/hacker assessment`, `/hacker exit`. Wires
  `HackerSessionTyped` (typestate capability gate) into the existing
  coordinator so every `/hacker run` passes through
  `authorize_operation()` before execution.

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

- **HACKER MODE (`src/hacker/`)** — an authorized cybersecurity operations
  environment *separate* from `ToolPolicy::Hacker`. It layers a typed
  capability-session state machine, an assessment scope, an independent
  authorization gate, an append-only audit trail, and a module registry on top
  of the existing nanomite agent runtime. Disabled by default.

  - `mod.rs` — `HackerSession` (enabled/active state, assessment, `resume`
    validation, gated auto-approve bypass flag), `HackerSessionTyped` typestate
    wrapper (`Discovered → Authorized → Active → Closed`).
  - `capability.rs` — `OpCapability` unforgeable token (scope_id, max_risk,
    allowed_modules, expiry), `CapabilitySession` typestate where
    `authorize_operation()` only exists on `Active`, `CapError` fail-fast
    domain errors, `CapEvent` audit trail on every attempt.
  - `context.rs` — `AssessmentContext` (scope, window, authorization ref).
  - `scope.rs` — IPv4/IPv6 CIDR + domain matching, resolved-address and
    metadata-endpoint guards (`169.254.169.254`).
  - `authorization.rs` — `authorize()` gate returning `Allowed{risk}` or
    `Denied{code, reason}` with 10 refusal conditions.
  - `risk.rs` — `RiskLevel` (informational < low < medium < high < critical),
    `OperationCategory` floors.
  - `audit.rs` — append-only JSONL audit log with secret scrubbing.
  - `registry.rs` — `HackerModule` trait + 12 built-in module descriptors
    (`recon`, `network`, `web`, `vuln`, `forensics`, `osint`, `threat-intel`,
    `password`, `wireless`, `exploitation`, `post-exploitation`, `reporting`).
  - `coordinator.rs` — single operation path:
    authorize → resolve/re-check → execute → persist → audit.
  - `modules/` — `recon`, `network`, `web`, `vuln`, `exploitation`, `kali`,
    `reporting`; pure logic + injected resolver/connector/fetcher/probe/process
    traits. `exploitation` is **plan-only** (no payloads). `kali` is a curated
    bridge to installed Kali assessment tools via fixed argv templates.
  - `findings.rs` — typed `Finding` / `Confidence` / `FindingStatus`.
  - `store.rs` — SQLite findings + evidence + assessments store with SHA-256
    evidence digests, `verify_evidence`, status triage, md/JSON/SARIF export,
    `spawn_blocking`-driven.

- **Board (`src/board/`)** — `personas.rs` defines Architect, Engineer,
  Scientist and Critic plus a Chair; `BoardEngine::convene` runs two rounds of
  parallel discussion through `Swarm` and then a chair synthesis, checking for
  cancellation between rounds; `store.rs` persists `board_messages` in SQLite
  under `~/.local/share/nanomite/board/` (`NANOMITE_BOARD_DIR`). Opened with
  `/panel <topic>`, streamed live into an overlay, `esc` cancels.

- **Self-improvement (`src/improve/`)** — `proposal.rs` validates staged
  proposals (size, extension, content-difference, root confinement); the hard
  self-edit denylist (`src/memory/security.rs`, `src/memory/policy.rs`,
  `src/main.rs`) was removed by owner decision 2026-10-30. `diff.rs` builds
  unified diff artifacts; `runner.rs` writes `.bak` backups, runs
  `cargo test --lib` and rolls back byte-exactly on failure; `audit.rs`
  appends every decision to a JSONL log. The agent can only stage
  (`propose_self_edit`); applying requires the user pressing `a` in `/improve`.

- **Learning (`src/learn/`)** — the daily learning loop is pure logic: cadence
  state (`state.json`), topic rotation, slugging and `learning_prompt`, the
  instruction text for a normal turn. `/learn <topic>` starts a research turn
  that saves lessons to memory (tags `learning` + `technique`) and stages
  improvements with `propose_self_edit`; `/learn daily on` opts into one
  automatic session per local day (`[learn] daily`, off by default). No network
  or model calls inside the module. Details in `docs/LEARNING.md`.

- **Providers (`src/config.rs` + `src/agent/openai.rs`)** — one HTTP client
  serves every preset. Setup persists to `~/.config/nanomite/config.toml`
  (atomic write, `0600`); precedence is built-in defaults < file < environment
  (`NANOMITE_*`). Per-provider API keys live in `[providers]` (the active key is
  mirrored in `api_key`), so `/model` can query every keyed provider in parallel
  and group the result under provider headers. The file also carries `theme`,
  `disabled_tools`, `tool_policy`, `prompt_file` and `[learn]`.

- **Sessions (`src/session.rs`)** — `StoredSession` v1 JSON: provider, model,
  cwd, transcript entries. Markdown export for `/export` and `/share`.

- **Memory (`src/memory/`)** — SQLite (bundled) is authoritative; markdown
  documents under the memory root are a one-way, block-scoped human-readable
  projection. FTS5 is probed at runtime with a LIKE fallback. **Attached in
  production**: `MemoryEngine` is created at startup, retrieval runs before each
  request and capture after each turn, all through `MemoryPolicy`; a failed
  attach degrades to a footer hint instead of an error. Details in
  `docs/MEMORY.md`.

- **Python toolkit (`src/nanomite/`)** — minimal-resource security and AI
  toolkit for old laptops:
  - `core/` — typestate pattern for `NetworkDevice`
    (`Discovered → Authenticated → Ready → Closed`) and `Session`
    (`Unauthenticated → Authenticated → Active → Closed`), frozen slotted
    dataclasses, logging instead of print, pure/Io separation.
  - `cracking/` — password-audit engine with context-managed sessions,
    `Result`/`Option` ADTs, streaming wordlist generator (O(1) RAM), CTI-grade
    reports (JSON/MD/SARIF), argparse CLI with exit codes.
  - `audit_tool.py` — reference implementation of the advanced Python coding
    standard.
  - `errors.py` — custom exception hierarchy (`NanomiteError` → domain errors).

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
5. A self-edit is staged for review and never applied by a tool: applying is
   reachable only from the user's `/improve` approval. The former hard denylist
   on `src/memory/security.rs`, `src/memory/policy.rs` and `src/main.rs` was
   removed by owner decision 2026-10-30; user approval and the regression gate
   remain the controls.
6. HACKER MODE capability sessions use typestate: `authorize_operation()` only
   exists on `Active` sessions, and invalid state transitions are compile-time
   unrepresentable (Rust) or runtime-guarded (Python).
