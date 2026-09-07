# Phase 4 — Flatpak (self-hosted repo)

**Lift:** L (weeks; **touches app code**). **Reach:** every distro, one package,
sandboxed, auto-updating via `flatpak update` / GNOME Software / KDE Discover.
**Needs from maintainer:** nothing ongoing — CI builds and signs it.

**Status: live.** `.github/workflows/repo.yml` builds the Flatpak from
`packaging/flatpak/` and bakes it into a GPG-signed OSTree repo on the `gh-pages`
branch, served at `https://seraphx2.github.io/dev-prompt/flatpak` next to the
apt/rpm/pacman trees. Users add it like any third-party remote:

```sh
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak remote-add --if-not-exists --user dev-prompt \
  https://seraphx2.github.io/dev-prompt/dev-prompt.flatpakrepo
flatpak install --user dev-prompt io.github.seraphx2.devprompt
```

Flathub itself is **parked** — see "Flathub, if ever pursued" at the bottom.

## Why it's the big one

dev-prompt's whole job is to **launch other programs on the host** — editors,
terminals, AI CLIs. A Flatpak runs in a sandbox where those binaries don't
exist. Making it work is an architecture task, not just a manifest.

## App-code changes (done — active whether or not it runs sandboxed)

### 1. Spawn host processes through `flatpak-spawn --host`

`src-tauri/src/launch.rs` — `in_flatpak()` (presence of `/.flatpak-info`) and
`spawn_detached()` prefix `flatpak-spawn --host --directory=<cwd> --` to every
external exec when sandboxed. The terminal, editor, AI-CLI and app-scope
`gtk-launch` paths all funnel through it. Needs `--talk-name=org.freedesktop.Flatpak`.

### 2. Host tool detection

`src-tauri/src/rules.rs` — `host_which()` probes the host via
`flatpak-spawn --host command -v`, memoised. Without it `requires:` / `needs:`
gating and the terminal resolver check the *sandbox* PATH, which has none of the
user's editors/CLIs, so every gated action vanishes.

### 3. App-scope discovery reaches the host

`src-tauri/src/apps.rs` — `app_dirs()` / `icon_roots()` add `/run/host/usr/**`
so the `>` scope enumerates host system apps and their theme icons (via
`--filesystem=host-os:ro`). Launching still goes through host `gtk-launch`.

### 4. Updater + autostart

`updater_mode()` returns `managed` under Flatpak (`flatpak update` owns
updates). "Start at login" still works: under Flatpak the toggle routes through
the `org.freedesktop.portal.Background` `RequestBackground` portal
(`src-tauri/src/autostart.rs`, `ashpd`) instead of `tauri-plugin-autostart` —
the portal writes the host `~/.config/autostart` entry after a one-time consent
dialog. `get_autostart` reads that file back (visible via `--filesystem=home`).

### 5. Single-instance

`tauri-plugin-single-instance` is skipped under Flatpak (`src-tauri/src/lib.rs`)
— Flatpak enforces single-instance itself, and the session-bus name registration
was briefly suspected in an early startup crash.

## The manifest

`packaging/flatpak/io.github.seraphx2.devprompt.yaml`:

- `runtime: org.gnome.Platform` / `sdk: org.gnome.Sdk`, **version `50`**. The
  EOL GNOME 48 runtime (WebKitGTK 2.48) crashed ~300 ms into startup in a
  GPU-less Hyper-V guest; 2.50 (runtime 50) is fine. `sdk-extensions:`
  `rust-stable` + `node22`. `default-branch: stable`.
- `finish-args` — every hole carries an inline reviewer note in the manifest.
  The load-bearing ones: `--socket=x11` (the global hotkey is an X11 grab via
  XWayland — full x11, not `fallback-x11`, which withholds it under Wayland),
  `--share=network` (WebKitGTK's network process won't start otherwise and then
  no page — even bundled `tauri://` assets — loads), `--talk-name=org.freedesktop.Flatpak`
  (host spawn), `--filesystem=host-os:ro` (the `>` scope),
  `--filesystem=xdg-run/tray-icon:create` (the tray PNG lives in the private
  runtime dir; without this one subdir shared the host SNI host sees nothing),
  `--filesystem=home` (repo scan).
- `build-commands` — `npm ci` then `npm run tauri build -- --no-bundle`
  (bare `cargo build` leaves the binary in dev mode, reaching for
  `localhost:1420`); install the binary + desktop/metainfo/icons rebased to the
  app-id. Tray backend from the `flathub/shared-modules` submodule
  (`libayatana-appindicator-gtk3` — the GNOME runtime has GTK3 but not the
  appindicator libs).
- **Network build.** `build-options.build-args: [--share=network]` and plain
  `npm ci` / `cargo`. This is fine for the self-hosted repo (our CI has
  network); Flathub's builders don't, so a submission there would swap this for
  offline `cargo-sources.json` / `node-sources.json` (below).

## The self-hosted repo pipeline

`.github/workflows/repo.yml`, on `release: published` (or `workflow_dispatch`
with a tag):

