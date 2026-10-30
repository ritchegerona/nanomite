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

## [2026-10-30] lint | pending
- AGENTS.md still says "no central authorization engine" — now partially stale:
  ToolPolicy (static) + Confirmer gate (interactive) exist; per-call authorization
  for core tools is covered for External calls, Write calls still rely on mode
  posture. Update when AGENTS.md is next touched by the owner.
- docs/SECURITY.md / docs/TOOLS.md predate all of this; trust code over docs.
