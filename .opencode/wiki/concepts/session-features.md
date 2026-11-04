---
title: Session export, history, fork and edit-and-resend
tags: [concept, feature, session, export]
sourceRefs: [src/app.rs, src/session.rs, src/commands.rs]
lastReviewed: 2026-10-30
---

# Session export, history, fork and edit-and-resend

## /export (alt+j) — markdown & HTML
- Default: `session::to_markdown` → `.nanomite-session-<stamp>.md`, then
  `Control::EditFile` opens `$EDITOR`. `/share` aliases it.
- `/export html` (or `/export web`): `session::to_html` renders a standalone page
  (inline `HTML_EXPORT_CSS`, role-colored bubbles, tool chips) →
  `nanomite-session-<stamp>.html`. All markup is entity-escaped by `html_escape`
  (built from `concat!("&","amp;")` pieces — see the HTML-entity gotcha below).

## /history <query>
FTS search over saved sessions (`session::search_saved`) → `Control::HistoryResults`
→ picker overlay. A backtick also opens fuzzy history search.

## /fork <n> and /edit <n> — edit & resend
- `/fork <n>`: saves the FULL original session first (`Control::ForkSaved` via
  `spawn_blocking`), then truncates entries at turn N (keeping it),
  `history.truncate(...)`, `turn_history.truncate(turn)`.
- `/edit <n>` (alt+w) builds on fork: strips `[image: ` badge lines from the old
  prompt (images are not re-attached), clears the composer, preloads turn N's user
  text, and toasts "editing turn N - send to resend" — the ChatGPT-style
  edit-and-resend flow. Busy turns are refused; turn range errors are toasts.

## Gotcha: HTML entities in tool-call arguments
The edit-tool transport decodes named HTML entities in parameters (`&`→`&`,
`<`→`<`, `"`→`"`); numeric `&#39;` survives. Never type entities literally in
edits — build strings from concat pieces and assert escaped output via `concat!`.
