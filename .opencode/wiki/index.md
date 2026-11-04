# NANOMITE Wiki

Persistent knowledge base for the nanomite Rust terminal coding agent.
Maintained incrementally; every page cross-references its sources.

## Pages

### Entities
- [App state machine](entities/app.md) — `src/app.rs`: keys, overlays, submit flow, Control channel, confirm modal
- [Agent layer](entities/agent.md) — provider, runtime, wire format, ToolPolicy and the Confirmer gate
- [Commands](entities/commands.md) — slash-command table, aliases, keybinding registry rules
- [Clickable links](entities/clickable-links.md) — plain-click URLs in the chat transcript open in the system browser (region scan → cache → hit test → `Control::OpenLink` → `xdg-open`)

### Concepts
- [Tool confirmation gate](concepts/confirm-gate.md) — interactive y/n/a/d approval for Risk::External tools (`confirm_external`)
- [Tool policy modes](concepts/tool-policy.md) — Plan/Coder/Build/Hacker authorization matrix in `src/agent/policy.rs`
- [Vision input](concepts/vision-input.md) — `/image` attachment pipeline and multimodal wire format
- [Session export & history](concepts/session-features.md) — `/export` (md/html), `/history`, `/fork`, `/edit` edit-and-resend
- [Scroll model](concepts/scroll-model.md) — bottom-anchored scrolling and the custom proportional scrollbar
- [Transcript row mapping](concepts/transcript-row-mapping.md) — how mouse coordinates map to entry/row/char positions, and the `rect.y` click gotcha for UI tests

## Working notes
- Verify with `cargo test --lib` (offline, no services) + `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check`.
- The `| tail` idiom masks exit codes — assert via `&&` chain plus a sentinel echo.
- Concurrent sessions: re-read `index.md`/`log.md` before rewriting — a parallel session once clobbered the catalog (restored 2026-10-30).
