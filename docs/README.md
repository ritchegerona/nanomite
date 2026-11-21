# nanomite — Documentation

Professional documentation for the nanomite terminal coding agent. Each file
describes one subsystem and carries its own status line. Trust the code over
the docs if they drift; this index is kept in step with the source.

| Document | Contents | Status |
|---|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Module map, layers, data flow, invariants | Current |
| [TOOLS.md](TOOLS.md) | Tool catalog, categories, limits, contracts | Current |
| [MEMORY.md](MEMORY.md) | Persistent memory: store, policy, capture, security | Current |
| [MODES.md](MODES.md) | Tool permission modes and policy ladder | Current |
| [SECURITY.md](SECURITY.md) | Trust boundaries, authorization, secrets handling | Current |
| [EXTENDING.md](EXTENDING.md) | Hooks, external tool SDK, adding tools | Current |
| [LEARNING.md](LEARNING.md) | Daily learning loop: `/learn`, cadence, saved lessons | Current |
| [UI.md](UI.md) | TUI layout, themes, rendering, keyboard navigation | Current |
| [PERFORMANCE.md](PERFORMANCE.md) | Benchmarks, methodology, optimization rules | Current |
| [INSTALLATION.md](INSTALLATION.md) | Build, install, configure, first run | Current |
| [RELEASES.md](RELEASES.md) | Release history, update workflow | Current |
| [OFFLINE_MODE.md](OFFLINE_MODE.md) | Offline behavior and network controls | Current |
| [HERDR.md](HERDR.md) | Herdr multiplexer integration | Current |

**Historical (do not use as a baseline):**
| Document | Contents |
|---|---|
| [AUDIT-2026-10-14.md](AUDIT-2026-10-14.md) | Dated audit snapshot from 2026-10-14 |
| [EVOLUTION.md](EVOLUTION.md) | Planning artifact, not a status report |

## Conventions

- **L3** — integrated and regression-tested (test suite passes).
- **L2** — implemented, verified under defined conditions.
- **L1** — implemented, not adequately verified.
- **L0** — not started.

All documentation is regenerated from the source tree; example commands and
test counts reflect the current `master` branch.
