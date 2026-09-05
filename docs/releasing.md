# Releasing

Releases are built by GitHub Actions and published to the repo's **Releases** page.
Windows (NSIS installer + portable zip) and Linux (`deb` / `rpm` / `AppImage`)
are built in one matrixed run, each with its auto-update artifacts. macOS is
still pending.

## Cutting a release

1. Push whatever you want shipped to `main`.
2. GitHub ▸ **Actions** ▸ **Release** ▸ **Run workflow**.
3. The job computes a version, builds, and **publishes** a release with the
   installer, the portable zip, and the updater files attached. Done.

Two optional checkboxes on the "Run workflow" form:

| Input | Default | Effect |
|---|---|---|
| **draft** | off | Create the release as a draft — nothing is public and no tag exists until you open it and click Publish. |
| **prerelease** | off | Publish, but mark it pre-release so the in-app updater (which only tracks the newest *full* release) skips it. Un-check "pre-release" in the GitHub UI when you're ready to ship it. |

Leave both off for a normal release and it goes live the moment the build passes.

**Branch:** the "Run workflow" form also has a ref picker. You can cut a build from
any feature branch — but a **full release (both boxes off) is only allowed from
`main`**. From any other branch the job fails fast unless `draft` or `prerelease`
is checked, so a feature build can never become the version users auto-update to.
The tag it creates points at that branch's HEAD commit.

## Versioning — CalVer, automatic

Format: `YYYY.(MM*100+DD).BUILD`

| Segment | Meaning | Example (2026-08-31) |
|---|---|---|
| major | calendar year | `2026` |
| minor | `month*100 + day` | `831` |
| build | `1` for the day's first release, `+1` per extra same-day build | `1` |

`scripts/version.mjs` derives `build` from existing `v<major>.<minor>.*` git tags,
then writes the version into `package.json`, `src-tauri/tauri.conf.json` and
`src-tauri/Cargo.toml` inside the CI job. Nothing is committed back — the git tag
the workflow creates (`v2026.831.1`) is the record of what shipped. The versions
checked into the repo stay at a placeholder and don't matter.

Run it locally to see what the next release would be called (it will rewrite the
three manifests — `git checkout` them afterwards):

```
npm run version:calver
```

## One-time setup

The updater signs its artifacts with a minisign keypair (this is **not** Windows
code signing — we don't do that; installers are unsigned and SmartScreen will warn
on first run).

The **public** key is committed in `src-tauri/tauri.conf.json` under
`plugins.updater.pubkey`. The **private** key must be added as repo secrets:

```
gh secret set TAURI_SIGNING_PRIVATE_KEY < path/to/devprompt-updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --body ""
```

(The key was generated with an empty password. If you ever regenerate it, update
the pubkey in `tauri.conf.json` too — and note that existing installs can only
auto-update to a build signed with the **same** key they were installed with.)

## What users get

Windows:

- **`dev-prompt_<version>_x64-setup.exe`** — NSIS installer, per-user, no admin
  prompt. Recommended.
- **`dev-prompt_<version>_x64-portable.zip`** — just the `.exe`, no install.
  Portable builds do **not** auto-update.

Linux:

- **`dev-prompt_<version>_amd64.deb`** / **`dev-prompt-<version>-1.x86_64.rpm`** —
  native packages for Debian/Ubuntu and Fedora/RHEL. The in-app updater points
  at the AppImage artifact, so treat these as updating through the system package
  manager (or a fresh download) rather than in-app.
- **`dev-prompt_<version>_amd64.AppImage`** — no-install, runs on any glibc
  distro (Arch/CachyOS included). This is the artifact the in-app updater
  installs, so an AppImage install **does** auto-update in place.

Shared:

- **`latest.json`** + the `*.nsis.zip` / `*.AppImage.tar.gz` archives + `*.sig` —
  consumed by the in-app updater; ignore them. `latest.json` carries a key per
  platform (`windows-x86_64`, `linux-x86_64`, …); each matrix leg merges its own
  key into the file the other leg wrote.

## Test a Linux build locally (CachyOS / Arch)

`.deb` / `.rpm` won't install on Arch, but the **AppImage** is the same artifact
other-distro users get, so it's the one to smoke-test.

```sh
# one-time: AppImage tooling (patchelf) + optional deb/rpm inspectors
sudo pacman -S --needed patchelf fuse2 file binutils dpkg rpm-tools

# fastest loop — the dev build, no packaging:
npm run tauri dev

# build the real AppImage. -c disables the updater artifacts so the build
# doesn't need the signing key (drop it if you've exported the key — see below).
npm run tauri build -- --bundles appimage -c '{"bundle":{"createUpdaterArtifacts":false}}'

BIN=$(ls src-tauri/target/release/bundle/appimage/dev-prompt_*_amd64.AppImage)
chmod +x "$BIN"
"$BIN"                       # launches the packaged app — check tray + hotkey

# optional: peek inside the deb/rpm payloads without installing
npm run tauri build -- --bundles deb,rpm -c '{"bundle":{"createUpdaterArtifacts":false}}'
dpkg-deb -c src-tauri/target/release/bundle/deb/*.deb
rpm -qlp   src-tauri/target/release/bundle/rpm/*.rpm
```

Notes:

- The Linux targets come from `src-tauri/tauri.linux.conf.json`, which Tauri
  auto-merges over `tauri.conf.json` on Linux (arrays replace, so its
  `bundle.targets` wins). `--bundles` on the CLI overrides both.
- The base config sets `createUpdaterArtifacts: true`, which makes `tauri build`
  **fail** (not just warn) when `TAURI_SIGNING_PRIVATE_KEY` isn't in the env —
  the bundles are still written, but the command exits non-zero. The `-c`
  override above turns that off for a local smoke-test. To exercise the updater
  path instead, `export TAURI_SIGNING_PRIVATE_KEY="$(cat
  path/to/devprompt-updater.key)"` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD=`,
  and drop the `-c` flag.
- `npm run version:calver` rewrites the three manifests to the CalVer value;
  `git checkout` them afterwards if you were only previewing.

## Later

- Linux distribution beyond GitHub Releases (AUR, apt/rpm repo, Flatpak, Snap):
  phased roadmap in [`linux-distribution/`](linux-distribution/README.md).
- macOS: add `macos-latest` to the matrix; produces `.dmg` + updater
  `.app.tar.gz` (also needs vibrancy + signing).
- Store: upload the `.exe` to the Microsoft Store (Store handles signing) — no
  MSIX authoring needed.
