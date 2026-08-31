# Releasing

Releases are built by GitHub Actions and published to the repo's **Releases** page.
Windows only for now (NSIS installer + portable zip + auto-update artifacts).

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

- **`dev-prompt_<version>_x64-setup.exe`** — NSIS installer, per-user, no admin
  prompt. Recommended.
- **`dev-prompt_<version>_x64-portable.zip`** — just the `.exe`, no install.
  Portable builds do **not** auto-update.
- **`latest.json`** + **`*.nsis.zip`** + **`*.sig`** — consumed by the in-app
  updater; ignore them.

## Later

- Linux: add `ubuntu-22.04` to the job matrix and `deb`/`rpm`/`appimage` to
  `bundle.targets`; Flatpak is a separate pipeline.
- macOS: add `macos-latest`, produces `.dmg` + updater `.app.tar.gz`.
- Store: upload the `.exe` to the Microsoft Store (Store handles signing) — no
  MSIX authoring needed.
