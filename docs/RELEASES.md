# Releases and updates

Status: **L1 — manual releases, v0.3.0 (2026-10-26).** Infrastructure for
authenticated updates is not implemented; this file specifies the required
workflow so any future implementation can be checked against it.

## Current release

- **Version:** `0.3.0` (see `Cargo.toml`)
- **Git tag:** `v0.3.0` (not yet pushed)
- **Build:** `cargo build --release` (static binary, ~8 MB)
- **Tests:** 409 lib + 8 integration = 417 passed
- **Benchmarks:** stable, no regressions
- **Install:** copy binary to `$PATH`; previous binary kept as rollback

## Required update workflow (Milestone 14)

Check configured release source → compare versions → show notes +
compatibility → verify authenticity (**signatures or equivalent; HTTPS alone
is not authentication**) → validate checksums → check platform, disk, deps →
stage download → validate artifact → preserve config + memory → require
configured approval → apply recoverably → verify version + post-update health
→ record result → support rollback where the install method permits.

Prohibited: executing fetched scripts, auto-updating to untrusted releases,
skipping signature validation, overwriting user data, weakening security
controls. Channels (stable/preview) only when the release infrastructure
supports them.

## Until infrastructure exists

- Releases are manual: `cargo build --release`, copy the binary, keep the
  previous binary as the rollback artifact.
- Last known-good baseline: git `54d55fe` (v0.3.0), 409/409 tests green.
- Versioning: semver + `v*` tags; generate notes from CHANGELOG entries,
  Git history, and test results — never from model invention.

## Version history

| Version | Date | Highlights |
|---|---|---|
| 0.3.0 | 2026-10-26 | Stages A-E: Authorization engine, Git tools (5), Test runner, Symbol index (3), Policy engine (Auto/Plan/Readonly), Plan system (5). 33 tools total. |
| 0.2.0 | 2026-10-15 | Memory (L3), extractors, board, self-improve, tool library, 18 tools. |
| 0.1.x | — | Early development, not released. |