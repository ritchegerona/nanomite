# Performance

Status: **L0 — unmeasured.** No telemetry, no benchmarks, no profiling hooks
exist. (`Cargo.toml` carries a `[[bench]] perf` section but `benches/` is
absent — the tree does not currently build because of it.) This file records
the baseline procedure and the rules for future optimization.

## Baselines (measured 2026-10-14, dev profile)

- `cargo test` (55 tests, pre-memory wiring): **0.36 s**, fully offline.
- `cargo build` (dev, warm): **0.16 s** incremental.
- Startup time, first-token latency, tool durations, retrieval latency, memory
  use, DB query times, cache hits: **not measured**.

## Rules

1. **Profile before optimizing.** Keep the tokio runtime, the sync
   `Mutex<Connection>` memory design, and the render pipeline unless
   measurements justify change.
2. Every optimization ships with a reproducible benchmark and before/after
   numbers in the CHANGELOG entry. Estimated tokens are labeled estimates;
   provider-reported usage is labeled as such; savings claims require a
   comparable baseline.
3. Budgets and caps are load-bearing: tool output caps, page limits
   (`HARD_ROW_CAP`), bounded queues, timeouts, and cancellation must survive
   any performance refactor.
4. Planned instrumentation (Milestones 4/10): per-turn latency/token/tool
   accounting, retrieval latency, DB query stats, UI-visible resource panel
   fed by real runtime state — never hardcoded values.

## Token/context efficiency (planned, Milestone 4)

Retrieval injection instead of whole-store dumps, chunk dedup, bounded tool
output, model-aware estimation, context budgeting, history compaction with
constraint preservation. Security policies and unresolved decisions are never
dropped to meet a token target.
