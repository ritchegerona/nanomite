# Offline mode

Status: **L0 — not implemented.** This document describes the current reality
and the target behavior so partial work is never mistaken for the feature.

## Today (verified)

- `ProviderPreset.offline` flags keyless localhost endpoints (Ollama, LM
  Studio, vLLM, llama.cpp server). Selecting one works with no API key and
  issues no cloud calls for *completions*.
- That is the entire offline story. There is **no** offline status indicator,
  no network gating, no queuing of network-dependent actions, no reporting of
  unavailable capabilities, and nothing stops the agent from calling `web_fetch`
  / `web_search` (or a cloud provider, after a preset switch) while the user
  believes they are offline. Do not describe nanomite as offline-capable.
- Network-dependent tools: `web_fetch`, `web_search`, `ssh_run`. Local-only
  tools: all 28 others (files, git, test, index, plan, memory, exec, improve).

## Target behavior (planned, Milestone 9)

- Explicit offline toggle + visible status indicator bound to real runtime
  state (provider locality × network reachability probe).
- Offline mode: only local providers/services may be called; local memory and
  file tools keep working; network-dependent tools are disabled or queued per
  policy with a clear message; sessions and work are preserved; the app never
  silently falls back to a cloud model.
- When a local model is unavailable, report the limitation and offer only
  configured, policy-permitted alternatives.
- Distinguish in UI and docs: fully-local operations · local inference ·
  network-dependent operations · cloud-required operations.
- Tests: network-isolated runs proving zero non-local connections, plus
  degradation-path tests.

## Provider configuration (available now)

Endpoint, model, timeouts, and step/tool budgets are configurable via
`NANOMITE_BASE_URL`, `NANOMITE_MODEL`, `NANOMITE_STEP_TIMEOUT_MS`,
`NANOMITE_MAX_STEPS`, `NANOMITE_MAX_FILE_BYTES`, `NANOMITE_CONTEXT_LIMIT`,
`NANOMITE_TOOL_POLICY`, `NANOMITE_DISABLED_TOOLS`. Per-provider context/output
limits, request timeouts, streaming flags, and fallback providers are **not**
implemented — see `docs/EVOLUTION.md`.