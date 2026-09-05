# Phase 2 — AUR (Arch User Repository)

**Lift:** S (hours once Phase 1 is done). **Reach:** Arch, CachyOS, Manjaro,
EndeavourOS, Garuda — i.e. the maintainer's own distro.
**Needs from maintainer:** an AUR account and an SSH key registered on it.

## Why first (after groundwork)

Cheapest real "in a package manager" win. The maintainer runs Arch/CachyOS, the
`PKGBUILD`s already exist from Phase 1, and updates flow through
`paru`/`yay` → `pacman -Syu` with no extra infrastructure to host or sign
(AUR stores only the `PKGBUILD`; users build locally).

## Packages to publish

| AUR name | Source | Audience |
|---|---|---|
| `dev-prompt-bin` | repackages the GitHub release `.deb` | most users — no toolchain, no compile |
| `dev-prompt` | builds from the release tag | users who want to build from source |
| `dev-prompt-git` | builds `dev` HEAD | testers / early adopters |

Start with `dev-prompt-bin` + `dev-prompt-git`. Add `dev-prompt` if there's
demand. All three are thin wrappers over `packaging/arch/`.

## One-time setup (maintainer)

1. Create an account at <https://aur.archlinux.org>.
2. `ssh-keygen -t ed25519 -f ~/.ssh/aur -C aur` and paste `~/.ssh/aur.pub` into
   AUR → My Account → SSH Public Key.
3. Add to `~/.ssh/config`:
   ```
   Host aur.archlinux.org
     IdentityFile ~/.ssh/aur
     User aur
   ```
4. For each package, first push is manual:
   ```sh
   git clone ssh://aur@aur.archlinux.org/dev-prompt-bin.git
   cd dev-prompt-bin
   cp ../dev-prompt/packaging/arch/PKGBUILD-bin ./PKGBUILD   # edit pkgver + sha256sums
   makepkg --printsrcinfo > .SRCINFO
   git add PKGBUILD .SRCINFO && git commit -m "Initial import" && git push
   ```
5. Add the private key as repo secret `AUR_SSH_KEY` for the automation below.

## Automation (release workflow)

New job in `.github/workflows/release.yml`, runs after the release is published:

```yaml
  publish-aur:
    needs: release
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - name: Compute version
        id: v
        run: echo "value=${GITHUB_REF_NAME#v}" >> "$GITHUB_OUTPUT"
      - name: Update AUR dev-prompt-bin
        uses: KSXGitHub/github-actions-deploy-aur@v3   # or hand-rolled ssh + git
        with:
          pkgname: dev-prompt-bin
          pkgbuild: ./packaging/arch/PKGBUILD-bin
          commit_username: seraphx2
          commit_email: seraphx2@live.com
          ssh_private_key: ${{ secrets.AUR_SSH_KEY }}
          # regenerate pkgver + sha256sums of the new .deb before pushing
```

The step must, before pushing:
- set `pkgver` to `${{ steps.v.outputs.value }}` (CalVer, dots are valid in
  `pkgver`; if a `-` ever appears, translate to `_`),
- download the release `dev-prompt_<ver>_amd64.deb` and put its real sha256 in
  `sha256sums`,
- run `makepkg --printsrcinfo > .SRCINFO`.

`dev-prompt-git` needs no per-release update (users rebuild to get HEAD); still
worth a monthly CI `--printsrcinfo` refresh if `depends` drift.

## App-side

`dev-prompt-bin`/`dev-prompt` install to `/usr/bin` (root-owned) → the in-app
updater has no valid path (no `__TAURI_BUNDLE_TYPE` marker → falls to the
AppImage path → fails). For now: document "update with `pacman -Syu`". If the
Settings update banner showing for pacman users is annoying, do the
Phase-README option 2 (detect system install, hide the update UI) — small Rust
command returning `tauri::utils::platform::bundle_type()` plus a frontend guard.

## Definition of done

- [ ] `dev-prompt-bin` and `dev-prompt-git` live on AUR, install cleanly with
      `paru -S`, launch, tray + hotkey work.
- [ ] Release workflow pushes a `pkgver` bump to `dev-prompt-bin` on every
      release; `.SRCINFO` regenerated; namcap clean.
- [ ] README install section lists the AUR names.
- [ ] `docs/linux-distribution/README.md` status table updated.
