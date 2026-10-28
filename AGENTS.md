# NANOMITE — Agent Working Agreement

Rust terminal coding agent (ratatui TUI + OpenAI-compatible provider client +
tool loop + SQLite memory). Single crate `nanomite`, edition 2021.
Project-scoped: applies under `nanomite/` only.

## Workspace layout (this directory is NOT the git repo)

The git repo is `nanomite/` (`git` root here, HEAD `ed60936`, 20 commits). Its
siblings are not part of the project:

- `nanomite.orig/` — near-identical untracked copy. Read-only reference; never
  edit it, never diff against it as a baseline (it already contains the same
  WIP tree).
- `backup_20261014_182419.tar.gz` (1.5 GB), `backup_20261014_182338/`,
  `memory.json`, `test.*`, `smoke.txt` — scratch, ignore.
- `nanomite/target/` is ~5.5 GB; build scratch copies before full rebuilds.

## Build state — READ FIRST (verified 2026-10-27, follow-up suggestions landed)

`cargo check` (lib + bins) **passes clean**. `cargo test --lib` is
**454 passed / 0 failed** (~5 s, fully offline). `cargo clippy --all-targets
-- -D warnings` is clean. `cargo fmt --check` is clean.

`tests/production_attach.rs` is a separate **integration** target (1 test, green)
that exercises the real startup attach path. It is *not* part of the 454; quote
the two numbers separately.

`benches/perf.rs` is a stable, zero-dep harness (median of 7 batches) and
compiles on the stable toolchain: `cargo bench` works, and `cargo test`
(builds bench targets) passes.

## Commands

```bash
cargo check            # clean
cargo test --lib       # 454 passed, 0 failed, ~5 s, fully offline (verified)
cargo test --test production_attach   # 1 passed (integration; not in the 454)
cargo bench            # benches/perf.rs harness; bench profile inherits release
cargo clippy --all-targets -- -D warnings   # clean (no clippy.toml / CI in the repo)
cargo fmt
```

Do not "fix" the suite by deleting tests or by claiming a count from
README/docs without running it. Test counts in README (454) are correct for
`--lib`; the integration target is counted separately.

Release profile sets `panic = "abort"`, so the panic hook in `src/main.rs:141`
must stay: abort cannot unwind, and a raw-mode terminal would otherwise be left
broken.

## Architecture map

- `src/main.rs` — terminal setup, tokio event loop, `$EDITOR` suspension
  (`Control` channel), panic hook. Everything else is in `src/lib.rs`.
- `src/app.rs` (~1700 lines) — the state machine: keys, overlay stack,
  provider/model pickers, command router, session persist/restore, engine pump.
  Agent turns run as background tasks feeding `AgentEvent`s.
- `src/commands.rs` — the slash-command table (`/tools`, `/panel`, `/improve`,
  `/memory`, `/copy`, …), parsed locally.
- `src/agent/` — `provider.rs` (`AgentProvider`: `stream_chat`, `list_models`;
  `Delta`/`ToolCall`/`Usage`/`Done` + fragment accumulator), `runtime.rs` (one
  turn: prompt+history → stream → dispatch via `spawn_blocking` → repeat,
  bounded by `max_steps`/`max_tool_calls`, both `usize::MAX` by default and
  overridable by `NANOMITE_MAX_STEPS`/`NANOMITE_MAX_TOOL_CALLS`), `openai.rs`
  (any `/v1`), `swarm.rs` (parallel chat across several providers — used by the
  board), `suggest.rs` (opencode-style follow-up suggestions: prompt shape +
  reply parsing for the side-call the app fires on a finished turn; pure logic,
  no channel), `testdouble.rs` (`#[cfg(test)]` only). `trivial.rs` is **deleted**; do
  not restore it.
