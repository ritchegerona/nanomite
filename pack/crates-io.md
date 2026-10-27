# crates.io publishing

`nanomite` ships as a binary crate; publishing needs the package metadata
to be complete. Checklist:

1. `Cargo.toml` has name/version/edition/license/repository/description.
   Add if missing:
   ```toml
   description = "A microscopic terminal coding agent"
   license = "MIT"
   repository = "https://github.com/ritchegerona/nanomite"
   ```
2. `cargo publish --dry-run` (checks the packaged file list; target/ and
   .nanomite/ must not leak - verify via `cargo package --list`).
3. `cargo login $CRATES_TOKEN` (token from https://crates.io/settings/tokens;
   never commit the token).
4. `cargo publish` right after tagging vX.Y.Z so the crate matches the tag.
