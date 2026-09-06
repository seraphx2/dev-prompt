# Phase 3 — self-hosted apt + rpm + pacman repository

**Lift:** M (a day or two). **Reach:** Debian/Ubuntu/Mint (`apt`),
Fedora/RHEL/openSUSE (`dnf`/`zypper`), **and Arch/CachyOS/Manjaro (`pacman`)** —
the last one covers what Phase 2 (AUR) would have, which matters while AUR
registration is closed.
**Needs from maintainer:** a GPG signing key (generated in this phase) and
GitHub Pages enabled on the repo.

## Why self-hosted (not OBS)

The release already builds `.deb` and `.rpm`; a native `pacman` package is one
`makepkg` away from `packaging/arch/PKGBUILD-bin`. So there's nothing to
*rebuild* — this phase only adds repo **metadata** on top of artifacts that
already exist, then publishes to GitHub Pages. No third-party account, no
build-farm to appease, stable `seraphx2.github.io` URLs, one signing key for all
three formats. OBS's value is multi-distro-version *building*; we don't need
that yet. (It stays available as a later add — see the appendix.)

The tradeoff accepted: one `.deb` (built on `ubuntu-22.04` → Debian 12 /
Ubuntu 22.04+ / Mint 21+) and one `.rpm` (recent Fedora). Older releases break;
revisit with OBS if that ever matters.

## Layout (published to the `gh-pages` branch → `https://seraphx2.github.io/dev-prompt/`)

```
/                     index.html — the install snippets below
/dev-prompt.asc       repo signing public key (ASCII-armored)
/deb/                 apt repo:  dists/stable/... + pool/main/d/dev-prompt/*.deb
/rpm/                 rpm repo:  repodata/ + *.rpm
/arch/                pacman repo: dev-prompt.db.tar.gz + *.pkg.tar.zst (+ .sig)
```

## The signing key

One GPG keypair, used only for repo metadata — **not** the minisign updater key.

- Generated in this phase (RSA 4096, no passphrase, UID
  `dev-prompt repository <seraphx2@live.com>`, key id `E3C07CD21A9A9BA5`).
- **Public** half committed at `packaging/repo/dev-prompt-repo.asc` and published
  as `/dev-prompt.asc`. The key id is not secret — it's hardcoded in
  `repo.yml` / `build-repo.sh`.
