---
title: Self-update mechanism
tags: [update, signing, hmac, release, cli]
sourceRefs: [src/update.rs, src/update_signing.rs, src/app/mod.rs, scripts/sign-release.sh]
lastReviewed: 2026-11-10
---

# Self-update mechanism

Nanomite is a git-checkout install: the binary at `~/.local/bin/nanomite`
symlinks to `target/release/nanomite` inside the repo, and updating means
`git pull --ff-only` + `cargo build --release` in place (the running process
keeps the old inode; the new binary takes effect on next start).

## Three entry points

1. **CLI** `nanomite update` → `update::run()` (`src/update.rs`): fetch →
   behind-count / newest-tag check → **signature gate** → pull → rebuild.
2. **TUI** `/update [check]` → `App::handle_update` / `update_and_rebuild`
   (`src/app/mod.rs`): same git steps, **no signature gate** (known
   inconsistency — the CLI's signing check is trivially bypassed by using
   `/update` instead).
3. **Startup** → `update::check_status()` → `UpdateStatus` toast only
   (fetch + compare; never auto-pulls).

## Signature gate (v1.3.1+, `src/update_signing.rs`)

- Scheme: base64(HMAC-SHA256(`NANOMITE_UPDATE_KEY`, **tagged commit SHA**)).
- Client reads the sig file as `origin/<branch>:<tag>.sig` and verifies before
  pulling. No key configured → `Unsigned` → refused unless
  `NANOMITE_UPDATE_ALLOW_UNSIGNED=1` (per-invocation bypass).
- **Original bug (fixed 2026-11-10):** the sig was read from the tagged
  commit's own tree (`<tag>:<tag>.sig`). Since the payload IS that commit's
  SHA, a signature over it can never be contained in the same commit — the
  circularity made `Verified` unreachable and *every* `nanomite update`
  refused with "refusing unsigned update". No release ever shipped a `.sig`.
- Fix: sig file is read from the release-channel tip
  (`origin/<branch>:<tag>.sig`), with the old tag-tree path as fallback.
  Threat model intact: pushing to the repo still doesn't forge an HMAC.

## Maintainer release flow

```bash
git tag v1.3.2 && git push origin v1.3.2
scripts/sign-release.sh v1.3.2          # writes v1.3.2.sig (HMAC of the tag's commit SHA)
git add v1.3.2.sig && git commit && git push
```

- Key lives at `~/.config/nanomite/update-key` (0600), exported into shells
  via a `~/.bashrc` line. Never commit the key.
- The client and the script are pinned together by
  `update_signing::tests::matches_reference_python_hmac`, a cross-implementation
  test vector (Rust `sign_payload` vs Python `hmac` + `base64`).

## Gotchas

- `nanomite update` refuses to run from a dirty tree (the ff-only pull aborts);
  stash WIP first — e.g. the 2026-11-10 update carried the OpenCode/OpenRouter
  preset work across via `git stash push -u`.
- Tag comparison uses `parse_tag` (digits only): `v1.3.1-word-diff` parses as
  (1,3,1) — a suffixed tag at the same version does not count as "newer".
- The installed binary can be *older/newer* than the working tree (release
  builds persist in `target/release/` across branch checkouts). Verify which
  tree the binary was built from before reasoning about its behavior.

## Related

- [[entities/app]] — TUI side (`/update`, startup status toast)
- [[concepts/tool-policy]] — unrelated `ToolPolicy::Hacker` (do not confuse)
