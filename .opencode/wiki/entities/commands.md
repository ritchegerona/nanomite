---
title: Commands (slash-command table)
tags: [entity, commands, ui]
sourceRefs: [src/commands.rs, src/app/mod.rs]
lastReviewed: 2026-11-10
---

# Commands (slash-command table)

`src/commands.rs` defines the `Command` enum (43 variants) and the `COMMANDS`
registry (`Entry { command, aliases, hint, kind }`), parsed locally by `parse()`
(leading slash-words only). Dispatch lives in `app/mod.rs::run_command`.

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
| `/refresh` | — | alt+shift+r | stop the hung turn and continue the task ([[concepts/session-features]]) |

Shift+Tab cycles [[concepts/tool-policy]] modes.

## /refresh resume semantics (2026-11-10)

`App::refresh` (`src/app/mod.rs`) finds the last `EntryRole::User` entry,
strips `[image: ` badge lines (same strip as `/edit`), then picks the
continuation prompt:

- **Turn made progress** (any Assistant entry after it with non-empty text or
  tool rows) **and is unfinished** (busy, or the tail entry is
  `Phase::Interrupted`/`Failed`) → sends a resume instruction:
  `[refresh] Continue the interrupted task from where it stopped. Do not
  repeat steps already completed; finish the remaining work.`
- **No progress** (hung before the first token) or the last turn **completed
  normally** → re-sends the original prompt verbatim (re-run semantics).

Busy path queues the prompt before `force_interrupt` — the interrupt path
(`finalize_interrupted` → `finish_turn`) drains `prompt_queue` and starts the
fresh turn itself; idle path calls `start_turn` directly. Tests pin all three
paths in `src/ui/tests.rs` (`refresh_*`).

The help sheet renders at 92x32 because 43 command rows no longer fit 30.
