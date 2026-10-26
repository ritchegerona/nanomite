# Wiki Log

Append-only timeline of wiki activity.

## [2026-10-16] seed | initial wiki created
Seeded the wiki from `docs/AUDIT-2026-10-14.md`, `docs/FEATURES.md`,
`docs/OFFLINE_MODE.md`, `docs/SECURITY.md`, `AGENTS.md`, and `src/`.
Created `concepts/capability-gaps-l0` and
`concepts/reasoning-and-context-controls`.

## [2026-10-16] query | are the four L0 gaps features to add?
Answered: all four are real but two are only half-true. Reasoning effort
(`/reasoning`) and `/compact` exist; central tool authorization and the
offline indicator do not. Filed the analysis into
`concepts/capability-gaps-l0`.

## [2026-10-16] ingest | Phase 1 implementation of central tool authorization engine
Implemented `ToolPolicy` (auto/plan/readonly) with risk classification,
central enforcement in `run_tool`, config integration, `/policy` command
and UI indicator. All 376 tests pass.

## [2026-10-16] ingest | Phase 1 implementation summary
Created `implementation-summary.md` documenting the complete Phase 1
implementation of the central tool authorization engine.
