# TODO

## Next build (2026-10-16)

- [ ] Improve the whole nanomite display UI — overhaul the terminal UI
  (transcript rendering, tool rows/previews, overlays/pickers, status/composer
  areas). Keep invariants: no blocking I/O on the UI thread, bounded previews
  (12 lines / 1,600 chars), `/details` collapse behavior preserved.
- [ ] Add self-improving capability to nanomite — agent can propose, apply and
  evaluate changes to its own prompt/tools/config. Scope: sandboxed edit +
  rollback, diff review gate before commit, regression check run (tests +
  smoke build) before keeping a change, full audit log. Safety: never
  self-modify during a user session without explicit approval; no changes to
  the safety/permission layer.

## Backlog
