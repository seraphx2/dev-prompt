# Code Review — Summary & Work Plan

**Reviewed:** `main..dev` (26 commits, ~10.3k LoC source) across 5 passes at `max` effort, local, 2026-09-01.
**Raw findings:** 41 (10 + 8 + 12 + 8 + 3) — see `01`–`05`.
**Consolidated:** 16 work items + 1 cleanup batch. The 41 collapse because several are one root cause seen from different call sites.

Per-pass detail:
[01 — rules/config engine](01-rules-config-engine.md) ·
[02 — trust boundary](02-trust-boundary.md) ·
[03 — app launcher + providers](03-app-launcher-providers.md) ·
[04 — frontend](04-frontend.md) ·
[05 — CI / release](05-ci-release.md)

---

## Tier 1 — fix before the next `dev→main` PR

| # | Work item | Findings | Root fix |
|---|---|---|---|
| **WI-1** | `npm test` runs zero tests, exits 1 — CI step will fail | P4 #1, P5 #1, P5 #2 | `test: { pool: 'vmThreads' }` in `vite.config.ts`; pin `vitest` exact; add `engines.node` |
| **WI-2** | Hotkey-save path — 4 defects, needs one redesign | P1 #1, P2 #2, P2 #5, P4 #3 | parse+validate both accelerators → diff by `Shortcut` (`shortcut_is`) → `save_user` → apply register/unregister only on success → roll back on failure. Frontend always sends effective `hotkey`/`apps_hotkey`. |
| **WI-3** | Windows arg splitting/quoting — breaks `C:\Users\First Last` + `.venv` | P1 #2, P1 #4, P2 #1, P3 #7 | one `CommandLineToArgvW`-style splitter + a proper quoter; substitute `{{cmd}}`/`{{dir}}` tokens *after* splitting, not before; spawn app exes directly instead of via `cmd /c` |
| **WI-4** | `app-usage.json` data-loss race + bump-before-launch | P3 #2, P2 #6 | `Mutex` in shared state + atomic write (temp + rename); bump only after `apps::launch` returns `Ok` |
| **WI-5** | `dedupe_by_product` drops side-by-side installs (Python 3.11/3.12, Chrome Beta, JDKs) | P3 #1 | add a version/path discriminator to the key, or merge instead of discard |
| **WI-6** | Wrong-app-launch race — background rescan yanks selection to row 0 | P4 #2 | guard *every* `refresh()` caller with `if (appScope) return`, or a dedicated `refreshApps()` that never touches repo `results`/`selected` |

## Tier 2 — should fix, not release-blocking

| # | Work item | Findings | Root fix |
|---|---|---|---|
| **WI-7** | Provider action-id collisions — same-named modules run wrong build | P1 #3 | lift dotnet's `used: HashSet` dedupe into a shared helper; apply in go-work / maven / gradle / flutter-android |
| **WI-8** | Provider parser fragility | P3 #3, P3 #4, P3 #6 | dotnet: accept single quotes + word-boundary attr match. maven: strip `<!-- -->`, ignore `<profile>` modules. gowork: close block on trimmed `ends_with(')')` |
| **WI-9** | Spurious empty "Detected · <dir>" groups | P3 #5, P3 #8 | `inspect.rs`: exclude VCS markers from `has_any_marker`; require a real Python marker, not bare `venv/` |
| **WI-10** | Terminal / shell selector half-wired | P1 #5, P1 #6, P2 #4, P4 #5 | wire the non-Windows `terminalize` branch through pin/template/shell; surface an unresolvable pin instead of silently substituting; `list_shells` carries the resolved git-bash path; `RunCommand` gets the fallback `<option>` |
| **WI-11** | `apps.enabled` ignored by frontend | P4 #6 | read the flag; skip `loadApps()`/rescan when false; gate `appScope` on it |
| **WI-12** | Persistence write patterns / bloat | P1 #9, P2 #7, P3 #9 | memoize `.gitmodules` parse per scan; partial-write one changed context instead of the whole `repos.json`; restore `#[serde(skip)]` on `Project.files` |
| **WI-13** | `flutter-android` rule has no `requires:` gate | P1 #7 | add `requires: [gradle]` (or gate on the wrapper existing) |
| **WI-14** | `run_command` token-substitutes free-form input | P2 #3 | spawn the parsed argv directly, no `{{token}}` pass for typed commands |
| **WI-15** | `onWindowMouse` has no `run-command` branch | P4 #4 | mirror the `onWindowKeydown` `run-command` branch — button 3 → back to action menu, button 4 → no-op |
| **WI-16** | No CI on direct `dev` pushes between PRs | P5 #3 | re-add a `push` trigger scoped to `dev` (or a light build+test job), or document the tradeoff in the workflow |

## Tier 3 — cleanup batch (do together, low risk)

| Finding | Location | Fix |
|---|---|---|
| P1 #8 | `config.rs:601` | correct the stale "shared by scan.rs" doc comment |
| P1 #10 | `rules.rs:1165` | `trace()` — compute `matched_files` once; extract the shared `Resolver` builder |
| P2 #8 | `commands.rs:479` | extract `default_shell()`; kill the 3 copies of `which("pwsh")` |
| P3 #10 | `discover_apps.ps1:42` | dispose `$ms`/`$bmp`/`$ic` in a `finally` |
| P3 #11 | `discover_apps.ps1:29` | invalidate the icon PNG cache (key on mtime+size, or clear on app-list refresh) |
| P3 #12 | `apps.rs:491` | gate the `C:\` path-literal tests with `#[cfg(windows)]` |
| P4 #7 | `HotkeyRecorder.svelte:98` | add `disabled={busy}` to the "turn off" button |
| P4 #8 | `AppList.svelte:25` | extract `use:scrollFollow={selected}` — kill the 3rd copy of the scroll effect |

---

## Suggested order

1. **WI-1** first — you can't trust green CI on any of the rest until `npm test` actually runs.
2. **WI-4** and **WI-2** next — data loss and a user-facing regression, both self-contained once you commit to the redesign.
3. **WI-3** — touches the most call sites; do it as one helper + a sweep so WI-7/WI-14 land on top of a correct splitter.
4. **WI-5, WI-6** — finish Tier 1.
5. Tier 2 in table order; **WI-9 + WI-11** are quick and visible in the action menu.
6. Tier 3 as one commit.

## Themes worth carrying forward

- **Windows argv handling** is the single most repeated defect — POSIX-style `shell_split` / `join(" ")` on Windows paths. A tested splitter/quoter pair pays for itself.
- **Silent fallbacks** — unresolvable terminal pin, missing shell, dropped hotkey, discarded app install. Several findings are "the feature does nothing and says nothing." Prefer surfacing the failure.
- **New providers skipped guards the dotnet provider already had** (dedupe, `requires`). Worth a checklist for the next provider.
- **`repos.json` is on a lot of hot paths** (every scan, every menu open, every startup) and keeps accreting fields — treat its shape and write frequency as a budget.
