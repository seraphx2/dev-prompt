# packaging/flatpak/

Flatpak manifest for dev-prompt (Phase 4). Design and rationale:
[`docs/linux-distribution/phase-4-flatpak.md`](../../docs/linux-distribution/phase-4-flatpak.md).

```
io.github.seraphx2.devprompt.yaml   the manifest
```

## Status

**Builds and installs locally** (`flatpak-builder`, GNOME 48). Verified:

- full build is clean — `npm ci`, `svelte-check` (0 errors), `vite build`,
  `cargo build --release`, the `libayatana-appindicator` tray module, and every
  install step.
- `flatpak run` starts the app and it **stays resident** (needs the
  single-instance plugin skipped under Flatpak — see `src-tauri/src/lib.rs`;
  `flatpak run` reserves the app-id on the session bus, so the plugin's own
  `RequestName` would see it taken and `exit(0)`).
- the **tray icon registers** — it shows up in
  `org.kde.StatusNotifierWatcher`'s `RegisteredStatusNotifierItems`.
- `flatpak-spawn --host` runs host commands (the core launcher mechanism).
- `libayatana-appindicator3.so.1` is bundled; exported `.desktop` / metainfo
  pass `desktop-file-validate` + `appstreamcli validate`.

Cosmetic: `flatpak-builder` logs `Ignoring release element without timestamp or
date` for the `0.0.0` metainfo placeholder — `release.yml` rewrites it with the
real version + date at tag time, so a real release is clean.

Not yet done: an **interactive** smoke test (overlay window actually shows +
renders on hotkey/tray-click, global hotkey via the GlobalShortcuts portal,
launching a host editor from the overlay), the offline source generators, the
runtime bump off EOL 48, and the Flathub PR.

App-code side (in the main tree, active whether or not it ever runs sandboxed):

- `flatpak-spawn --host` wrapping — `src-tauri/src/launch.rs` (`in_flatpak()`).
- updater disabled under Flatpak — `updater_mode` returns `managed`.
- autostart toggle hidden under Flatpak — `is_flatpak` command + Settings.
- global hotkey: relies on the `GlobalShortcuts` portal; tray is the fallback.

## Before the first build / Flathub PR

1. **Offline dependency sources** (Flathub disallows network during build):

   ```sh
   pip install --user flatpak-cargo-generator flatpak-node-generator   # or pipx
   flatpak-cargo-generator src-tauri/Cargo.lock -o packaging/flatpak/cargo-sources.json
   flatpak-node-generator npm package-lock.json  -o packaging/flatpak/node-sources.json
   ```

   Uncomment the two `- *-sources.json` lines in the manifest. Regenerate both
   whenever `Cargo.lock` / `package-lock.json` change (worth a CI check).

2. **Tray module** — done: `packaging/flatpak/shared-modules/` is the
   [flathub/shared-modules](https://github.com/flathub/shared-modules) submodule,
   and the manifest pulls
   `shared-modules/libayatana-appindicator/libayatana-appindicator-gtk3.json`.
   `git submodule update --init` after a fresh clone.

3. **Runtime** — `runtime-version: '48'` is EOL; bump to `49` / `50` (the
   `rust-stable` / `node22` extension versions follow the SDK automatically).

## Local build + test

```sh
sudo pacman -S flatpak-builder                     # one-time
flatpak install flathub org.gnome.Platform//48 org.gnome.Sdk//48 \
  org.freedesktop.Sdk.Extension.rust-stable//24.08 \
  org.freedesktop.Sdk.Extension.node22//24.08

flatpak-builder --user --install --force-clean build-dir \
  packaging/flatpak/io.github.seraphx2.devprompt.yaml

flatpak run io.github.seraphx2.devprompt
```

Smoke test: tray icon appears; the hotkey (or tray ▸ Show) opens the overlay;
"Open in terminal" / "Open in VS Code" on a repo launches the **host** program;
Settings shows no "Start at login" checkbox.

For a quick loop without the source generators, add `--share=network` under
`build-options` temporarily — never commit that; Flathub's builders have no net.

## Flathub submission

Fork `flathub/flathub`, branch `io.github.seraphx2.devprompt`, add the manifest,
open the PR. Must pass `flatpak-builder --lint` and `appstreamcli validate` on
`packaging/linux/io.github.seraphx2.devprompt.metainfo.xml`, with the screenshot
URLs reachable (they point at `main`, so merge `dev` first). After merge, a new
release is a manifest commit/version bump PR.
