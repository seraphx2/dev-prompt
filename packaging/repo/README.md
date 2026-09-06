# packaging/repo/

The self-hosted apt + rpm + pacman repository (Phase 3). Full design:
[`docs/linux-distribution/phase-3-apt-rpm-repo.md`](../../docs/linux-distribution/phase-3-apt-rpm-repo.md).

```
dev-prompt-repo.asc   repo signing public key (committed; published as /dev-prompt.asc)
build-repo.sh         rebuild + sign the three repo trees under a gh-pages checkout
render-index.sh       emit the landing page (index.html)
```

`.github/workflows/repo.yml` runs on `release: published`: builds a pacman
package from `../arch/PKGBUILD-bin` in an Arch container, then on a second job
downloads the release `.deb`/`.rpm`, imports the signing key, checks out
`gh-pages`, runs `build-repo.sh`, and force-pushes the result.

## Signing key

One GPG keypair, **repo metadata only** — separate from the minisign updater key.

- Public: `dev-prompt-repo.asc` here, key id `E3C07CD21A9A9BA5` (hardcoded in
  `repo.yml` and `build-repo.sh` — a key id isn't secret).
- Private: repo secret `REPO_GPG_PRIVATE_KEY` (ASCII-armored). Not in the repo.
- Generated with `gpg --batch --gen-key` (RSA-4096, no passphrase, UID
  `dev-prompt repository <seraphx2@live.com>`, no expiry). To rotate: generate a
  new pair, replace `dev-prompt-repo.asc` + the key id in `repo.yml` /
  `build-repo.sh`, update the secret, re-publish — existing installs must
  re-import the key.

## Local test

```sh
mkdir /tmp/art && cd /tmp/art
gh release download vX.Y.Z -p '*_amd64.deb' -p '*.x86_64.rpm' -p '*.pkg.tar.zst'
# (or build the pacman pkg from packaging/arch/PKGBUILD-bin)
cd /path/to/repo
GNUPGHOME=... packaging/repo/build-repo.sh /tmp/art /tmp/site E3C07CD21A9A9BA5
```

Needs `apt-ftparchive` (apt-utils), `createrepo_c`, `gpg`; `repo-add` if present,
else `docker`.
