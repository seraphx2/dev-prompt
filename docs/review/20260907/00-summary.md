# Code Review — Summary (2026-09-07)

**Reviewed:** the Linux distribution work, `6dcaf5c..8b474d9` (~28 commits since
the last review on 2026-09-03). Scope: `packaging/**`,
`.github/workflows/repo.yml` + `release.yml`, `.github/dependabot.yml`,
`src-tauri/src/apps.rs` (`mod linux`), `launch.rs`, `autostart.rs`, and the
`in_flatpak()` branches in `rules.rs` / `commands.rs` / `lib.rs`, plus the
frontend deltas (`Settings.svelte`, `App.svelte`, `AppRow.svelte`, `ipc.ts`).
One pass, `high` effort, local.

**Findings:** 6. None are release-blocking crashes. Three are real quality gaps
in the `>` app-scope under Flatpak / on non-GNOME desktops; three are latent /
cosmetic.

**No dead code.** The 5-iteration `flatpak-repo` fix loop (the `lib64` /
inline-ayatana / `post-install` attempts) reverted cleanly — `repo.yml` and the
manifest carry only explanatory comments, no scaffolding. Every finding below is
pre-existing logic (mostly the `mod linux` app scope from `1e44bac` / `67de459`),
surfaced now because it had never been reviewed.

Detail: [01 — Linux app launcher](01-linux-app-launcher.md),
[02 — repo pipeline](02-repo-pipeline.md).

---

## Findings

| # | File | Severity | Finding | Proposed fix | When |
|---|---|---|---|---|---|
| **R0907-1** | `apps.rs` `app_dirs` (~489) | medium | Under Flatpak, `XDG_DATA_HOME` is remapped to `~/.var/app/<id>/data`, so the `>` scope never scans the real `~/.local/share/applications` — every user-level `.desktop` entry and every `--user` Flatpak (incl. how dev-prompt installs itself) is missing from the list. | In the `in_flatpak()` block, also push `$HOME/.local/share/applications` + `.../flatpak/exports/share/applications` via `dirs::home_dir()` — mirror what `autostart.rs::is_enabled` already does to dodge the same remapping. | before merge |
| **R0907-2** | `apps.rs` `linux::launch` (~429) | medium | `launch()` re-derives the gtk-launch id as `Path::file_stem(entry.exec)`, dropping the subdir prefix that `walk()` baked into the desktop-file ID (`kde/systemsettings.desktop` → id `kde-systemsettings`, but `launch` computes `systemsettings`). Nested entries launch the wrong app or nothing; `spawn_detached` never checks exit status, so it's silent. | Derive the id the way `walk()` does — split on `/applications/`, strip `.desktop`, `/`→`-` — or thread the real id onto `AppEntry`. | before merge |
| **R0907-3** | `apps.rs` `configured_icon_theme` / `icon_candidates` (~799) | medium (perf) | `resolve_icon` → `icon_candidates` rebuilds `icon_roots()` + `icon_themes()` per app, and `configured_icon_theme()` **forks `gsettings`** per app on any desktop without `gtk-icon-theme-name` in a GTK `settings.ini` (KDE, Sway, minimal WMs). 150–400 subprocess spawns + hundreds of `PathBuf` allocs per `rescan_apps`. Hits KDE — the primary dev box. | Compute `icon_roots()` + `icon_themes()` once per `discover()` pass (thread through, or `OnceLock`); call `configured_icon_theme()` once. | before merge (cheap) |
| **R0907-4** | `apps.rs` `linux::launch` (~437) | low | The `gio launch` fallback passes `entry.exec` (a `/run/host/usr/...` sandbox path under Flatpak) to host `gio` via `flatpak-spawn --host` — the path doesn't exist in the host mount namespace. Fallback-of-a-fallback (`gtk-launch` is a hard dep), so near-zero real impact. | Skip the `gio` fallback under Flatpak, or translate `/run/host` → host path. | defer / fold into R0907-2 |
| **R0907-5** | `packaging/repo/build-repo.sh` (~88) | low (latent) | The per-`.rpm` `--addsign` loop runs **after** `shopt -u nullglob`; an empty `$SITE/rpm/` makes `for f in "$SITE"/rpm/*.rpm` iterate over the literal glob and `rpm --addsign` aborts the whole `publish` under `set -e`. The deb path is nullglob-protected; the rpm path isn't. Bites a fresh `gh-pages` or an rpm-naming drift. | Wrap the loop in `shopt -s nullglob` (or `[ -e "$f" ] || continue`). | defer (housekeeping) |
| **R0907-6** | `.github/dependabot.yml` (~7) | low (nit) | The `github-actions` block omits `target-branch: dev` that `npm` / `cargo` set. Harmless today (default branch *is* `dev`), but if the default ever moves to `main`, action bumps drift onto the release branch while the other two stay pinned. | Add `target-branch: dev` to the `github-actions` block for parity. | defer (nit) |

## Resolution

All six fixed in the same commit as this write-up (harden now rather than
carry them as known issues):

| # | Fix |
|---|---|
| **R0907-1** | `app_dirs()` derives `data_home` from `$HOME/.local/share` under Flatpak instead of the redirected `XDG_DATA_HOME`, so `~/.local/share/applications` + the `--user` Flatpak exports are scanned. |
| **R0907-2** | `linux::launch` derives the id by splitting `entry.exec` on `/applications/` and applying `/`→`-`, matching `walk()`; `file_stem` stays only as the last-ditch fallback. |
| **R0907-3** | `icon_roots()` / `icon_themes()` memoised in `OnceLock` — one `gsettings` probe per process, not per app. Theme-change-mid-session now needs a restart (noted in-code). |
| **R0907-4** | the `gio launch` fallback strips a `/run/host` prefix off `entry.exec` so the host sees a real path. |
| **R0907-5** | the rpm `--addsign` loop skips the literal glob on an empty pool (`[ -e ] || continue`); the pacman block switched to a `nullglob` array and bails cleanly when the pool is empty. |
| **R0907-6** | `target-branch: dev` added to the `github-actions` Dependabot block for parity with npm / cargo. |

Verified: `cargo clippy` clean, 69 Rust + 13 JS tests pass, `svelte-check` 0
errors, `bash -n` on `build-repo.sh`.
