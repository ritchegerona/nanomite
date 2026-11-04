---
title: Agent layer (src/agent/)
tags: [entity, provider, runtime, policy]
sourceRefs: [src/agent/provider.rs, src/agent/openai.rs, src/agent/runtime.rs, src/agent/policy.rs]
lastReviewed: 2026-10-30
---

# Agent layer

## provider.rs
- `ChatMessage { role, content, images: Option<Vec<String>>, tool_calls, tool_call_id }`
  — `images` holds `data:` URLs (serde default, skipped when `None`);
  `ChatMessage::user_with_images(content, images)`.
- `ToolCall { id, name, arguments (raw JSON), index }` + `ToolCallAccumulator`
  reassembles streamed fragments by index.
- `StreamEvent` includes `Delta`, `ToolCall(ToolCall)`, `Done`, …

## openai.rs — wire format
`OpenAiProvider::message_to_wire` is the single choke point. User messages with
non-empty `images` become a multimodal parts array: `{type:text}` (only when content
is non-empty) + `{type:image_url, image_url:{url}}` per image. Everything else stays
a plain JSON string (assistant/tool never emit images).

## runtime.rs — one turn
`AgentRuntime::run` streams → dispatches tools via `spawn_blocking(run_tool)`.
`run_tool` gates in order: disabled tools → `ToolPolicy::authorize` → pre-hooks →
arg parse → match dispatch. The run loop asks the optional `Confirmer` **before**
`Risk::External` calls (see [[concepts/confirm-gate]]); a denial feeds
`{"error":"user declined to run <tool> this session","code":"user_declined"}` back
to the model as the tool result and the turn continues.

## policy.rs — authorization
[[concepts/tool-policy]] — `ToolPolicy` modes (Plan/Coder/Build/Hacker) +
`Risk::of(name)` classification driving the confirm gate.

## Test patterns
- `RecordingProvider` + `futures_util::stream::iter(vec![...])` scripted streams.
- `run_turn` / `collect_turn` helpers drive a full turn against an unbounded channel.
- `FixedConfirmer { allow: bool }` — clones the flag into the async block (the
  future must be `'static`; moving `&self` directly fails E0521).
