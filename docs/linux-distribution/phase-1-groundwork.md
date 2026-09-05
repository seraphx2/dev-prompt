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

### 1. `packaging/linux/io.github.seraphx2.devprompt.desktop`

One canonical desktop entry. Base it on what Tauri generates plus the ad-hoc
Arch one:

```ini
[Desktop Entry]
Type=Application
Name=dev-prompt
Comment=Command-palette overlay for launching dev repositories
Exec=dev-prompt
Icon=io.github.seraphx2.devprompt
Categories=Development;Utility;
Terminal=false
StartupWMClass=dev-prompt
Keywords=launcher;palette;repositories;projects;
```

Notes:
- `Icon=` uses the app-id so themed icons resolve; install icon files as
  `io.github.seraphx2.devprompt.png` in `hicolor/<size>/apps/`.
- Wire it into `tauri.conf.json` so the bundlers use this file instead of
  generating one: `bundle.linux.deb.desktopTemplate` /
  `bundle.linux.rpm.desktopTemplate` (and the AppImage picks up the deb one).
  Confirm the template placeholders Tauri expects (`{{exec}}`, `{{icon}}`, …)
  or ship it as a static file via `bundle.linux.*.files`.

### 2. `packaging/linux/io.github.seraphx2.devprompt.metainfo.xml`

AppStream metainfo. Minimum Flathub-acceptable set:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>io.github.seraphx2.devprompt</id>
  <metadata_license>MIT</metadata_license>
  <project_license>MIT</project_license>
  <name>dev-prompt</name>
  <summary>Command-palette overlay for launching dev repositories</summary>
  <description>
    <p>
      dev-prompt is a global-hotkey overlay that finds your local git
      repositories and launches them in the editor, terminal, or tool of your
      choice.
    </p>
  </description>
  <launchable type="desktop-id">io.github.seraphx2.devprompt.desktop</launchable>
  <url type="homepage">https://github.com/seraphx2/dev-prompt</url>
  <url type="bugtracker">https://github.com/seraphx2/dev-prompt/issues</url>
  <developer id="io.github.seraphx2"><name>seraphx2</name></developer>
  <content_rating type="oars-1.1"/>
  <screenshots>
    <screenshot type="default">
      <image>https://raw.githubusercontent.com/seraphx2/dev-prompt/main/docs/img/overlay.png</image>
      <caption>The overlay searching repositories</caption>
    </screenshot>
  </screenshots>
  <releases>
    <release version="0.0.0" date="1970-01-01"/>
  </releases>
</component>
```

- Needs at least one **screenshot** at a stable public URL — capture one, commit
  it under `docs/img/`.
- The `<releases>` block should be regenerated at release time (script in CI:
  prepend `<release version="$VERSION" date="$(date -I)"/>`). Flathub lints
  that the top release matches the built version.
- Validate with `appstreamcli validate` (or `flatpak run
  org.freedesktop.appstream-glib validate`) in CI.

### 3. `packaging/arch/PKGBUILD` + `packaging/arch/PKGBUILD-bin`

Two variants (both used in Phase 2):

**`PKGBUILD`** — build from a release tag:

```bash
pkgname=dev-prompt
pkgver=0.0.0          # pkgver() overrides from the tag; keep a placeholder
pkgrel=1
pkgdesc="Command-palette overlay for launching dev repositories"
arch=('x86_64')
url="https://github.com/seraphx2/dev-prompt"
license=('MIT')
depends=('webkit2gtk-4.1' 'gtk3' 'libsoup3' 'libayatana-appindicator')
makedepends=('rust' 'nodejs' 'npm' 'git')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')   # real sum injected by the release job

build() {
  cd "$srcdir/$pkgname-$pkgver"
  npm ci
  npm run tauri build -- --no-bundle \
    -c '{"bundle":{"createUpdaterArtifacts":false}}'
}

package() {
  cd "$srcdir/$pkgname-$pkgver"
  install -Dm755 src-tauri/target/release/dev-prompt \
    "$pkgdir/usr/bin/dev-prompt"
  install -Dm644 packaging/linux/io.github.seraphx2.devprompt.desktop \
    "$pkgdir/usr/share/applications/io.github.seraphx2.devprompt.desktop"
  install -Dm644 packaging/linux/io.github.seraphx2.devprompt.metainfo.xml \
    "$pkgdir/usr/share/metainfo/io.github.seraphx2.devprompt.metainfo.xml"
  for s in 32 128; do
    install -Dm644 "src-tauri/icons/${s}x${s}.png" \
      "$pkgdir/usr/share/icons/hicolor/${s}x${s}/apps/io.github.seraphx2.devprompt.png"
  done
  install -Dm644 "src-tauri/icons/128x128@2x.png" \
    "$pkgdir/usr/share/icons/hicolor/256x256/apps/io.github.seraphx2.devprompt.png"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
```

**`PKGBUILD-bin`** — repackage the release `.deb` (no compile for the user):
`source=(...download the _amd64.deb...)`, `package()` runs
`bsdtar -xf data.tar.gz -C "$pkgdir"` then fixes the `.desktop`/icon names to the
app-id. `depends` same as above minus `makedepends`.

Reference (the ad-hoc prebuilt version that was verified working on CachyOS):
`depends=('webkit2gtk-4.1' 'gtk3' 'libsoup3' 'libappindicator')`, installed
`/usr/bin/dev-prompt` + `.desktop` + hicolor 32/128/256 + `LICENSE`, built with
`makepkg` and installed via `pacman -U`. Switch the dep to
`libayatana-appindicator` for the committed version (canonical; provides the
same `libappindicator3.so.1`).

### 4. `docs/linux-distribution/` release checklist

Add a `CHECKLIST.md` (or a section in `README.md`) enumerating, per release,
which channels need a manual nudge vs. which the workflow handles. Keep it in
sync as phases land.

## Definition of done

- [ ] `packaging/linux/` holds the `.desktop` + `metainfo.xml` + a committed
      screenshot, and `appstreamcli validate` passes in CI.
- [ ] `tauri.conf.json` bundlers use the committed `.desktop` (verify a local
      `deb`/`rpm`/`appimage` build ships `io.github.seraphx2.devprompt.desktop`
      and the metainfo).
- [ ] `packaging/arch/PKGBUILD` + `PKGBUILD-bin` committed; `makepkg` builds
      each clean on a current Arch box.
- [ ] `docs/releasing.md` "Later" list points here.
