# dev-prompt

**A command palette for every repo you own — one that reads each project and
hands you its actual scripts, builds, and sub-packages.**

Press a global shortcut from anywhere and a fuzzy-searchable list of every
repository under your code folders appears. Type a few letters, hit `Enter`, and
you're in a terminal at its root.

Most launchers stop at "open the folder." dev-prompt opens the action menu it
built by _inspecting the project_: the exact `package.json` scripts it defines
(run through the right package manager), `cargo test`, the `.sln` opened in
Visual Studio, `docker compose up`, a jump straight into the `packages/api`
sub-project. No file manager, no `cd`, no remembering whether it's `npm run dev`
or `pnpm start`.

It adapts to your machine — actions for tools you haven't installed simply don't
appear — and the entire editor-and-toolchain brain is a YAML file you can extend
in a few lines. Appears instantly, vanishes the moment it loses focus, never
touches your taskbar.

Cross-platform, built with **Tauri v2** (Rust) + **Svelte 5** + **Tailwind
CSS** — a small single binary with no runtime dependencies beyond the system
WebView.

## What it does

- **Global-hotkey overlay** — frameless, centered, dismiss-on-blur. Toggle with
  `Ctrl/Cmd+Shift+Space` (rebindable via a click-to-record field in Settings).
- **Multi-root scan + cache** — walks the directories you list, respects
  `.gitignore`, collapses nested repos, caches the result for instant startup and
  refreshes in the background when stale.
- **Fuzzy search** — fzf-style ranking over repo name and path, with match
  highlighting.
- **Context detection** — on selecting a repo it inspects the tree (root plus
  side-by-side and `packages/*`-style sub-projects) for `.sln` / `.csproj`,
  `package.json` scripts, `Cargo.toml`, `go.mod` / `go.work`, Python
  (`pyproject.toml` / `requirements.txt`, uv / poetry), `docker-compose`, and more.
- **Config-driven rule engine** — every editor / build-tool / launcher mapping
  lives in a bundled `default_config.yaml` (markers, program resolution, rules,
  universal actions). A hand-edited `rules.yaml` layers your own on top.
- **Action menu** — always-available actions (terminal, file manager, copy path)
  first, then detected per-ecosystem actions; multi-project repos collapse into
  drill-in submenus. Actions gracefully disappear when the tool they need isn't
  installed.
- **Settings screen** — roots, hotkey, scan depth, cache lifetime, a read-only
  view of the active rules/programs, and a software-update check. Reachable from
  the overlay (`Ctrl+,`) or the tray icon.
- **System tray** — Show / Settings / Quit; the overlay otherwise has no taskbar
  presence.
- **In-app auto-update** — checks GitHub Releases, downloads and installs signed
  updates, relaunches. See [`docs/releasing.md`](docs/releasing.md).

## Configuration

