# Code Review — Summary & Work Plan

**Reviewed:** `main..dev` (26 commits, ~10.3k LoC source) across 5 passes at `max` effort, local, 2026-09-01.
**Raw findings:** 41 (10 + 8 + 12 + 8 + 3) — see `01`–`05`.
**Consolidated:** 16 work items + 1 cleanup batch. The 41 collapse because several are one root cause seen from different call sites.

**Progress:** Tier 1 (WI-1…WI-6) complete, plus WI-16 (CI on `dev` pushes) and Tier-3 P4 #7. Tier 2 (WI-7…WI-15) and the rest of the Tier-3 batch open. Resolved findings in `01`–`05` are collapsed to one-line stubs with their commit hash.

Per-pass detail:
[01 — rules/config engine](01-rules-config-engine.md) ·
[02 — trust boundary](02-trust-boundary.md) ·
[03 — app launcher + providers](03-app-launcher-providers.md) ·
[04 — frontend](04-frontend.md) ·
[05 — CI / release](05-ci-release.md)

---

## Tier 1 — complete

| # | Work item | Findings | Commit(s) |
|---|---|---|---|
| **WI-1** | `npm test` runs zero tests, exits 1 | P4 #1, P5 #1, P5 #2 | `7bd4b5d` |
| **WI-2** | Hotkey-save path — 4 defects, one redesign | P1 #1, P2 #2, P2 #5, P4 #3 (+ P4 #7) | `42a4570` |
| **WI-3** | Windows arg splitting/quoting | P1 #2, P1 #4, P2 #1, P3 #7 | `e743bad`, `a661a03` |
| **WI-4** | `app-usage.json` data-loss race + bump-before-launch | P3 #2, P2 #6 | `5b09b87` |
| **WI-5** | `dedupe_by_product` drops side-by-side installs | P3 #1 | `e26e1f4` |
| **WI-6** | Wrong-app-launch race — rescan yanks selection to row 0 | P4 #2 | `f57947c` |
| **WI-16** | No CI on direct `dev` pushes between PRs | P5 #3 | `ci-dev.yml` — separate workflow, composite `.github/actions/ci`, doc-only pushes skipped |

## Tier 2 — should fix, not release-blocking

Each is an edge case: a specific project layout, config value, or input triggers
it. None is "the app is broken." Ordered as in the table; trigger / symptom /
fix per item.

