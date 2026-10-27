# Changelog

Every entry describes a verified change. Labels: implemented+tested,
implemented unverified, planned, experimental, unsupported. Planned work is
never listed as released.

## [0.5.0] — 2026-10-27

### Added

- **Plan mode enforcement**: `ToolPolicy::Plan` allows the plan tools
  (create/get/update/list) alongside Safe tools; every other mutation is
  refused at dispatch with guidance to `create_plan`. The footer shows a
  persistent plan-mode badge with the newest plan's name. Status: implemented
  and tested (437-test suite).
- **Session persistence v2**: settings (tool policy, disabled tools,
  reasoning, theme) and plan snapshots travel with the conversation; restore
  applies them back to the live config/runtime. The `/sessions` picker groups
  per workspace. Save filenames are nanosecond-unique (fixed a concurrent-save
  overwrite). Status: implemented and tested (v2 round-trip integration test).
- **Hooks / event system** (`src/hooks.rs`): five lifecycle events
  (pre/post tool call, turn start/end, policy change), JSON payloads on
  stdin, per-hook timeouts with process-group kill, `hooks.json` manifest +
  auto-discovery under `hooks/<event>/`, `NANOMITE_HOOKS=0` kill switch.
  Deny hooks block pre-tool calls (`code: hook_denied`); all other hook
  failures are advisory. Status: implemented and tested.
- **External tool SDK** (`src/tools/external_tools.rs`): TOML manifests in
  `~/.config/nanomite/tools/` (`NANOMITE_TOOLS_DIR` override) with
  load-time schema validation; arguments as JSON on stdin (no shell
  interpolation), bounded stdout back to the model, dispatch through the same
  policy/hook gate as built-ins; listed in `/tools`. Status: implemented and
  tested (incl. end-to-end run + timeout kill tests).
- **Parallel worktrees** (`src/worktree.rs`): `nanomite --worktree <branch>`
  creates/reuses a managed worktree under `.nanomite/worktrees/` and opens
  the agent there; `/worktrees` lists all with `[nanomite]`/`[other]` tags.
  Removal restricted to managed paths; branch names validated. Status:
  implemented and tested (isolation verified).
- **CI + packaging**: GitHub Actions (check/fmt/clippy/test/bench, tag builds
  with binary upload), COPR spec (`pack/fedora/`), Flatpak manifest
  (`pack/flatpak/`), crates.io checklist + full package metadata. Status:
  implemented.
- **Documentation overhaul**: all subsystem docs refreshed; new
  `PLAN_MODE.md` and `EXTENDING.md`; `docs/README.md` rewritten as a real
  index. Status: implemented.

### Fixed

- **Clippy fully clean** (`cargo clippy --all-targets -- -D warnings`: 0
  errors) after accruing drift during rapid development; dead code removed,
  idioms fixed. The new CI would have caught this - it now gates every push.
- **ToolPolicy gate restored**: the dispatch-time policy check in
  `run_tool` had been lost in a checkout revert; Plan/Readonly modes were
  not enforced. Restored and pinned by dispatch-level tests.

## [0.4.0] — 2026-10-27

### Added

- **Central tool authorization engine** (`src/agent/policy.rs`): `ToolPolicy`
  (Auto / Plan / Readonly) enforced at dispatch; every tool carries a `Risk`
  class (Safe / Write / External). `/policy` (alias `/mode`) cycles modes with
  immediate effect; persisted in `config.toml`, `NANOMITE_TOOL_POLICY`
  overrides. Status: implemented and tested (part of the 417-test suite),
  verified in live use.

- **Git tool suite** (5 tools): `git_status`, `git_diff`, `git_log`,
  `git_commit`, `git_branch` — `git` invoked directly (no shell),
  workspace-confined, timeout-bounded. Status: implemented and tested.

- **Test runner tool**: `run_tests` over cargo, npm, pytest, jest, go and
  custom commands with structured pass/fail summaries. Status: implemented
  and tested.

- **Symbol index tools** (3): `index_symbols` (ctags with regex fallback),
  `find_symbol` (wildcards, kind/language filters), `find_references`.
  Status: implemented and tested.

- **Plan/task list system** (5 tools): `create_plan`, `get_plan`,
  `update_task`, `list_plans`, `delete_plan` with dependencies, tags and
  statuses, persisted under `.nanomite/plans/`. Status: implemented and
  tested.

