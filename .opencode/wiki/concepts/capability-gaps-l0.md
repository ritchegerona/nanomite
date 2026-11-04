---
title: Capability Gaps (L0)
tags: [audit, roadmap, security, gaps, l0]
sourceRefs: [docs/AUDIT-2026-10-14.md, docs/FEATURES.md, docs/OFFLINE_MODE.md, docs/SECURITY.md, AGENTS.md, src/memory/policy.rs, src/agent/runtime.rs, src/app.rs, src/agent/openai.rs, README.md]
lastReviewed: 2026-10-16
---

# Capability Gaps (L0)

Maturity levels come from `docs/EVOLUTION.md`: **L0** not implemented ·
**L1** implemented, unverified · **L2** tested under defined conditions ·
**L3** integrated + regression-tested · **L4** benchmark-validated + monitored.

## Summary of Implementation Status

Four gaps were repeatedly cited as missing. This document tracks which ones are now implemented.

## 1. Reasoning / execution mode toggle — PARTIAL

Implemented: `/reasoning` (alias `/effort`) and `Ctrl+R` cycle the `Reasoning`
enum and forward `reasoning_effort` to providers that support it.

- `src/commands.rs:116` — `/reasoning`, `/effort`
- `src/app.rs:2511` — `cycle_reasoning()`
- `src/config.rs:238`, `:751` — `Reasoning` type, `supports_reasoning_effort()`
- `src/agent/openai.rs:111` — emits `reasoning_effort` in the request body
- `/thinking` toggles only the *display* of the thinking indicator
  (`src/app.rs:1404`), not a behavior mode.

Missing: a plan-vs-execute / task-strategy mode (fast / deep / coding /
research), complexity assessment, per-mode metrics. Audit table calls this
**L0** ("only provider `reasoning_effort` passthrough").

## 2. Central tool authorization engine — L1 (partial)

**NOW IMPLEMENTED** (initial draft, L1 only):

- New `ToolPolicy` enum with three modes: **auto**, **plan**, **readonly**
  (`src/agent/policy.rs:1-78`)
- Risk classification of tools into `Safe`, `Write`, and `External`
  (`src/agent/policy.rs:33-50`)
- Central authorization at the single choke point `runtime::run_tool`
  (`src/agent/runtime.rs:563-620`)
- Configuration via `tool_policy` field in `Config` (`src/config.rs:136-138`)
  with support for:
  - Environment variable: `NANOMITE_TOOL_POLICY`
  - Stored file: `tool_policy = "auto"`
  - Default: `ToolPolicy::Auto`
- `/policy` command and `Ctrl+P` keybinding (`src/commands.rs:138-142`)
- UI header indicator: model · reasoning · **tool_policy** (`src/ui/panels.rs:58-70`)
- Plan/readonly modes refuse mutations with machine-readable JSON errors:
  ```
  {"error":"plan mode is active: mutating tools are disabled. ...","code":"plan_mode"}
  ```

### What still needs to be done (Phase 2):

- Add interactive approve overlay (follow-up phase, per user design decision)
- Ensure policy is correctly applied across all tool calls (including memory, improve, etc.)
- Add tests for policy enforcement with plan/readonly modes

## 3. Offline mode indicator — L1 (implemented)

**IMPLEMENTED** in audit documentation (`docs/OFFLINE_MODE.md`) — no code yet.

### Current state:
- `ProviderPreset.offline` exists and keyless localhost endpoints work
- No UI indicator, no network-gating policy, no queued actions
- Web tools always attempt the network

## 4. Token budgeting / compact quality — L1 (existing)

**ALREADY EXISTS**:

- Context meter: `NANOMITE_CONTEXT_LIMIT` (default 200000), README.md:142
- `/compact` (alias `/summarize`) exists and summarizes the session head
- What is **unmeasured**: `/compact` summarization quality and actual context savings

## Recommendation

1. **Central tool authorization engine** — L1 implemented. Complete with:
   - Interactive approve overlay (Phase 2, per design)
   - Full coverage across all tool call sites
   - Comprehensive tests for plan/readonly enforcement
2. **Offline mode indicator** — add UI status indicator per existing spec
3. **Reasoning/execution modes** — only partial; plan/execute mode remains L0
4. **Compact quality** — continue measuring existing `/compact` behavior

## See also

- [[concepts/reasoning-and-context-controls]]
- `docs/AUDIT-2026-10-14.md` §5 (gaps), §6 (maturity table)
- `docs/FEATURES.md` (honest capability inventory)

```
@7@