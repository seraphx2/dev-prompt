# dev-prompt — remaining work

Everything below is discussed-but-not-built, ordered **easiest → hardest**.
The config schema itself (markers / programs / rules / universal, templates,
merge model) is implemented — read `src-tauri/src/default_config.yaml` for the
canonical shape.

## 6. `task-targets` provider · _medium, three small parsers_

Parse target lists and emit one terminal action per target.

- `Makefile`: lines matching `^([A-Za-z0-9_.-]+):(?!=)` minus `.PHONY` etc.
- `justfile`: `just --summary` if `just` on PATH, else parse `^([a-z0-9_-]+)`.
- `Taskfile.yml`: YAML parse, keys under `tasks:`.

Cap at ~20. Register `"task-targets"` in `rules.rs::provider_actions`, then add
the `[Makefile, justfile, Taskfile.yml]` rule to `default_config.yaml`.

Files: `rules.rs` (or a `providers/` split if it grows), `default_config.yaml`.

---

## 9. `prompt: true` / "Run command…" · _medium-hard, new frontend mode_

A dynamic action: prompt for a one-off command, run it in the terminal at the
repo dir.

- `RuleAction.prompt: bool`; `{{input}}` template var.
- Frontend: a fourth overlay mode (or an inline input on the action row) that
  captures the string, then calls `run_action` with it threaded through.
- Backend: `run_action` needs an optional `input` arg that `expand()` picks up.

Files: `config.rs`, `rules.rs`, `commands.rs`, `App.svelte`, a new component,
`ipc.ts`.

---

## 10. Linux terminal picker · _hard — and untestable without a GUI Linux_

`terminalize()` on non-Windows just spawns the bare command (no window). Needs:

- a per-emulator table: working-dir flag + command-exec syntax
  - `alacritty --working-directory D -e CMD`
  - `kitty --directory D CMD`
  - `wezterm start --cwd D -- CMD`
  - `gnome-terminal --working-directory=D -- CMD`
  - `konsole --workdir D -e CMD`
  - `foot --working-directory=D CMD`
  - `xterm -e sh -c 'cd D; CMD; exec $SHELL'`
- "first of `programs.terminal.linux` that resolves" → pick its table entry.
- `terminal:` in `config.yaml` to pin one; raw-template override for exotic setups.

Files: `rules.rs`, `default_config.yaml`. Verify on WSLg / a VM.

---

## 11. fs watcher / incremental reindex · _hard — new dep, concurrency, cross-platform_

Avoid the "stale until next open + rescan" model.

- `notify` crate (+ `notify-debouncer-full`).
- **Shallow, non-recursive** watch on each root (recursive inotify on `~/git`
  blows past `max_user_watches` on Linux). React only to dir create/remove/rename.
- Debounce ~750 ms → `scan_root(one_root)` (new incremental fn) → `cache::merge`
  → emit `repos:updated`.
- Watchers start in `setup()`, restart when `roots` changes; keep the TTL rescan
  as the backstop for deeper changes a shallow watch misses.

Files: new `watch.rs`, `scan.rs` (`scan_root`), `lib.rs`, `commands.rs`, `Cargo.toml`.

---

## 12. Linux X11/Wayland hotkey hardening · _hardest — partly out of our hands_

`tauri-plugin-global-shortcut` → `global-hotkey` is X11-only on Linux. Wayland
has no global-grab; it needs the `GlobalShortcuts` XDG portal (compositor
support varies).

- Detect session type via `tauri-plugin-os`.
- On Wayland: attempt the portal; if unavailable, surface a clear message and
  fall back (tray-only activation).
- Document the limitation prominently.

Files: `lib.rs`, docs. Needs real X11 + Wayland sessions to validate.

---

## 13. Eclipse / Anypoint project provisioner · _milestone-sized, Eclipse-version-coupled_

Eclipse-family IDEs have no CLI to open an arbitrary project — only
`-data <workspace>`, and the project must already be registered in that
workspace's `.metadata`. The fatten pass ships the naive form ("Open in
Anypoint Studio" = `-data {{path}}`), which is one-click only when the scanned
folder is the workspace.

A real "open this repo as a project" needs a **pre-launch provisioner** —
a new action-type concept (`provision:` step that runs before the spawn):

- **Option A (preferred):** write
  `<workspace>/.metadata/.plugins/org.eclipse.core.resources/.projects/<name>/.location`
  (semi-documented binary URI format, stable since 3.x) for a repo that has a
  `.project` on disk, then `-data <workspace>` launch. Nothing to ship; degrades
  to "import once by hand" if the format drifts. Needs an
  `anypoint: { workspace: <path> }` config knob.