- **Sudo password modal** — when an agent command needs elevation, the
  non-interactive guard (`sudo -n` probe / `su` refusal) fails fast instead of
  hanging on /dev/tty, and the UI opens a themed password modal (rounded
  border, accent title, masked input, key hints; follows all four themes).
  Submitting runs the command via `sudo -S` with the password piped to stdin
  only — never a terminal, never logged — and appends the output to the
  transcript. `su -c` commands are translated to sudo; `sudo -v` validates
  and caches the credential ticket so the agent's retry succeeds. Status:
  implemented, tested (fake-sudo pipe mechanics + themed render assertions),
  and verified working in live use (2026-10-27).

### Fixed

- **SGR mouse fragments no longer leak into the composer**: the root cause
  was elevation commands (`sudo`/`su`) opening /dev/tty and racing nanomite's
  event reader for tty bytes, desyncing the escape-sequence parser. No child
  process touches /dev/tty anymore, so mouse reports parse cleanly.

## [0.3.0] — 2026-10-26

### Added

- **Prompt Queue**: Messages sent while the agent is busy are now queued (FIFO) and displayed in a visible panel between the chat transcript and the composer. Shows count, first queued message, and "N more" indicator. Toast notifications: "queued (1 pending)" / "queued (N pending)". Automatic processing after each turn completes.
- **OpenCode-style Chat UI**: Complete visual overhaul with message bubbles, role badges (YOU/AI), timestamps, tool pills with status indicators, mini-diff chips for edit_file, streaming cursor (▍), and full markdown rendering.
- **Welcome Screen**: Centered bordered card with logo, model/provider/workspace info, and keyboard hints.
- **Thinking Indicator**: Spinner + "thinking" + elapsed time + step/tool counters + token count.
- **Modular App Architecture**: Split `app.rs` into focused submodules (`entry`, `picker`, `overlays`, `engine`, `actions`, `commands`, `session`, `provider`, `board`, `tools`, `improve`, `memory`, `input`, `visual`, `history`, `composer`).
- **Integration Tests**: 7 new tests covering ToolSet file/exec operations, session persistence, memory engine, config loading, and document extraction.
- **CI Pipeline**: GitHub Actions workflow with test, clippy, fmt, bench (nightly), cargo-deny, cargo-audit, docs, and release jobs.
- **Security Updates**: Updated tokio 1.54.1, rustls 0.23.46, rustls-webpki 0.103.16 for CVE fixes.

### Changed

- **`y` Key Behavior**: Types normally in composer; copies selection only in visual mode (v/V); no-op in transcript normal mode (prevents accidental clipboard copies).
- **ESC Key**: Interrupts agent during streaming, tool execution, and board convene; closes overlays; clears composer.
- **Virtualization**: Fixed estimated height calculation to account for role badges and timestamps.
- **Dependency**: Added `chrono` for timestamp formatting.

### Fixed

- **Release Binary**: Built with `cargo build --release` (optimized, stripped, LTO).
- **All Quality Gates**: 383 tests pass (375 unit + 7 integration + 1 production); clippy clean; fmt clean; cargo-deny clean; cargo-audit clean.

### Known Limitations

- `ttf-parser` (transitive via `lopdf` → `pdf-extract`) is unmaintained (RUSTSEC-2026-0192). Monitored for migration to `skrifa`.


### Added

- **Multi-agent board** (`src/board/`): `/panel <topic>` convenes four personas
  in parallel — Architect, Engineer, Scientist and Critic — for two rounds of
  discussion, then a Chair synthesizes their answers into one recommendation.
  Messages stream live into an overlay with a spinner; `esc` cancels a running
  convene and the engine is cancel-aware between rounds. History is kept
  verbatim in SQLite under `~/.local/share/nanomite/board/board.db`
  (`NANOMITE_BOARD_DIR` overrides it). `Swarm` (parallel chat across several
  providers) is now real, and the board runs on it.
- **Self-improvement, gated on you** (`src/improve/`): the agent can stage a
  proposed change to its config, its prompt file or a source file with
  `propose_self_edit`, and list what is pending with `list_self_edits`.
  `/improve` shows the staged diff; `a` approves (running the regression gate on
  a background task) and `r` rejects. **There is deliberately no agent-callable
  apply** — human approval is the only path to disk. Every apply writes a `.bak`
  backup, runs `cargo test --lib`, and rolls back byte-exactly if the suite goes
  red; every decision is appended to a JSONL audit log. A hard denylist
  (`src/memory/security.rs`, `src/memory/policy.rs`, `src/main.rs`) keeps the
  safety and permission layer from rewriting itself.
