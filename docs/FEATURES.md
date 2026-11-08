# Features

Honest capability inventory, updated 2026-11-06 (original audit 2026-10-14).
Levels: **L0** not implemented ·
**L1** implemented, not adequately verified · **L2** tested under defined
conditions · **L3** integrated and regression-tested · **L4**
benchmark-validated and monitored. Evidence in `docs/ARCHITECTURE.md` and test
suite.

## Working (verified by the 774-test suite)

- Streaming chat with any OpenAI-compatible endpoint (L3) — 21 presets,
  keyless localhost support, `/provider` `/models` `/connect` menus.
- **Multi-provider setup** (L3): per-provider API keys persist in
  `[providers]`, the legacy single key migrates, and `/model` queries every
  keyed provider in parallel, grouping results under non-selectable provider
  headers. Picking a model switches provider + key + model in one step.
- **Offline mode** (L2): `NANOMITE_OFFLINE=1` or `offline = true` disables
  network tools, shows a `⚡` header indicator, and refuses `web_fetch` /
  `web_search` instead of silently reaching the network — see
  `docs/OFFLINE_MODE.md`.
- **Authenticated self-update** (L3): `nanomite update` verifies detached
  `<tag>.sig` artifacts with HMAC-SHA256 before pull; unsigned releases are
  refused unless `NANOMITE_UPDATE_ALLOW_UNSIGNED=1` — see `src/update_signing.rs`.
- Bounded agent loop with **34 tools**, interrupts, usage accounting (L3).
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
- Tool library `/tools` (34 tools, ✓/✗, persisted; disabled tools are filtered
  from the schema list and refused at dispatch) (L3).
- Self-improvement `/improve`: propose → review diff → approve (with `.bak`,
  `cargo test --lib` gate and byte-exact rollback) or reject; append-only JSONL
  audit log (the hard self-edit denylist was removed by owner decision
  2026-10-30) — see `docs/SECURITY.md`.
- **Daily learning loop** `/learn`: research a topic with the agent's own tools,
  save durable lessons to memory (tags `learning` + `technique`), and stage
  concrete improvements for `/improve`. `/learn daily on` opts into one
  automatic session per local calendar day (off by default); `/learn status`,
  `digest`, `topics` and `topic <t>` manage the loop. Pure cadence logic, no
  network or model calls in `src/learn/` (L3) — see `docs/LEARNING.md`.
- **Hacker knowledge recall**: `/hacker techniques [query]` surfaces
  `technique`-tagged memories, and HACKER MODE's prompt directs recall/save of
  reusable tradecraft (L3) — see `docs/HACKER_MODE.md`.
- **Two-stage Esc**: one Esc cooperatively interrupts a turn; a second within
  700 ms (or any Esc once cancelling) force-stops it, killing the blocked
  tool's process group (L3).
- File-intelligence extractors + the `read_document` tool; library-crate split
  (`src/lib.rs`) and the `benches/perf` harness (L3).
- **Central policy engine** (`agent/policy.rs`): `ToolPolicy` (Auto/Plan/Readonly)
  enforced at dispatch; `/policy` (alias `/mode`) cycles modes with immediate
  effect (L3).
- **Plan/task list system**: `create_plan`/`get_plan`/`update_task`/`list_plans`/`delete_plan`
  with dependencies, tags, statuses, JSON persistence (L3).
- **VPN/TOR modules** (feature-gated): `nanomite-vpn` binary and Tor control
  library behind `--features vpn,tor` (913 tests with both features enabled).

## Landing (verify before claiming)

- Token budgeting and `/compact` summarization quality (unmeasured).

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
- Verification snapshot: `cargo test --lib` **774 passed / 0 failed**
  (fully offline) plus `tests/production_attach.rs` (1 integration test) and
  `tests/agent_integration.rs` (8 integration tests). With `--features vpn,tor`
  the suite runs **913 tests**.

## Planned (L0 — not started)

Reasoning/execution modes · evolution lifecycle beyond `/improve`
(experiment → evaluate → release).
