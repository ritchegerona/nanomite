# Evolution

How NANOMITE improves itself: deliberately, measurably, reversibly. The full
lifecycle (proposal → approval → isolated experiment → tests/security checks →
baseline comparison → evaluation → controlled release → verification →
changelog → history) is **planned, not implemented**. This file defines the
maturity scale, the milestone order adapted to the actual code, and the rules
that already bind any change.

## Capability maturity levels

| Level | Meaning | Promotion criteria |
|---|---|---|
| L0 | Not implemented | — |
| L1 | Implemented, not adequately verified | code exists and compiles |
| L2 | Tested under defined conditions | unit/integration tests incl. negative cases, all green |
| L3 | Integrated and regression-tested | wired into runtime/UI, covered by the suite, docs updated |
| L4 | Benchmark-validated and monitored | reproducible benches + in-use metrics, no regressions |

Levels move on **evidence** (tests, benches, review), never on code volume or
changelog prose. Current levels: `docs/AUDIT-2026-10-14.md` §6.

## Milestones (adapted to the 2026-10-14 tree)

1. **Audit + baseline** — done (`docs/AUDIT-2026-10-14.md`). Keep
   `d59f2ef`/55-green as the rollback reference until superseded.
2. **Authorization + memory integration.** Central policy engine for tool
   calls (deny-by-default for privileged ops, user approvals, audit log);
   wire the memory subsystem into runtime/UI (`memory_tools.rs` is landing —
   finish, review, regression-test); root-cause the open consolidation
   failure. Do not proceed until the suite is fully green.
3. **File intelligence.** Land the extractor/index workstream with per-format
   extractors, incremental indexing, containment + macro-safety tests.
4. **Tokens + performance.** Instrumentation first (latency/token/tool stats),
   then retrieval injection, budgeting, compaction — every claim measured.
5. **Validated learning.** Capture task outcomes → lessons with provenance +
   validation state; dedupe/conflict/expiry; user review + poison resistance.
6. **Capability registry + history.** Machine-readable registry with levels,
   evidence links, and limitations; `/capabilities` UI; persisted evolution log.
7. **Isolated self-maintenance.** Branch/worktree workflow, fmt/build/
   lint/test/security gates, diff review, benchmark comparison, release
   candidate + verified rollback. Never touches the live checkout.
8. **Secure self-update.** Per `docs/RELEASES.md`; blocked on real release
   infrastructure — ship interfaces + honest "unsupported" status if absent.
9. **Local models + offline.** Health-checked local providers, offline
   indicator + gating + queueing, network-isolation tests.
10. **UI evolution.** Live indicators (provider locality, offline, memory,
    approvals, updates, evolution history) fed by real state; small-terminal
    and no-color safe.
11. **Docs + changelog + review.** This set of files maintained per-change;
    release notes generated from records, history, and test results.

## Rules (binding now)

- Reviewable milestones; no unreviewable mega-patches.
- Failing evaluations never replace working implementations.
- Destructive/privileged changes need explicit approval per policy.
- Docs and CHANGELOG update in the same change as the feature.
- Never report planned work as complete; label L1/L2 honestly.