- **Tool library** (`/tools`): all 18 tools listed by category with ✓/✗ toggles,
  persisted to `disabled_tools` in `config.toml`. A disabled tool is filtered
  out of the schema list before it is sent to the provider *and* refused again at
  dispatch, so it cannot be reached by guessing. `NANOMITE_DISABLED_TOOLS`
  overrides the file.
- **`read_document`**: the agent can read PDF, DOCX, ODT, XLSX, ODS, RTF, CSV,
  Markdown and JSON through the `extract/` registry — bounded output,
  workspace-confined, read-only.
- **Unified tool catalog** (`src/tools/catalog.rs`): the inventory is one list
  of `ToolSpec { name, description, category }` driving the schema list, the
  `/tools` overlay and the dispatch refusal, with tests pinning catalog and
  schemas together. `search` gained the schema it was missing, so every tool the
  runtime can dispatch is actually callable.
- **Custom system prompt**: `~/.config/nanomite/prompt.md`
  (`NANOMITE_PROMPT_FILE`) is appended to the system prompt on every turn — and
  it is the file the self-improvement engine may propose to edit.
- **Theme persistence**: the `/themes` choice is saved to the config `theme` key
  and restored at startup (`ThemeId::from_label`), and every glyph now comes
  from the `theme::glyph` constants instead of scattered literals.
- **Header shows the workspace path** plus a busy/ready status; the footer
  carries live step and tool-call progress.

### Changed

- **Persistent memory is live in production.** `MemoryEngine` attaches at
  startup, so the six `memory_*` tools are callable and retrieval and capture
  run per policy on every turn. All `NANOMITE_MEMORY*` knobs are honored (`0` on
  the master switch disables the subsystem; `_DIR`, `_DB`, `_READONLY`, `_AUTO`,
  `_MARKDOWN` included). If memory cannot attach, nanomite keeps running and
  says so in the footer — a quiet hint, never an error.
- `/memory` is now a working async browser (recent records plus a record-body
  overlay) instead of a one-line status line.

### Fixed

- **Mouse movement no longer spews `ESC[<…M` fragments into the transcript.**
  Two bugs, both in `src/main.rs`:
  (1) crossterm's async `EventStream` was polled inside `tokio::select!` beside
  the 90 ms animation tick; every tick cancelled the in-flight read and threw
  away tty bytes it had already consumed, so multi-byte SGR mouse sequences
  arrived truncated and their tails were re-parsed as keyboard input. Input is
  now read **synchronously** via `event::read()` on a dedicated OS thread (which
  only ever returns complete events), relayed through a std channel and a
  `spawn_blocking` bridge into the tokio loop — cancellation releases the
  receiver, never a half-parsed sequence.
  (2) terminal capture ordering was inverted: mouse capture was enabled before
  raw mode (echoing motion reports as text) and raw mode was disabled before
  capture (printing pending reports literally) on suspend/restore/panic. All
  paths now disable capture **before** raw mode and enable raw mode **before**
  capture, with comments explaining the ordering.
  Verified: `cargo test --lib` 371 passed (3 pre-existing failures unchanged
  from HEAD), release build and clippy clean.
