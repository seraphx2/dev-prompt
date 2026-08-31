# dev-prompt — remaining work

Everything below is discussed-but-not-built, ordered **easiest → hardest**.
The config schema itself (markers / programs / rules / universal, templates,
merge model) is implemented — read `src-tauri/src/default_config.yaml` for the
canonical shape.

---

## 1. Fatten `default_config.yaml`  ·  ✅ DONE (2026-08-31)

Shipped: VCS markers (`.bzr`/`_darcs`/`.fslckout`) + `pnpm-workspace.yaml` +
`*.code-workspace`; programs for cursor/zed/subl/idea/webstorm/pycharm/goland,
fork/gitkraken/smerge/lazygit, and anypoint/eclipse/sts; `requires:`-gated
terminal rules for maven/gradle/cmake/bundler/mix/composer/deno/nix
(flake+shell)/docker-image; `csproj-editors` rule; `go` rule extended to
`go.work`; an `eclipse-project` rule (`.project` / `mule-artifact.json`) that
launches Anypoint/Eclipse/STS with `-data {{path}}`; matching universal
"Open in …" actions. Two parse-level tests in `config.rs`.

Deferred from this pass: `Makefile`/`Rakefile`/`justfile`/`Taskfile.yml`
(→ #6), `mvnw`/`gradlew` wrapper-only repos, per-task list providers (→ #6/#7).

*Original scope, kept for reference:*

Add markers + programs + rules for the ecosystems the parity config skips.

**Plain rules** (reuse existing templates, gate with `requires: [<binary>]`):
- `pom.xml` → `mvn compile|test|package`
- `build.gradle` / `build.gradle.kts` → `gradle build|test` (tasks provider is #6)
- `CMakeLists.txt` → `cmake -B build` / `cmake --build build`
- `Gemfile` → `bundle install`; `Rakefile` → `rake` (targets = #6)
- `mix.exs` → `mix deps.get|compile|test`
- `composer.json` → `composer install`
- `deno.json` / `deno.jsonc` → `deno task` list (provider) or `deno run`
- `flake.nix` / `shell.nix` → `nix develop`
- `Dockerfile` → `docker build -t {{name}} .`

**More `markers`:** `.bzr`, `_darcs`, `.fslckout`, `pnpm-workspace.yaml`,
`*.csproj`, `*.fsproj`, `go.work`, `*.code-workspace`, the build files above.

**More `programs`** (all `needs:`-gated, so only installed ones show):
- editors: `cursor`, `zed`, `subl`, `idea`, `webstorm`, `pycharm`, `goland`
- git GUIs: `fork`, `gitkraken`, `smerge` (Sublime Merge), `lazygit` (terminal)

**More `universal`:** "Open in Cursor / Zed / Sublime", "Open in <git GUI>".

Files: `src-tauri/src/default_config.yaml` only. Add `bundled_defaults()` test
assertions for a couple of the new rules.

---

## 2. `markers.kind: vcs` → row badge  ·  ✅ DONE (2026-08-31, PR #1)

`config::vcs_markers()` + `Marker::kind()`/`label()`; `scan()` splits a repo's
matched hits into `Repo.vcs` (first `kind: vcs` marker's label) vs `sentinels`
(the rest). `.git` is now a `kind: vcs` marker. `ResultRow.svelte` renders
`repo.vcs` as a tinted badge ahead of the plain sentinel chips.

Files: `config.rs`, `scan.rs`, `default_config.yaml`, `types.ts`,
`ResultRow.svelte`.

---

## 3. "Browse…" folder picker for roots  ·  ✅ DONE (2026-08-31, PR #1)

`tauri-plugin-dialog`; `ipc::pickDirectories()` wraps
`open({ directory: true, multiple: true })`; a "Browse…" button beside
"+ Add root" appends the chosen paths (dropping blank rows).

Files: `Cargo.toml`, `lib.rs`, `capabilities/default.json`, `ipc.ts`,
`Settings.svelte`.

---

## 4. Autostart ("start at login")  ·  ✅ DONE (2026-08-31, PR #1)

`tauri-plugin-autostart`; `get_autostart` / `set_autostart` commands wrap
`app.autolaunch()`. A "Start at login" checkbox in `Settings.svelte` applies
immediately (no Save) and reverts on failure. The OS is the source of truth.

Files: `Cargo.toml`, `lib.rs`, `capabilities/default.json`, `commands.rs`,
`ipc.ts`, `Settings.svelte`.

---

## 5. `collapse_nested` toggle  ·  *small Rust*

`scan.rs` always drops a repo whose ancestor is also a repo — monorepos with
independent sub-repos can't opt out.

- `config.rs`: `scan.collapse_nested: bool` (default `true`).
- `scan.rs`: guard the collapse pass on that flag.
- Optionally surface it in the settings viewer / a checkbox.

Files: `config.rs`, `scan.rs`, `default_config.yaml`.

---

## 6. `task-targets` provider  ·  *medium, three small parsers*

Parse target lists and emit one terminal action per target.
- `Makefile`: lines matching `^([A-Za-z0-9_.-]+):(?!=)` minus `.PHONY` etc.
- `justfile`: `just --summary` if `just` on PATH, else parse `^([a-z0-9_-]+)`.
- `Taskfile.yml`: YAML parse, keys under `tasks:`.

Cap at ~20. Register `"task-targets"` in `rules.rs::provider_actions`, then add
the `[Makefile, justfile, Taskfile.yml]` rule to `default_config.yaml`.

Files: `rules.rs` (or a `providers/` split if it grows), `default_config.yaml`.

---

## 7. `dotnet` provider  ·  *medium, one parser*

Parse `.sln` `Project("{GUID}") = "Name", "rel\path.csproj", "{GUID}"` lines
(skip solution folders — the ones whose path isn't a `.csproj`/`.vbproj`/`.fsproj`).
Per project: `dotnet build <proj>`, `dotnet run --project <proj>` (terminal).

Add a rule `{ match: "*.sln", provider: dotnet, requires: [dotnet] }` alongside
the existing editor rule.

Files: `rules.rs`, `default_config.yaml`.

---

## 8. Per-repo "what matched" in the rules viewer  ·  *medium, lower value*

The action menu already shows what a repo produces. A settings-side version
would need a repo dropdown + calling `build_actions` for the chosen repo and
listing rule → action outcomes. Nice for debugging config, not essential.

Files: `commands.rs`, `rules.rs`, `types.ts`, `Settings.svelte`.

---

## 9. `prompt: true` / "Run command…"  ·  *medium-hard, new frontend mode*

A dynamic action: prompt for a one-off command, run it in the terminal at the
repo dir.
- `RuleAction.prompt: bool`; `{{input}}` template var.
- Frontend: a fourth overlay mode (or an inline input on the action row) that
  captures the string, then calls `run_action` with it threaded through.
- Backend: `run_action` needs an optional `input` arg that `expand()` picks up.

Files: `config.rs`, `rules.rs`, `commands.rs`, `App.svelte`, a new component,
`ipc.ts`.

---

## 10. Linux terminal picker  ·  *hard — and untestable without a GUI Linux*

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

## 11. fs watcher / incremental reindex  ·  *hard — new dep, concurrency, cross-platform*

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

## 12. Linux X11/Wayland hotkey hardening  ·  *hardest — partly out of our hands*

`tauri-plugin-global-shortcut` → `global-hotkey` is X11-only on Linux. Wayland
has no global-grab; it needs the `GlobalShortcuts` XDG portal (compositor
support varies).
- Detect session type via `tauri-plugin-os`.
- On Wayland: attempt the portal; if unavailable, surface a clear message and
  fall back (tray-only activation).
- Document the limitation prominently.

Files: `lib.rs`, docs. Needs real X11 + Wayland sessions to validate.

---

## 13. Eclipse / Anypoint project provisioner  ·  *milestone-sized, Eclipse-version-coupled*

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

## Done

- **#1** Fatten `default_config.yaml` — 2026-08-31
- **#2** VCS row badge — 2026-08-31 (PR #1)
- **#3** Folder picker for roots — 2026-08-31 (PR #1)
- **#4** Start at login — 2026-08-31 (PR #1)

Remaining, hardest-first: #6 task-targets provider, #7 dotnet provider, #5
`collapse_nested` toggle, #8 per-repo "what matched", #9 `prompt:` action, #10
Linux terminal picker, #11 fs watcher, #12 Wayland hotkey, #13 Eclipse
provisioner.

## Not on this list (shipped alongside)

Release pipeline (CalVer + GitHub Actions + signed auto-update), `config.yaml` /
`rules.yaml` split, per-action icons + Settings icon browser, proactive update
notifications (launch + daily check, footer chip, tray tooltip, system
notification), CI workflow + `main` branch protection, empty-state guidance,
mouse back/forward navigation, hover/scroll fix.