- **Private** half → the one repo secret `REPO_GPG_PRIVATE_KEY` (ASCII-armored).
  The generate step wrote it to `~/dev-prompt-repo-signing-key.asc`; add it with
  `gh secret set REPO_GPG_PRIVATE_KEY < ~/dev-prompt-repo-signing-key.asc`, then
  delete the file (it's also saved in Bitwarden).

## The publish workflow — `.github/workflows/repo.yml`

Trigger: `on: release: types: [published]` — fires when a release goes public,
skips drafts by construction, and re-runs cleanly on re-publish.

Jobs:

1. **`pacman-pkg`** (`container: archlinux:latest`) — install `base-devel`, make
   a non-root build user, `pkgver`/`sha256sums`-stamp `packaging/arch/PKGBUILD-bin`
   against the release `.deb`, `makepkg`, upload the `*.pkg.tar.zst` as a
   workflow artifact.
2. **`publish`** (`ubuntu-latest`, `needs: pacman-pkg`):
   - checkout the repo; checkout `gh-pages` into `./site` (create the orphan
     branch if missing).
   - download the release's `.deb` + `.rpm`; download the `pacman-pkg` artifact.
   - import `REPO_GPG_PRIVATE_KEY`.
   - **apt:** copy `.deb` into `site/deb/pool/main/d/dev-prompt/`, prune to the
     last 10 versions, `apt-ftparchive packages` → `Packages`(+`.gz`),
     `apt-ftparchive release` → `Release`, `gpg --clearsign` → `InRelease` and
     `gpg -abs` → `Release.gpg`.
   - **rpm:** copy `.rpm` into `site/rpm/`, prune to 10, `createrepo_c
     --update site/rpm`, `gpg --detach-sign --armor site/rpm/repodata/repomd.xml`.
   - **pacman:** copy `*.pkg.tar.zst` into `site/arch/`, prune to 10, `repo-add`
     the `.db` (via `docker run archlinux` since the runner has no `repo-add`),
     materialise the `$repo.db`/`$repo.files` symlinks into real files (Pages
     doesn't serve symlinks), then `gpg --detach-sign` the db + each package.
   - write `site/dev-prompt.asc`, `site/index.html`, `site/.nojekyll`.
   - commit + force-push `site` to `gh-pages` (single-snapshot branch, history
     not meaningful).

**Built:** `packaging/repo/build-repo.sh` (ingest → prune → apt + rpm + pacman
metadata → sign → index), `packaging/repo/render-index.sh` (the landing page),
`.github/workflows/repo.yml`. `build-repo.sh` is runnable locally against a dir
of downloaded `v*` artifacts (apt-ftparchive / createrepo_c must be installed;
`repo-add` falls back to `docker run archlinux`).

## Retention

Keep the **last 10** versions in each pool so pinning / downgrade works; drop
older. CalVer sorts lexically for same-width fields — sort with `sort -V` to be
safe.

## Install snippets (also rendered into `index.html`)

**Debian / Ubuntu**
```sh
curl -fsSL https://seraphx2.github.io/dev-prompt/dev-prompt.asc \
  | sudo gpg --dearmor -o /usr/share/keyrings/dev-prompt.gpg
echo "deb [signed-by=/usr/share/keyrings/dev-prompt.gpg] https://seraphx2.github.io/dev-prompt/deb stable main" \
  | sudo tee /etc/apt/sources.list.d/dev-prompt.list
sudo apt update && sudo apt install dev-prompt
```

**Fedora / RHEL**
```sh
sudo tee /etc/yum.repos.d/dev-prompt.repo <<'EOF'
[dev-prompt]
name=dev-prompt
baseurl=https://seraphx2.github.io/dev-prompt/rpm
enabled=1
gpgcheck=1
gpgkey=https://seraphx2.github.io/dev-prompt/dev-prompt.asc
EOF
sudo dnf install dev-prompt
```

**Arch / CachyOS** — add to `/etc/pacman.conf`:
```ini
[dev-prompt]
SigLevel = Required
Server = https://seraphx2.github.io/dev-prompt/arch
```
then `sudo pacman-key --add <(curl -fsSL https://seraphx2.github.io/dev-prompt/dev-prompt.asc)`,
`sudo pacman-key --lsign-key <KEYID>`, `sudo pacman -Sy dev-prompt`.

## Maintainer one-time setup

1. Run the key-generate step (this phase) and add the two secrets.
2. **Settings → Pages → Source: `gh-pages` branch, `/` root.** (Can't be done
   via API without a token scope we don't grant CI; do it by hand once.)
3. Re-publish the latest release (or cut a new one) to trigger `repo.yml`.

## App-side

A package installed from this repo carries the `__TAURI_BUNDLE_TYPE` marker, so
the in-app updater *could* self-update it — but it shouldn't; `apt`/`dnf`/`pacman`
own that. Apply option 1 or 2 from the roadmap README's updater section
(document it, or detect a system install and hide the update UI).

## Definition of done

- [x] repo GPG key generated; pubkey at `packaging/repo/dev-prompt-repo.asc`
      (key id `E3C07CD21A9A9BA5`).
- [x] `packaging/repo/` holds the pubkey + `build-repo.sh` + `render-index.sh`;
      `.github/workflows/repo.yml` added. `build-repo.sh` dry-run passes locally
      (pacman path + signing + prune verified; apt/rpm sections need their tools).
- [x] secret `REPO_GPG_PRIVATE_KEY` set; Pages enabled (`gh-pages`, `/`).
- [x] `repo.yml` publishes `deb/` + `rpm/` + `arch/` with signed metadata,
      auto-dispatched from `release.yml` — validated end-to-end on `v2026.906.2`
      (all endpoints 200, signatures verify).
- [x] Arch: installed on the maintainer's CachyOS box from the live repo.
- [ ] Debian + Fedora containers: `apt`/`dnf install` then `upgrade` between two
      published versions. One-time sanity check — the metadata is standard
      `apt-ftparchive` / `createrepo_c` output and the sigs verify, so low risk;
      not worth a standing CI test.
- [x] `README.md` (repo root) — "Install" section with the three blocks.
- [x] `docs/linux-distribution/README.md` status + key table updated.

## Appendix — OBS as a later add

If older-distro coverage or many rpm targets become worth it, add an
**openSUSE Build Service** project (`home:seraphx2:dev-prompt`) that *builds*
`.deb`/`.rpm` per target from a source tarball (`cargo vendor` committed) and
hosts them. It'd sit alongside this repo, not replace it. OBS signs metadata
with its own key, so no extra key management. The cost is making the Tauri
(Rust + npm) build work inside OBS's environment.
