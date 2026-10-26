---
title: Reasoning and Context Controls
tags: [reasoning, context, compact, tokens, commands]
sourceRefs: [src/commands.rs, src/app.rs, src/config.rs, src/agent/openai.rs, README.md, docs/FEATURES.md]
lastReviewed: 2026-10-16
---

# Reasoning and Context Controls

What is *actually* implemented today — the part that makes the "reasoning modes
(L0)" and "token/context management (L0)" gaps only half-true.

## Reasoning effort (`/reasoning`, `/effort`, `Ctrl+R`)

- `/reasoning` (alias `/effort`) — `src/commands.rs:116`.
- `Ctrl+R` and `Action::CycleReasoning` both call `cycle_reasoning()`
  (`src/app.rs:2511`, `:2637`, `:2689`); it toasts `reasoning: <label>`.
- `Reasoning` enum lives in `src/config.rs:238`; `as_str()`/`label()`.
- Only sent when the provider supports it:
  `supports_reasoning_effort()` (`src/config.rs:751`).
- `src/agent/openai.rs:111` injects `body["reasoning_effort"]`.
- Config key `reasoning = "..."`, env `NANOMITE_REASONING`.

## Thinking indicator (`/thinking`)

Toggles `app.show_thinking` (default `true`). It only controls whether the
"thinking" status row is rendered (`src/ui/chat.rs:74`, `:236`) — it is a
*display* toggle, not a reasoning mode.

## Context meter

`NANOMITE_CONTEXT_LIMIT` (default `200000`) — "Token budget behind the context
meter" (README.md:142). A display meter only; it does not drive compaction or
retrieval budgets.

## `/compact` (alias `/summarize`)

- `src/commands.rs:62`; handler `run_compact()` at `src/app.rs:1422`.
- Asks the model to summarize the session head ("Keep decisions, file paths
  and open questions"), inserts a system entry `compacted N turns`.
- `trim_history()` (`src/app.rs:1269`) runs on several paths.
- **Unmeasured**: summarization quality and real context savings are not
  verified — `docs/FEATURES.md` keeps it under "Landing (verify before
  claiming)".

## See also

- [[concepts/capability-gaps-l0]]
