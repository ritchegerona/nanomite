# Evolution

How NANOMITE improves itself: deliberately, measurably, reversibly. The full
lifecycle (proposal → approval → isolated experiment → tests/security checks →
baseline comparison → evaluation → controlled release → verification →
changelog → history) is **partially implemented** — see milestones below.
This file defines the maturity scale, the milestone order adapted to the
actual code, and the rules that already bind any change.

## Capability maturity levels

| Level | Meaning | Promotion criteria |
|---|---|---|
| L0 | Not implemented | — |
| L1 | Implemented, not adequately verified | code exists and compiles |
| L2 | Tested under defined conditions | unit/integration tests incl. negative cases, all green |
| L3 | Integrated and regression-tested | wired into runtime/UI, covered by the suite, docs updated |
| L4 | Benchmark-validated and monitored | reproducible benches + in-use metrics, no regressions |

Levels move on **evidence** (tests, benches, review), never on code volume or
changelog prose. Current levels: `docs/AUDIT-2026-10-14.md` §6 (outdated —
see `docs/ARCHITECTURE.md` for current module map).

## Milestones (adapted to the 2026-10-26 tree)

| # | Milestone | Status | Notes |
|---|---|---|---|
| 1 | Audit + baseline | ✅ Done | `docs/AUDIT-2026-10-14.md` |
| 2 | Authorization + memory integration | ✅ Done | Central `ToolPolicy` (Auto/Plan/Readonly) + memory L3 |
| 3 | Git tool suite | ✅ Done | 5 tools: status/diff/log/commit/branch |
| 4 | Test runner | ✅ Done | `run_tests` (cargo/npm/pytest/jest/go/custom) |
| 5 | Symbol index | ✅ Done | `index_symbols`/`find_symbol`/`find_references` |
| 6 | Policy engine + `/policy` | ✅ Done | Auto/Plan/Readonly, persisted, immediate effect |
| 7 | Plan/task list system | ✅ Done | 5 tools with deps/tags/status, JSON persistence |
| 8 | **Plan mode enforcement** | 🔄 Next | Restrict to Safe + plan tools in Plan mode |
| 9 | Session persistence | ⬜ Backlog | Serialize/restore conversation + plan state |
| 10 | Hooks/event system | ⬜ Backlog | Pre/post tool call hooks, guardrails |
| 11 | External Tool SDK | ⬜ Backlog | User-defined tools via `~/.config/nanomite/tools/*.toml` |
| 12 | Parallel worktrees | ⬜ Backlog | Multiple agents on git worktrees |
| 13 | File intelligence (extractors/indexing) | ⬜ Backlog | Per-format extractors, incremental indexing |
| 14 | CI + Packaging | ⬜ Backlog | GitHub Actions, crates.io, COPR, Flatpak |
| 15 | Docs overhaul | ⬜ In progress | 11 docs being updated |

## Rules (binding now)

- Reviewable milestones; no unreviewable mega-patches.
- Failing evaluations never replace working implementations.
- Destructive/privileged changes need explicit approval per policy.
- Docs and CHANGELOG update in the same change as the feature.
- Never report planned work as complete; label L1/L2 honestly.