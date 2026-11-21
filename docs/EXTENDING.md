# Extending nanomite

Three extension points, all human-configured and none model-authorizable:
hooks, external tools, and the built-in tool library. Status: **L3**.

## Hooks

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

## External tools

User-defined tools from TOML manifests.

- **Location**: `~/.config/nanomite/tools/*.toml` (`NANOMITE_TOOLS_DIR`
  overrides).
- **Manifest**: `name` (snake_case), `description`, `command`, `parameters`
  (OpenAI function schema; optional), `timeout_ms` (default 60s, max 120s).
- **Contract**: arguments arrive as JSON on **stdin** — no `$arg` interpolation
  exists, so argument values can never become shell syntax. stdout returns to
  the model as the tool result.
- **Validation**: schema validation at load time; required/unknown arg keys
  checked at call time.
- **Dispatch**: through the same gate as built-ins (disabled filter, policy,
  hooks). Listed in `/tools` under "external", toggleable.

## Tool library

`src/tools/catalog.rs` holds the built-in inventory as
`ToolSpec { name, description, category }`. Tests pin it to the exposed schema
list exactly. To add a built-in tool, add it to `catalog.rs` and the schema
list, implement its dispatch arm in `runtime.rs::run_tool`, and add tests.

Never add an agent-callable apply path for self-modification — the `/improve`
human-approval gate is the only route.
