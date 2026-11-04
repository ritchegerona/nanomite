---
title: Scroll model and custom scrollbar
tags: [concept, ui, scrolling]
sourceRefs: [src/app.rs, src/ui/chat.rs, src/ui/tests.rs]
lastReviewed: 2026-10-30
---

# Scroll model and custom scrollbar

## Bottom-anchored offsets

nanomite scroll offsets are measured **from the bottom**:
`View::scroll_by(positive)` scrolls UP/away from the tail (`scroll_from_bottom`
increases); a negative delta scrolls back toward the tail. `View::window()` computes
`top = max_scroll - scroll_from_bottom`; follow mode ⟺ `scroll_from_bottom == 0`.
Tests that want "scrolled to bottom" pass a huge negative delta or leave offsets at
zero, not a large positive one.

## Custom proportional scrollbar (chat view)

ratatui 0.30's `Scrollbar` stops the thumb short of the track ends at full scroll
(thumb_start = position*track/(content-1+viewport)), so `chat::render` uses a custom
1-column scrollbar instead:

- `thumb_len = ((viewport*track + content/2) / content).clamp(1, track_len)`
- `thumb_start = ((top*(track_len-thumb_len) + max_scroll/2) / max_scroll)
  .min(track_len-thumb_len)` — flush at BOTH ends (top=0 and top=max).
- Rendered as per-row `Paragraph` of `█` (accent) / `│` (border) spans.

Regression test `scrollbar_thumb_reaches_track_ends` (ui/tests.rs) renders a 40-entry
app at 92x24 via TestBackend and asserts flush-bottom in follow state and flush-top
after a huge scroll-up. Click/drag mapping (`scrollbar_jump` in app.rs) already used
the matching full-track proportional mapping.
