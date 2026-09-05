# packaging/

Distribution artifacts that live outside the Tauri bundler. See the phased plan
in [`docs/linux-distribution/`](../docs/linux-distribution/README.md).

```
packaging/
  linux/
    dev-prompt.desktop                          canonical .desktop entry
    io.github.seraphx2.devprompt.metainfo.xml    AppStream metainfo
  arch/
    PKGBUILD        AUR "dev-prompt"     — builds from a release tag
    PKGBUILD-bin    AUR "dev-prompt-bin" — repackages the release .deb
```

## `linux/dev-prompt.desktop`

One source of truth for the desktop entry. Used two ways:

- **Tauri deb/rpm bundles** reference it as a Handlebars `desktopTemplate` in
  `src-tauri/tauri.linux.conf.json`. It contains no `{{variables}}`, so the
  template engine passes it through verbatim; Tauri still names the installed
  file `dev-prompt.desktop`. The AppImage reuses the deb's desktop file.
- **Hand-install channels** (AUR, Flatpak, Snap) copy it directly.

Keep `Name` / `Exec` / `Icon` as literal `dev-prompt` — they never vary, and
literal values are what lets the same file double as the template.

## `linux/io.github.seraphx2.devprompt.metainfo.xml`

AppStream metainfo. Shipped in the deb/rpm/AppImage via
`bundle.linux.*.files`, and installed directly by the AUR/Flatpak/Snap
packaging. `<launchable>` points at `dev-prompt.desktop` (the name Tauri
installs), while `<id>` is the reverse-DNS app-id used everywhere else.

Outstanding before Phase 4 (Flathub):
- commit a real screenshot at `docs/img/overlay.png`
- have the release workflow prepend a `<release>` entry at tag time
- run `appstreamcli validate` in CI

## `arch/`

Both PKGBUILDs carry placeholder `pkgver=0.0.0` / `sha256sums=('SKIP')`; the
release workflow rewrites them per tag before pushing to the AUR. To test
locally, build the binary first (`npm run tauri build -- --no-bundle`) and adapt
`PKGBUILD` to install from the working tree, or just `makepkg` against a real
tag once one exists.

Runtime `depends` are pinned to the maintained tray fork
**`libayatana-appindicator`** — the ancient `libappindicator` shim (still in
Arch repos) does not reliably register a tray icon under KDE/Wayland.
