---
title: Commands (slash-command table)
tags: [entity, commands, ui]
sourceRefs: [src/commands.rs, src/app.rs]
lastReviewed: 2026-10-30
---

# Commands (slash-command table)

`src/commands.rs` defines the `Command` enum (40 variants) and the `COMMANDS`
registry (`Entry { command, aliases, hint, kind }`), parsed locally by `parse()`
(leading slash-words only). Dispatch lives in `app.rs::run_command`.

## Registry invariants (enforced by tests)

- Every command's hint MUST advertise a keybinding prefix — "alt+", "ctrl+",
  "shift+tab", "f1", "f2" (`every_command_advertises_a_keybinding`).
- Names and aliases are unique; every command parses under its name and aliases;
  the opencode command set is covered.

## ChatGPT-inspired set (2026-10-30)

| Command | Alias | Key | What it does |
|---|---|---|---|
| `/image <path>` | `/img` | alt+v | attach image to next message ([[concepts/vision-input]]) |
| `/edit <n>` | — | alt+w | rewind to turn N, preload its prompt for resend ([[concepts/session-features]]) |
| `/export [html]` | `/share` | alt+j | markdown or standalone HTML page |
| `/history <q>` | — | (backtick) | FTS search over saved sessions |
| `/fork <n>` | — | alt+f | save full session, truncate to turn N |

Shift+Tab cycles [[concepts/tool-policy]] modes.
