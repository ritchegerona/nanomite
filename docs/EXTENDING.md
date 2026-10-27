# Extending nanomite

Three extension points, all human-configured, none model-authorizable: hooks,
external tools, and the built-in tool library. Status: **L3**.

## Hooks (`src/hooks.rs`)

Lifecycle scripts run at fixed points. Everything is advisory except
deny-hooks, which are the one guardrail with teeth.

- **Location**: `~/.config/nanomite/hooks/` (`NANOMITE_HOOKS_DIR` overrides;
  `NANOMITE_HOOKS=0` disables the subsystem).
- **Manifest**: `hooks.json` — an array of
  `{ "event", "command", "timeout_ms?", "deny?" }`.
- **Auto-discovery**: an executable dropped into `hooks/<event>/` runs for
  that event (5s cap, never deny).
- **Events**: `pre_tool_call`, `post_tool_call`, `turn_start`, `turn_end`,
  `policy_change`.
- **Payload**: the event as JSON on stdin. Output captured (4k cap), bounded
  per-hook (max 30s), process-group killed on timeout — grandchildren die with
  the group, so nothing leaks or holds pipes.

**Deny hooks** (`deny: true`, `pre_tool_call` only): a non-zero exit refuses
the tool call before dispatch; the hook's stderr becomes the refusal the model
sees (`code: hook_denied`). Example: block `git push` from the agent while
allowing everything else.

Hooks never block the turn except a deny gate; post hooks run detached, and a
failing hook is advisory — the tool call still happened.

## External tools (`src/tools/external_tools.rs`)

User-defined tools from TOML manifests.

- **Location**: `~/.config/nanomite/tools/*.toml` (`NANOMITE_TOOLS_DIR`
  overrides).
- **Manifest**: `name` (snake_case), `description`, `command`, `parameters`
  (OpenAI function schema; optional), `timeout_ms` (default 60s, max 120s).
- **Contract**: arguments arrive as JSON on **stdin** — no `$arg` interpolation
  exists, so argument values can never become shell syntax. stdout returns to
  the model (16k cap); the process runs in its own group and is killed whole
  on timeout.
- **Validation at load**: bad manifests are skipped with recorded errors and
  never break the other tools. Required/unknown argument keys are checked
  against the schema at call time.
- **Risk**: classified `Safe` — the schema and `/tools` gate apply like any
  built-in; the command itself runs with your permissions (same boundary as
  `run_command`, `docs/SECURITY.md`).

Schemas are appended to what the model sees; `/tools` lists them under
`external` and they honor the enable/disable toggle.

## Built-in tool library

`/tools` toggles any of the 33 built-ins; disabled tools vanish from the
schema list *and* are refused at dispatch (`docs/TOOLS.md`). External tools
honor the same list.

## What extension points deliberately do NOT exist

Model-authorizable tool creation, network-loaded manifests, auto-updating
hooks — all refused by design: the model must never be able to grow its own
permissions (`docs/SECURITY.md`).