### WI-7 — provider action-id collisions
- **Trigger:** a monorepo with two sub-modules sharing a leaf name — Maven `service-a/api` + `service-b/api`, Gradle `:app:api` + `:lib:api`, `go.work` with two `worker/` dirs.
- **Symptom:** clicking the second module's row silently runs the **first** module's build in the first's directory (`find_action` returns the first id match).
- **Fix:** lift dotnet's `used: HashSet` + `key.push('_')` dedupe into a shared helper; apply in go-work / maven / gradle / flutter-android.
- Source: [P1 #3](01-rules-config-engine.md)

### WI-8 — provider parser fragility
- **Trigger / symptom:**
  - `.slnx` with single-quoted `Path='src/App.csproj'` → dotnet provider emits **nothing** for that solution. Also `SomePath="x" Path="real"` → grabs `x` (substring match on the attr name).
  - `pom.xml` with `<!-- <module>legacy</module> -->` or `<module>` inside `<profile>` → phantom `legacy` action whose terminal `cd dir/legacy` fails.
  - `go.work` `use(` block closing as `\t./b)` instead of a bare `)` → `./b)` pushed as a bogus module; every following line consumed to EOF.
- **Fix:** dotnet — accept both quote styles, match attr on a word boundary. maven — strip `<!-- … -->` spans, ignore `<module>` under `<profile>`. gowork — trim then `ends_with(')')` to close.
- Source: [P3 #3](03-app-launcher-providers.md), [P3 #4](03-app-launcher-providers.md), [P3 #6](03-app-launcher-providers.md)

### WI-9 — spurious empty "Detected · &lt;dir&gt;" groups
- **Trigger:** a repo with a git **submodule** / vendored clone at e.g. `libs/shared` (has `.git`, no language marker); or a leftover `venv/` / `.venv/` dir (project moved away, or a venv beside non-Python code).
- **Symptom:** an empty "Detected · &lt;dir&gt;" group in the action menu — the dir is surfaced as a sub-project but its provider emits no actions.
- **Fix:** `inspect.rs` — exclude VCS markers from the set `has_any_marker` checks; require a real Python marker (`pyproject.toml`, `requirements*.txt`, `setup.py`, `manage.py`, top-level `*.py`), treat `venv/` as corroborating only.
- Source: [P3 #5](03-app-launcher-providers.md), [P3 #8](03-app-launcher-providers.md)

### WI-10 — terminal / shell selector half-wired
- **Trigger / symptom:**
  - Set `terminal: wezterm` in config, wezterm not on PATH → actions silently open in Windows Terminal, no hint the pin was ignored.
  - Pick "bash" as the shell when Git-Bash isn't on PATH (found only at an absolute path) → `config.shell = "bash"` → terminal can't resolve it → command-failed tab. The one case the git-bash special-casing exists for is the one it breaks.
  - `RunCommand` shell `<select>` with a configured-but-missing shell → UI shows "default" while actually passing the missing name.
  - Linux/macOS: the `#[cfg(not(windows))]` `terminalize` branch ignores the pin, template, and `config.shell` entirely (moot until non-Windows is a target).
- **Fix:** surface an unresolvable non-empty pin (toast / Settings validation); carry git-bash's resolved absolute path as the shell value; give `RunCommand` the same fallback `<option>` as `Settings.svelte`; wire the non-Windows branch through pin/template/shell or hide the dropdown there.
- Source: [P1 #5](01-rules-config-engine.md), [P1 #6](01-rules-config-engine.md), [P2 #4](02-trust-boundary.md), [P4 #5](04-frontend.md)

### WI-11 — `apps.enabled` ignored by the frontend
- **Trigger:** set `apps.enabled: false` in Settings.
- **Symptom:** `onMount` still calls `loadApps()`; an empty cache triggers a full `rescan_apps` + `apps.json` write + "Scanning for apps…" flash on every launch. Typing `>` still swaps to an always-empty `AppList` instead of falling through to repo search.
- **Fix:** read the flag into frontend state; skip `loadApps()`/rescan when false; gate `appScope` on it.
- Source: [P4 #6](04-frontend.md)

### WI-12 — persistence write patterns / bloat (not user-visible; "doesn't scale")
- **Trigger:** many repos, or repos with large top-level dirs, or one you're actively editing.
- **Symptom:**
  - `Project.files` is now serialized into `repos.json` (root + up to 12 sub-projects per repo), written pretty every scan, parsed every startup.
  - `refresh_repo_context` fires on **every action-menu open**; if the repo changed on disk since the last scan it rewrites the **entire** `repos.json` (multi-MB for a big workspace) on the async thread.
  - `collapse_nested: auto` re-reads & re-parses each ancestor `.gitmodules` once per nested repo per scan.
- **Fix:** restore `#[serde(skip)]` on `Project.files` (persist only names rule eval matches); partial-write just the one changed context; memoize `.gitmodules` per scan in a `HashMap<PathBuf, …>`.
- Source: [P1 #9](01-rules-config-engine.md), [P2 #7](02-trust-boundary.md), [P3 #9](03-app-launcher-providers.md)

### WI-13 — `flutter-android` rule has no `requires:` gate
- **Trigger:** a Flutter checkout whose `android/gradlew.bat` is gitignored/removed, on a machine with no global `gradle`.
- **Symptom:** `gradle_cmd` falls back to `"gradle"`; the menu shows `gradle :app:build (android)` actions that fail with a spawn error on click.
- **Fix:** add `requires: [gradle]` (or gate on the wrapper existing) to the rule, like `gradle-modules` has.
- Source: [P1 #7](01-rules-config-engine.md)

### WI-14 — `run_command` token-substitutes free-form input
- **Trigger:** type a command containing a literal `{{file}}` / `{{name}}` / `{{path}}` / `{{dir}}` into the Run-command box.
- **Symptom:** the token is rewritten to a repo value, or (when empty) the arg is dropped by `.filter(|a| !a.is_empty())` — `mytool --template {{file}}` → `mytool --template` with no value.
- **Fix:** for `run_command`, spawn the parsed argv directly without the `{{token}}` pass. Expansion belongs only to rule-derived actions.
- Source: [P2 #3](02-trust-boundary.md)

### WI-15 — `onWindowMouse` has no `run-command` branch
- **Trigger:** use the mouse back/forward side-buttons while on the "Run command…" screen.
- **Symptom:** back (button 3) calls `hideOverlay()` instead of returning to the action menu; forward (button 4) falls through to `openActions(results[selected])` — jumps to a random selected repo's action menu, abandoning the typed command.
- **Fix:** mirror the `mode === 'run-command'` branch `onWindowKeydown` already has — button 3 → back to action menu, button 4 → no-op.
- Source: [P4 #4](04-frontend.md)

### Which would actually bite a Windows user day-to-day
- **WI-9** if any of your repos have git submodules or stray `venv/` dirs — visible phantom menu entries.
- **WI-10** if you ever pin a terminal or shell that isn't installed.
- **WI-11** if you ever turn off app indexing.
- **WI-12** only at hundreds of repos.
- The rest need a fairly specific monorepo layout, config, or input you could go months without producing.

## Tier 3 — cleanup batch (do together, low risk)

| Finding | Location | Fix | Status |
|---|---|---|---|
| P1 #8 | `config.rs:601` | correct the stale "shared by scan.rs" doc comment | open |
| P1 #10 | `rules.rs:1165` | `trace()` — compute `matched_files` once; extract the shared `Resolver` builder | open |
| P2 #8 | `commands.rs:479` | extract `default_shell()`; kill the 3 copies of `which("pwsh")` | open |
| P3 #10 | `discover_apps.ps1:42` | dispose `$ms`/`$bmp`/`$ic` in a `finally` | open |
| P3 #11 | `discover_apps.ps1:29` | invalidate the icon PNG cache (key on mtime+size, or clear on app-list refresh) | open |
| P3 #12 | `apps.rs:491` | gate the `C:\` path-literal tests with `#[cfg(windows)]` | open |
| P4 #7 | `HotkeyRecorder.svelte:98` | add `disabled={busy}` to the "turn off" button | done — `42a4570` |
| P4 #8 | `AppList.svelte:25` | extract `use:scrollFollow={selected}` — kill the 3rd copy of the scroll effect | open |

---

## Suggested order (remaining)

1. Tier 2 in listed order; **WI-9 + WI-11** are quick and visible in the action menu.
2. **WI-7** lands cleanly now that WI-3 gave it a correct splitter/quoter; **WI-14** likewise.
3. Tier 3 as one commit.

## Themes worth carrying forward

- **Windows argv handling** was the single most repeated defect — POSIX-style `shell_split` / `join(" ")` on Windows paths. WI-3 added the tested splitter/quoter pair; new call sites should use it.
- **Silent fallbacks** — unresolvable terminal pin, missing shell, dropped hotkey, discarded app install. Several findings are "the feature does nothing and says nothing." Prefer surfacing the failure. (WI-10, WI-11 remain.)
- **New providers skipped guards the dotnet provider already had** (dedupe, `requires`). Worth a checklist for the next provider. (WI-7, WI-13 remain.)
- **`repos.json` is on a lot of hot paths** (every scan, every menu open, every startup) and keeps accreting fields — treat its shape and write frequency as a budget. (WI-12 remains.)
