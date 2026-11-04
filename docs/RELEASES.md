# Releases and updates

Status: **L2 — tagged manual releases, v1.0.0 (2026-11-04).** Infrastructure
for authenticated updates is not implemented; this file specifies the required
workflow so any future implementation can be checked against it.

## Current release

- **Version:** `1.1.0`
- **Git tag:** `v1.1.0`
- **Branch:** `master`
- **Build:** `cargo build --release`
- **Tests:** 707 Rust + 76 Python = 783 total, all passing; clippy clean
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
- Last known-good baseline: v0.8.0, 698 lib + 8 integration tests green,
  clippy clean.
- Versioning: semver + `v*` tags; generate notes from CHANGELOG entries,
  Git history, and test results — never from model invention.

## Version history

| Version | Date | Highlights |
|---|---|---|
| 1.1.0 | 2026-11-04 | **Config fail-fast validation**: `HackerConfig::validate()` wired into `Config::build()` — enabled hacker mode without valid scope loudly fails and self-disables. `types.rs` extracted from `app/mod.rs` (pure UI data types, no `App` dependency). All HACKER MODE capabilities preserved (typestate gate, `OpCapability`, scope validation, audit trail, findings store, SARIF export). Repo cleaned: 120 legacy scratch files removed. |
| 1.0.0 | 2026-11-04 | **Advanced capability layer**: typestate `HackerSessionTyped` (`Discovered→Authorized→Active→Closed`), `OpCapability` unforgeable token with `CapEvent` audit trail, `CapError` fail-fast domain errors, scope + risk-ceiling checks. Python cracking toolkit under `src/nanomite/cracking/` with context-managed sessions, streaming wordlist generator, CTI-grade reports. Installer scripts fixed (`install.sh`, `install.ps1`) with Python deps. Full documentation overhaul: `ARCHITECTURE.md`, `HACKER_MODE.md`, `HACKER_SECURITY.md`, `INSTALLATION.md`. |
| 0.8.0 | 2026-11-02 | HACKER MODE Phase 4: assessment lifecycle (`list|use|complete`), reporting operations (`summary|export`), evidence verification (`verify <id>`), finding triage (`triage|accept|close`), JSON+SARIF export, more finding producers. Double-Esc interrupt. Tool confirmation prompts Plan-only. Footer status strip moved to bottom edge. |
| 0.7.0 | 2026-10-28 | opencode-style chat UI, transcript selection, performance pass, tool display levels, compact density, ETA estimates, `nanomite update`, browser automation, `/team`, time machine, transcript search, focus mode, `/doctor`, `/hud`, alt+letter chords, crash forensics. |
| 0.6.0 | 2026-10-27 | Follow-up suggestions, prompt queue drain, per-mode colours, four tool modes (plan/coder/build/hacker), Shift+Tab cycle fix. |
| 0.5.0 | 2026-10-27 | Plan mode enforcement, session persistence v2, hooks/event system, external tool SDK, parallel worktrees, CI+packaging, documentation overhaul. |
| 0.4.0 | 2026-10-27 | Central tool authorization engine, git tool suite, test runner, symbol index, plan/task system, sudo password modal. |
| 0.3.0 | 2026-10-26 | Prompt queue, OpenCode-style chat UI, welcome screen, thinking indicator, modular app architecture, integration tests, CI pipeline, security updates. |
