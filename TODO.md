# TODO — nanomite roadmap

## Stage F: Plan Mode Enforcement (next)
- [ ] Enforce `ToolPolicy::Plan` in runtime: restrict to `Safe` tools + plan tools (`create_plan`, `update_task`, `get_plan`, `list_plans`)
- [ ] Require `create_plan` before any `Write`/`External` tool in Plan mode
- [ ] Show active plan status in footer when `tool_policy == Plan`
- [ ] Auto-switch to `Auto` after user approves plan (optional: `/policy` to cycle)

## Stage G: Session Persistence
- [ ] Serialize full conversation history + plan state + context to disk
- [ ] Restore on startup with `/sessions` picker (already has `Sessions` command)
- [ ] Per-workspace session directory: `.nanomite/sessions/`
- [ ] Include plan state, tool policy, disabled tools, reasoning level in session

## Stage H: Hooks / Event System
- [ ] Define `HookEvent` enum: `PreToolCall`, `PostToolCall`, `TurnStart`, `TurnEnd`, `PolicyChange`
- [ ] Config: `hooks = { pre_tool = "~/.config/nanomite/hooks/pre.sh", ... }`
- [ ] Hooks run in `spawn_blocking`, timeout-bounded, stdout/stderr captured
- [ ] Built-in hooks: log to file, notify (desktop), guardrails (regex deny)

## Stage I: External Tool SDK
- [ ] Tool manifest: `~/.config/nanomite/tools/*.toml` with name, description, schema, command
- [ ] Sandbox: `bubblewrap`/`firejail` or `run_command` with restricted env
- [ ] Schema validation at load time (matches OpenAI function schema)
- [ ] Tools appear in `/tools` overlay, can be toggled like built-ins
- [ ] `NANOMITE_TOOLS_DIR` override for project-local tools

## Stage J: Parallel Worktrees
- [ ] `nanomite --worktree <branch>` creates git worktree, launches agent
- [ ] Shared `.nanomite/` config, isolated session/plan state per worktree
- [ ] `/worktrees` command to list/attach/kill parallel agents
- [ ] Merge helper: show diffs between worktrees, auto-rebase on main

## Stage K: CI + Packaging
- [ ] GitHub Actions: `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt` on PR
- [ ] `cargo publish` workflow for crates.io
- [ ] COPR build for Fedora (your channel)
- [ ] Flatpak manifest for Flathub
- [ ] Release checklist: changelog, tag, binary uploads

## Stage L: Documentation Overhaul
- [ ] Update all 11 docs in `/docs` (currently stale per AGENTS.md)
- [ ] `ARCHITECTURE.md` — current module map, data flow
- [ ] `TOOLS.md` — all 33 tools with schemas, examples
- [ ] `MEMORY.md` — policy, capture, retrieval, security
- [ ] `SECURITY.md` — authz, denylists, sandbox, credentials
- [ ] `PERFORMANCE.md` — benchmarks, tuning
- [ ] `EVOLUTION.md` — roadmap, decisions
- [ ] `RELEASES.md` — version history
- [ ] `FEATURES.md` — what works, what's experimental
- [ ] `AUDIT-2026-10-14.md` — keep as historical snapshot
- [ ] New: `PLAN_MODE.md` — Plan mode workflow, policy, hooks
- [ ] New: `EXTENDING.md` — External Tool SDK, hooks

## Completed (Stages A–E)
- [x] Stage A: Authorization Engine + Git Tools (5 tools)
- [x] Stage B: Test Runner Tool (1 tool)
- [x] Stage C: Symbol Index (3 tools: index_symbols, find_symbol, find_references)
- [x] Stage D: Policy Engine + `/policy` command (Auto/Plan/Readonly)
- [x] Stage E: Plan/Task List System (5 tools: create_plan, get_plan, update_task, list_plans, delete_plan)

**Total tools: 33** — Files(5) Exec(2) Web(2) Memory(5) Document(1) SelfImprove(2) Git(5) Test(1) Index(3) Plan(5)