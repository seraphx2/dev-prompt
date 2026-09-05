# Linux distribution roadmap

Goal: ship dev-prompt through every channel a Linux user might reach for, so
people can install with the tool they already know **and** get updates through
their normal system-update flow instead of re-downloading from a website.

Each phase file is self-contained and written to be executed from a cold start
(a fresh conversation, a new maintainer). Work them in order — later phases
reuse groundwork from earlier ones.

| Phase | Channel | Reach | Lift | Needs from maintainer | Status |
|---|---|---|---|---|---|
| [1](phase-1-groundwork.md) | In-repo packaging groundwork | all channels | S | nothing | **in progress** — files landed; screenshot + CI validate outstanding |
| [2](phase-2-aur.md) | AUR | Arch / CachyOS / Manjaro / EndeavourOS | S | AUR account + SSH key | not started |
| [3](phase-3-apt-rpm-repo.md) | Own apt + rpm repo | Debian/Ubuntu, Fedora/RHEL | M | OBS account **or** a GPG repo key | not started |
| [4](phase-4-flatpak.md) | Flatpak / Flathub | every distro (sandboxed) | L | Flathub PR review | not started |
| [5](phase-5-snap.md) | Snap Store | Ubuntu-centric (sandboxed) | M (after 4) | Snapcraft account | not started |
| [6](phase-6-distro-repos.md) | Official distro repos | max trust | — | a distro maintainer adopting it | passive |

Lift: S = hours, M = a day or two, L = weeks and touches app code.

## What already exists (this is the baseline all phases build on)

- **GitHub Releases + in-app updater.** `.github/workflows/release.yml` builds a
  Windows/Linux matrix via `tauri-apps/tauri-action` and attaches, per release:
  Windows NSIS `.exe` + portable zip; Linux `.deb` / `.rpm` / `.AppImage`;
  `latest.json` + minisign `.sig` files for the updater.
- **`src-tauri/tauri.linux.conf.json`** — auto-merged over `tauri.conf.json` on
  Linux; sets `bundle.targets` to `["deb","rpm","appimage"]` and
  `category` to `DeveloperTool`.
- **In-app updater** (`tauri-plugin-updater`, `src/lib/updater.ts`,
  `src/lib/updateStore.svelte.ts`): polls
  `releases/latest/download/latest.json` on launch + daily.
- See [`docs/releasing.md`](../releasing.md) for the release process and the
  local Linux smoke-test recipe.

## Cross-cutting facts (true regardless of channel)

- **App ID / identifier:** `io.github.seraphx2.devprompt`. Reuse verbatim
  everywhere — Flatpak app-id, AppStream `<id>`, D-Bus name, `.desktop`
  basename should all match or derive from it.
- **Binary / productName:** `dev-prompt`. `.desktop` `StartupWMClass=dev-prompt`.
- **Version scheme:** CalVer `YYYY.(MM*100+DD).BUILD` from `scripts/version.mjs`,
  written into the three manifests inside CI, tagged `v<version>`, never
  committed back. Packaging that needs a version reads the git tag.
- **Runtime library deps** (verified against a real `.deb`; Tauri auto-derives
  them, don't hand-maintain unless a channel needs explicit names):
  - linked: `webkit2gtk-4.1`, `gtk-3`, `libsoup-3.0`, `cairo`, `pango`,
    `glib-2.0`, `json-glib-1.0`, `gdk-pixbuf-2.0`, `javascriptcoregtk-4.1`
  - dlopen'd for the tray: `libayatana-appindicator3.so.1` (tried first) then
    `libappindicator3.so.1`
  - Debian package names Tauri emits: `libwebkit2gtk-4.1-0`, `libgtk-3-0`,
    `libappindicator3-1`
- **Tray lib gotcha:** Tauri wants **`libayatana-appindicator3`** (the
  maintained fork). On distros that only have the ancient `libappindicator`
  (e.g. Arch's `libappindicator` 12.10.1) the tray icon silently fails to
  register. Any channel that bundles libraries (AppImage, Flatpak, Snap) must
  bundle the ayatana one. The CI AppImage is fine because it builds on
  `ubuntu-22.04` where `libayatana-appindicator3-dev` is installed.
- **Autostart:** `tauri-plugin-autostart` writes
  `~/.config/autostart/dev-prompt.desktop` with
  `Exec=<binary> --autostart` (starts silent in the tray). Inside a sandbox
  this path is unavailable — use the `org.freedesktop.portal.Background`
  portal instead (Phase 4).
- **Signing keys — keep them straight:**
  | Key | Purpose | Where |
  |---|---|---|
  | minisign keypair | in-app updater artifact signatures | pubkey in `tauri.conf.json`; private key = repo secrets `TAURI_SIGNING_PRIVATE_KEY` (+ empty `_PASSWORD`) |
  | GPG key | signs an apt/rpm repo's metadata | Phase 3 — new, does **not** exist yet |
  | SSH key | pushes to `aur.archlinux.org` | Phase 2 — new |
  | (Flathub signs its own builds; nothing to manage) | | |

## How the in-app updater interacts with system packages

`tauri-plugin-updater` (2.10+) stamps a `__TAURI_BUNDLE_TYPE` marker into the
binary **at bundle time**. At runtime `bundle_type()` reads it and dispatches:

| Install form | Marker | Updater behavior |
|---|---|---|
| AppImage | `appimage` | full self-update: download `.AppImage.tar.gz`, verify sig, rewrite own file, relaunch |
| `.deb` | `deb` | download `.deb`, `dpkg -i` via pkexec/sudo prompt — **but** `tauri-action` only writes AppImage keys into `latest.json`, so nothing to fetch |
| `.rpm` | `rpm` | as deb, via `rpm -U` |
| pacman / hand-built binary | *none* | falls through to `install_appimage` → tries to rewrite a root-owned `/usr/bin/dev-prompt` → fails |

**Implication for every non-AppImage channel:** the app should not offer an
in-app update. Options, cheapest first:
1. Document "update via your package manager" and accept that the Settings
   update-check may still show a version banner (it only compares version
   strings against `latest.json`).
2. Detect a system install (`bundle_type()` via a Rust command, or an env/
   build flag) and hide the update UI / swap it for a "open Releases" link.
3. (Flatpak/Snap) compile the updater out when `FLATPAK_ID` / `SNAP` is set.

Phases 2–5 each note which they use. Wiring per-format artifacts + `latest.json`
keys so deb/rpm self-update is possible but deliberately out of scope — the
package manager is the right updater for those.
