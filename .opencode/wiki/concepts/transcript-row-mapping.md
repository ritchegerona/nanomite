---
title: Transcript row mapping and mouse hit-testing
tags: [concept, ui, mouse, testing, gotcha]
sourceRefs: [src/app.rs, src/ui/chat.rs, src/ui/tests.rs]
lastReviewed: 2026-10-30
---

# Transcript row mapping

How screen coordinates become transcript text positions — and the trap for
UI tests.

## The mapping

`App::mouse_to_text_position(mouse_row, mouse_col, inner_width)` in
`src/app.rs`:

- `virtual_row = view.window().top + (event.row - chat_rect.y)`; entries are
  walked in order, each contributing `entry.slot(compact)` rows.
- Inside an entry, the first `entry.status_rows()` rows are skipped — but
  `status_rows()` is **1 only while `Phase::Streaming`**; a completed entry
  has **zero** status rows and no header row.
- Rows beyond `cached.lines.len()` (blank separators, padding) return `None`
  → the selection stays **unanchored** (`usize::MAX`) and the mouse-up
  handler bails before any link/copy/error fallback.
- Columns go through `display_col_to_char` (wide glyphs occupy two cells), so
  link regions and code-block hits all live in **char-index space** over
  `cached.lines`.

## Body layout of a finished assistant entry

`chat.rs::build`, `EntryRole::Assistant`, `Phase::Done`, no reasoning/tools:
the body is exactly the markdown lines, each prefixed with a 3-space indent
span, starting at the entry's first transcript row. A one-line reply is one
row.

## Test gotcha (cost a failed test)

Clicking `rect.y + 2` on a fresh single-line assistant entry lands *past* the
body → `None` → unanchored → early return, no toast. To click the first
entry's first body row, click **`rect.y` exactly**. (The older
error-expansion test got away with `rect.y + 2` only because error entries
render taller bodies.)

Related: [[clickable-links]]
