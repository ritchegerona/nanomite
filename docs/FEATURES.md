# Features

Honest capability inventory. Levels: **L0** not implemented · **L1**
implemented, not adequately verified · **L2** tested under defined conditions ·
**L3** integrated and regression-tested · **L4** benchmark-validated and
monitored.

## Working (verified by the test suite)

- **Streaming chat** with any OpenAI-compatible endpoint (L3) — 21 presets,
  keyless localhost support, `/provider` `/models` `/connect` menus.
- **Multi-provider setup** (L3): per-provider API keys persist in `[providers]`,
  the legacy single key migrates, and `/model` queries every keyed provider in
  parallel, grouping results under provider headers. Picking a model switches
  provider, key, and model in one step.
- **Offline mode** (L2): `NANOMITE_OFFLINE=1` or `offline = true` disables
  network tools, shows a header indicator, and refuses `web_fetch` /
  `web_search` instead of silently reaching the network — see
  `docs/OFFLINE_MODE.md`.
- **Authenticated self-update** (L3): `nanomite update` verifies signed
  artifacts before pull; unsigned releases are refused unless explicitly
  allowed — see `src/update_signing.rs`.
- **Persistent skills registry** (L3): `SkillRegistry` records every tool as a
  skill — name, description, category, install method, package name, first/
  last-used timestamps, use count, and presence flag. Agent-callable tools:
  `skills_list`, `skills_known`. Persists to JSON across restarts.
- **Performance** (L3): removed per-call policy enforcement overhead, removed
  synchronous confirmation gates for external tools in non-Plan modes, auto-
  install only triggers on genuinely missing binaries.
- **Bounded agent loop** with interrupts, usage accounting (L3).
- **Workspace-sandboxed file tools**; shell + key-only SSH; web fetch/search
  (L3); git status/diff/log/commit/branch (L3); test runner (L3); symbol index
  (L3); plan/task list (L3) — see `docs/TOOLS.md`.
- **Ratatui TUI**: virtualized transcript, markdown, four themes (persisted),
  pickers, overlays, resize-safe down to 30×12 (L3).
- **Config persistence** (`~/.config/nanomite/config.toml`, `0600`), sessions
  with persist/resume/markdown export (L3/L2).
- **Persistent memory** attached in production: SQLite + FTS5 + markdown
  projection + the six `memory_*` tools, retrieval before each turn and capture
  after (L3) — see `docs/MEMORY.md`.
- **Multi-agent board**: `/panel <topic>` convenes four personas plus a chair,
  streamed live, persisted in SQLite (L3) — see `docs/ARCHITECTURE.md`.
- **Tool library** `/tools` (persisted; disabled tools are filtered from the
  schema list and refused at dispatch) (L3).
- **Self-improvement** `/improve`: propose → review diff → approve (with `.bak`,
  `cargo test --lib` gate and byte-exact rollback) or reject; append-only JSONL
  audit log — see `docs/SECURITY.md`.
- **Daily learning loop** `/learn`: research a topic with the agent's own tools,
  save durable lessons to memory (tags `learning` + `technique`), and stage
  concrete improvements for `/improve`. `/learn daily on` opts into one
  automatic session per local calendar day (off by default); `/learn status`,
  `digest`, `topics` manage the loop. Pure cadence logic, no network or model
  calls in `src/learn/` (L3) — see `docs/LEARNING.md`.
- **Two-stage Esc**: one Esc cooperatively interrupts a turn; a second within
  700 ms (or any Esc once cancelling) force-stops it, killing the blocked
  tool's process group (L3).
- **File-intelligence extractors** + the `read_document` tool; library-crate
  split (`src/lib.rs`) and the `benches/perf` harness (L3).
- **Central policy engine** (`agent/policy.rs`): `ToolPolicy` (Plan/Coder/
  Build/Hacker) enforced at dispatch; `/policy` (alias `/mode`) cycles modes
  with immediate effect (L3).
- **Plan/task list system**: `create_plan`/`get_plan`/`update_task`/
  `list_plans`/`delete_plan` with dependencies, tags, statuses, JSON
  persistence (L3).

## Known limitations

- Plan mode is enforced at dispatch (Plan ⊂ Coder ⊂ Build ⊂ Hacker), but the
  agent does not auto-create a plan when entering Plan mode — the user or
  model must call `create_plan` explicitly.
- Board messages and memory records are untrusted data and cannot authorize
  anything.
- The self-improvement regression gate runs `cargo test --lib`, so it is only
  meaningful inside a checkout.
- `/compact` summarization quality is unmeasured; context savings are unclaimed.
- On macOS the installer re-signs the copied binary ad-hoc
  (`codesign --force --sign -`), because the linker-only signature is rejected
  once the binary is copied to a new path.
- The daily learning turn spends model tokens and needs working tools
  (`web_fetch`/`browse` for external topics); it is off by default.
