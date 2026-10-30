---
title: Clickable links in the chat transcript
tags: [feature, ui, mouse, links, xdg-open]
sourceRefs: [src/ui/text.rs, src/ui/chat.rs, src/app.rs, src/main.rs, src/ui/tests.rs]
lastReviewed: 2026-10-30
---

# Clickable links

Owner request (2026-10-30): links in the chat should be visible and a plain
click should open them in the system browser.

## Data flow

1. **Rendering needs no change.** The markdown renderer already emits
   `[label](url)` as the label followed by a dim ` (url)` span, so URLs are
   always visible text.
2. **Region scan** — `pub fn link_regions(lines: &[Line<'static>]) ->
   Vec<(usize, usize, usize, String)>` in `src/ui/text.rs` (before `fn find`):
   scans each row's concatenated span chars for `http(s)://` URLs, with token
   boundaries mirroring `match_inline` (whitespace, `)`, `,`). URLs hard-wrapped
   across rows are merged: one region is emitted **per covered row**, each
   carrying the **full** URL.
3. **Cache** — `Rendered.links: Vec<(u16, u16, u16, String)>` (row, start, end,
   url) computed in `ensure_rendered` (`src/ui/chat.rs`); `usize`→`u16` cast
   map applied there.
4. **Hit test** — `App::link_at(entry, row, col)` in `src/app.rs` finds the
   region containing the char column.
5. **Click path** — `handle_chat_mouse` `Up` branch: after the unanchored
   guard, a plain click (`sel.start == sel.end`) that hits a link sends
   `Control::OpenLink(url)` + toast `"opening {url}"` **before** the
   copy/code-block/error fallbacks. Drags still select/copy.
6. **Spawn** — `run_control` in `src/main.rs` early-returns on
   `Control::OpenLink` → `open_in_browser`: `std::process::Command` `xdg-open`
   with the URL as a **single arg** (no shell → injection-safe for untrusted
   model text), stdio nulled, fire-and-forget; spawn error →
   `App::on_link_opened(false)` toasts `could not open browser (xdg-open)`.

## Tests

- `link_regions_find_bare_urls_and_markdown_targets` (text.rs)
- `link_regions_merge_hard_wrapped_urls_into_one_link` (text.rs)
- `clicking_a_link_opens_it_in_the_browser` (ui/tests.rs)

Verified 2026-10-30: `cargo test --lib` 530 passed / 0 failed, clippy 0
warnings, fmt clean.

See [[transcript-row-mapping]] for why the test clicks `rect.y` exactly.