- **Model picker reads as live, and stale endpoint replies can no longer fill it.**
  Smoke-testing the real binary against the endpoint's 85-model list showed the
  fetch itself was already dynamic — but the picker *rendered* like a static
  table: the window pinned the highlighted row to the bottom edge
  (`start = cursor − (visible−1)`), so a model sorted deep in the list opened as
  an unchanging wall of rows above the highlight, and nothing on screen said the
  list had just been fetched. Three changes: the title now carries the live
  count (`model · 85`, filtered `model · 36/85`); the window centres the
  highlighted row (clamped at both ends) so there is always context below it;
  and `ModelsResult` is tagged with the base URL that produced it (same
  discipline as `BoardEvent`'s generation tag) so a slow reply from a previous
  `/connect` endpoint is dropped in `apply_models_result` instead of populating
  the current endpoint's picker or toast.
  Verified: `cargo test --lib` 374 passed / 0 failed; `cargo clippy --lib
  --bins -- -D warnings` clean; `cargo fmt --check` clean on tracked sources;
  end-to-end pty smoke against a mock serving the endpoint's real payload
  (open → `model · 85`, filter `nvidia` → `model · 36/85`, top and bottom of
  the list reachable, current model highlighted with context below).
- **Header context meter no longer clipped on narrow terminals.**
  `ui::panels::header` budgeted the right side (model · reasoning · 🧠
  `used/limit · pct`) against the full terminal width while rendering it on the
  same row as the brand + workspace path, so when the two collided the tail —
  the absolute token readout — was silently cut (`15.0k/200.0` with no `k · 7%`).
  The right side is now budgeted against `width − left_width − 1` and
  head-truncated (`truncate_spans_head`) so the readout at the tail always
  survives. Also fixes the two tests left red by the theme-default switch
  (`theme_roundtrips…` still asserted the old `mono` fallback;
  `theme_switch_persists…` applied Umbrella, which is now the default and
  short-circuits as a no-op): both updated, and the full suite is green again —
  374 passed, 0 failed.
- `edit_file` previews render a real unified diff (`diff_add`/`diff_delete`
  roles via `ui/diff.rs`) instead of a flat replacement blob.
- Composer caret math is measured in display columns, so CJK and other wide
  characters no longer drift the cursor.
- `/copy` actually copies the last assistant message, via OSC 52.
- `/undo` and `/redo` cut the transcript at exact message boundaries.
- The help sheet now matches the real key handling: `home`/`end` move the prompt
  caret, `ctrl+home`/`ctrl+end` scroll the transcript.
- Narrow terminals (30×12) are covered by golden frame tests.
- Deleted the dead `src/agent/trivial.rs`.

### Known limitations

- Board messages are stored verbatim and are **untrusted data**: nothing a
  persona writes can authorize a tool call.
- The self-improvement regression gate runs `cargo test --lib`, so it is only
  meaningful inside a checkout; a failed run rolls back automatically, but the
  gate cannot prove anything outside the repo.

Verified on `ed60936`: `cargo test --lib` **370 passed / 0 failed** (fully
offline), `cargo test --test production_attach` 1 passed, clippy `--all-targets
-D warnings` clean, `cargo fmt --check` clean.

## [0.2.0] — 2026-10-15

### Added

- **Live tool previews in the transcript**: tool rows now show what the agent
  is creating and building while it happens — `write_file` renders the code
  being written, `edit_file` a compact `-old`/`+new` diff of the replacement,
  and `run_command` the command line. Previews are built from the tool-call
  arguments at start time and hard-bounded (12 lines / 1,600 chars, with a
  `… +N more lines` marker), so huge writes can never flood the UI; `/details`
  collapses them back to bare rows. Status: implemented and tested (part of
  the 261-test green suite).

- **Persistent memory subsystem** (`src/memory/`): SQLite (bundled rusqlite)
  as the authoritative store, human-editable markdown as a one-way projection,
  FTS5 search probed at runtime with `LIKE` fallback, six `memory_*` tools,
  credential detection/redaction on the way in and out, path confinement,
  integrity-checked backups and validated import/export. Status: implemented
  and tested (part of the 261-test green suite).
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
- Transcript scrolling now works with the mouse and matches OpenCode/standard
  direction: wheel up scrolls toward older messages, wheel down toward newer
  (scrolling back to the bottom re-follows the live feed). Mouse capture is
  enabled on terminal setup and released on suspend/exit/panic — previously
  wheel events never reached the app at all (dead handler in the alternate
  screen). Arrow keys and PageUp/PageDown use the same Up = older convention
  as the wheel (previously inverted).
- README refreshed: 261-test suite, unlimited step defaults, `extract/` in
  the architecture map, `NANOMITE_MAX_TOOL_CALLS` documented.
- **Tool-layer performance pass** (profiled before optimizing, per
  `docs/PERFORMANCE.md`; numbers from `cargo bench`, `benches/perf.rs`):
  `read_file` now seeks and reads only the requested window instead of
  loading whole files (4 KB slice of a 1.3 MB file: 91 µs → 8.7 µs, ~10×);
  case-insensitive `search` folds ASCII lines with a hand-rolled matcher and
  falls back to Unicode `to_lowercase` only for non-ASCII lines (byte-identical
  results; matcher core ~2.4× on mostly-ASCII trees, parity on the mixed
  fixture); TOML rendering takes a scalar fast path. A stable zero-dep
  benchmark harness (`benches/perf.rs`, `cargo bench`, median of 7 batches)
  replaces the never-compiled nightly-only stub. Status: implemented and
  tested (261 passed / 0 failed, clippy clean).

### Fixed

- Full build is green again: `cargo check` (lib + bin) passes and the offline
  suite is 261 passed / 0 failed (`cargo test --lib`, ~1.3 s). The previously
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

## [0.1.0] — 2026-10-13 (prior work, per git history)

- Initial release: ratatui TUI coding agent with OpenAI-compatible streaming,
  9 tools, 21 provider presets, sessions, 4 themes, 55 offline tests.
  Status: implemented and tested (last green baseline: `d59f2ef`).
