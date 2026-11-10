#!/bin/sh
# Sign a release tag for authenticated `nanomite update`.
#
# Usage:
#   NANOMITE_UPDATE_KEY=<secret> tools/release/sign_tag.sh <tag>
#
# The signed payload is the commit SHA the (annotated) tag points at:
#   payload=$(git rev-list -n 1 <tag>)
#   sig=$(printf '%s' "$payload" | openssl dgst -sha256 -hmac key -binary | base64)
# The base64 sig is committed to the `signatures` branch as `<tag>.sig`.
# It cannot live inside the tag's own tree: the file would change the very
# commit hash it signs. `nanomite update` verifies against
# refs/remotes/origin/signatures/<tag>.sig.
#
# The key must be identical on the signer and every verifying device:
#   export NANOMITE_UPDATE_KEY=$(cat ~/.config/nanomite/update-key)
set -eu

TAG=${1:?usage: sign_tag.sh <tag>}
KEY=${NANOMITE_UPDATE_KEY:-$(cat "${NANOMITE_UPDATE_KEY_FILE:-$HOME/.config/nanomite/update-key}")}
[ -n "$KEY" ] || { echo "error: no signing key (NANOMITE_UPDATE_KEY)" >&2; exit 1; }

cd "$(git rev-parse --show-toplevel)"

PAYLOAD=$(git rev-list -n 1 "$TAG")
SIG=$(printf '%s' "$PAYLOAD" | openssl dgst -sha256 -hmac "$KEY" -binary | base64 | tr -d '[:space:]')

WORK=$(mktemp -d "${TMPDIR:-/tmp}/nanomite-sign.XXXXXX")
git worktree add --detach "$WORK" >/dev/null 2>&1
trap 'git worktree remove --force "$WORK" 2>/dev/null; rm -rf "$WORK"' EXIT

cd "$WORK"
printf '%s\n' "$SIG" > "$TAG.sig"
git add "$TAG.sig"
# The sig commit contains only release signatures; do not stack code on it.
git -c user.name="${GIT_AUTHOR_NAME:-nanomite-release}" \
    -c user.email="${GIT_AUTHOR_EMAIL:-release@nanomite.local}" \
    commit -q -m "release signature for $TAG"

# Update refs/heads/signatures (create or fast-forward onto the new commit).
NEW=$(git rev-parse HEAD)
OLD=$(git rev-parse --verify refs/heads/signatures 2>/dev/null || true)
if [ -n "$OLD" ]; then
    git update-ref refs/heads/signatures "$NEW" "$OLD"
else
    git update-ref refs/heads/signatures "$NEW"
fi
git worktree remove --force "$WORK" 2>/dev/null || true
trap - EXIT
rm -rf "$WORK"

cd - >/dev/null
echo "signed $TAG (payload $PAYLOAD) -> signatures/$TAG.sig"
echo "push with: git push origin signatures $TAG"
