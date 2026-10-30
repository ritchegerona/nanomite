---
title: Tool confirmation gate
tags: [concept, security, authorization, confirm]
sourceRefs: [src/agent/policy.rs, src/agent/runtime.rs, src/app.rs, src/ui/panels.rs, src/config.rs]
lastReviewed: 2026-10-30
---

# Tool confirmation gate

Interactive y/n/a/d approval for **Risk::External** tools (`run_command`, `ssh_run`,
`memory_delete`) when the current [[concepts/tool-policy]] mode permits them.

## Design

- `Confirmer` trait in `policy.rs`:
  `fn confirm(&self, tool: &str, preview: Option<String>) -> Pin<Box<dyn Future<Output = bool> + Send>>`
  (returns `'static` futures — clone captured data first).
- `AgentRuntime` holds `Option<Arc<dyn Confirmer>>` (`with_confirmer`). Before
  dispatching an External call it `select!{ confirm vs cancel }`; `false` produces a
  `ToolEnd{ok:false}` and a `user_declined` JSON error as the model's tool result —
  the turn continues.
- UI side (`app.rs`): `UiConfirmer` checks `SessionGrants` (allow/deny HashSets,
  session-scoped) first — a hit answers instantly without prompting; otherwise it
  queues a `ConfirmRequest` over an unbounded channel. `pump_engine` surfaces it as
  `pending_confirm`; `on_key` intercepts it: **y** allow, **a** allow + remember,
  **n**/**Esc** deny, **d** deny + remember.
- Footer (`ui/panels.rs`): hourglass prompt with the tool name plus the request
  preview (single line, truncated at 48 chars) so the user sees what would run.
- Config: `confirm_external: bool` (default **true**); `NANOMITE_CONFIRM_EXTERNAL`
  env override; written to config only when disabled.

## Invariants

- The confirmer never blocks the UI thread (oneshot + channels only).
- A dead UI (`tx.send` error) answers **false** — refuse rather than run unasked.
- `test_config()` sets `confirm_external = false` so offline tests never wait.
