# Code Review — Pass 1: Rules / Config Engine

**Scope:** `rules.rs`, `config.rs`, `scan.rs`, `cache.rs`, `index.rs`, `default_config.yaml` (plus `commands.rs` where the engine is driven)
**Effort:** `max` (local)
**Basis:** `main..dev` diff, 26 commits
**Date:** 2026-09-01

Tick each item as you address it. Severity: 🔴 high (wrong behaviour / silent breakage) · 🟡 medium (feature silently dead / misleading) · ⚪ low (cleanup / perf / docs).
Resolved items are collapsed to a one-line stub with the commit that closed them; `git show <hash>` for the detail.

---

## 🔴 High

### [x] 1. Saving unrelated settings kills the default app-launcher hotkey until restart — `42a4570`

### [x] 2. `shell_wrap` breaks on an absolute `argv[0]` containing a space — `a661a03`

### [ ] 3. Provider action-id collision → the wrong module's command runs
[rules.rs:822](../../../src-tauri/src/rules.rs#L822)

go-work / maven-modules / gradle-modules / flutter-android derive the action-id key from `slug(leaf name)` with **no dedupe guard**. A Maven reactor with `service-a/api` and `service-b/api` (or `go.work` with two `worker` dirs, or gradle `:app:api` + `:lib:api`) produces two actions with the same id `mvnmod:root:api:compile`. `find_action` does `.find(|a| a.id == action_id)` → returns the first, so clicking the second module's row runs the first module's build in the first module's directory. The dotnet provider avoids this with a `used: HashSet` + `key.push('_')`; the four newer providers don't.

**Fix:** lift dotnet's `used` dedupe into a shared helper and apply it in all five providers.

### [x] 4. `terminalize` `{{cmd}}` expansion corrupts argv elements with spaces — `a661a03`

---

## 🟡 Medium

### [ ] 5. Terminal-selector settings are dead on Linux / macOS
[rules.rs:462](../../../src-tauri/src/rules.rs#L462)

The `#[cfg(not(windows))]` branch never consults `resolver.terminal`, `resolver.terminal_template`, or `resolver.shell`. On Linux, `list_terminals` populates the Settings dropdown from `programs.terminal.linux`, the user picks `wezterm`, it's persisted to `config.terminal` — and every terminal action still ignores the pin, the template, and `config.shell` with no error or UI hint.

**Fix:** either wire the non-Windows branch through the same pin/template/shell resolution as Windows, or hide the terminal dropdown on non-Windows until it's supported.

### [ ] 6. An unresolvable `config.terminal` pin is silently discarded
[rules.rs:405](../../../src-tauri/src/rules.rs#L405)

`resolver.terminal.and_then(|t| which(t))` → `None` when the pinned bare name isn't on PATH, and `.or_else(|| resolver.resolve("terminal"))` falls through to the first `programs.terminal` candidate (usually `wt.exe`). The user set `terminal: wezterm`, wezterm isn't on PATH, actions open in Windows Terminal, and nothing says the pin was ignored.

**Fix:** when a non-empty pin fails to resolve, surface it (toast / Settings validation) rather than silently substituting.

### [ ] 7. `flutter-android` rule has no `requires:` gate
[default_config.yaml:273](../../../src-tauri/src/default_config.yaml#L273)

Unlike `gradle-modules` (`requires: [gradle]`), `flutter-android` has no gate. On a checkout whose `android/gradlew.bat` is gitignored/removed and a machine with no global `gradle`, `gradle_cmd` falls back to `"gradle"` and the menu shows `gradle :app:build (android)` actions that fail with a spawn error when clicked.

**Fix:** add `requires: [gradle]` (or gate on the wrapper existing) to the `flutter-android` rule.

---

## ⚪ Low

### [ ] 8. Stale doc comment: `compile_globset` is not shared with `scan.rs`
[config.rs:601](../../../src-tauri/src/config.rs#L601)

The doc comment says "Shared by scan.rs (finding repos) and inspect.rs", but `scan.rs` has its own `build_globset`; only inspect (via `commands.rs`) calls `compile_globset`. The two helpers are near-duplicates that have already diverged (`build_globset` also returns the valid-glob list for index alignment).

**Fix:** correct the comment, and consider collapsing the two helpers into one that optionally returns the valid-glob list.

### [ ] 9. `collapse_nested: auto` re-reads `.gitmodules` once per nested repo
[scan.rs:138](../../../src-tauri/src/scan.rs#L138)

`is_independent_nested` calls `declared_submodules(ancestor)`, which opens and line-parses `<ancestor>/.gitmodules` from disk every time. A monorepo with a 30-entry `.gitmodules` does ~30 redundant reads/parses per scan. Opt-in mode, so bounded, but pure repeated I/O on the scan path.

**Fix:** parse each ancestor's `.gitmodules` once per scan and cache the result in a `HashMap<PathBuf, Vec<..>>`.

### [ ] 10. `trace()` duplicates `matched_files` work and the `Resolver` builder chain
[rules.rs:1165](../../../src-tauri/src/rules.rs#L1165)

`trace()` computes `matched_files` for every (rule × project) then calls `rule_project_actions`, which recomputes `matched_files` (and rebuilds the globset) for the same pair. It also copies the `Resolver::new(...).with_terminal(...).with_shell(...)` chain verbatim from `evaluate()`. For the Settings "Trace a repo" view with N sub-projects × ~25 rules that's ~25×N globsets built and run twice each on the main thread — negligible for typical repos, entirely wasted.

**Fix:** compute `matched_files` once and pass it into `rule_project_actions`; extract the shared `Resolver` builder into one helper used by both `evaluate` and `trace`.

---

## Notes

- `cache.rs` and `index.rs` were in scope but produced no findings this pass.
