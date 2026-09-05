# Phase 1 — in-repo packaging groundwork

**Lift:** S (hours). **Needs from maintainer:** nothing — all committable now.
**Unblocks:** every later phase.

## Why

Right now packaging metadata is scattered and implicit: the `.desktop` file is
either hand-written (the ad-hoc Arch package) or generated on the fly by
`tauri-action` for deb/rpm/appimage, there is no AppStream metainfo (required by
Flathub, and what makes the app render in GNOME Software / KDE Discover), and the
one working `PKGBUILD` lived in a scratch dir and was lost. Consolidate it into
`packaging/` so every channel pulls from one source of truth.

## Deliverables

### 1. `packaging/linux/dev-prompt.desktop`

**Built:** `packaging/linux/dev-prompt.desktop`. Literal values (no Handlebars
vars) so the one file doubles as the deb/rpm `desktopTemplate` *and* the
verbatim file that hand-install channels copy. `Categories=Development;` only
(a second main category makes menus list the app twice). See
`packaging/README.md`.

### 2. `packaging/linux/io.github.seraphx2.devprompt.metainfo.xml`

**Built.** `<id>` is the reverse-DNS app-id; `<launchable>` is
`dev-prompt.desktop` (the basename Tauri's bundler installs — it can't be
renamed without renaming the binary). Passes `appstreamcli validate`.

Outstanding:
- one **screenshot** at `docs/img/overlay.png` — the metainfo already references
  it; it 404s until committed (see `docs/img/README.md`). Flathub build fails
  without it; `appstreamcli validate` doesn't care.
- `<releases>`: the release workflow should prepend
  `<release version="$VERSION" date="$(date -I)"/>` at tag time. Flathub lints
  that the top release matches the built version.
- wire `appstreamcli validate` + `desktop-file-validate` into CI.

### 3. `packaging/arch/PKGBUILD` + `packaging/arch/PKGBUILD-bin`

**Built** (both used in Phase 2). See the files — summary:

- **`PKGBUILD`** (AUR `dev-prompt`): `source` = release tag archive; `build()`
  runs `npm ci` + `npm run tauri build -- --no-bundle -c
  '{"bundle":{"createUpdaterArtifacts":false}}'`; `package()` installs the
  binary + `packaging/linux/dev-prompt.desktop` + the metainfo + hicolor icons
  (32/128/256, named `dev-prompt.png`) + `LICENSE`.
- **`PKGBUILD-bin`** (AUR `dev-prompt-bin`): `source` = the release `.deb`;
  `package()` is just `bsdtar -xf …deb` then `bsdtar -xf data.tar.* -C
  $pkgdir`. The `.deb` already carries the desktop file, icons, and metainfo
  (via `bundle.linux.deb.files`), so nothing to relocate.
- Both: `depends=('webkit2gtk-4.1' 'gtk3' 'libsoup3' 'libayatana-appindicator'
  'hicolor-icon-theme')`, placeholder `pkgver=0.0.0` / `sha256sums=('SKIP')`
  that the Phase 2 workflow rewrites per tag.

The ad-hoc prebuilt PKGBUILD verified working on CachyOS earlier used
`libappindicator` (what was installed) — the committed version pins
`libayatana-appindicator` (canonical; the old shim doesn't reliably show a tray
on KDE/Wayland).

### 4. release checklist

Deferred to Phase 2 — the first channel with per-release automation. Track it in
`docs/linux-distribution/README.md` for now.

## Definition of done

- [x] `packaging/linux/` holds `dev-prompt.desktop` + the metainfo.
      `appstreamcli validate` and `desktop-file-validate` pass locally.
- [ ] a real screenshot committed at `docs/img/overlay.png` (metainfo points at
      it; currently 404s — see `docs/img/README.md`).
- [ ] `appstreamcli validate` + `desktop-file-validate` wired into CI.
- [x] `tauri.linux.conf.json` bundlers ship the canonical `.desktop` (verbatim
      via `desktopTemplate`) and the metainfo (via `linux.*.files`). Verified a
      local `deb` + `rpm` + `appimage` build: all three carry
      `/usr/share/applications/dev-prompt.desktop` and
      `/usr/share/metainfo/io.github.seraphx2.devprompt.metainfo.xml`.
- [x] `packaging/arch/PKGBUILD` + `PKGBUILD-bin` committed. **Not yet
      `makepkg`-tested** — `PKGBUILD` sources a release tag archive and none
      exists yet; `PKGBUILD-bin` sources a release `.deb` asset. Both get a real
      `pkgver`/`sha256sums` from the Phase 2 workflow. Test on the first tagged
      release.
- [x] `docs/releasing.md` "Later" list points here.

**Note on naming:** kept the installed `.desktop` / icon basename as
`dev-prompt` (what Tauri's bundler emits — it can't be renamed without also
renaming the binary). The metainfo `<id>` is the reverse-DNS app-id;
`<launchable>` points at `dev-prompt.desktop`. Flatpak (Phase 4) builds its own
file tree and can use app-id naming throughout there.
