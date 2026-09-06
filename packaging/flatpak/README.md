# packaging/flatpak/

Flatpak manifest for dev-prompt (Phase 4). Design and rationale:
[`docs/linux-distribution/phase-4-flatpak.md`](../../docs/linux-distribution/phase-4-flatpak.md).

```
io.github.seraphx2.devprompt.yaml   the manifest
```

## Status

Written, **not yet built** — the dev box has `flatpak` but not
`flatpak-builder`, and the GNOME SDK + rust/node extensions are a multi-GB
pull. The app-code side is done and lives in the main tree:

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

2. **Tray module** — add `libayatana-appindicator` (+ `libdbusmenu`,
   `libayatana-indicator`). Easiest: reference
   `shared-modules/libappindicator/libappindicator-gtk3-12.10.json` from
   [flathub/shared-modules](https://github.com/flathub/shared-modules) as a git
   submodule, or vendor the three tarballs with sha256s.

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
