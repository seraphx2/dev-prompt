# Code Review — Pass 2: Trust Boundary

**Scope:** `commands.rs`, `lib.rs`, `launch.rs`, `src/lib/ipc.ts`, `src/lib/types.ts` (Tauri command surface, IPC contract, process spawning)
**Effort:** `max` (local)
**Basis:** `main..dev` diff
**Date:** 2026-09-01

Tick each item as you address it. Severity: 🔴 high · 🟡 medium · ⚪ low.

---

## 🔴 High

### [ ] 1. `cmd /c` quote-stripping corrupts app launches when an argument also needs quoting
[launch.rs:58](../../src-tauri/src/launch.rs#L58)

`spawn_detached` builds `cmd /c "C:\Program Files\Foo\foo.exe" --data "C:\Some Path"`. `cmd` sees more than two quote characters on the line and applies its fallback rule — strip the first and last quote — then tries to execute `C:\Program`. The app fails to start. App discovery already yields fully resolved absolute paths, so the `cmd /c` PATHEXT shim buys nothing here.

**Fix:** spawn `entry.exec` directly (with its arg vector) instead of routing through `cmd /c`. Keep `cmd /c` only for the cases that genuinely need PATHEXT resolution, if any remain.

### [ ] 2. Hotkey (un)registration compares raw accelerator strings instead of parsed shortcuts
[commands.rs:388](../../src-tauri/src/commands.rs#L388)

`save_config` decides register/unregister with `new_hotkey != old_hotkey` on the raw strings. Stored `CmdOrCtrl+Shift+Space`, user types the equivalent `Shift+CmdOrCtrl+Space` → strings differ → `register(new)` (plugin parses to the same `Shortcut`) then `unregister(old_hotkey)` tears that same OS shortcut back down. Result: no working hotkey until restart, or a misleading "already in use by another app" error for the user's own key. Same raw-string compare for `apps_hotkey` at [commands.rs:392](../../src-tauri/src/commands.rs#L392) / [commands.rs:397](../../src-tauri/src/commands.rs#L397).

**Fix:** compare with the existing `shortcut_is` helper (parsed-`Shortcut` equality), not `!=` on strings.

### [ ] 3. `run_command` runs repo-token substitution over free-form typed commands
[commands.rs:504](../../src-tauri/src/commands.rs#L504)

Typed commands go through `launch::launch`, which runs `{{path}}` / `{{dir}}` / `{{file}}` / `{{name}}` substitution over every arg and then drops empty args. Type `mytool --template {{file}}` → `{{file}}` becomes `""` → `.filter(|a| !a.is_empty())` deletes the arg → `mytool` gets `--template` with no value. `{{name}}` / `{{dir}}` / `{{path}}` in a typed command are silently rewritten to repo values rather than passed literally.

**Fix:** for `run_command`, spawn the parsed argv directly without the `{{token}}` pass (or escape/opt-out). Token expansion belongs only to rule-derived actions.

---

## 🟡 Medium

### [ ] 4. `list_shells` returns bare `"bash"` that the launch path can't resolve
[commands.rs:452](../../src-tauri/src/commands.rs#L452)

The block special-cases "Git installed but `bash` not on PATH" by locating git-bash at an absolute path — then pushes the bare name `"bash"` and discards the path. User picks it → `config.shell = "bash"` → `shell_wrap` emits `["bash","-c",...]` → `wt.exe -d <cwd> bash -c ...` → Windows Terminal can't find `bash` on PATH → command-failed tab. The one case the special-casing exists for is the one it breaks.

**Fix:** carry the resolved absolute path (store it as the shell value, or return a `{label, path}` pair) so downstream spawning can use it.

### [ ] 5. Live shortcut registration is mutated before the config is persisted
[commands.rs:389](../../src-tauri/src/commands.rs#L389)

Doc comment claims "a bad accelerator is rejected before anything is persisted." Actual order: `register(&new_hotkey)?` + `unregister(old_hotkey)`, *then* `register(apps_hotkey)?`. If the apps-hotkey register fails (claimed by another app), the function returns `Err` having already live-changed the repo hotkey, and `save_user` / `config::load` never run — running app and `config.yaml` now disagree until restart. Same exposure if `save_user` or `config::load` fails after the register calls.

**Fix:** validate/parse both accelerators up front; only apply register/unregister after `save_user` succeeds, or roll back on failure.

### [ ] 6. Frecency count is bumped before the launch is attempted
[commands.rs:586](../../src-tauri/src/commands.rs#L586)

`crate::usage::bump(&exec)` runs unconditionally, then `apps::launch(&entry)` may return `Err` (exe removed since scan, or the `cmd /c` bug above). The failed entry's count in `app-usage.json` still rises, so a broken/stale app floats up the `>` list on every open.

**Fix:** bump only after `apps::launch` returns `Ok`.

### [ ] 7. `refresh_repo_context` rewrites the entire `repos.json` on every action-menu open
[commands.rs:220](../../src-tauri/src/commands.rs#L220)

`openActions` fires `refreshRepoContext` each time the menu opens. For a repo you're actively editing, `inspect` differs from the cached context → `changed` is true → `cache::save(&repos, &contexts)` serializes the whole map (hundreds of repos, each with file lists, pretty-printed) to disk — a multi-MB write on the async executor thread for a large workspace, on every menu open.

**Fix:** persist just the one changed context (partial write), or debounce/skip the write and let the next full scan flush it.

---

## ⚪ Low

### [ ] 8. `which("pwsh") ? "pwsh" : "powershell"` fallback is now duplicated three times
[commands.rs:479](../../src-tauri/src/commands.rs#L479)

Same pwsh/powershell preference lives here plus twice in `rules::shell_wrap` / `terminalize`. Changing the fallback needs three synchronized edits.

**Fix:** extract `default_shell()` in `rules.rs`; call it from `run_command`, `shell_wrap`, `terminalize`.

---

## Notes — checked and clean

- serde shapes between `types.ts` and the Rust structs (`AppEntry`, `AppsPayload`/`AppListPayload`, `TerminalOption`, `RepoTrace`, `ConfigPatch`, `AppConfig`) all line up, including snake_case `AppConfig`/`scan` fields and the `CollapseNested` bool/`"auto"` custom (de)serialize.
- `overlay:shown` payload migration from `()` to a scope string is fully propagated (only `onOverlayShown` consumes it).
- `overlay:hidden` is not emitted while the settings screen is open (Escape there routes to `backToList`, not `hideOverlay`), so the new emit in `hide_overlay` won't discard unsaved settings.
- `context_for` releases its `if let` lock guard before taking the `state.config` lock — no deadlock.
