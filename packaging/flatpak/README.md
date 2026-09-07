# packaging/flatpak/

Flatpak manifest for dev-prompt (Phase 4). Design and rationale:
[`docs/linux-distribution/phase-4-flatpak.md`](../../docs/linux-distribution/phase-4-flatpak.md).

```
io.github.seraphx2.devprompt.yaml   the manifest
```

## Status

**Working** — built with `flatpak-builder` (GNOME 50) and run on CachyOS (KDE,
Wayland, in a Hyper-V guest). Overlay renders, **global hotkey summons it**, tray
icon shows, the `>` scope lists host apps with icons, and repo actions launch
host editors / terminals.

Getting there took seven fixes, each a real Tauri-on-Flatpak gotcha:

| Fix | Symptom it cured |
| --- | --- |
| `runtime-version: '50'` (was 48, EOL) | crash ~300 ms into startup (WebKitGTK 2.48) |
| `--share=network` | WebKit's network process won't init → every page load (incl. bundled `tauri://`) errors |
| `--filesystem=xdg-run/tray-icon:create` | tray SNI registers but the icon PNG (in the private runtime dir) is invisible to the host |
| `tauri build --no-bundle` (was bare `cargo build`) | binary ran in dev mode → tried to reach `localhost:1420` |
| `rules::host_which` (`src-tauri/src/rules.rs`) | `requires:`/`needs:` and the terminal resolver checked the *sandbox* PATH → editors/CLIs invisible |
| `--filesystem=host-os:ro` + `/run/host/usr` dirs in `apps.rs` | `>` scope only saw `~/.local/share` apps |
| `--socket=x11` (was `fallback-x11`) | global hotkey (an X11 grab via XWayland) had no X11 socket |

Also: `tauri-plugin-single-instance` is skipped under Flatpak (`src-tauri/src/lib.rs`)
— redundant there and it was briefly suspected in the startup crash.

Cosmetic: `flatpak-builder` logs `Ignoring release element without timestamp or
date` for the `0.0.0` metainfo placeholder — `release.yml` rewrites it with the
real version + date at tag time.

Not yet done: the offline source generators and the Flathub PR (below).

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


## Local build + test

```sh
sudo pacman -S flatpak-builder                     # one-time
flatpak install flathub org.gnome.Platform//50 org.gnome.Sdk//50 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08 \
  org.freedesktop.Sdk.Extension.node22//25.08

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
