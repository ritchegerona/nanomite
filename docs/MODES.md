# Tool Modes

Four permission modes cycled with **Shift+Tab**, each granting strictly more
power than the last: **Plan ⊂ Coder ⊂ Build ⊂ Hacker**. Status: **L3**
(implemented, tested, wired into dispatch).

## The modes

| Mode | Reads | Plan tools | File edits | Commands/git | SSH/remote |
|---|---|---|---|---|---|
| `plan` | ✓ | ✓ | ✗ | ✗ | ✗ |
| `coder` | ✓ | ✓ | ✓ | ✗ | ✗ |
| `build` (default) | ✓ | ✓ | ✓ | ✓ | ✗ |
| `hacker` | ✓ | ✓ | ✓ | ✓ | ✓ |

- **Plan** — research and plan. Reads + plan tools only; every other mutation
  is refused with guidance to `create_plan`. The footer shows
  `◐ plan mode · <newest plan>`.
- **Coder** — write and edit code files, build the symbol index, stage
  self-edits, write memory. No command execution, no git history changes.
- **Build** (default) — full local power: `run_command`, `git_commit`,
  `git_branch`, `delete_plan`. Build, test, commit — everything local.
- **Hacker** — everything, including SSH to remote hosts. No guardrails.

Legacy config labels migrate: `auto` → `hacker`, `readonly` → `plan`.

## Colour codes

Each mode has a dedicated colour, stable across themes so a mode is
recognisable by colour alone - shown in the footer badge and tinting the
composer border while typing:

| Mode | Colour (universal 16-colour code) |
|---|---|
| plan | **amber / yellow** - caution: research only |
| coder | **blue / cyan** - focused code editing |
| build | **green** - everything works, full local power |
| hacker | **red** - unrestricted, danger |

Each theme tunes the exact RGB values to its ground (a palette test keeps the
four from collapsing); 16-colour terminals get the universal codes above.

## Cycling

**Shift+Tab** (in the composer/chat view; overlays keep their own navigation)
cycles plan → coder → build → hacker → plan. `/policy` (alias `/mode`) does
the same. The footer badge shows the active mode — warn for plan, accent for
coder, danger for hacker; build (default) stays quiet. The choice persists in
`config.toml` (`tool_policy`); `NANOMITE_TOOL_POLICY` overrides it per run.

## The gate

All modes enforce in one place: `runtime::run_tool`, the single choke point
between the model and every tool. Refusals reach the model as structured JSON
(`code: plan_mode | coder_mode | build_mode`) it can read and adapt to — each
refusal names the mode and the Shift+Tab escape hatch.

## Risk classes

Risk (`Safe`/`Write`/`External`) still classifies every tool for the policy
ladder and future per-argument rules (`agent/policy.rs`): **Safe** = reads,
**Write** = workspace/state mutations, **External** = leaves the machine.

## Testing

- `agent::policy::tests::*` — per-mode allow sets, ladder strictness
  (`modes_form_a_strict_ladder`), migration parsing, 4-mode cycle
- `agent::runtime::tests::plan_mode_refuses_writes_at_dispatch_through_run_tool`
- `ui::tests::shift_tab_cycles_all_four_modes`, `footer_shows_mode_badge_for_non_default_modes`
