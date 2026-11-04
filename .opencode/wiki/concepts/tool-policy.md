---
title: Tool policy modes
tags: [concept, security, authorization]
sourceRefs: [src/agent/policy.rs]
lastReviewed: 2026-10-30
---

# Tool policy modes

`ToolPolicy` in `src/agent/policy.rs` is the static authorization layer applied to
every tool call inside `run_tool` (before hooks and dispatch):

| Mode | Posture |
|---|---|
| `Plan` | read-only analysis tools |
| `Coder` | code tools, no shell |
| `Build` (default) | build + shell allowed |
| `Hacker` | everything |

- `authorize(name) -> Result<(), String>` returns JSON error payloads
  (`{"error": "...", "code": "policy_denied"}`) the model sees as the tool result.
- `Risk::of(name)` classifies calls: **Safe** / **Write** (write_file, edit_file,
  memory_save/update, propose_self_edit, git_commit/branch, plans/tasks,
  index_symbols) / **External** (run_command, ssh_run, memory_delete).
- `parse()` accepts legacy aliases; `next()` is the Shift+Tab cycle; the choice is
  persisted as `cfg.tool_policy` and shown as a UI mode badge.

The **dynamic** layer on top of this is [[concepts/confirm-gate]] — interactive
approval for External calls regardless of mode posture.

Note: the self-edit `SOURCE_DENYLIST` was removed by owner decision 2026-10-30;
self-edits remain gated by user approval in `/improve` plus the `cargo test --lib`
regression gate and the JSONL audit log.
