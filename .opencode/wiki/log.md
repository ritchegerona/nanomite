# Wiki log

## [2026-10-30] seed | initial wiki
Seeded from the 2026-10-30 session: scrollbar fix, vision input, edit-and-resend,
HTML export, tool confirmation gate. Pages: index, app, agent, commands,
confirm-gate, tool-policy, vision-input, session-features, scroll-model.

## [2026-10-30] ingest | concepts/confirm-gate
Interactive y/n/a/d approval for Risk::External tools; Confirmer trait, SessionGrants
fast-path, footer with request preview; `confirm_external` config default true.

## [2026-10-30] ingest | concepts/session-features
/export html variant, /edit <n> edit-and-resend, /fork, /history — plus the
HTML-entity transport gotcha and its escape strategy.

## [2026-10-30] ingest | concepts/tool-policy
Documented the pre-existing ToolPolicy modes and Risk classification; noted the
owner-removal of the self-edit SOURCE_DENYLIST (2026-10-30).

## [2026-10-30] ingest | concepts/vision-input
/image pipeline, 4-image/10 MB limits, multimodal wire format, provider-support
caveat.

## [2026-10-30] ingest | concepts/scroll-model
Bottom-anchored scroll offsets and the custom flush-at-both-ends chat scrollbar.

## [2026-10-30] ingest | entities/clickable-links
Feature complete and verified (530/530 lib tests, clippy 0 warnings, fmt clean).
Data flow, injection-safe `xdg-open` spawn, test inventory. Click test locates
its target from the link's own cached region (row + start + 2), like the
code-block copy test.

## [2026-10-30] ingest | concepts/transcript-row-mapping
Documented `mouse_to_text_position` semantics: `status_rows()` is streaming-only,
unanchored selections bail before fallbacks, char-index space for columns.
Recorded the `rect.y` vs `rect.y + 1` test gotcha (line 0 of a block is a spacer
row for user entries; locate clicks from cache instead of guessing).

## [2026-10-30] merge | concurrent-session clobber restored
A parallel session rewrote index.md/log.md as if the wiki were empty, dropping
the catalog and log. Merged back: full page list + both sessions' entries; added
a working note warning parallel sessions to re-read before rewriting.

## [2026-10-30] lint | pending
- AGENTS.md still says "no central authorization engine" — now partially stale:
  ToolPolicy (static) + Confirmer gate (interactive) exist; per-call authorization
  for core tools is covered for External calls, Write calls still rely on mode
  posture. Update when AGENTS.md is next touched by the owner.
- docs/SECURITY.md / docs/TOOLS.md predate all of this; trust code over docs.
