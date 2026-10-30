---
title: App state machine (src/app.rs)
tags: [entity, ui, state-machine]
sourceRefs: [src/app.rs, src/main.rs]
lastReviewed: 2026-10-30
---

# App state machine

`src/app.rs` (~7k lines) holds the whole TUI state machine; `src/main.rs` only sets
up the terminal, the tokio event loop and the `Control` channel dispatch.

## Key flow

- `on_key`: overlay check first → pending tool confirmation → suggestion strip →
  alt-key bindings → plain editing keys.
- `submit()` parses slash commands first (`run_command`); unknown bare `/word`
  becomes a toast. Otherwise `composer.submit()` — if a turn is busy the prompt is
  queued (`prompt_queue` FIFO).
- `start_turn` is the single funnel for direct and queued prompts: takes pending
  images (`std::mem::take`), pushes the user `Entry` (with `[image: name · size]`
  badges), pushes `ChatMessage::user` / `user_with_images`, spawns the streaming
  turn.
- `pump_engine` drains all event channels (`try_recv` loop): agent events, models,
  memory, board, improve, suggestions, and `confirm_rx` (tool-approval requests).

## Control channel

`pub enum Control` (derives `Debug, Clone`) — UI-thread actions coming back from
background tasks: `EditComposer`, `EditFile`, `Copy`, `SudoResult`, `RevertDone`,
`DoctorReport`, `HistoryResults`, `ResumeHint`, `ForkSaved`, `ImageAttached`.
Dispatched in `src/main.rs::run_control` at **three exhaustive match sites** — a new
variant must touch all three (the compiler enforces it).

## Interactive confirmation

`ConfirmRequest { tool, preview, respond: oneshot::Sender<bool> }` + `SessionGrants`
+ `UiConfirmer` (impl of `agent::policy::Confirmer`) live above the `Control` enum.
`pending_confirm: Option<ConfirmRequest>` is `pub(crate)` — `ui/panels.rs` renders
the footer prompt. Keys: y/a allow (a = always this session), n/Esc/d deny
(d = always deny). See [[concepts/confirm-gate]].

## Related

- [[entities/agent]] for the runtime that asks for confirmation
- [[concepts/session-features]] for `/edit` rewinding (`edit_turn` truncates
  entries/history/turn_history and preloads the composer)
