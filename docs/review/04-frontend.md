# Code Review — Pass 4: Frontend

**Scope:** `App.svelte`, `src/lib/components/*`, `hotkeys.ts`, `fuzzy.ts`, `icons.ts`, `text.ts`, `updateStore.svelte.ts`, `updater.ts`
**Effort:** `max` (local)
**Basis:** `main...HEAD` diff. Reviewer also ran `svelte-check` (clean) and `vitest` (fails — see #1).
**Date:** 2026-09-01

Tick each item as you address it. Severity: 🔴 high · 🟡 medium · ⚪ low.

---

## 🔴 High

### [ ] 1. `npm test` does not run at all — the new suites execute zero tests and exit 1
[vite.config.ts:36](../../vite.config.ts#L36)

`vitest@4.1.11` fails to load every test file: `src/lib/fuzzy.test.ts` and `src/lib/hotkeys.test.ts` both report `(0 test)` with `TypeError: Cannot read properties of undefined (reading 'config')` at the first `describe()`; `vitest run` exits 1. Reproduced with an isolated minimal config and a trivial inline test, so vitest 4.1.11 itself is non-functional here (Windows + Node 22.22, no `engines` pin). The diff also adds a `Frontend unit tests: npm test` step to `.github/workflows/ci.yml`, so the next `dev→main` PR's CI fails at that step — and the commit "Add test coverage for the app launcher + hotkey helpers" currently delivers **no** coverage for `fuzzy.ts` / `hotkeys.ts`.

**Fix:** pin `vitest` (and `@vitest/*`) to a version that works on this Node/OS — try the 2.x or 3.x line — and add an `engines.node` pin to `package.json`. Verify `npm test` is green locally and in CI before the next PR. Overlaps pass 5 (CI).

### [ ] 2. Background repo rescan in `>` app scope yanks the selection to the top mid-navigation
[App.svelte:546](../../src/App.svelte#L546)

Open the launcher when the repo cache is older than `cache_ttl_secs` (default 900s). `onOverlayShown` sets `query='>'` and `loadInitial()` starts a background `rescan()`. Arrow down to app #6 (`selected=5`). When `rescan()` finishes it calls `refresh()` directly *and* emits `repos:updated → onReposUpdated → refresh()`; `refresh()` runs `searchRepos('>chr')`, which matches no repos (`>` can't occur in Windows paths/names), sets `results=[]`, and runs `selected = Math.max(0, results.length - 1) = 0`. The app highlight jumps to row 0; an **Enter in that ~1s window launches the wrong app**. The `query` `$effect` guards this with `if (appScope) return`, but `onReposUpdated`, `rescan()`, and `loadInitial()` all call `refresh()` without that guard.

**Fix:** guard every `refresh()` caller (not just the `$effect`) with `if (appScope) return`, or route app-scope refreshes through a dedicated `refreshApps()` that never touches `results`/`selected` for the repo list.

### [ ] 3. The main "Save" button silently discards an uncommitted manual hotkey entry
[Settings.svelte:279](../../src/lib/components/Settings.svelte#L279)

Open Settings → "type it manually" → type `CmdOrCtrl+Shift+K` → click the top-right "Save". `persist()` (which replaced `save()`) omits `hotkey`/`apps_hotkey` from the `saveConfig` payload — the old `save()` sent `hotkey: hotkey.trim()`. `HotkeyRecorder`'s manual input binds only to its local `typed` state; nothing propagates to the parent unless the user presses Enter or the adjacent "Set". Result: "Saved." is shown, everything else persists, the hotkey edit is lost with no indication.

**Fix:** either have `persist()` include the current effective `hotkey`/`apps_hotkey`, or have `HotkeyRecorder` propagate `typed` to the parent on change (and have the parent Save flush it). Part of the hotkey-save cluster — see cross-pass note.

---

## 🟡 Medium

### [ ] 4. `onWindowMouse` has no `run-command` branch — side-buttons abandon the typed command
[App.svelte:500](../../src/App.svelte#L500)

`onWindowMouse` checks only settings / action-menu / appScope. In run-command mode, mouse button 4 (forward) falls through to the final `else` → `if (results[selected]) openActions(results[selected])` — switches to the action menu for whichever repo is selected in the hidden repo list (not necessarily `activeRepo`), abandoning the `RunCommand` input. Button 3 (back) calls `hideOverlay()` instead of returning to the action menu. `onWindowKeydown` got an explicit `mode === 'run-command'` branch in this same diff; `onWindowMouse` didn't.

**Fix:** add a `mode === 'run-command'` branch to `onWindowMouse` mirroring the keydown handler — button 3 → back to action menu, button 4 → no-op (or run).

### [ ] 5. `RunCommand` shell `<select>` has no fallback option for a configured-but-missing shell
[RunCommand.svelte:81](../../src/lib/components/RunCommand.svelte#L81)

Set `shell: nu` while `nu` isn't on PATH, open "Run command…". `onMount` sets `shellSel='nu'` but no `<option value="nu">` is rendered, so the browser displays the first option ("default") while `shellSel` stays `'nu'` and is passed to `run_command`. The command runs in `nu` as configured, but the UI reads "default". `Settings.svelte`'s equivalent select handles this with `{#if shellSel && !shells.includes(shellSel)}<option>`.

**Fix:** copy the `Settings.svelte` fallback-`<option>` pattern into `RunCommand.svelte`.

### [ ] 6. `apps.enabled` is never read by the frontend
[App.svelte:301](../../src/App.svelte#L301)

With `apps.enabled: false`: `onMount` still calls `loadApps()` unconditionally; an empty app cache triggers `rescanAppsNow()` → `rescan_apps` IPC + `apps.json` write + `apps:updated` event on every launch, with a "Scanning for apps…" flash. And `appScope = query.startsWith('>')` isn't gated on the flag, so typing `>` swaps the repo list for an always-empty `AppList` ("No apps match.") instead of falling through to repo search.

**Fix:** read `apps.enabled` into frontend state; skip `loadApps()`/rescan when false, and gate `appScope` on it so `>` falls through to normal search.

---

## ⚪ Low

### [ ] 7. `HotkeyRecorder` "turn off" button is not `disabled={busy}`
[HotkeyRecorder.svelte:98](../../src/lib/components/HotkeyRecorder.svelte#L98)

While a save is in flight (`busy=true`), the `{#if clearable && value}` "turn off" button stays clickable and fires `onsave('') → persistHotkey({apps_hotkey:''}) → ` a second overlapping `saveConfig()`. Backend mutexes serialize them so no corruption, but the two races on `load_user()`/state and the "Hotkey saved." feedback can reflect the wrong one. Sibling "Set" / "Use it anyway" buttons are already `disabled={busy}`.

**Fix:** add `disabled={busy}` to the "turn off" button.

### [ ] 8. `AppList` selection-scroll `$effect` is a third verbatim copy
[AppList.svelte:25](../../src/lib/components/AppList.svelte#L25)

The ~10-line `skipScroll`/`scrollIntoView` effect (its own comment says "see the note in ResultList") is now duplicated across `AppList.svelte`, `ResultList.svelte`, and `ActionMenu.svelte`. Any fix to the hover/scroll feedback-loop logic must be made in three places.

**Fix:** extract a Svelte action — `use:scrollFollow={selected}` — or a shared helper.

---

## Cross-pass note — the hotkey-save subsystem needs one coherent pass

Four findings across the review touch the same `save_config` / hotkey flow, and fixing them piecemeal will leave gaps:

| Pass | Finding | Symptom |
|---|---|---|
| 1 | [#1](01-rules-config-engine.md) — `commands.rs:367` | `new_apps_hotkey` missing fallback → Save kills the default hotkey |
| 2 | [#2](02-trust-boundary.md) — `commands.rs:388` | raw-string `!=` instead of `shortcut_is` → equivalent spelling tears down the key |
| 2 | [#5](02-trust-boundary.md) — `commands.rs:389` | register/unregister before persist → app & config disagree on failure |
| 4 | #3 — `Settings.svelte:279` | main Save omits `hotkey`/`apps_hotkey` → uncommitted manual entry lost |

Recommend redesigning the save path as: parse+validate both accelerators → diff by parsed `Shortcut` → `save_user` → apply register/unregister only on success → roll back on failure. The frontend should always send the effective hotkey values (or the recorder should propagate on change).
