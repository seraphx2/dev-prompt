# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

dev-prompt is a cross-platform command-palette overlay (global hotkey → fuzzy
repo list → context-aware action menu) built with **Tauri v2** (Rust backend)
+ **Svelte 5** + **Tailwind CSS** (frontend). It doesn't just open a folder —
it inspects each repo's manifests and offers the actions that project
actually supports (npm scripts through the right package manager, `cargo
test`, a `.sln` opened in Visual Studio, `docker compose up`, drilling into a
`packages/*` sub-project, …).

## Platform support

**Windows and Linux only. macOS is not a target and never will be.**

Do not add anything that only exists to support macOS — this includes (but
isn't limited to):

- `#[cfg(target_os = "macos")]` branches, or any `cfg` scoped to exclude only
  macOS (e.g. `all(unix, not(target_os = "macos"))`) where a plain `unix`
  cfg would do.
- `macos:` candidate lists in `programs` (`default_config.yaml`, `rules.yaml`),
  or a `macos` field on `ProgramSpec`.
- `when: macos` (or `mac` / `darwin`) support in the rule engine.
- macOS bundle/signing/notarization config, or a `macos-latest` leg in CI.
- "macOS status" notes, TODOs, or roadmap items in docs (README, CONTRIBUTING,
  `docs/*.md`) — if a future-work item only matters for macOS, drop it rather
  than track it.

If a change would only be useful for macOS, skip it. If removing dead
Windows/Linux-only logic would also delete a macOS accommodation as a side
effect, that's fine — do it.

The one exception: cross-platform library/plugin APIs (e.g.
`tauri_plugin_autostart`'s `MacosLauncher` enum) that require a value
regardless of target OS. Leave those as-is; they aren't something the project
chose to add for macOS support, and there's no way to omit them without
dropping the dependency itself.

## Commands

Frontend commands run from the repo root; Rust commands run from `src-tauri/`.

```sh
# Setup + dev
npm install
npm run tauri dev                 # Vite on :1420 + the overlay (hidden until the hotkey)

# Frontend checks
npm run check                     # svelte-check (types only)
npm run build                     # svelte-check + vite build (what CI runs)
npm run test                      # vitest run, whole suite
npm run check:tauri               # Tauri npm packages ↔ Rust crates on the same major.minor
npm run screenshots               # regenerate docs/img/ — demo GIF + stills (headless, fake backend — see scripts/screenshots/)
npm run screenshots:wsl           # the same from Windows, under Linux in WSL (fonts match the AppStream shots)
npx vitest run src/lib/fuzzy.test.ts   # a single test file
npx vitest run -t "name substring"     # tests matching a name

# Rust checks (cd src-tauri first)
cargo check
cargo test                        # whole suite, colocated `#[cfg(test)] mod tests` per file
cargo test settings_file_carries_filemanager   # a single test (substring match)
cargo clippy --all-targets -- -D warnings      # required clean before merge

# Full pre-merge check, mirrors .github/workflows/ci.yml
npm run check:tauri && npm run build && cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test
```

Every GitHub Action in `.github` is pinned to a full commit SHA with its
version in a comment (`@<sha> # v7.0.1`), because the release job holds the
updater signing key; Dependabot bumps both, including under `.github/actions`.
Pin any new action the same way. `dtolnay/rust-toolchain` tracks a branch, so
its pin is bumped by hand. Dependabot updates npm and Cargo in separate
groups, so a `@tauri-apps/*` package and its Rust crate can drift a minor
apart — which `tauri build` (release only) refuses; `check:tauri` catches it
in PR CI. Fix with `cargo update -p <crate>` or the matching npm bump. `sync-dev.yml` fast-forwards `dev` to `main` after
every push to `main`, so the PR's merge commit never leaves `dev` "out of
date"; it leaves `dev` alone if it has newer commits.

The Rust code is **hand-formatted, not `rustfmt`-shaped** — match the
surrounding style rather than running `cargo fmt`.

## Architecture

### Request flow, end to end

1. **`scan.rs`** walks the configured root directories → `Vec<Repo>`.
   Discovery is driven by `markers` (`.git`, `*.sln`, `package.json`, …) and
   every rule's own `match` glob; nested repos collapse per `scan.collapse_nested`.
   With `scan.top_level_folders`, every folder directly in a root is listed
   too, marker or not, unless it only groups other repos.
2. **`cache.rs`** persists that list to `repos.json` for instant startup; a
   stale cache triggers a background rescan rather than blocking the overlay.
   **`watch.rs`** watches the roots while the app runs and rescans
   (debounced) when a marker or a repo folder comes or goes — it drops
   everything below `.git` / `node_modules` / … so git churn costs nothing.
   On Linux it holds one inotify watch per folder down to the scan depth,
   adding watches as folders appear rather than rebuilding. A sync whose
   roots / depth / markers are unchanged keeps the running watcher (the app
   watcher likewise keeps its sources), so autosaves cost nothing there.
   **`app_watch.rs`** does the same for the `>` app list: Start Menu /
   `%LOCALAPPDATA%\Programs` / `extra_dirs` folders plus the Uninstall and
   Store registry keys (`RegNotifyChangeKeyValue`), or the XDG application
   folders on Linux. `apps::update` re-gathers only the origins a change
   touched (every row in `apps::Index` carries one) and must stay equal to a
   full gather — the end-to-end tests in `app_watch.rs` check exactly that,
   against real OS events in throwaway folders and registry keys.
   **`removal.rs`** is "Move to Recycle Bin": never a permanent delete
   (`trash` crate; Windows keeps its own nuke warning), paths re-validated in
   the backend (listed, strictly inside a root, not a link), and an
   assessment per VCS of what exists only locally — anything it can't check
   counts as at risk, never as clean.
   **`vcs_state.rs`** is that per-VCS reading (`RepoStatus`: branch, upstream,
   ahead/behind, changed/untracked/conflicted, stashes, unpushed, an operation
   in progress, jj's conflicted/divergent changes), offline, for Git, jj,
   Mercurial, Subversion, Fossil, Breezy and Darcs; shared by removal and the
   repo list's status flags. Reads must never write a file the watcher
   follows (Git's set `GIT_OPTIONAL_LOCKS=0`). `limits` holds what a system
   can't tell offline — removal counts it as unknown, the row doesn't flag it.
   **`status.rs`** keeps every repo's status current on 3 background threads
   — asked for by the watcher (a working-tree file or one of the VCS state
   files `watch.rs`'s `vcs_state_file` allows, debounced per repo), by the
   overlay opening (anything older than `SWEEP_AGE`), and by a menu opening —
   and persists it to `status.json`. **`vcs_ops.rs`** dispatches the branch
   switcher, second working copies (worktree / workspace / share / checkout)
   and remote steps to `git_ops.rs`, `jj_ops.rs`, `hg_ops.rs`, `svn_ops.rs`,
   `fossil_ops.rs`, `brz_ops.rs`, and refuses a switch while something's
   half-done or where it would merge uncommitted changes. The line it holds:
   one keystroke, fully described by its label, never forced — anything
   needing review (staging, commits, conflicts) hands off to a GUI. Breezy on
   Windows resolves relative file arguments against the drive root: pass it
   absolute paths. See `docs/vcs-status.md`; `scripts/vcs-scenarios.sh <root>
   [systems]` builds a test repo per state (`--remove` deletes them), and `DP_SCENARIO_ROOT=<root> cargo
   test scenario_report -- --ignored --nocapture` prints what each reads as.
3. On selecting a repo, **`inspect.rs`** walks its tree once into a
   `RepoContext` — root plus side-by-side / `packages/*`-style sub-projects,
   each with its manifest details (npm scripts + package manager, Cargo.toml,
   `.sln`/`.csproj`, `go.mod`/`go.work`, Python runner, compose files, …).
4. **`rules.rs`** evaluates `RepoContext` against the merged config
   (`default_config.yaml` + `rules.yaml`) into the actual `Action` list. This
   is deliberately **two-phase**: `evaluate_universal` (terminal, file
   manager, AI-CLI launchers, …) needs no `PATH` walk and renders instantly;
   `evaluate_detected` (per-ecosystem, manifest-driven actions) walks `PATH`
   for each rule's `requires:`/`needs:` and is only fast after the first hit
   per process (memoized). `commands.rs` exposes this split as
   `build_universal_actions` (sync) then `build_detected_actions` (async) —
   preserve the split when touching this path; don't make universal actions
   pay for a `PATH` walk.
5. **`commands.rs`** is the `#[tauri::command]` surface and owns `AppState`
   (loaded config, scanned repos, per-repo contexts, installed-apps cache).
   **`pty.rs`** backs "Open shell": the user's shell in a pseudo-terminal
   (ConPTY / Unix PTY via `portable-pty`), output streamed over a Tauri
   `Channel` to xterm.js (`lib/shells.svelte.ts`). Sessions outlive the
   screen and the overlay hide; ending one kills the shell and hangs up the
   PTY, like closing a terminal tab. On Windows, ConPTY holds the program
   until the terminal answers its opening cursor query — xterm.js does that
   in the app; the tests in `pty.rs` answer it themselves.
6. The Svelte frontend (`App.svelte` + `lib/ipc.ts` typed wrappers) renders
   repo list → action menu → settings and drives everything through
   `invoke()`
   Settings lives in `lib/components/settings/`: a sidebar shell that owns
   the form and autosaves (`save(patch, delay)` merges and serialises
   partial `save_config` patches; `rescanNeeds` decides what a save
   rescans), plus one component per page. There's no Save button — a new
   field saves its own slice of the form via a patch builder in `model.ts`.

### Config layering — three files, precise merge semantics

- **`default_config.yaml`** — bundled into the binary at compile time
  (`include_str!`), the canonical schema (inline-documented). Never
  user-edited.
- **`config.yaml`** — user settings (hotkey, roots, scan, `terminal`/`shell`/
  `filemanager` picks, apps launcher). Owned entirely by the Settings screen.
- **`rules.yaml`** — hand-authored rule-engine overrides: `markers` append,
  `programs` merge **by key** (a key you set fully replaces the built-in
  candidate list for that key, it isn't merged item-by-item —
  `cfg.programs.insert(k, v)` in `config.rs`), `rules` prepend, `universal`
  supports `.add` / `.disable`, `provision` merges by `id` (plus
  `provision_disable`). The Settings screen never rewrites this file,
  so hand-written comments survive.
- `merge_settings()` applies `config.yaml`; `merge_overrides()` applies
  `rules.yaml` — both in `config.rs`, both scalar-present-wins / list-append
  semantics as appropriate per field. See `docs/rules-engine.md` for the
  user-facing version of this.

### Program resolution (`Resolver` in `rules.rs`)

- A `programs.<key>` entry lists `any` (tried on every OS, PATH lookup) plus
  per-OS candidates (bare name → PATH, absolute path, or a glob) — first that
  resolves wins. Resolution is memoized process-wide (`program_cache()` /
  `which_cache()`), invalidated by `clear_program_cache()` on every config
  save.
- **Terminal** and **File manager** both follow the same three-tier pattern:
  Auto (first resolving candidate) → pin a specific detected one
  (`config.terminal` / `config.filemanager`) → a raw template escape hatch
  (`terminal_template` / `filemanager_template`, `{{dir}}`/`{{cmd}}` or
  `{{path}}`) for anything the built-in invocation table doesn't know.
  `terminalize()` holds the actual per-emulator invocation table (`TermKind`:
  Windows Terminal / Alacritty / WezTerm are known; anything else gets a
  best-effort raw-argv-plus-cwd fallback). On Windows, `Auto` also honors an
  explicit "Console Host" choice in the OS's own default-terminal registry
  setting (`HKCU\Console\%%Startup`) rather than force-opening `wt.exe`
  against it.
- File manager has no per-program invocation table the way Terminal does —
  "accepts a bare path argument" is treated as the near-universal contract,
  so every resolving candidate is offered without a known-invocation gate.

### Gating — the core invariant

- `requires: [bin]` on a **rule** hides it unless `bin` is on `PATH`.
- `needs: [key]` on an **action** hides it unless that `programs` key resolves.
- **Nothing ever errors when a tool is missing — it just doesn't appear.**
  Preserve this when adding rules or actions; don't add error paths for an
  absent tool.

### Tests

- Rust: colocated `#[cfg(test)] mod tests` per file, run with `cargo test`
  from `src-tauri/`. `config.rs`'s `bundled_defaults()`-based tests assert the
  embedded `default_config.yaml` parses and specific programs/rules exist —
  add an assertion there when adding a rule (see `CONTRIBUTING.md`).
- Frontend: Vitest, files under `src/**/*.test.ts`. The Vite config pins
  `pool: "vmThreads"` — the default pool crashes when the cwd resolves with a
  lowercase Windows drive letter.

### Adding support for a new tool

Usually pure data in `src-tauri/src/default_config.yaml` (a `markers` entry,
a `programs` candidate list, a `rules` entry with `match`/`actions`) — no Rust
required. Full field reference in `docs/rules-engine.md`.
