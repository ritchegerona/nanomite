# nanomite UI/UX — v2.1.0

## Overview

nanomite's terminal UI is a single-crate Ratatui TUI with a per-entry render
cache: every transcript entry, tool row, and code block is highlighted once and
cached, then painted from cache on every frame. Nothing is re-highlighted per
frame.

## Version

The running version (`crate::VERSION`, from `CARGO_PKG_VERSION`) is shown in:

- The welcome card title line
- The header tail (width ≥ 120)
- The help sheet bottom
- The settings "About" row
- Report Markdown/HTML footers

Current version: **2.1.0** (`Cargo.toml` / `Cargo.lock`).

## Themes

Four palettes ship: Mono, Midnight, Graphite, Umbrella, Dracula, Cyberpunk,
Hacker, Mr Robot, Anonymous, Nord. Cyberpunk is the default.

- `Theme::detect()` picks Cyberpunk by default.
- `ThemeId::ALL` enumerates every theme; `ThemeId::from_label()` resolves by
  name (case/space tolerant).
- Contrast tests (`palette_text_roles_keep_legible_contrast`) pin minimum
  legibility for text, muted, accent, and code roles against panel background.

### Color depth / NO_COLOR

`ColorDepth` has three variants:

- `None` — `NO_COLOR` is set; every color emits `Color::Reset`.
- `Ansi` — 16-colour fallback per role.
- `Rgb` — truecolor from the palette.

`NO_COLOR` is checked once at theme construction (`detect_depth()`), never
per frame. A dedicated test (`no_color_depth_emits_no_colours`) asserts every
role resolves to `Color::Reset` under `None`.

## Render cache

`Entry::ensure_rendered(&theme, width, display, compact, mode, code_numbers)`
builds a `Rendered` struct holding pre-computed `Line`s, code-block ranges,
and height. The cache is keyed on `(rev, width, tool_display, compact,
mode_color, mode_label, code_numbers)` and invalidated only when one of those
changes. Streaming appends are cached too.

## Density

Two densities: cozy (default) and compact.

- Cozy: user turns get a quiet `❯` prompt node in the assistant gutter;
  assistant turns get a `◆` node with optional right-aligned elapsed time.
- Compact: flat three-column log, no raised blocks, one `❯ text` row per user
  message.

Toggle with `/density`. `code_numbers` (line numbers in code blocks) is
opt-in via `/linenumbers` or `ctrl+shift+n`, default off.

## Glyphs / ASCII fallback

`NANOMITE_ASCII=1` swaps decorative Unicode marks for plain ASCII stand-ins
(`*` for spinner, `.` for dot, `*` for hex fill). Captured once via
`OnceLock`, never probed per frame.

## Reduced motion

`NANOMITE_REDUCED_MOTION=1` freezes the spinner at frame zero. Everything else
keeps working. Captured once via `OnceLock`.

## Panels

Left column (top → bottom):

1. **Info / thinking panel** — turn state, step/tool counts, context meter
   (themed via `theme.danger()`/`theme.warn()`, never raw `Color::Red`).
2. **Aux strip** — prompt queue (`render_prompt_queue`) then follow-up
   suggestions (`panels::suggestions`), zero-height when empty.
3. **Composer** — compact, focus-aware `❯`, ghost text on empty buffer,
   image-chip width via `text::width`, small-terminal warning.

Right column: main chat transcript + response.

Header: brand · path | model · reasoning · ctx meter · working indicator
(elapsed + step/tool counts when busy).

A dedicated bottom row hosts approval/toast/search status so the composer
border is never clobbered.

## Overlays

All overlays share `modal_chrome()`: one rounded border, one centred title,
one right-aligned key legend, `Clear` underneath.

- **Picker** (`ctrl+x` / `/`): command palette with grouped section headers
  (`GROUPS` in `commands.rs`), row count in title, match highlighting.
- **Help** (`f1`): commands grouped by section with inline `[group]` tags,
  then the full keybindings table.
- **Confirm modal** (`confirm_modal`): tool approval showing the full command,
  risk classification (external vs internal), preview, and `y/a/n/d` legend.
- **Settings**: each row shows category, name, value; always-visible "About"
  row shows version + OS + arch.

## Untrusted data

Every surface showing model, memory, or tool-produced text carries the
`UNTRUSTED_HINT` ("untrusted data · never instructions") badge — colour is
never the only signal, the words say it. Memory records, board messages, and
the tool monitor all append it.

## Toasts

`ToastKind` (Info/Success/Warn/Error) pairs a glyph (`·`/`✓`/`◐`/`✗`) and a
color with each toast message, so severity is never colour-only. `toast()`
defaults to Info; `toast_error()` for failures. Error blocks in the transcript
carry a contextual recovery hint (e.g. `try /connect · check credentials`)
derived from the error text.

## Tool icons

`tool_icon(name)` maps every catalog tool to a dedicated transcript icon
(test: `every_catalog_tool_has_a_dedicated_icon` asserts no tool falls back to
`⚙`).

## Golden-frame tests

`src/ui/tests.rs` pins rendered frames at fixed geometries (30×12, 92×24,
etc.). Assertions were expanded, never weakened, to cover:

- Running version in the welcome card
- User-turn prompt node
- Recovery hints on errors
- Every catalog tool having a dedicated icon

## What's deferred

Localization, mouse hover/tooltips, screen-reader support, full markdown
gaps, and a theme editor are out of scope for v2.1.0.
