# Code Review — Summary (2026-09-03)

**Reviewed:** commit `6dcaf5c` — "move the launch/action-menu path off the UI thread"
(converts `build_actions` / `run_action` / `run_command` / `run_app` to `async` +
`spawn_blocking`, adds a startup program-cache prewarm). One pass, local.

**Findings:** 5. Four fixed in follow-up `45febed`; one deferred (below).

Detail: [01 — async launch path](01-async-launch-path.md)

---

## Fixed — `45febed`

| # | Finding | Fix |
|---|---|---|
| **1** | `run_app` fired `usage::bump` as a detached `spawn_blocking` — a launch is lost if the process exits (tray quit, updater relaunch) before the task is scheduled | `await` the bump again; it's a sub-ms JSON rewrite, detaching bought nothing |
| **2** | `build_actions` turned a `spawn_blocking` `JoinError` (closure panic) into a silent empty menu via `.unwrap_or_default()`, unlike its three siblings | log the `JoinError`; still `Ok`-only so the frontend contract holds |
| **3** | the cache-miss "compile globset + `inspect`" fallback was copy-pasted into `build_actions` and `run_action` instead of reusing `context_for` | extract free fn `inspect_cold(path, cfg)`; all three share it |
| **4** | startup prewarm can race `clear_program_cache()` and leave a stale entry until the next reload | comment only — ~1s window at startup, self-heals; a cache generation counter isn't worth it |

## Deferred — Tier 2

| # | Work item | Root fix |
|---|---|---|
| **R0903-1** | `build_actions` / `run_action` / `run_command` deep-clone the entire `Config` on every action-menu open and every launch, to move it into the `spawn_blocking` closure | wrap `Config` in `Arc` inside `AppState` so the hot paths clone a pointer, not the `BTreeMap` + rules `Vec` |

Not release-blocking — the clone is a few KB and is dwarfed by the filesystem work
the same commit moved off-thread. It's called out because the pattern is now in
three hot spots and the fix touches every `state.config` reader. Full write-up:
[01 — async launch path](01-async-launch-path.md), finding 5.
