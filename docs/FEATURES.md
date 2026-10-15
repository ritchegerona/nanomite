# Features

Honest capability inventory as of 2026-10-14. Levels: **L0** not implemented ·
**L1** implemented, not adequately verified · **L2** tested under defined
conditions · **L3** integrated and regression-tested · **L4**
benchmark-validated and monitored. Evidence in `docs/AUDIT-2026-10-14.md`.

## Working (verified by the test suite)

- Streaming chat with any OpenAI-compatible endpoint (L2) — 21 presets,
  keyless localhost support, `/provider` `/models` `/connect` menus.
- Bounded agent loop with 9 tools, interrupts, usage accounting (L3).
- Workspace-sandboxed file tools; shell + key-only SSH; web fetch/search (L2,
  L2, L1) — see `docs/TOOLS.md`.
- Ratatui TUI: virtualized transcript, markdown, 4 themes (default
  `umbrella`, red-on-black), pickers, overlays, resize-safe (L3).
- Config persistence (`~/.config/nanomite/config.toml`, `0600`), sessions
  with persist/resume/markdown export (L3/L2).

## Landing (concurrent workstream — verify before claiming)

- Persistent memory: SQLite + FTS5 + markdown projection + `memory_*` tools
  (currently L1; 1 open test failure, integration in flight).
- File-intelligence extractors/index (`extract`, `index`, `html` modules +
  optional format deps: CSV/XML/TOML/YAML/ZIP/DOCX/PDF).
- Library-crate split (`src/lib.rs`) and `benches/perf` harness.

## Planned (L0 — not started)

Reasoning/execution modes · token budgeting and compaction · measured
performance work · offline mode with status + gating · capability registry ·
evolution lifecycle (propose → approve → experiment → evaluate → release →
rollback) · authenticated self-update · audit logging.

## Known limitations

- No central tool authorization; destructive core-tool calls are not confirmed.
- Web tools have no offline behavior; the app has no offline indicator.
- `/compact` summarization quality is unmeasured; context savings are unclaimed.
- Release installation is manual; no signature infrastructure.
- The working tree does not build at audit close (missing `benches/perf.rs`
  manifest target); last green baseline is git `d59f2ef` at 55 tests.
