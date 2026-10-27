# Plan Mode

Plan mode turns nanomite into a read-only planner: the agent can inspect
everything but cannot mutate anything until you review its plan and switch
back. Status: **L3** (implemented, tested, wired into dispatch).

## The modes

`/policy` (alias `/mode`) cycles **auto → plan → readonly → auto**; the choice
persists in `config.toml` and `NANOMITE_TOOL_POLICY` overrides it per run.

| Mode | Safe tools | Plan tools | Write/External |
|---|---|---|---|
| `auto` (default) | ✓ | ✓ | ✓ |
| `plan` | ✓ | ✓ | ✗ refused |
| `readonly` | ✓ | ✗ | ✗ refused |

Risk classes (`agent/policy.rs`): **Safe** = reads (files, git status/diff/log,
tests, symbol search, memory reads); **Write** = mutations (write/edit_file,
git_commit/branch, plan creation, index build); **External** = leaves the
machine (run_command, ssh_run, memory_delete). The gate lives in
`runtime::run_tool` — the single choke point between the model and every tool
— and refusals reach the model as structured JSON it can read and adapt to.

## The Plan-mode workflow

1. `/policy` → plan mode. The footer shows a persistent `◐ plan mode` badge
   with the newest plan's name.
2. Ask for the work. The agent reads the repo (all Safe tools work) and builds
   a plan with `create_plan` — steps, dependencies, tags. These tools are
   explicitly allowed in Plan mode; building the plan is the mode's purpose.
3. You review: `/policy` back to `auto` (the badge clears), then tell the
   agent to execute. Task state tracks progress via `update_task`.

In Plan mode every other mutation is refused with a message pointing at
`create_plan`, so the model never flails against a wall it cannot see.

## Hooks and plan mode

`policy_change` hooks fire whenever `/policy` switches modes (`docs/EXTENDING.md`)
- a natural place to log mode switches or ping a channel.

## Testing

- `agent::policy::tests::plan_mode_allows_plan_tools_but_not_other_writes`
- `agent::policy::tests::plan_mode_refusal_points_at_create_plan`
- `agent::runtime::tests::plan_mode_refuses_writes_at_dispatch_through_run_tool`
  (the dispatch gate itself, with a plan-tool control)