First run creates two files in your OS config directory (`%APPDATA%\dev-prompt\`,
`~/.config/dev-prompt/`, or `~/Library/Application Support/dev-prompt/`):

- **`config.yaml`** — your settings (hotkey, roots, scan depth, cache lifetime).
  Managed entirely by the **Settings** screen; you don't normally touch it.
- **`rules.yaml`** — overrides for the rule engine: extra `markers`, `programs`,
  `rules`, and `universal` actions layered over the bundled defaults. Hand-edited
  (Settings ▸ Rules ▸ Open rules file), ships as a commented scaffold.

Settings reference: [`docs/configuration.md`](docs/configuration.md). The rule
engine — merge rules, every field, and worked examples (pin an editor path, add a
build rule, disable a built-in, change the Enter action) —
[`docs/rules-engine.md`](docs/rules-engine.md); the canonical schema with inline
docs is [`src-tauri/src/default_config.yaml`](src-tauri/src/default_config.yaml).

The discovered repo list is cached at `<OS cache dir>/dev-prompt/repos.json`.

### How detection works

1. **Discovery** — a directory is a "repo" if it contains any `markers` entry
   _or_ matches any rule's `match` glob (`.git`, `*.sln`, `package.json`,
   `Cargo.toml`, `pom.xml`, `Gemfile`, …).
2. **Inspection** — on selection, the repo tree is read for project manifests and
   their details (scripts, package manager, Python runner, compose files).
3. **Rules** — `default_config.yaml` maps manifests to actions. `requires: [bin]`
   hides a rule unless the binary is on `PATH`; `needs: [key]` hides an action
   unless that program resolves. Nothing errors when a tool is missing — it just
   doesn't appear.

## Platform support

|                   | Status                                                                                                                                                                                                                                                                                                                     |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Windows 10/11** | Built, packaged (NSIS installer + portable zip), and tested. Acrylic blur + rounded corners, Windows Terminal integration, Visual Studio / Rider detection.                                                                                                                                                                |
| **Linux**         | Same codebase, compiles. Hotkey works on X11; Wayland needs the XDG global-shortcuts portal (tray-click fallback otherwise). Terminal-command actions need per-emulator working-dir flags (in progress) — plain "open a terminal" works. Panel is translucent but unblurred (no compositor backing yet), so it paints a little more solid than on Windows. Packaging (`deb`/`rpm`/`AppImage`) not yet in CI. |
| **macOS**         | Same codebase, compiles; not yet run on a Mac. Global hotkey and process launching are supported by the underlying plugins; vibrancy and `.dmg` packaging are unimplemented.                                                                                                                                               |

The architecture is platform-neutral — program paths and OS quirks are isolated
in the `programs` config (`any` / `windows` / `linux` / `macos` candidate lists)
and a handful of `#[cfg]` blocks. The remaining cross-platform work is tracked in
[`docs/future-work.md`](docs/future-work.md).

## Prerequisites

| Tool                 | Notes                                                                                                                                                                                                                               |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Node.js 18+          | frontend build (`npm`)                                                                                                                                                                                                              |
| Rust (stable, 1.77+) | <https://rustup.rs>                                                                                                                                                                                                                 |
| Platform toolchain   | **Windows:** MSVC Build Tools ("Desktop development with C++") + WebView2 (preinstalled on Win 11). **Linux:** `webkit2gtk-4.1`, `libayatana-appindicator3`, `librsvg2`, standard build tools. **macOS:** Xcode Command Line Tools. |

## Run (development)

```sh
npm install
npm run tauri dev
```

Starts Vite on :1420 and launches the overlay (hidden until you press the
hotkey). The tray icon appears immediately.

## Build

```sh
npm run tauri build
```

Produces a Windows NSIS installer today; other platform bundles are wired up in
the release workflow (see [`docs/releasing.md`](docs/releasing.md)).

## Usage

**Repo list**

| Key                  | Action                                                |
| -------------------- | ----------------------------------------------------- |
| type                 | fuzzy-filter repos                                    |
| `Up` / `Down`        | move selection                                        |
| `Enter`              | run the repo's default action (open a terminal there) |
| `Tab` / `Ctrl+Enter` | open the full action menu for the selected repo       |
| `Ctrl+R`             | force a rescan                                        |
| `Delete`             | clear the query                                       |
| `Ctrl+,`             | open Settings                                         |
| `Esc` / click away   | hide the overlay                                      |

**Action menu**

| Key     | Action                                               |
| ------- | ---------------------------------------------------- |
| type    | filter actions (reaches into sub-projects)           |
| `Enter` | run the selected action, or drill into a sub-project |
| `Tab`   | drill into the selected sub-project                  |
| `Esc`   | step back one level (sub-project → menu → repo list) |

The mouse **back / forward** buttons work throughout: back == `Esc` for the
current screen, forward == `Tab` (open actions / drill into a sub-project).

## Releasing

Manual GitHub Actions workflow computes a CalVer version, builds, and publishes a
release with the installer, a portable zip, and signed updater artifacts. Full
details — versioning, the draft/prerelease toggles, the one-time signing-key
setup — in [`docs/releasing.md`](docs/releasing.md).

## Contributing

Issues and PRs welcome. Adding support for an editor or build tool is usually
just data in `default_config.yaml` — no Rust. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Project layout

```
src/                        Svelte frontend (overlay UI)
  App.svelte                repo list + action menu + settings, window-level keys
  lib/ipc.ts                typed wrappers over Tauri commands + events
  lib/fuzzy.ts              fzf-style scorer
  lib/updater.ts            auto-update check / install
  lib/components/           SearchInput, ResultList, ResultRow, ActionMenu,
                            Settings, Highlight, ClearButton
src-tauri/                  Rust backend
  src/config.rs             config schema, load/merge, path expansion
  src/default_config.yaml   bundled markers / programs / rules / universal
  src/scan.rs               directory walk -> Vec<Repo>, discovery globset
  src/inspect.rs            per-repo context (projects, manifests, compose)
  src/rules.rs              rule engine: program resolver, templates, providers
  src/cache.rs              repos.json read/write, staleness, merge
  src/index.rs              nucleo fuzzy ranking
  src/launch.rs             detached process spawning
  src/commands.rs           #[tauri::command] surface + shared state
  src/lib.rs                plugin wiring, window setup, hotkey, tray
  src/rules_template.yaml    scaffold written to the user's rules.yaml
scripts/version.mjs         CalVer generator for releases
.github/workflows/          release.yml
docs/                       configuration.md, rules-engine.md, releasing.md, future-work.md
```

## Tests

```sh
cd src-tauri && cargo test    # config merge/expansion, discovery,
                              # nested-repo collapse, cache staleness,
                              # rule evaluation, fuzzy ranking
npm run check                 # svelte-check (types)
```

## License

MIT — see [LICENSE](LICENSE).

## Install note

The Windows installer is unsigned, so SmartScreen shows a warning on first run —
**More info ▸ Run anyway**. Updater artifacts are minisign-signed (public key in
`src-tauri/tauri.conf.json`); the app verifies every update before applying it.