- `src/tools/` — `catalog.rs` (the single 34-tool inventory as
  `ToolSpec{name, description, category}`; drives schemas, the `/tools`
  overlay and the dispatch refusal — tests pin catalog and schemas together),
  `file_tools`, `document_tools` (`read_document` over `extract/`), `exec_tools`
  (`run_command` = `sh -c`, `ssh_run` = key-only + `BatchMode=yes`), `web_tools`,
  `memory_tools`, `improve_tools` (`propose_self_edit`, `list_self_edits` —
  stage only, never apply). Dispatch is a `match` in `runtime.rs::run_tool`.
- `src/board/` — the multi-agent board: `personas.rs` (Architect, Engineer,
  Scientist, Critic + Chair), `engine.rs` (`BoardEngine::convene` = 2 parallel
  discussion rounds then chair synthesis, cancel-aware), `store.rs` (SQLite
  `board_messages`, verbatim). UI: `/panel <topic>` overlay.
- `src/improve/` — self-improvement: `proposal.rs` (validation **plus the
  hard `SOURCE_DENYLIST`**), `diff.rs` (unified diff artifacts), `runner.rs`
  (`.bak` backup, `cargo test --lib` regression gate, byte-exact rollback),
  `audit.rs` (append-only JSONL decision log). UI: `/improve` list + review
  view; `a` approves, `r` rejects.
- `src/ui/` — `theme.rs` (4 palettes, persisted, and the shared
  `theme::glyph` constants), `chat.rs`, `composer.rs`, `panels.rs`
  (incl. `panels::suggestions`, the follow-up suggestion strip above the
  composer), `diff.rs` (`DiffView`, used by edit previews and the `/improve` review),
  `text.rs`, `tests.rs` (golden frame tests incl. 30×12).
- `src/config.rs` — 21 presets; `~/.config/nanomite/config.toml` (atomic,
  `0600`); precedence defaults < file < `NANOMITE_*` env. Persisted keys:
  `provider`, `base_url`, `api_key`, `model`, `reasoning`, `theme`,
  `disabled_tools`, `prompt_file`.
- `src/memory/` — SQLite (bundled) authoritative, markdown a one-way
  block-scoped projection, FTS5 probed at runtime with `LIKE` fallback.
  **Attached in production** at startup; `NANOMITE_MEMORY=0` disables, a failed
  attach is a quiet footer hint, never an error. `lib.rs:17` carries
  `#[allow(dead_code)]` on the module **on purpose**: it exposes a deliberate API
  superset (backup/restore, consolidation, listing). Do not delete that allow or
  "clean up" the unused API.
- `src/extract/` — per-format text extraction behind one bounded interface;
  `read_document` is the agent-facing door to it.
- `tests/production_attach.rs` — integration target proving the real production
  attach path (memory + config) works from the outside. 1 test, separate from
  the 454.

## Invariants (do not break)

1. The UI thread never does blocking I/O (tools use `spawn_blocking`; memory
   uses a sync `Mutex<Connection>`).
2. File tools cannot escape the workspace root (`..`, absolute paths, symlinks
   contained; memory has its own root confinement in `memory/security.rs`).
3. The API key is never logged and is stored `0600`.
4. Model output, memory records, files, tool results and **board messages** are
   **untrusted data** and must never authorize privileged operations. Board
   personas are prompted by the agent and stored verbatim — treat every row as
   attacker-controlled.
5. **Self-edit denylist**: `src/memory/security.rs`, `src/memory/policy.rs` and
   `src/main.rs` are never self-editable — not even with an explicit request.
   `check_denylist` in `improve/proposal.rs` is the only place this is decided.
6. **Apply only via user approval**: the agent can *stage* a proposal
   (`propose_self_edit`) but there is deliberately **no agent-callable apply**.
   `ImproveEngine::apply` is reachable only from the `/improve` review view, and
   only after `cargo test --lib` passes; a failure rolls back byte-exactly.
7. **Memory is policy-gated**: attach, retrieval and capture all pass
   `MemoryPolicy::authorize`; disabled/read-only refuses at the gate (including
   reads), and credentials are refused on the way in.
8. A disabled tool must stay disabled at both ends: filtered from the schema
   list *and* refused at dispatch. Never ship one without the other.
