# 01 — Linux app launcher (`apps.rs` `mod linux`)

The `>` scope's freedesktop `.desktop` discovery + launch, added in `1e44bac` and
extended for Flatpak in `67de459`. Never reviewed. Four findings, all in
`src-tauri/src/apps.rs`.

---

## R0907-1 — Flatpak `>` scope misses every user-level app

`app_dirs()`:

```rust
let home = dirs::home_dir();
let data_home = std::env::var_os("XDG_DATA_HOME")
    .map(PathBuf::from)
    .filter(|p| p.is_absolute())
    .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
if let Some(dh) = &data_home {
    push_dir(&mut dirs, dh.join("applications"));
    push_dir(&mut dirs, dh.join("flatpak/exports/share/applications"));
}
```

Flatpak always sets `XDG_DATA_HOME` to `~/.var/app/io.github.seraphx2.devprompt/data`
(absolute), regardless of `--filesystem` grants. So the `.filter(is_absolute)`
branch always wins inside the sandbox and the `home.join(".local/share")`
fallback is dead code there. `app_dirs` then scans:

- `~/.var/app/<id>/data/applications` — empty
- `~/.var/app/<id>/data/flatpak/exports/share/applications` — nonexistent

instead of the real `~/.local/share/applications` and
`~/.local/share/flatpak/exports/share/applications`, both of which **are**
reachable (`--filesystem=home` keeps `$HOME` pointing at the real home; only the
`XDG_*` vars are redirected).

**Effect:** in the Flatpak build the `>` scope shows system apps
(`/run/host/usr/...`) and nothing the user installed themselves — including every
`flatpak install --user` app, which is the common case and how dev-prompt's own
README tells people to install it. This is part of why the box showed "only 57
apps" during Phase 4 testing.

`autostart.rs::is_enabled()` already sidesteps this exact remapping by building
its path from `std::env::var("HOME")` rather than `XDG_CONFIG_HOME`. `app_dirs`
should do the same.

**Fix:** in the existing `if crate::launch::in_flatpak()` block, add

```rust
if let Some(h) = dirs::home_dir() {
    push_dir(&mut dirs, h.join(".local/share/applications"));
    push_dir(&mut dirs, h.join(".local/share/flatpak/exports/share/applications"));
}
```

(placed to keep user dirs ahead of the `/run/host` system dirs in precedence).

---

## R0907-2 — nested `.desktop` entries launch the wrong app or nothing

`walk()` computes the desktop-file ID the freedesktop way — path relative to the
`applications/` root, `.desktop` stripped, `/` → `-`:

```rust
let id = rel.to_string_lossy()
    .strip_suffix(".desktop").unwrap_or_default()
    .replace('/', "-");           // kde/systemsettings.desktop -> "kde-systemsettings"
```

`discover()` keys `by_id` by that and passes it to `parse_entry(&file, &id)` —
but `parse_entry` only uses `id` as a fallback `Name`, and `AppEntry` has no `id`
field. So `launch()` reconstructs it from the path:

```rust
let id = Path::new(&entry.exec)      // entry.exec = the full .desktop path
    .file_stem()                     // "systemsettings"  — the "kde-" is gone
    .map(...).unwrap_or_default();
if !id.is_empty() && which("gtk-launch").is_some() {
    return crate::launch::spawn("gtk-launch", &[id], "");
}
```

`gtk-launch systemsettings` → `g_desktop_app_info_new("systemsettings.desktop")`.
GLib only does the `-`→`/` subdir lookup for IDs that contain `-`; `systemsettings`
has none, so the `kde/` entry is never found. Result: the app is in the `>` list
(discovery found it) but Enter does nothing — and `spawn_detached` never checks
the child exit status, so there's no error surfaced. Worst case it matches an
unrelated top-level `systemsettings.desktop`.

Minority of entries (KDE ships a few nested ones; most `.desktop` files are
top-level and `file_stem` == id for those), but a silent correctness bug.

**Fix:** derive the id the same way `walk()` does, from the path:

```rust
let id = entry.exec
    .rsplit_once("/applications/")
    .map(|(_, rel)| rel.trim_end_matches(".desktop").replace('/', "-"))
    .unwrap_or_else(|| Path::new(&entry.exec).file_stem()
        .map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
```

or add `id: String` to `AppEntry` and thread it through `run_app`.

---

## R0907-3 — icon resolution forks `gsettings` once per app

`resolve_icon(name)` → `icon_candidates(name)` calls `icon_roots()` **and**
`icon_themes()` on every invocation, and `icon_themes()` calls
`configured_icon_theme()`:

```rust
fn configured_icon_theme() -> Option<String> {
    let cfg = dirs::config_dir()?;
    for f in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] { /* read, grep */ }
    let out = std::process::Command::new("gsettings")           // <-- per call
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output().ok()?;
    ...
}
```

`resolve_icon` runs once per `parse_entry`, i.e. once per app. On any desktop
that doesn't write `gtk-icon-theme-name` into a GTK `settings.ini` — KDE, Sway,
i3, most minimal WMs — every one of 150–400 apps forks a `gsettings` process
during `discover()` / `rescan_apps`. On top of that, `icon_candidates` rebuilds
`icon_roots()` (reads `XDG_DATA_DIRS`, ~8 `PathBuf`s) and `icon_themes()` per
icon, then materialises `roots × themes × SIZES × EXTS` ≈ ~1k `PathBuf`s and
`stat`s each.

None of `icon_roots()` / `icon_themes()` / `configured_icon_theme()` varies
within a single discovery pass. The primary dev box (CachyOS / KDE) is exactly
the affected case.

**Fix:** compute `roots` + `themes` once in `discover()` and pass them down
(`parse_entry` → `resolve_icon` → `icon_candidates`), or memoise via
`OnceLock` for the process. At minimum, memoise `configured_icon_theme()`.

---

## R0907-4 — `gio launch` fallback passes a sandbox path to the host

```rust
if which("gio").is_some() {
    return crate::launch::spawn("gio", &["launch".into(), entry.exec.clone()], "");
}
```

Under Flatpak this becomes `flatpak-spawn --host gio launch
/run/host/usr/share/applications/foo.desktop`. `/run/host/...` is a sandbox-only
mount; host `gio` can't see it, so the launch fails.

Impact is small — this is the fallback after `gtk-launch`, which is a hard
runtime dependency and effectively always present. Fold the fix into R0907-2
(e.g. skip the `gio` branch when `in_flatpak()`, or map `/run/host/usr` back to
`/usr` for the host call).
