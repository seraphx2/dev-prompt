# Phase 3 — your own apt + rpm repository

**Lift:** M (a day or two). **Reach:** Debian/Ubuntu/Mint (apt) and
Fedora/RHEL/openSUSE (dnf/zypper).
**Needs from maintainer:** a decision (OBS vs self-hosted) and, if self-hosted,
a dedicated GPG repo-signing key.

## Why

The release already produces `.deb` and `.rpm`. Attaching them to a GitHub
release means users manually download every update. Putting them in a *repo*
means the user adds one source once, then `apt upgrade` / `dnf upgrade` picks up
new versions like any other package.

## Option A — openSUSE Build Service (recommended)

OBS builds **and hosts** deb + rpm for many distro/version targets for free.

- Project: `home:seraphx2:dev-prompt` on <https://build.opensuse.org>.
- Feed it either a source tarball + spec/dsc, or a `_service` file that pulls
  the git tag (`obs_scm` + `tar` + `recompress` services). Rust/npm build inside
  OBS needs the deps vendored or an `BuildRequires` on the distro's toolchain —
  the tarball approach with `cargo vendor` committed is most reliable.
- OBS auto-rebuilds on new tags if the `_service` tracks
  `<param name="revision">` = latest tag.
- Users add, e.g.:
  ```
  # Fedora
  dnf config-manager --add-repo https://download.opensuse.org/repositories/home:/seraphx2:/dev-prompt/Fedora_40/home:seraphx2:dev-prompt.repo
  ```
  OBS generates these `.repo` / `.list` snippets and signs the metadata with its
  own key (published on the project page) — **no GPG key for you to manage**.
- Downside: build config per target distro; OBS's Rust/Node story is fiddlier
  than a normal CI.

## Option B — self-hosted on GitHub Pages

Everything under `github.com/seraphx2`. More moving parts.

- Release workflow, after building `.deb`/`.rpm`:
  - **apt:** `aptly repo add` / `aptly publish` (or `reprepro`) into a `pool/` +
    `dists/` tree; sign `Release` with a GPG key.
  - **rpm:** drop the `.rpm` in a dir, `createrepo_c .`, `gpg --detach-sign`
    `repomd.xml`.
  - Push the trees to a `gh-pages` branch (or a separate `dev-prompt-repo`
    repo). Served at `https://seraphx2.github.io/dev-prompt/{apt,rpm}/`.
- **New key required:** a GPG keypair used *only* for repo metadata. Public key
  committed + published; private key = repo secret `REPO_GPG_KEY`. This is
  **not** the minisign updater key and not the AUR SSH key.
- Keep N previous versions in the pool so downgrades/pinning work.
- Users:
  ```
  curl -fsSL https://seraphx2.github.io/dev-prompt/apt/pubkey.gpg | sudo gpg --dearmor -o /usr/share/keyrings/dev-prompt.gpg
  echo "deb [signed-by=/usr/share/keyrings/dev-prompt.gpg] https://seraphx2.github.io/dev-prompt/apt stable main" | sudo tee /etc/apt/sources.list.d/dev-prompt.list
  ```

## Recommendation

Start with **OBS** — no key management, it hosts for you, and it covers more rpm
distros than a hand-rolled repo would. Revisit self-hosting only if OBS's build
environment can't accommodate the Tauri build or you want the URL under your own
domain.

## App-side

deb/rpm installed from a repo carry the `__TAURI_BUNDLE_TYPE` marker (they're
built by `tauri-action`/tauri-bundler), so `tauri-plugin-updater` *could*
self-update them via pkexec — but don't feed that path. Let `apt`/`dnf` do it.
Apply Phase-README option 1 or 2 for the update UI.

## Definition of done

- [ ] A repo exists (OBS project or Pages tree) carrying the current release's
      `.deb` + `.rpm` with signed metadata.
- [ ] Fresh Debian/Ubuntu and Fedora VMs can add the repo, `install`, then
      `upgrade` to a newer release.
- [ ] README has copy-paste "add the repo" blocks.
- [ ] Release workflow refreshes the repo on every release.
- [ ] `docs/linux-distribution/README.md` status + key table updated.
