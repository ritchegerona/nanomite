# Releases and updates

Status: **L0 — no infrastructure.** This file specifies the required workflow
so any future implementation can be checked against it. Nothing below is
implemented; the README's "From a release" section points at a releases page
that does not exist.

## Required update workflow (Milestone 8)

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

## Until then

- Releases are manual: `cargo build --release`, copy the binary, keep the
  previous binary as the rollback artifact.
- Last known-good baseline: git `d59f2ef`, 55/55 tests green (without
  `src/memory/` wired).
- Versioning: `Cargo.toml` `0.1.0`; no tagging/branching convention adopted
  yet — adopt semver + `v*` tags when the first real release is cut, and
  generate notes from CHANGELOG entries, Git history, and test results, never
  from model invention.
