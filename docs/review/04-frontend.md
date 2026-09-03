# Code Review — Pass 4: Frontend

**Scope:** `App.svelte`, `src/lib/components/*`, `hotkeys.ts`, `fuzzy.ts`, `icons.ts`, `text.ts`, `updateStore.svelte.ts`, `updater.ts`
**Effort:** `max` (local)
**Basis:** `main...HEAD` diff. Reviewer also ran `svelte-check` (clean) and `vitest` (fails — see #1).
**Date:** 2026-09-01

Tick each item as you address it. Severity: 🔴 high · 🟡 medium · ⚪ low.
Resolved items are collapsed to a one-line stub with the commit that closed them; `git show <hash>` for the detail.

---

## 🔴 High

### [x] 1. `npm test` does not run at all — the new suites execute zero tests and exit 1 — `7bd4b5d`

### [x] 2. Background repo rescan in `>` app scope yanks the selection to the top mid-navigation — `f57947c`

### [x] 3. The main "Save" button silently discards an uncommitted manual hotkey entry — `42a4570` (manual entry removed; recorder always commits)

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

### [x] 7. `HotkeyRecorder` "turn off" button is not `disabled={busy}` — `42a4570`

### [ ] 8. `AppList` selection-scroll `$effect` is a third verbatim copy
[AppList.svelte:25](../../src/lib/components/AppList.svelte#L25)

The ~10-line `skipScroll`/`scrollIntoView` effect (its own comment says "see the note in ResultList") is now duplicated across `AppList.svelte`, `ResultList.svelte`, and `ActionMenu.svelte`. Any fix to the hover/scroll feedback-loop logic must be made in three places.

**Fix:** extract a Svelte action — `use:scrollFollow={selected}` — or a shared helper.

---

## Cross-pass note — the hotkey-save subsystem

All four findings that touched the `save_config` / hotkey flow (P1 #1, P2 #2, P2 #5, P4 #3) were redesigned together in **`42a4570`**: parse+validate both accelerators → diff by parsed `Shortcut` → `save_user` → apply register/unregister only on success → roll back on failure; the frontend's manual-entry path was dropped so the recorder always commits.
