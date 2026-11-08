# Offline mode

Status: **L2 — implemented and verified** (part of v1.3.0).

## What changed

- `NANOMITE_OFFLINE=1` (or `offline = true` in `config.toml`) puts nanomite into
  offline mode. The header shows a `⚡` indicator so you never mistake a local
  run for a cloud one.
- Network-dependent tools (`web_fetch`, `web_search`) refuse with
  `"refused: ... is disabled in offline mode"` instead of reaching the network.
  They do not silently fall back to a cloud model.
- All local tools (files, git, test, index, plan, memory, exec, improve) keep
  working unchanged.
- Local providers (Ollama, LM Studio, vLLM, llama.cpp server) still work with no
  API key; they were already keyless/offline-capable.

## Configuration

`NANOMITE_OFFLINE` accepts `1`, `true`, `yes`, `on` (case-insensitive).

```toml
# ~/.config/nanomite/config.toml
offline = true
```

Env var takes precedence over the file, like every other config key.

## What stays networked

- Autoupdate (`nanomite update`) still needs network to fetch origin / download
  releases — that is intentional and out of offline mode's scope.
- A cloud provider (OpenAI, Anthropic, etc.) still needs network for
  completions. Offline mode does not switch your provider; it only disables
  network *tools*. If you want fully local inference, switch to a keyless
  localhost preset (`/provider ollama` etc.) before turning offline mode on.

## Tests

- `tools::web_tools::tests::offline_refuses_fetch_and_search` — offline gate
  refuses both fetch and search with an "offline" refusal.
- `config::tests::offline_flag_env_and_persistence` — env override, file
  persistence round-trip, and defaults-off behavior.
- `cargo test --lib` runs fully offline: 768 passed / 0 failed.

## Before this change

The entire offline story was just `ProviderPreset.offline` flags on keyless
localhost presets — no indicator, no gating, nothing stopped `web_fetch` /
`web_search` from firing while the user believed they were offline.
