# Code Review — Pass 5: CI / Release

**Scope:** `.github/workflows/*.yml`, `package.json`, `scripts/build-icons.mjs`
**Effort:** `max` (local)
**Basis:** `main...HEAD` diff. Reviewer executed the test path directly to isolate the root cause.
**Date:** 2026-09-01

Tick each item as you address it. Severity: 🔴 high · 🟡 medium · ⚪ low.

---

## 🔴 High

### [ ] 1. The new `npm test` CI step runs zero tests and exits 1
[ci.yml:47](../../.github/workflows/ci.yml#L47) · supersedes [pass 4 #1](04-frontend.md)

Root cause, now isolated: `vitest@4.1.11`'s default `forks` pool (and `threads`) crashes with `TypeError: Cannot read properties of undefined (reading 'config')` / "Vitest failed to find the current suite" when the working directory resolves with a **lowercase Windows drive letter** — e.g. `d:\git\dev-prompt`, the maintainer's own checkout path. `cd D:\git\dev-prompt` (uppercase) or `--pool=vmThreads` runs the same **13 assertions green**. `npm test` is therefore broken for typical Windows dev checkouts on the project's only supported OS, and the CI step fails for any runner whose workspace path resolves with a lowercase drive.

The test files themselves are **correct** — all 13 assertions pass against `fuzzy.ts` / `hotkeys.ts` under `--pool=vmThreads`. This is purely a runner/pool defect.

**Fix (pick one, prefer the first):**
- Add `test: { pool: 'vmThreads' }` to `vite.config.ts`.
- Or pin `vitest` to a working `3.x` line (see #2).
- Belt-and-braces: also normalize the drive letter in the test script, but the pool fix is the real one.

Verify `npm test` is green **locally and in a CI run** before the next `dev→main` PR.

### [ ] 2. `"test": "vitest run"` rides an unpinned, non-functional major
[package.json:12](../../package.json#L12)

`vitest@^4.1.11` with nothing setting `test.pool`, so `npm test` always hits the broken default `forks` pool. `--pool=threads` / `--pool=forks --singleFork` / `--no-isolate` all fail identically (0 tests, exit 1), reproduced even with a trivial inline test and a plugin-free minimal config; only `--pool=vmThreads` works. No `engines.node` pin, and a `^` range on a brand-new major means `npm update` can drift the behavior further.

**Fix:**
- Set the pool in `vite.config.ts` (as in #1) **and** pin `vitest` + `@vitest/*` to an exact working version (drop the `^`).
- Add `"engines": { "node": ">=20 <23" }` (or whatever you actually test against) to `package.json`.

---

## 🟡 Medium

### [ ] 3. Dropping the `push` trigger leaves direct `dev` commits with no CI between PRs
[ci.yml:6](../../.github/workflows/ci.yml#L6) · intentional per commit `65ed182`

The workflow is: long-lived `dev`, direct commits to `dev`, periodic `dev→main` merge PR. With the `push: branches-ignore: [main]` trigger removed, commits pushed to `dev` after a PR merges and before the next PR opens get **no** build / clippy / `cargo test` / `npm test`. A broken commit sits on `dev` undetected until the next PR, which then runs CI once over the cumulative delta — making a failure hard to attribute to a commit.

**Fix (if you want the coverage back):** re-add a `push` trigger scoped to `dev` only, or a lightweight `push`-triggered job (build + test, skip the heavier matrix). If the removal was deliberate to save minutes, note the tradeoff in the workflow file so it's not "fixed" later by mistake.

---

## Notes — checked and clean

- `scripts/build-icons.mjs`: adding `dotnet` / `gradle` to `BRAND` is correct — `siDotnet` / `siGradle` exist in the installed `simple-icons`, and `src/lib/icons.ts` was regenerated with valid path data for both.
- The two new test files (`fuzzy.test.ts`, `hotkeys.test.ts`) are correct — 13/13 assertions pass once the runner works.
- No `release.yml` findings in this pass. The `TAURI_SIGNING_PRIVATE_KEY` secret is still owed (per project notes) — the release workflow can't produce signed updater artifacts until that repo secret is set, but that's a setup task, not a code defect.
