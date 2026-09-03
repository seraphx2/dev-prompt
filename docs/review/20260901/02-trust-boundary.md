# Code Review — Pass 2: Trust Boundary

**Scope:** `commands.rs`, `lib.rs`, `launch.rs`, `src/lib/ipc.ts`, `src/lib/types.ts` (Tauri command surface, IPC contract, process spawning)
**Effort:** `max` (local)
**Basis:** `main..dev` diff
**Date:** 2026-09-01

Tick each item as you address it. Severity: 🔴 high · 🟡 medium · ⚪ low.
Resolved items are collapsed to a one-line stub with the commit that closed them; `git show <hash>` for the detail.

---

## 🔴 High

### [x] 1. `cmd /c` quote-stripping corrupts app launches when an argument also needs quoting — `e743bad`

### [x] 2. Hotkey (un)registration compares raw accelerator strings instead of parsed shortcuts — `42a4570`

### [ ] 3. `run_command` runs repo-token substitution over free-form typed commands
[commands.rs:504](../../../src-tauri/src/commands.rs#L504)

Typed commands go through `launch::launch`, which runs `{{path}}` / `{{dir}}` / `{{file}}` / `{{name}}` substitution over every arg and then drops empty args. Type `mytool --template {{file}}` → `{{file}}` becomes `""` → `.filter(|a| !a.is_empty())` deletes the arg → `mytool` gets `--template` with no value. `{{name}}` / `{{dir}}` / `{{path}}` in a typed command are silently rewritten to repo values rather than passed literally.

**Fix:** for `run_command`, spawn the parsed argv directly without the `{{token}}` pass (or escape/opt-out). Token expansion belongs only to rule-derived actions.

---

## 🟡 Medium

### [ ] 4. `list_shells` returns bare `"bash"` that the launch path can't resolve
[commands.rs:452](../../../src-tauri/src/commands.rs#L452)

The block special-cases "Git installed but `bash` not on PATH" by locating git-bash at an absolute path — then pushes the bare name `"bash"` and discards the path. User picks it → `config.shell = "bash"` → `shell_wrap` emits `["bash","-c",...]` → `wt.exe -d <cwd> bash -c ...` → Windows Terminal can't find `bash` on PATH → command-failed tab. The one case the special-casing exists for is the one it breaks.

**Fix:** carry the resolved absolute path (store it as the shell value, or return a `{label, path}` pair) so downstream spawning can use it.

### [x] 5. Live shortcut registration is mutated before the config is persisted — `42a4570`

### [x] 6. Frecency count is bumped before the launch is attempted — `5b09b87`

### [ ] 7. `refresh_repo_context` rewrites the entire `repos.json` on every action-menu open
[commands.rs:220](../../../src-tauri/src/commands.rs#L220)

`openActions` fires `refreshRepoContext` each time the menu opens. For a repo you're actively editing, `inspect` differs from the cached context → `changed` is true → `cache::save(&repos, &contexts)` serializes the whole map (hundreds of repos, each with file lists, pretty-printed) to disk — a multi-MB write on the async executor thread for a large workspace, on every menu open.

**Fix:** persist just the one changed context (partial write), or debounce/skip the write and let the next full scan flush it.

---

## ⚪ Low

### [ ] 8. `which("pwsh") ? "pwsh" : "powershell"` fallback is now duplicated three times
[commands.rs:479](../../../src-tauri/src/commands.rs#L479)

Same pwsh/powershell preference lives here plus twice in `rules::shell_wrap` / `terminalize`. Changing the fallback needs three synchronized edits.

**Fix:** extract `default_shell()` in `rules.rs`; call it from `run_command`, `shell_wrap`, `terminalize`.

---

## Notes — checked and clean

- serde shapes between `types.ts` and the Rust structs (`AppEntry`, `AppsPayload`/`AppListPayload`, `TerminalOption`, `RepoTrace`, `ConfigPatch`, `AppConfig`) all line up, including snake_case `AppConfig`/`scan` fields and the `CollapseNested` bool/`"auto"` custom (de)serialize.
- `overlay:shown` payload migration from `()` to a scope string is fully propagated (only `onOverlayShown` consumes it).
- `overlay:hidden` is not emitted while the settings screen is open (Escape there routes to `backToList`, not `hideOverlay`), so the new emit in `hide_overlay` won't discard unsaved settings.
- `context_for` releases its `if let` lock guard before taking the `state.config` lock — no deadlock.
