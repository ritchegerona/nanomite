# TODO — nanomite roadmap

**All stages complete (F–L).** Stages A–E landed in v0.4.0; F–L finalized in v0.5.0 (commit ed56eac, live on GitHub).

## Stage F: Plan Mode Enforcement
- [x] Enforce `ToolPolicy::Plan` in runtime: restrict to `Safe` tools + plan tools
- [x] Plan-mode refusals direct the model to `create_plan` (plan tools stay callable)
- [x] Active plan status in footer when `tool_policy == Plan`
- [x] Mode switching via `/policy` (auto-switch deemed unnecessary - the badge makes the state visible)

## Completed — Stage G: Session Persistence
- [x] Serialize conversation history + plan state + context (v2 schema)
- [x] Restore with `/sessions` picker, settings applied back to live config
- [x] Per-workspace grouping in the picker (sessions stay in the data dir -
      a `.nanomite/sessions/` inside the repo would risk being committed)
- [x] Tool policy, disabled tools, reasoning, theme + plans in the session

## Completed — Stage H: Hooks / Event System
- [x] `HookEvent` enum: PreToolCall, PostToolCall, TurnStart, TurnEnd, PolicyChange
- [x] Config: `hooks.json` manifest + auto-discovery in `hooks/<event>/`
- [x] Hooks run timeout-bounded with process-group kill, stdin JSON payloads
- [x] Deny hooks block pre-tool calls (code: hook_denied); others advisory

## Completed — Stage I: External Tool SDK
- [x] TOML manifests in `~/.config/nanomite/tools/*.toml` (NANOMITE_TOOLS_DIR override)
- [x] stdin-JSON contract (no shell interpolation) + process-group kill on timeout
- [x] Schema validation at load time; required/unknown arg keys checked at call time
- [x] Tools listed in the `/tools` overlay under "external", toggleable
- [x] Dispatch through the same gate as built-ins (disabled filter + policy + hooks)

## Completed — Stage J: Parallel Worktrees
- [x] `nanomite --worktree <branch>` creates/reuses managed worktrees
- [x] Isolated per-worktree session/plan state (verified by test); config global
- [x] `/worktrees` (alias /wt) lists worktrees tagged [nanomite]/[other]
- [x] Removal restricted to managed paths; merge helper deferred (git tooling
      already covers diff/rebase from inside any worktree)

## Completed — Stage K: CI + Packaging
- [x] GitHub Actions: check/fmt/clippy/test + bench + tag-triggered binary upload
- [x] crates.io checklist (pack/crates-io.md) + complete package metadata
- [x] COPR spec (pack/fedora/nanomite.spec) + setup/release instructions
- [x] Flatpak manifest (pack/flatpak/dev.ritchegerona.nanomite.yml)
- [x] Release checklist in docs/RELEASES.md

## Completed — Stage L: Documentation Overhaul
- [x] All 11 docs refreshed (F-L commit earlier) + 2 new: PLAN_MODE.md, EXTENDING.md
- [x] docs/README.md rewritten as an index of the real 13 docs
- [x] AUDIT-2026-10-14.md kept as historical snapshot
- [x] EVOLUTION.md milestone table: all 15 milestones done

## Completed (Stages A–E)
- [x] Stage A: Authorization Engine + Git Tools (5 tools)
- [x] Stage B: Test Runner Tool (1 tool)
- [x] Stage C: Symbol Index (3 tools: index_symbols, find_symbol, find_references)
- [x] Stage D: Policy Engine + `/policy` command (Auto/Plan/Readonly)
- [x] Stage E: Plan/Task List System (5 tools: create_plan, get_plan, update_task, list_plans, delete_plan)

**Total built-in tools: 33** — Files(5) Exec(2) Web(2) Memory(5) Document(1) SelfImprove(2) Git(5) Test(1) Index(3) Plan(5). Plus user-defined external tools from TOML manifests.