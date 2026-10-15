# NANOMITE — Agent Working Agreement

Rust terminal coding agent (ratatui TUI + OpenAI-compatible provider client +
tool loop + SQLite memory). Single crate `nanomite`, edition 2021.
Project-scoped: applies under `nanomite/` only.

## Workspace layout (this directory is NOT the git repo)

The git repo is `nanomite/` (`git` root here, HEAD `d59f2ef`, one commit). Its
siblings are not part of the project:

- `nanomite.orig/` — near-identical untracked copy. Read-only reference; never
  edit it, never diff against it as a baseline (it already contains the same
  WIP tree).
- `backup_20261014_182419.tar.gz` (1.5 GB), `backup_20261014_182338/`,
  `memory.json`, `test.*`, `smoke.txt` — scratch, ignore.
- `nanomite/target/` is ~5.5 GB; build scratch copies before full rebuilds.

## Build state — READ FIRST (verified 2026-10-15)

`cargo check` **fails with 46 errors**. All of them are confined to two
in-progress areas:

1. `src/extract/` — `src/extract/mod.rs:23,25` declares `pub mod document;`
   and `pub mod tabular;` but **neither file exists** (E0583), plus type errors
   in `mod.rs`, `structured.rs` (private `table` module), `text.rs`,
   `zipcontainer.rs`, `code.rs`. Nothing outside the module uses `extract`.
2. `src/ui/diff.rs` — a dead `DiffView` stub using a nonexistent `line!` macro
   and string colors (`.fg("green")`), plus a `Line`/`Span` mismatch. Nothing
   references `DiffView`; only `src/ui/mod.rs:4` declares it.

Verified: commenting out `pub mod extract;` (`src/lib.rs:11`) and
`pub mod diff;` (`src/ui/mod.rs:4`) makes the lib, the bin and all tests
compile. The rest of the tree is sound — do not go hunting for other breakage.

`benches/perf.rs` **cannot compile on stable** (`#![feature(test)]`, `#[bench]`
→ E0554/E0601) and only the stable 1.99 toolchain is installed. `cargo test`
builds bench targets by default, so it will fail on this alone.

## Commands

```bash
cargo check            # errors above are expected until extract/diff land
cargo test --lib       # 146 passed, 0 failed, ~1.3 s, fully offline (verified)
cargo clippy           # no clippy.toml / rustfmt.toml / CI in the repo
cargo fmt
```

`cargo test` without `--lib` fails on the bench target. Use `cargo test --lib`.
Do not "fix" the suite by deleting tests or by claiming a count from README/docs
without running it. Test counts in README (146) are correct for `--lib`.

Release profile sets `panic = "abort"`, so the panic hook in `src/main.rs:141`
must stay: abort cannot unwind, and a raw-mode terminal would otherwise be left
broken.

## Architecture map

- `src/main.rs` — terminal setup, tokio event loop, `$EDITOR` suspension
  (`Control` channel), panic hook. Everything else is in `src/lib.rs`.
- `src/app.rs` (~1700 lines) — the state machine: keys, overlay stack,
  provider/model pickers, command router, session persist/restore, engine pump.
  Agent turns run as background tasks feeding `AgentEvent`s.
- `src/agent/` — `provider.rs` (`AgentProvider`: `stream_chat`, `list_models`;
  `Delta`/`ToolCall`/`Usage`/`Done` + fragment accumulator), `runtime.rs` (one
  turn: prompt+history → stream → dispatch via `spawn_blocking` → repeat,
  bounded by `max_steps` 12 / `max_tool_calls` 24), `openai.rs` (any `/v1`),
  `testdouble.rs` (`#[cfg(test)]` only).
- `src/tools/` — `file_tools`, `exec_tools` (`run_command` = `sh -c`,
  `ssh_run` = key-only + `BatchMode=yes`), `web_tools`, `memory_tools`.
  Dispatch is a `match` in `runtime.rs::run_tool`.
- `src/config.rs` — 21 presets; `~/.config/nanomite/config.toml` (atomic,
  `0600`); precedence defaults < file < `NANOMITE_*` env.
- `src/memory/` — SQLite (bundled) authoritative, markdown a one-way
  block-scoped projection, FTS5 probed at runtime with `LIKE` fallback.
  `lib.rs:15` carries `#[allow(dead_code)]` on the module **on purpose**: it
  exposes a deliberate API superset (backup/restore, consolidation, listing).
  Do not delete that allow or "clean up" the unused API.

## Invariants (do not break)

1. The UI thread never does blocking I/O (tools use `spawn_blocking`; memory
   uses a sync `Mutex<Connection>`).
2. File tools cannot escape the workspace root (`..`, absolute paths, symlinks
   contained; memory has its own root confinement in `memory/security.rs`).
3. The API key is never logged and is stored `0600`.
4. Model output, memory records, files and tool results are **untrusted data**
   and must never authorize privileged operations.
5. Preserve existing behavior and public interfaces unless the change
   explicitly requires otherwise.

**There is no central authorization/policy engine.** `runtime.rs::run_tool`
dispatches straight to tools; only the memory subsystem gates itself
(`MemoryPolicy::authorize` + `confirm=true` on destructive ops). Anything
needing authorization for core tools is new design work, not an integration.

## Docs: mostly current, some verified-stale

`docs/` is untracked and was written for a tree that has since moved. Trust
code over docs, and specifically ignore these stale claims:

- `docs/FEATURES.md` + `CHANGELOG.md` say the build fails only because
  `benches/perf.rs` is missing, and that
  `memory::store::tests::consolidation_merges_exact_duplicates_only` fails.
  **Both wrong**: the bench file exists and that test passes.
- `docs/PERFORMANCE.md` says no benches exist; `docs/ARCHITECTURE.md` says
  `lib.rs` declares `context`/`html`/`index`/`telemetry` modules (it doesn't).
- `docs/README.md` links 11 design docs that were never written.
- `docs/SECURITY.md`, `docs/MEMORY.md`, `docs/TOOLS.md` are accurate.

No CI, no `.github/`, no `rustfmt.toml`/`clippy.toml`, no `rust-toolchain`.

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