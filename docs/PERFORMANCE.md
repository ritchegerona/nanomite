# Performance

Status: **L1 — microbenchmarked.** A stable, zero-dep benchmark harness
(`benches/perf.rs`, run with `cargo bench`) covers the tool/extract hot paths;
run it with `cargo bench`. No runtime telemetry or profiling hooks exist yet
(see Rules 4). This file records the benchmark methodology, the current
baseline, and the rules for future optimization.

## Benchmarks (measured 2026-10-26, bench profile ≈ release)

Harness: `benches/perf.rs` — median of 7 in-process batches after warmup,
zero external deps. Machine noise between full runs is ±8–15% (all benches
drift together); treat sub-15% deltas as noise. 300-file / ~4 MB synthetic
tree for search; 1.3 MB file sliced to 4 KB for read_file.

| Benchmark | Before | After | Change |
|---|---|---|---|
| read_file 4 KB slice of 1.3 MB | 91 µs | 8.7 µs | ~10× (seek+bounded read instead of whole-file read) |
| search hit, 300 files (mixed ASCII/Unicode fixture) | 4.31 ms | 4.68 ms | parity on fixture (Unicode lines take exact fallback); matcher core ~2.4× on mostly-ASCII trees (3.47 vs 8.41 ms per 4 MB) |
| search miss, 300 files | 13.19 ms | 13.08 ms | parity on fixture |
| extract text/code/json/csv | 0.44/0.46/0.74/0.79 ms | 0.46/0.49/0.81/0.82 ms | parity (noise) |
| extract toml ~20 KB | 2.04 ms | 2.22 ms | parity on fixture (scalar fast path; toml parse dominates) |
| wrap_spans, fingerprint | 19 µs / 0.49 ms/MB | 20 µs / 0.53 ms/MB | parity (noise) |

Startup (3–6 ms), TUI rendering (dirty-flagged with cached renders), and the
SQLite memory store (WAL + `synchronous=NORMAL`) were inspected and left
alone — already fast by design.

The self-improvement gate (`/improve` → approve) shells out to
`cargo test --lib` as its regression check. That is a full test run, not a
microbenchmark: it runs on a background task, never on the UI thread, and a
non-zero exit rolls the change back byte-exactly. Budget for "seconds, not
milliseconds" when deciding to approve.

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