# Phase 4 — Flatpak / Flathub

**Lift:** L (weeks; **touches app code**). **Reach:** every distro, one package,
sandboxed, auto-updating via GNOME Software / KDE Discover / `flatpak update`.
**Needs from maintainer:** a Flathub submission PR + review.

## Why it's the big one

dev-prompt's whole job is to **launch other programs on the host** — editors,
terminals, AI CLIs. A Flatpak runs in a sandbox where those binaries don't
exist. Making it work is an architecture task, not just a manifest.

## Blocking app-code changes (do these first, they're useful anyway)

### 1. Spawn host processes through `flatpak-spawn --host`

Everywhere the app execs an external program (`src-tauri/src/rules.rs`
`terminalize()` and the action-launch path — grep for `Command::new`), when
running under Flatpak it must prefix `flatpak-spawn --host`:

- Detect sandbox: `std::env::var("FLATPAK_ID").is_ok()` (or presence of
  `/.flatpak-info`).
- Wrap: `flatpak-spawn --host --env=… -- <argv>`; working directory via
  `--directory=`.
- Requires the sandbox hole `--talk-name=org.freedesktop.Flatpak`.
- Test matrix: terminal launch, editor launch, AI-CLI launch, each with a repo
  path containing spaces.

### 2. Global hotkey via the XDG GlobalShortcuts portal

The sandbox cannot take a raw X11/Wayland global grab.
`tauri-plugin-global-shortcut` needs to be on a version with
`org.freedesktop.portal.GlobalShortcuts` support, or the app registers shortcuts
through the portal directly (user approves them once in a system dialog; they're
reconfigurable in system settings, not the app). Confirm current plugin
capability before committing to a timeline — this may need an upstream bump or a
Linux-specific code path.

### 3. Autostart via the Background portal

`~/.config/autostart` isn't writable from the sandbox. Use
`org.freedesktop.portal.Background` `RequestBackground` with `autostart=true`.
`tauri-plugin-autostart` may already do this when sandboxed — verify; if not,
add a portal path.

### 4. Disable the in-app updater under Flatpak

When `FLATPAK_ID` is set: no update polling, no update UI (Flatpak updates
itself). Gate `pollUpdates()` / the Settings section on a
`is_flatpak` command, or compile the updater plugin out via a cargo feature for
the Flatpak build.

### 5. Tray

Bundle/rely on `libayatana-appindicator3` (the GNOME/freedesktop runtime has the
SNI stack). Add `--talk-name=org.kde.StatusNotifierWatcher` and
`--talk-name=org.freedesktop.Notifications`.

## The manifest

`packaging/flatpak/io.github.seraphx2.devprompt.yaml`:

- `runtime: org.gnome.Platform` / `sdk: org.gnome.Sdk` (GNOME runtime ships
  WebKitGTK 4.1 + libsoup3), `sdk-extensions: org.freedesktop.Sdk.Extension.rust-stable`
  and `.node<NN>`.
- **Offline build** (Flathub requirement — no network during build):
  - `flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json`
  - `flatpak-node-generator npm package-lock.json -o node-sources.json`
  - regenerate both on every dependency change (CI check).
- `finish-args` (minimum):
  ```
  --socket=wayland --socket=fallback-x11 --share=ipc
  --device=dri
  --talk-name=org.freedesktop.Flatpak            # flatpak-spawn --host
  --talk-name=org.kde.StatusNotifierWatcher      # tray
  --talk-name=org.freedesktop.Notifications      # notifications
  --system-talk-name=… only if needed
  --filesystem=home                              # it scans the user's repos
  ```
  Justify every hole in the Flathub PR; `--talk-name=org.freedesktop.Flatpak`
  plus `--filesystem=home` will draw review scrutiny — explain the launcher
  use-case.

## Flathub submission

- Fork `flathub/flathub`, add the manifest on a branch named
  `io.github.seraphx2.devprompt`, open a PR.
- Passes: `flatpak-builder --lint`, `appstreamcli validate` on the Phase-1
  metainfo, screenshot reachable.
- After merge, Flathub builds and hosts; new releases go out by PRing a manifest
  version/commit bump (or wiring `flathub/…` to track the git tag).

## Definition of done

**Done in-tree** (app code + manifest, not yet build-verified):

- [x] Host launches routed through `flatpak-spawn --host` when sandboxed —
      `src-tauri/src/launch.rs` `in_flatpak()` / `spawn_detached`. Covers the
      terminal, editor, AI-CLI and `gtk-launch` (app-scope) paths, since they
      all funnel through `spawn_detached`. Needs `--talk-name=org.freedesktop.Flatpak`.
- [x] Updater UI absent under Flatpak — `updater_mode` returns `managed` when
      `/.flatpak-info` is present, which the frontend already treats as
      "don't poll, don't show".
- [x] Autostart toggle hidden under Flatpak — `is_flatpak` command; Settings
      shows a "use your desktop's autostart" note instead. (Background-portal
      `RequestBackground` is the eventual real fix — deferred; needs `ashpd`
      or raw zbus and a sandbox to test.)
- [x] Manifest written **and built** —
      `packaging/flatpak/io.github.seraphx2.devprompt.yaml`, `flatpak-builder`
      against GNOME 48. Clean through `npm ci` / `svelte-check` / `vite build` /
      `cargo build --release` / the `libayatana-appindicator` tray module
      (flathub/shared-modules submodule at `packaging/flatpak/shared-modules/`) /
      every install step. `finish-args` + app-id-rebased desktop/metainfo/icons.
- [x] Sandbox checks — `flatpak-spawn --host` executes host commands (the
      launcher's core mechanism), `libayatana-appindicator3.so.1` is bundled,
      exported `.desktop` + metainfo pass `desktop-file-validate` /
      `appstreamcli validate`.

**Pending** (needs an interactive session and/or a Flathub account):

- [ ] Interactive smoke test — tray icon visible, overlay opens, launching a
      host editor/terminal from the overlay works, on a non-GNOME desktop.
- [ ] Global hotkey via the `GlobalShortcuts` portal actually registers (confirm
      `tauri-plugin-global-shortcut` supports it — may need an upstream bump).
      Tray ▸ Show is the documented fallback.
- [ ] Bump `runtime-version` off EOL 48 → 49/50.
- [ ] Generate `cargo-sources.json` / `node-sources.json` (needs
      `flatpak-cargo-generator` / `flatpak-node-generator`); flip the manifest
      back to `--offline` (it already is — just uncomment the source lines).
- [ ] Flathub PR: `flatpak-builder --lint` clean, screenshots reachable (merge
      `dev` → `main` first), permissions justified in the PR body.

- [x] `docs/linux-distribution/README.md` status updated.
