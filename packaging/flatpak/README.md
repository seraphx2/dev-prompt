# packaging/flatpak/

Flatpak manifest + remote descriptors for dev-prompt (Phase 4). Design and
rationale: [`docs/linux-distribution/phase-4-flatpak.md`](../../docs/linux-distribution/phase-4-flatpak.md).

```
io.github.seraphx2.devprompt.yaml       the manifest
dev-prompt.flatpakrepo                  remote descriptor  -> published at /dev-prompt.flatpakrepo
io.github.seraphx2.devprompt.flatpakref one-click install  -> published at /io.github.seraphx2.devprompt.flatpakref
shared-modules/                         flathub/shared-modules submodule (ayatana tray)
```

## Status

**Live, self-hosted.** `.github/workflows/repo.yml` (`flatpak-repo` +
`publish` jobs) builds this manifest on every release, GPG-signs it with the
repo key (`E3C07CD21A9A9BA5`), and publishes the OSTree repo to
`https://seraphx2.github.io/dev-prompt/flatpak` alongside the apt/rpm/pacman
trees. Install:

```sh
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak remote-add --if-not-exists --user dev-prompt \
  https://seraphx2.github.io/dev-prompt/dev-prompt.flatpakrepo
flatpak install --user dev-prompt io.github.seraphx2.devprompt
```

Built with `flatpak-builder` (GNOME 50) and run on CachyOS (KDE, Wayland, in a
Hyper-V guest): overlay renders, **global hotkey summons it**, tray icon shows,
the `>` scope lists host apps with icons, and repo actions launch host editors /
terminals.

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
date` for the `0.0.0` metainfo placeholder — the `flatpak-repo` job rewrites it
with the real version + date at build time (same `sed` as `release.yml`).

## Descriptors

`dev-prompt.flatpakrepo` (add-remote) and
`io.github.seraphx2.devprompt.flatpakref` (one-click add-remote + install) are
committed with the signing key baked in as `GPGKey=` (base64 of the de-armored
public key). `build-repo.sh` copies both to the site root. On a key rotation,
regenerate the `GPGKey=` value:

```sh
gpg --dearmor < ../repo/dev-prompt-repo.asc | base64 -w0
```

`shared-modules/` is the [flathub/shared-modules](https://github.com/flathub/shared-modules)
submodule — `git submodule update --init` after a fresh clone; CI checks out
`submodules: recursive`.

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
the `>` scope lists host apps; Settings shows no "Start at login" checkbox.

To rehearse the signed-repo path, add `--repo=/tmp/dpr --gpg-sign=<your key>` to
the builder, then
`flatpak build-update-repo --gpg-sign=<your key> --prune /tmp/dpr`.

## Flathub

Parked — the manifest's `--socket=x11` / `--talk-name=org.freedesktop.Flatpak` /
`--filesystem=host-os:ro` draw review scrutiny against Flathub's "use the portal
where one exists" rule, and a submission additionally needs offline
`cargo-sources.json` / `node-sources.json` (the node generator's last output was
incomplete). Details in
[`docs/linux-distribution/phase-4-flatpak.md`](../../docs/linux-distribution/phase-4-flatpak.md)
("Flathub, if ever pursued").
