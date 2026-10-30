# Features

Honest capability inventory, updated 2026-10-26 (original audit 2026-10-14).
Levels: **L0** not implemented ·
**L1** implemented, not adequately verified · **L2** tested under defined
conditions · **L3** integrated and regression-tested · **L4**
benchmark-validated and monitored. Evidence in `docs/ARCHITECTURE.md` and test
suite.

## Working (verified by the 409-test suite)

- Streaming chat with any OpenAI-compatible endpoint (L3) — 21 presets,
  keyless localhost support, `/provider` `/models` `/connect` menus.
- Bounded agent loop with **33 tools**, interrupts, usage accounting (L3).
- Workspace-sandboxed file tools; shell + key-only SSH; web fetch/search (L3);
  git status/diff/log/commit/branch (L3); test runner (L3); symbol index (L3);
  plan/task list (L3) — see `docs/TOOLS.md`.
- Ratatui TUI: virtualized transcript, markdown, 4 themes (default
  `umbrella`, red-on-black, persisted), pickers, overlays, resize-safe down to
  30×12 (L3).
- Config persistence (`~/.config/nanomite/config.toml`, `0600`), sessions
  with persist/resume/markdown export (L3/L2).
- Persistent memory attached in production: SQLite + FTS5 + markdown
  projection + the six `memory_*` tools, retrieval before each turn and capture
  after (L3) — see `docs/MEMORY.md`.
- Multi-agent board: `/panel <topic>` convenes four personas plus a chair,
  streamed live, persisted in SQLite (L3) — see `docs/ARCHITECTURE.md`.
- Tool library `/tools` (33 tools, ✓/✗, persisted; disabled tools are filtered
  from the schema list and refused at dispatch) (L3).
- Self-improvement `/improve`: propose → review diff → approve (with `.bak`,
  `cargo test --lib` gate and byte-exact rollback) or reject; append-only JSONL
  audit log (the hard self-edit denylist was removed by owner decision
  2026-10-30) — see `docs/SECURITY.md`.
- File-intelligence extractors + the `read_document` tool; library-crate split
  (`src/lib.rs`) and the `benches/perf` harness (L3).
- **Central policy engine** (`agent/policy.rs`): `ToolPolicy` (Auto/Plan/Readonly)
  enforced at dispatch; `/policy` (alias `/mode`) cycles modes with immediate
  effect (L3).
- **Plan/task list system**: `create_plan`/`get_plan`/`update_task`/`list_plans`/`delete_plan`
  with dependencies, tags, statuses, JSON persistence (L3).

## Landing (verify before claiming)

- Token budgeting and `/compact` summarization quality (unmeasured).
- Offline mode with status + gating — still L0 (`docs/OFFLINE_MODE.md`).

## Planned (L0 — not started)

Reasoning/execution modes · evolution lifecycle beyond `/improve` (experiment
→ evaluate → release) · authenticated self-update.

## Known limitations

- Plan mode enforcement not yet active (policy=Plan restricts tools but
  agent doesn't auto-create plan; Stage F work).
- Board messages and memory records are untrusted data and cannot authorize
  anything.
- The self-improvement regression gate runs `cargo test --lib`, so it is only
  meaningful inside a checkout.
- Web tools have no offline behavior; the app has no offline indicator.
- `/compact` summarization quality is unmeasured; context savings are unclaimed.
- Release installation is manual; no signature infrastructure.
- Verification snapshot: `cargo test --lib` **409 passed / 0 failed**
  (fully offline) plus `tests/production_attach.rs` (1 integration test) +
  `tests/agent_integration.rs` (7 integration tests).