status: active
phase: 0 (recon complete) → planning gate
task: Close the four L0 capability gaps (reasoning/execution modes, central tool
      authorization, offline mode, /compact quality) — all four requested by user.
slug: nanomite-capability-gaps
next: present phased plan + 2 design decisions, then Phase 1 (auth engine)

## Frozen constraints
- Rust only; no new deps unless justified.
- Keep `cargo test --lib` (currently 370 passed / 0 failed) green; clippy -D warnings clean; fmt clean.
- Do NOT edit parent snapshots (nanomite.orig/, backup_*/). Repo root = /home/ritche/Project/NANOMITE/nanomite.
- Never break the async worker: file IO already runs via spawn_blocking; a new
  approval gate must not block a runtime worker.
- Subagent models (haiku/sonnet) are unavailable on this host — implement in the
  orchestrator; deepwork @oracle gates are substituted with self-review + user checkpoints.

## Phases (order = dependency order)
1. Central tool authorization engine — `src/agent/policy.rs`: risk classes
   (safe / write / external), policy modes (auto / plan / full / readonly),
   enforced at the single choke point `run_tool`, plus schema filtering, config
   `tool_policy` + env, tests.
2. Reasoning/execution modes — `AgentMode` (plan/execute), `/mode` command +
   keybind, header display; plan mode maps onto the policy engine (deny/ask writes).
3. Offline mode — config `offline` + env, gate web tools at `run_tool`, header
   indicator, tests.
4. /compact quality measurement — compact report (tokens before/after) + a
   deterministic retention probe test + docs update.

## Gates
- After each phase: `cargo test --lib`, `cargo test --test production_attach`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`; update this
  file; user checkpoint.

## Log
- [x] recon: read runtime.rs (run_tool choke point), config.rs (Config/Stored),
  tools/catalog.rs (18 tools), commands.rs, ui/mod.rs (render surfaces).
- [x] confirmed 4 gaps real (2 partial): reasoning_effort + /compact exist;
  central auth + offline indicator do not. See .opencode/wiki/concepts/capability-gaps-l0.md.
