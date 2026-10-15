# Changelog

Every entry describes a verified change. Labels: implemented+tested,
implemented unverified, planned, experimental, unsupported. Planned work is
never listed as released.

## [0.2.0] — 2026-10-15

### Added

- **Persistent memory subsystem** (`src/memory/`): SQLite (bundled rusqlite)
  as the authoritative store, human-editable markdown as a one-way projection,
  FTS5 search probed at runtime with `LIKE` fallback, six `memory_*` tools,
  credential detection/redaction on the way in and out, path confinement,
  integrity-checked backups and validated import/export. Status: implemented
  and tested (part of the 242-test green suite).
- **File-intelligence extractors** (`src/extract/`): one document model plus
  per-format extractors — text/markdown/html/code/jsonl/rtf always compiled;
  xml/json/toml/yaml, docx/odt/pdf and csv/xlsx/ods behind cargo features.
  Originals are never modified; all work is bounded (size, output, sections,
  entries, time). Status: implemented and tested.
- **Edit tracking in the transcript**: `edit_file` tool rows now carry the
  edited file path and replacement count (`ToolRow::edit_path`,
  `ToolRow::edit_replacements`), shown in the tool detail line — you can see
  which file the agent changed and by how much. Status: implemented,
  build-verified.
- `/memory` and `/copy` commands wired into the command router
  (`PickerMode::Memory`, `App::open_memory_picker`, `App::copy_selection`).
  Status: implemented, build-verified.
- `docs/`: `ARCHITECTURE.md`, `TOOLS.md`, `MEMORY.md`, `SECURITY.md`,
  `OFFLINE_MODE.md`, `PERFORMANCE.md`, `EVOLUTION.md`, `RELEASES.md`,
  `FEATURES.md`, `AUDIT-2026-10-14.md`.

### Changed

- **Unlimited agent loop by default**: `max_steps` and `max_tool_calls` now
  default to `usize::MAX` (previously 12 / 24). `NANOMITE_MAX_STEPS` and
  `NANOMITE_MAX_TOOL_CALLS` environment variables override them. The
  "(stopped after N steps)" note only appears when a limit is explicitly set.
- `AgentConfig` is initialized from `Config` with provider/model labels used
  for memory provenance only (never for behaviour).
- Mouse scrolling reversed (`ScrollUp`/`ScrollDown` deltas swapped) — natural
  scrolling direction in the transcript.
- README refreshed: 242-test suite, unlimited step defaults, `extract/` in
  the architecture map, `NANOMITE_MAX_TOOL_CALLS` documented.

### Fixed

- Full build is green again: `cargo check` (lib + bin) passes and the offline
  suite is 242 passed / 0 failed (`cargo test --lib`, ~1.3 s). The previously
  reported broken state (46 errors in `src/extract/`, dead `DiffView` stub,
  missing bench target blocking `cargo test`, failing consolidation test) is
  resolved.
- `src/memory/store.rs` `export()`: dropped the connection mutex guard before
  the nested `markdown_index()` call, fixing a deadlock that hung every
  export-path test.
- `src/memory/security.rs`: byte-to-char deref in JWT scanning and a
  multi-character literal in the placeholder filter.

### Known limitations

- `benches/perf.rs` uses `#![feature(test)]` and only compiles on nightly;
  on stable use `cargo test --lib` (plain `cargo test` also builds bench
  targets and will fail with E0554/E0601). Documented, not a regression.
- No central tool authorization engine (memory gates itself); offline mode,
  telemetry, self-update and the evolution lifecycle remain future work — see
  `docs/FEATURES.md`.

## [Unreleased]

Nothing pending.

## [0.1.0] — 2026-10-13 (prior work, per git history)

- Initial release: ratatui TUI coding agent with OpenAI-compatible streaming,
  9 tools, 21 provider presets, sessions, 4 themes, 55 offline tests.
  Status: implemented and tested (last green baseline: `d59f2ef`).
