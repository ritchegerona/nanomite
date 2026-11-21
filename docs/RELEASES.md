# Releases and updates

Status: **L2** — tagged manual releases. Infrastructure for authenticated
updates is specified here so any future implementation can be checked against
it.

## Current release

- **Version:** `2.1.0`
- **Git branch:** `master`
- **Build:** `cargo build --release`
- **Tests:** `cargo test --lib` passes; `cargo clippy --all-targets -- -D warnings`
  clean; `cargo fmt --check` clean; `cargo bench` completes
- **Install:** copy binary to `$PATH`; keep the previous binary as rollback

## Required update workflow

1. Check configured release source → compare versions
2. Show notes + compatibility
3. Verify authenticity (**signatures or equivalent** — HTTPS alone proves
   nothing)
4. Validate checksums
5. Check platform, disk, dependencies
6. Stage download → validate artifact
7. Preserve config + memory
8. Require configured approval → apply recoverably
9. Verify version + post-update health → record result
10. Support rollback where the install method permits

Prohibited: executing fetched scripts, auto-updating to untrusted releases,
skipping signature validation, overwriting user data, weakening security
controls. Channels (stable/preview) only when the release infrastructure
supports them.

## Until infrastructure exists

- Releases are manual: `cargo build --release`, copy the binary, keep the
  previous binary as the rollback artifact.
- Versioning: semver + `v*` tags; generate notes from CHANGELOG entries, Git
  history, and test results — never from model invention.

## Version history

| Version | Date | Highlights |
|---|---|---|
| **2.1.0** | 2026-11-20 | Perf: HTTP client reuse, faster file edits, UI render pre-allocation, disk cleanup; UI left panel shows thinking only, right panel shows response; design system and clippy fixes |
| 2.0.0 | 2026-11-20 | opencode-style Provider, Connect, Model and Settings menus; full filesystem access; design system; version display |
| 1.3.7 | 2026-11-19 | Evidence locker, auto-reporting, neural AI agent, auto-install missing tools |
| 1.3.6 | 2026-11-18 | Refusals removed from all AI models; performance improvements; auto-install missing tools; skills tracking |
| 1.3.5 | 2026-11-11 | Herdr integration; OCR via tesseract; catalog 35→36 tools |
| 1.3.4 | 2026-11-09 | OSINT tooling wrappers; run-anywhere tool shims |
| 1.3.0 | 2026-11-08 | `/settings` command, `/perf` command, runtime telemetry, CI VPN/Tor job |
| 1.2.0 | 2026-11-07 | Hacker-tile UI redesign: three-tile layout, thinking panel, rail transcript, three new themes, daily learning loop |