- **Option B:** bundle a ~50-line headless Eclipse plugin
  (`loadProjectDescription` → `create` → `open` → open workbench) in
  `dropins/`. Correct, but you then maintain it per Studio / Eclipse / STS
  version.

Files: new provisioner hook in `rules.rs` / `launch.rs`, `config.rs` (workspace
knob), `default_config.yaml`.

---

## 14. More workspace-manifest providers · _bigger passes_

Same `manifest → [(name, dir)] → per-module terminal actions` shape as the
`dotnet` / `go-work` / `maven-modules` / `gradle-modules` providers, for the
ecosystems that need more than a flat parse:

- **Cargo workspaces** — root `Cargo.toml` `[workspace] members = [...]` incl.
  globs (`crates/*`). Needs a TOML parser (new dep) or a careful hand-parse.
  `cargo build -p <member>` from the root. Largely redundant with `inspect`'s
  `crates/` discovery — low ROI.
- **Xcode** — `.xcworkspace/contents.xcworkspacedata` (XML FileRefs) →
  `.xcodeproj`s, then `xcshareddata/xcschemes/*.xcscheme` per project.
  `xcodebuild -workspace X -scheme Y`. macOS-only; untestable off a Mac.
- **Nx / Turborepo / Bazel** — build graphs, not flat lists: `nx.json` +
  scattered `project.json`, `turbo.json` + workspace globs, or `bazel query
  //...`. Heavier; niche.

Files: `rules.rs` (one provider arm each) + a small parser module per tool;
`default_config.yaml`.

---

## Done

- **#1** Fatten `default_config.yaml` — 2026-08-31
- **#2** VCS row badge — 2026-08-31 (PR #1)
- **#3** Folder picker for roots — 2026-08-31 (PR #1)
- **#4** Start at login — 2026-08-31 (PR #1)
- **#5** `collapse_nested` toggle (`true` / `false` / `auto`) — 2026-09-01
- **#8** per-repo rule trace in Settings ("Trace a repo") — 2026-09-01
- **#7** `dotnet` provider — `.sln` / `.slnx` / lone `.??proj` → build/run/test — 2026-09-01
- **go-work / maven-modules / gradle-modules** providers (#14's cheap tier) — 2026-09-01
- **Sub-project discovery generalized** — `has_any_marker()` used to hardcode
  5 ecosystems (`.sln`, package.json, Cargo.toml, go.mod, Python); the other 13
  rule-based ones (compose, Maven, Gradle ×2, CMake, Bundler, Mix, Composer,
  Deno, Nix ×2, Docker, Eclipse) only ever worked at a repo root, invisible as
  a sub-project. Now checks `proj.files` against the same `discovery_globs`
  scan.rs uses to find repos, so any rule — present or future — is
  automatically enough. — 2026-09-01
- **Flutter/Dart support** — `pubspec.yaml` rule (pub get/run/build/test), plus
  `flutter-android` reaching into `android/` for the Gradle project Flutter
  generates there (one level below the project root, invisible to
  `gradle-modules`); uses the bundled `gradlew` wrapper when present. — 2026-09-01
- **Python provider fleshed out** — was just pip/uv/poetry install. Now:
  venv-aware interpreter (`.venv`/`venv` → `Scripts/python.exe`), Django
  (`manage.py` → runserver/migrate/test), pytest (`conftest.py`/`pytest.ini`/
  `tox.ini`/`tests/`), run entry (`main.py`/`app.py`/`__main__.py`), pipenv +
  pdm runners, and more markers (`setup.py`, `setup.cfg`, `Pipfile`,
  `manage.py`) so non-`requirements.txt` projects get detected. — 2026-09-01

Remaining, hardest-first: #6 task-targets provider, #9 `prompt:` action, #14
(Cargo-workspace / Xcode / Nx-Turbo-Bazel), #10 Linux terminal picker, #11 fs
watcher, #12 Wayland hotkey, #13 Eclipse provisioner.

## Not on this list (shipped alongside)

Release pipeline (CalVer + GitHub Actions + signed auto-update), `config.yaml` /
`rules.yaml` split, per-action icons + Settings icon browser, proactive update
notifications (launch + daily check, footer chip, tray tooltip, system
notification), CI workflow + `main` branch protection, empty-state guidance,
mouse back/forward navigation, hover/scroll fix.
