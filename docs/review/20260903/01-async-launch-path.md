# Code Review — Async launch / action-menu path

**Scope:** commit `6dcaf5c` — `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`
**Effort:** `max` (local), single pass
**Basis:** the commit diff. `cargo test` (74) + `cargo clippy -D warnings` + `svelte-check` all clean before and after.
**Date:** 2026-09-03

Tick each item as you address it. Severity: 🔴 high · 🟡 medium · ⚪ low.
Resolved items are collapsed to a one-line stub with the commit that closed them; `git show <hash>` for the detail.

---

## 🟡 Medium

### [x] 1. `run_app` frecency bump is fire-and-forget — increments lost on early exit — `45febed`

### [x] 2. `build_actions` swallows a `spawn_blocking` panic into an empty menu — `45febed`

---

## ⚪ Low

### [x] 3. Cache-miss `inspect` fallback duplicated across `build_actions` / `run_action` — `45febed`

### [x] 4. Startup prewarm races `clear_program_cache()` — `45febed` (comment only; accepted)

---

## Deferred to Tier 2

### [ ] 5. `Config` is deep-cloned on every action-menu open

[commands.rs:190](../../../src-tauri/src/commands.rs#L190) ·
[commands.rs:269](../../../src-tauri/src/commands.rs#L269) ·
[commands.rs:573](../../../src-tauri/src/commands.rs#L573)

**Work-item id:** R0903-1

`6dcaf5c` moved the launch path off the UI thread by cloning state out from under
the `Mutex` guards before entering `spawn_blocking` (the guards aren't `Send`).
For `Config` that clone is a full recursive copy:

```rust
// build_actions, run_action, run_command — once per call
let cfg = state.config.lock().unwrap().clone();
```

`Config` carries `programs: BTreeMap<String, ProgramSpec>`, `rules: Vec<Rule>`,
`markers: Vec<Marker>`, `universal`, plus the scalar settings. Every rule holds
its own `match` / `actions` / template `Vec`s and `String`s. A fattened
`default_config.yaml` is ~20 rules and ~15 program recipes, so each clone is a few
KB across dozens of allocations.

**Call frequency:**

| Command | Fires on |
|---|---|
| `build_actions` | every action-menu open, and every `activateRepo` (Enter on a repo) |
| `run_action` | every action launched from the menu |
| `run_command` | every "Run command…" invocation |

So the common interaction — open the overlay, arrow to a repo, hit Enter — pays
two full `Config` clones back to back (`build_actions` then `run_action`).

**Why it's not release-blocking:**

- The clone is heap traffic, not I/O — sub-millisecond. The same commit moved the
  filesystem globbing, PATH scans and `vswhere` call (tens of ms, cold) off the
  UI thread; the clone is noise next to what it replaced.
- `config_summary` / `rescan_repos` / `rescan_apps` already use this exact
  clone-then-`spawn_blocking` shape and were accepted in review.

**Why it's worth recording:**

- The pattern is now in **three** hot commands, not one. If `Config` keeps
  accreting fields (the review's standing "treat its shape as a budget" theme),
  the per-open cost grows silently.
- The clean fix is small in concept but wide in blast radius, so it wants to be
  a deliberate change, not a drive-by.

**Fix:** hold the config as `Arc<Config>` in `AppState`.

```rust
// AppState
config: Mutex<Arc<Config>>,

// hot path — clones a pointer, not the tree
let cfg = state.config.lock().unwrap().clone();       // Arc<Config>
tauri::async_runtime::spawn_blocking(move || { /* &*cfg */ });

// reload path (rescan_repos / save_config) — swap the whole Arc
*state.config.lock().unwrap() = Arc::new(config::load()?);
```

Touches every reader of `state.config` (`list_repos`, `repo_rule_trace`,
`list_terminals`, `payload`, …) — mostly mechanical (`&*guard` instead of
`&guard`), but it's the whole command surface, hence Tier 2. Reload semantics are
unchanged: writers already replace the value wholesale rather than mutating in
place, which is exactly what `Arc` swap wants.

---

## Notes — checked and clean

- The `async` + `spawn_blocking` conversions preserve behaviour: `run_action` /
  `run_command` still propagate a real launch error (`?` after
  `.map_err(JoinError → AppError)`); `build_actions` was `Vec`-returning and
  stays effectively infallible (`Ok`-only) so `invoke<Action[]>` on the frontend
  needs no change.
- `run_app` still bumps frecency only after `apps::launch` succeeds — a stale
  entry whose exe is gone doesn't climb the list.
- The prewarm task reads the `AppState` config snapshot once and resolves every
  `programs` key plus `"terminal"`; it can't deadlock (no other lock held) and a
  failure is `let _ =`-ignored, matching the "resolution is best-effort" model
  elsewhere.
- No frontend or `ipc.ts` change was required by the commit.
