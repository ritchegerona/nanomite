#!/usr/bin/env bash
# Sign a release tag so `nanomite update` accepts it.
#
# The client (src/update.rs) verifies base64(HMAC-SHA256(key, <tag commit sha>))
# read from `origin/<branch>:<tag>.sig` — the signature file travels on the
# release branch, NOT inside the tagged commit (the signed payload is the
# tagged commit's SHA, so it can never contain its own signature).
#
# Maintainer flow:
#   1. tag the release:            git tag v1.3.2 && git push origin v1.3.2
#   2. sign it (from repo root):   scripts/sign-release.sh v1.3.2
#   3. publish the signature:      git add v1.3.2.sig && git commit && git push
#
# The key comes from $NANOMITE_UPDATE_KEY or ~/.config/nanomite/update-key
# (0600). Never commit the key.

set -euo pipefail

tag="${1:?usage: sign-release.sh <tag>}"
keyfile="$HOME/.config/nanomite/update-key"

if [[ -z "${NANOMITE_UPDATE_KEY:-}" && ! -r "$keyfile" ]]; then
  echo "error: set NANOMITE_UPDATE_KEY or create $keyfile (0600)" >&2
  exit 1
fi
export NANOMITE_UPDATE_KEY

python3 - "$tag" <<'PY'
import base64, hashlib, hmac, os, pathlib, subprocess, sys

tag = sys.argv[1]
key = os.environ.get("NANOMITE_UPDATE_KEY", "")
if not key:
    key = pathlib.Path.home().joinpath(".config/nanomite/update-key").read_text().strip()

commit = subprocess.check_output(["git", "rev-list", "-n", "1", tag], text=True).strip()
sig = base64.b64encode(hmac.new(key.encode(), commit.encode(), hashlib.sha256).digest()).decode()
out = pathlib.Path(f"{tag}.sig")
out.write_text(sig + "\n")
print(f"wrote {out} (HMAC over commit {commit})")
PY

echo "next: git add $tag.sig && git commit -m 'chore(release): sign $tag' && git push"
echo "the machine running 'nanomite update' needs NANOMITE_UPDATE_KEY set (bashrc exports it from $keyfile)"