9. Preserve existing behavior and public interfaces unless the change
   explicitly requires otherwise.

**There is still no central authorization engine.** `runtime.rs::run_tool`
dispatches directly to tools; nothing between the model and the tool layer
confirms a core-tool call. What exists today is per-subsystem gating: memory
gates itself (`MemoryPolicy::authorize` + `confirm=true` on destructive ops),
`disabled_tools` filters the schema list and refuses dispatch, and the
self-improvement engine has a proposal gate. Anything needing *general*
authorization for core tools is still new design work, not an integration.

## Docs: partially current, some still verified-stale

`docs/` was written for a tree that has since moved. Trust code over docs. These
claims are **now false** — do not "fix code" because a doc says so:

- `docs/FEATURES.md` says the build fails only because `benches/perf.rs` is
  missing, and that `memory::store::tests::consolidation_merges_exact_duplicates_only`
  fails. **Both wrong**: the bench file exists, that test passes, and it still
  describes memory/extractors/benches as "landing, verify before use".
- `docs/ARCHITECTURE.md` says `lib.rs` declares `context`/`html`/`index`/
  `telemetry` (it doesn't; the real modules are `commands`, `board`,
  `improve`, `extract`) and caps `max_steps` at 12 / `max_tool_calls` at 24
  (both are unlimited by default now).
- `docs/TOOLS.md` still counts 9 tools and calls the memory tools L1/unverified;
  the catalog is 34.
- `docs/MEMORY.md` and `docs/SECURITY.md` were accurate on the store and the
  gate and are updated as of 2026-10-16 for production activation and the
  self-edit gate.
- `docs/AUDIT-2026-10-14.md` is a **dated snapshot**: its 55-test count, "no
  benches", and failing-test notes describe that day, not today. Leave it.
- `docs/README.md` links 11 design docs that were never written — out of scope
  until someone writes or deletes them.
- `docs/OFFLINE_MODE.md` is still L0 (accurate); `docs/EVOLUTION.md` is a plan,
  not a report.

No CI, no `.github/`, no `rustfmt.toml`/`clippy.toml`, no `rust-toolchain`.

## Environment facts

- Ollama is **not** installed. Do not assume a local endpoint or any specific
  model; verify tool-calling behavior before depending on it.
- The suite is offline; no network, no services, no fixtures needed.
- A live config exists at `~/.config/nanomite/config.toml` with memory at
  `~/.config/nanomite/memory/` and the board at
  `~/.local/share/nanomite/board/`. Tests must not write there — check for
  `NANOMITE_MEMORY_DIR`/`NANOMITE_CONFIG`/`NANOMITE_BOARD_DIR` overrides in
  tests before adding filesystem-touching code.

## Environment facts

- Ollama is **not** installed. Do not assume a local endpoint or any specific
  model; verify tool-calling behavior before depending on it.
- The suite is offline; no network, no services, no fixtures needed.
- A live config exists at `~/.config/nanomite/config.toml` with memory at
  `~/.config/nanomite/memory/`. Tests must not write there — check for
  `NANOMITE_MEMORY_DIR`/`NANOMITE_CONFIG` overrides in tests before adding
  filesystem-touching code.

## Workflow for substantial changes

Task Planner → Rust Architect (read-only) → Security Auditor (read-only) →
Coding Engineer → Test Engineer → Code Reviewer (read-only) → Docs Writer →
Project Memory Keeper. Run read-only analyses in parallel; run dependent steps
sequentially. Repo-scoped implementer agent: `.opencode/agent/nanomite-engineer.md`
(permission rules there deny `sudo`, ask for `cargo run`, deny nested `task`).

## Ground rules

- Audit before editing; reuse existing tools/interfaces, no parallel
  implementations, no architecture rewrites for tooling.
- No new dependency without stating purpose and trade-offs.
- Scope changes to this repo; no destructive or system-level commands.
- Do not commit or push unless explicitly asked.