- **`flatpak-repo` job** — runs inside the
  `ghcr.io/flathub-infra/flatpak-github-actions:gnome-50` container
  (`options: --privileged` for `flatpak-builder`'s bwrap sandbox). A bare
  `ubuntu-latest` runner's newer freedesktop SDK made CMake install the ayatana
  tray libs to `/app/lib64`, which the shared-modules chain and the runtime
  loader don't expect; the container's toolchain matches Flathub's, so the
  stock module include just works. The job checks out the tagged tree
  (`submodules: recursive`), adds the flathub remote, imports the signing key
  (`REPO_GPG_PRIVATE_KEY`), stamps the real version into the metainfo
  `<release>`, then:
  ```sh
  flatpak-builder --user --install-deps-from=flathub \
    --repo=flatpak-repo --gpg-sign=E3C07CD21A9A9BA5 \
    --force-clean --disable-rofiles-fuse \
    build-dir packaging/flatpak/io.github.seraphx2.devprompt.yaml
  flatpak build-update-repo --gpg-sign=E3C07CD21A9A9BA5 \
    --generate-static-deltas --prune --prune-depth=20 flatpak-repo
  ```
  `.flatpak-builder` (the cargo/vite build cache) is cached, keyed on the
  manifest + `Cargo.lock` + `package-lock.json`; the container carries the SDK.
  The signed OSTree repo is tarred and handed to `publish` as an artifact.
  Best-effort: if this job fails, `publish` still ships the other trees and the
  previous Flatpak repo stays in place.
- **`publish` job** — after placing deb/rpm/pacman, wholesale-replaces
  `site/flatpak` with the fresh (already signed + summary-updated) OSTree repo,
  then `build-repo.sh` drops in `dev-prompt.flatpakrepo` +
  `io.github.seraphx2.devprompt.flatpakref` and the landing page gains a
  "Flatpak — any distro" section.

No history preservation across releases (each build starts from an empty repo
dir): `flatpak update` re-pulls the app (~tens of MB — the runtime is shared and
unaffected), no cross-version static deltas. Fine at this project's release
cadence; switch to `ostree pull-local` into the existing `site/flatpak` if it
ever matters.

## Signing / trust

Same GPG key as the apt/rpm/pacman repo (id `E3C07CD21A9A9BA5`,
`packaging/repo/dev-prompt-repo.asc`, private half = secret
`REPO_GPG_PRIVATE_KEY`). `GPGKey=` in the two `.flatpakre{po,f}` descriptors is
base64 of the de-armored public key; regenerate on a rotation with
`gpg --dearmor < packaging/repo/dev-prompt-repo.asc | base64 -w0`.

This is the same trust posture as the Phase 3 repo: no third-party review, no
discovery in software centres beyond what the `.flatpakref` gives, but a
GPG-signed repo over HTTPS from a public, MIT-licensed, CI-built source. The
audience (developers installing a dev tool) adds third-party remotes routinely.

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
the `>` scope lists host apps; "Start at login" prompts for consent then persists.

To exercise the signed-repo path locally, add `--repo=/tmp/dpr
--gpg-sign=<your key>` to the builder and
`flatpak build-update-repo --gpg-sign=<your key> /tmp/dpr`.

## Definition of done

**Done:**

- [x] App-code (§1–5 above), active sandboxed or not.
- [x] Manifest — `packaging/flatpak/io.github.seraphx2.devprompt.yaml`, network
  build, `default-branch: stable`, reviewer notes inline.
- [x] Verified on CachyOS (KDE, Wayland, Hyper-V guest): overlay renders,
  **global hotkey summons it**, tray icon shows, `>` lists host apps with icons,
  repo actions launch host programs.
- [x] `repo.yml` `flatpak-repo` + `publish` jobs build, GPG-sign and publish the
  OSTree repo to `gh-pages`.
- [x] `dev-prompt.flatpakrepo` + `io.github.seraphx2.devprompt.flatpakref`
  descriptors; landing page + README install section.
- [x] Autostart via `org.freedesktop.portal.Background` `RequestBackground`
  (`src-tauri/src/autostart.rs`, `ashpd`) — the "Start at login" toggle works
  in the Flatpak like it does natively, minus a one-time consent dialog.
- [x] `docs/linux-distribution/README.md` status row updated.

**Not done:**

- [ ] Global hotkey via `org.freedesktop.portal.GlobalShortcuts` instead of the
  X11 grab — blocked on `tauri-plugin-global-shortcut`, which has no portal
  support upstream. The X11 grab (via XWayland) works today; this is the main
  thing between the current manifest and a Flathub submission.
- [ ] Cross-version static deltas (`ostree pull-local` into the live repo) —
  optional; `flatpak update` just re-pulls the (small) app without them.

## Flathub, if ever pursued

Parked, not abandoned. The blocker is permission review: `--socket=x11`,
`--talk-name=org.freedesktop.Flatpak`, `--filesystem=host-os:ro` and
`--filesystem=home` together are a wide sandbox, and Flathub's "use the portal
where one exists" rule points straight at the unfinished GlobalShortcuts work
above. A submission would also need:

- **Offline dependency sources** (Flathub builders have no network):
  `flatpak-cargo-generator src-tauri/Cargo.lock -o cargo-sources.json` and
  `flatpak-node-generator npm package-lock.json -o node-sources.json`, wired
  into the manifest `sources:` with `--share=network` removed. Last attempt the
  node generator's output was incomplete (missing `zimmerframe`) and the offline
  `npm ci` failed `ENOTCACHED` — needs debugging.
- PR against `flathub/flathub` `new-pr` branch (not `master`), the submission
  template, `flatpak-builder-lint` clean via `org.flatpak.Builder`, screenshot
  URLs reachable (they point at `main`).

Self-hosting first isn't a detour: the app-code and manifest are the same, and
the Flatpak gets real-world mileage before any review.
