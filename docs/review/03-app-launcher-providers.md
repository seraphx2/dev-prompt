# Code Review — Pass 3: App Launcher + Workspace Providers

**Scope:** `apps.rs`, `dotnet.rs`, `gradle.rs`, `gowork.rs`, `maven.rs`, `usage.rs`, `inspect.rs`, `discover_apps.ps1`
**Effort:** `max` (local)
**Basis:** `main..dev` diff (7 new files + `inspect.rs` changes)
**Date:** 2026-09-01

Tick each item as you address it. Severity: 🔴 high · 🟡 medium · ⚪ low.
Resolved items are collapsed to a one-line stub with the commit that closed them; `git show <hash>` for the detail.

---

## 🔴 High

### [x] 1. `dedupe_by_product` silently drops distinct side-by-side installs — `e26e1f4`

### [x] 2. `bump()` does an unsynchronized read-modify-write of `app-usage.json` — `5b09b87`

### [ ] 3. `tag_attr` assumes double-quoted values and matches the attribute name as a substring
[dotnet.rs:148](../../src-tauri/src/dotnet.rs#L148)

`.strip_prefix('"')` returns `None` for a single-quoted value, so a `.slnx` with `<Project Path='src/App.csproj' />` (legal XML) yields **no projects** and the dotnet provider produces nothing. Separately, `tag.find(name)` matches inside other attribute names, so `<Project SomePath="x" Path="real.csproj">` returns `x`.

**Fix:** accept both quote styles; match the attribute name on a word boundary (`\bPath\s*=`), not a bare substring. A tiny attribute scanner beats `find` + `strip_prefix` here.

---

## 🟡 Medium

### [ ] 4. `maven::modules()` has no XML-comment or `<profile>` awareness
[maven.rs:20](../../src-tauri/src/maven.rs#L20)

Raw-text scan for `<module>`. A `pom.xml` with `<!-- <module>legacy</module> -->` (the usual way to disable one) or `<module>` entries inside a `<profile>` → `legacy` is returned → `rules.rs` emits `mvn -B compile · legacy` actions whose cwd is `dir/legacy`, a directory that often doesn't exist → the terminal action fails to `cd`.

**Fix:** strip `<!-- … -->` spans before scanning; ignore `<module>` inside `<profile>` (or only read modules from the top-level `<modules>`).

### [ ] 5. `has_any_marker` now matches VCS-dir markers, surfacing every submodule as a sub-project
[inspect.rs:88](../../src-tauri/src/inspect.rs#L88)

It now matches the discovery globset, which includes `.git`, `.hg`, … A repo with a submodule or vendored clone at `libs/shared` (has `.git`, no language marker): pre-change `has_any_marker()` was false and the dir was ignored; post-change `discovery.is_match(".git")` is true → `inspect()` pushes `libs/shared` as a sub-project. `libs/` and `modules/` are `CONTAINER_DIRS`, so nested submodules are reached too — one spurious "Detected · <dir>" group each.

**Fix:** exclude the VCS markers from the set `has_any_marker` checks (keep them for repo discovery only).

### [ ] 6. Block-mode `use()` only terminates on a line exactly equal to `)`
[gowork.rs:48](../../src-tauri/src/gowork.rs#L48)

A `go.work` whose `use(` block closes as `\t./b)` or `./b )` → the line isn't `")"` → `in_block` stays true → `./b)` is pushed as a bogus module and every following line is consumed as a phantom use path (`replace example.com/x => ./vendor/x` yields a module `x`), swallowing directives until EOF or a bare `)`.

**Fix:** trim the line and check `ends_with(')')` to close the block (and strip a trailing `)` off the last path); or use a real tokenizer.

### [x] 7. `.lnk` Arguments split with `rules::shell_split` eats an unquoted apostrophe — `e743bad`

### [ ] 8. A bare `venv/` / `.venv/` dir is enough to surface a zero-action Python sub-project
[inspect.rs:208](../../src-tauri/src/inspect.rs#L208)

`is_python` is now satisfied by `venv`/`.venv` alone. A directory with only a leftover venv (project moved away, or a venv beside non-Python code) → `inspect()` marks it Python → surfaced as a sub-project, but the python provider emits nothing → an empty "Detected · <dir>" group. Same UX symptom as #5.

**Fix:** require a real Python marker (`pyproject.toml`, `requirements*.txt`, `setup.py`, `manage.py`, `*.py` at top level) — treat `venv/` as corroborating, not sufficient.

### [ ] 9. `Project.files` went from `#[serde(skip)]` to serialized — `repos.json` bloat
[inspect.rs:66](../../src-tauri/src/inspect.rs#L66)

`repos.json` now embeds every top-level entry name for the root + up to 12 sub-projects of every repo. Written pretty on every scan, parsed on every startup. For a user with many repos that have large top-level dirs this grows substantially — offline rule eval only needs the marker/glob-matching names, not the full listing.

**Fix:** restore `#[serde(skip)]` and persist only the names rule eval actually matches against, or drop `files` from the persisted shape entirely and recompute on load.

---

## ⚪ Low

### [ ] 10. `IconFor` disposes `$ms`/`$bmp`/`$ic` only on the success path
[discover_apps.ps1:42](../../src-tauri/src/discover_apps.ps1#L42)

If `$bmp.Save(...)` or `[IO.File]::WriteAllBytes(...)` throws, `catch { return '' }` runs but the MemoryStream / Bitmap / Icon were never disposed — one leaked GDI+/stream handle per failing icon across an enumeration that can touch hundreds of executables.

**Fix:** dispose in a `finally` block.

### [ ] 11. The per-exe icon PNG cache is never invalidated
[discover_apps.ps1:29](../../src-tauri/src/discover_apps.ps1#L29)

Served whenever `<sha1>.png` exists — unlike the 24h-TTL app list. After an app updates its icon (or a path is reused), the launcher shows the stale icon forever, short of manually clearing `%LOCALAPPDATA%\dev-prompt\cache\app-icons`.

**Fix:** key the cache filename on exe mtime+size (or the app-list TTL), or clear the icon dir when the app list refreshes.

### [ ] 12. `prune_scanned` / `looks_like_main_binary` tests use `C:\` literals without `#[cfg(windows)]`
[apps.rs:491](../../src-tauri/src/apps.rs#L491)

On a non-Windows `cargo test`, `Path::parent()`/`file_stem()` treat `C:\a\GitHubDesktop\tool.exe` as one component → `parent_dir()` returns `""` for every entry → all entries in one group → the substring check keeps `tool.exe` → `assert!(!names.contains(&"tool"))` fails. CI is Windows-only so it's green there, but the suite isn't portable.

**Fix:** gate these tests with `#[cfg(windows)]`, or build the fixture paths with `PathBuf` from components so they're platform-neutral.

---

## Cross-pass links

- **#5** and **#8** produce the same user-visible bug: empty "Detected · <dir>" groups in the action menu.
