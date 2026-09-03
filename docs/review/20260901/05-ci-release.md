# Code Review — Pass 5: CI / Release

**Scope:** `.github/workflows/*.yml`, `package.json`, `scripts/build-icons.mjs`
**Effort:** `max` (local)
**Basis:** `main...HEAD` diff. Reviewer executed the test path directly to isolate the root cause.
**Date:** 2026-09-01

Tick each item as you address it. Severity: 🔴 high · 🟡 medium · ⚪ low.
Resolved items are collapsed to a one-line stub with the commit that closed them; `git show <hash>` for the detail.

---

## 🔴 High

### [x] 1. The new `npm test` CI step runs zero tests and exits 1 — `7bd4b5d`

### [x] 2. `"test": "vitest run"` rides an unpinned, non-functional major — `7bd4b5d`

---

## 🟡 Medium

### [x] 3. Dropping the `push` trigger leaves direct `dev` commits with no CI between PRs — `ci-dev.yml` (+ shared `.github/actions/ci`)

---

## Notes — checked and clean

- `scripts/build-icons.mjs`: adding `dotnet` / `gradle` to `BRAND` is correct — `siDotnet` / `siGradle` exist in the installed `simple-icons`, and `src/lib/icons.ts` was regenerated with valid path data for both.
- The two new test files (`fuzzy.test.ts`, `hotkeys.test.ts`) are correct — 13/13 assertions pass once the runner works.
- No `release.yml` findings in this pass. The `TAURI_SIGNING_PRIVATE_KEY` secret is still owed (per project notes) — the release workflow can't produce signed updater artifacts until that repo secret is set, but that's a setup task, not a code defect.
