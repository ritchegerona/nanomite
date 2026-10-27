# Fedora COPR packaging

COPR repo: https://copr.fedorainfracloud.org/coprs/ritchegerona/nanomite/

Setup (once):
1. Upload your RPM SSH key to COPR, then create a chroot project named
   `nanomite` for fedora-rawhide + fedora-40/41 (x86_64).
2. Add `nanomite.spec` as the package source (spec from this directory),
   pointing at the git tag `v0.4.0`.

Releasing a new version:
```
git tag vX.Y.Z && git push --tags
# then in COPR: rebuild nanomite (it picks up the new tag via the spec's Source0)
```

Local test build: `rpmbuild -ba pack/fedora/nanomite.spec` with the tarball
from `git archive v0.4.0 | gzip > ~/rpmbuild/SOURCES/nanomite-0.4.0.tar.gz`.
