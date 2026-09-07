//! Installed-application discovery for the `>` app-launcher scope.
//!
//! **Windows:** enumeration is one embedded PowerShell script (`DISCOVER_PS1`)
//! that unions four sources — `Get-StartApps` (Win32 + Store), Start Menu `.lnk`
//! targets, the three Uninstall registry hives, and a bounded `*.exe` scan of
//! `%LOCALAPPDATA%\Programs` plus any user `extra_dirs`. The script also extracts
//! and disk-caches each app's icon. Rust does the culling: [`keep_entry`] drops
//! installer/updater/helper noise, [`prune_scanned`] keeps only the "main
//! binary" per folder for the raw-scan tiers, [`dedupe_by_product`] collapses
//! same-vendor duplicates, and [`dedupe`] merges entries that point at the same
//! executable.
//!
//! **Linux:** [`discover`] parses freedesktop `.desktop` entries from the XDG
//! application directories (plus Flatpak / Snap exports and `extra_dirs`); see
//! the `linux` submodule. Launch goes through `gtk-launch` so `Exec` field
//! codes, `Terminal=true` and D-Bus activation are handled by the platform.
//!
//! **Other platforms:** [`discover`] returns an empty list.

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    /// `exec` is a filesystem path to an executable.
    #[default]
    Exe,
    /// `exec` is an AppUserModelID, launched via `explorer shell:AppsFolder\…`.
    Aumid,
    /// `exec` is an absolute path to a freedesktop `.desktop` file (Linux).
    /// Launched via `gtk-launch` by its id; `args` holds the parsed `Exec`
    /// line (field codes stripped) as a fallback, `terminal` mirrors
    /// `Terminal=` for that fallback.
    Desktop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    pub name: String,
    /// Executable path (`Exe`), AppUserModelID (`Aumid`) or `.desktop` path
    /// (`Desktop`).
    pub exec: String,
    pub kind: AppKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// `data:image/png;base64,…` / `data:image/svg+xml;base64,…` when an icon
    /// was resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// `Desktop` only: the entry declared `Terminal=true`. Ignored unless the
    /// `gtk-launch` / `gio` fallbacks are all unavailable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub terminal: bool,
    /// `start-menu` | `store` | `uninstall` | `scan` | `extra` | `desktop`.
    pub source: String,
}
// Launch frecency is *not* a field here — it's derived per read from
// `app-usage.json` and carried only on `commands::AppView`, so it never touches
// `apps.json`.

/// Source ranking for dedupe — a curated Start Menu entry beats a raw scan hit.
fn source_rank(source: &str) -> u8 {
    match source {
        "start-menu" => 4,
        "store" | "desktop" => 3,
        "uninstall" => 2,
        _ => 1, // "scan" / "extra"
    }
}

/// Letters + digits, lowercased — for loose name comparisons.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn path_stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn parent_dir(path: &str) -> String {
    std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn dir_leaf(dir: &str) -> &str {
    dir.rsplit(['\\', '/']).next().unwrap_or(dir)
}

/// Does this exe look like "the app" for its folder — its basename echoes the
/// folder name or the product name (either containing the other)?
fn looks_like_main_binary(exec: &str, folder: &str, product: &str) -> bool {
    let s = norm(&path_stem(exec));
    if s.is_empty() {
        return false;
    }
    let f = norm(folder);
    let p = norm(product);
    (!f.is_empty() && (f.contains(&s) || s.contains(&f)))
        || (!p.is_empty() && (p.contains(&s) || s.contains(&p)))
}

/// Reject installer stubs, updaters, redistributables and OS components — the
/// noise that dominates a raw executable enumeration.
pub fn keep_entry(name: &str, path: &str) -> bool {
    let n = name.to_lowercase();
    let p = path.replace('/', "\\").to_lowercase();

    if name.trim().is_empty() {
        return false;
    }

    const NAME_BAD: &[&str] = &[
        "uninstall", "unins00", "setup", "installer", "update", "updater",
        "crashpad", "crash handler", "crashhandler", "crash reporter",
        "crashreporter", "helper", "vc_redist", "vcredist", "redistributable",
        "web installer", "repair", "elevate", "squirrel", "bootstrapper",
    ];
    if NAME_BAD.iter().any(|b| n.contains(b)) {
        return false;
    }

    const PATH_BAD: &[&str] = &["\\windows\\", "\\winsxs\\", "\\system32\\", "\\syswow64\\"];
    if PATH_BAD.iter().any(|b| p.contains(b)) {
        return false;
    }

    let file = p.rsplit('\\').next().unwrap_or(&p);
    const FILE_BAD: &[&str] = &[
        "unins", "setup.exe", "update.exe", "updater.exe", "crashpad_handler.exe",
        "vcredist", "vc_redist", "dxsetup.exe", "notification_helper.exe",
        "elevate.exe", "squirrel.exe",
    ];
    if FILE_BAD.iter().any(|b| file.contains(b)) {
        return false;
    }

    true
}

/// Collapse entries that point at the same executable, keeping the one from the
/// highest-ranked source and preferring one that carries an icon. `Aumid`
/// entries are keyed by their id and never merged with `Exe` ones.
pub fn dedupe(entries: Vec<AppEntry>) -> Vec<AppEntry> {
    use std::collections::HashMap;
    let mut best: HashMap<String, AppEntry> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for e in entries {
        let key = match e.kind {
            AppKind::Exe => format!("exe:{}", e.exec.replace('/', "\\").to_lowercase()),
            AppKind::Aumid => format!("aumid:{}", e.exec.to_lowercase()),
            AppKind::Desktop => format!("desktop:{}", e.exec.to_lowercase()),
        };
        match best.get_mut(&key) {
            None => {
                order.push(key.clone());
                best.insert(key, e);
            }
            Some(cur) => {
                let better_source = source_rank(&e.source) > source_rank(&cur.source);
                let gains_icon = cur.icon.is_none() && e.icon.is_some();
                if better_source || gains_icon {
                    // Keep whichever fields are richer.
                    if e.icon.is_some() {
                        cur.icon = e.icon.clone();
                    }
                    if better_source {
                        cur.name = e.name.clone();
                        cur.source = e.source.clone();
                        cur.args = e.args.clone();
                    }
                }
            }
        }
    }

    let mut out: Vec<AppEntry> = order.into_iter().filter_map(|k| best.remove(&k)).collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

// --- Windows discovery ------------------------------------------------------

#[cfg(windows)]
const DISCOVER_PS1: &str = include_str!("discover_apps.ps1");

#[cfg(windows)]
pub fn discover(cfg: &Config) -> Vec<AppEntry> {
    if !cfg.apps.enabled {
        return Vec::new();
    }

    let extra = cfg
        .apps
        .extra_dirs
        .iter()
        .filter(|d| !d.trim().is_empty())
        .map(|d| format!("'{}'", d.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");

    let icon_dir = crate::config::cache_dir()
        .map(|p| p.join("app-icons"))
        .unwrap_or_else(|_| std::path::PathBuf::from(r"dev-prompt\cache\app-icons"));
    let script = DISCOVER_PS1
        .replace("__ICON_DIR__", &icon_dir.to_string_lossy())
        .replace("__EXTRA_DIRS__", &extra)
        .replace("__ICON_CAP__", "320");

    let json = match run_powershell(&script) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let raw: Vec<RawApp> = serde_json::from_str(trimmed).unwrap_or_default();
    let scored = raw
        .into_iter()
        .filter_map(|r| r.into_scored(cfg))
        .collect::<Vec<_>>();
    let scored = prune_scanned(scored);
    let scored = dedupe_by_product(scored);
    dedupe(scored.into_iter().map(|s| s.entry).collect())
}

/// For the raw-scan tiers (`scan` / `extra`), keep only the executable that
/// looks like the app in each folder — the one whose name echoes the folder or
/// product name. When none do, keep just the largest. Folders with a single
/// candidate, and all curated sources, pass through untouched.
fn prune_scanned(scored: Vec<Scored>) -> Vec<Scored> {
    use std::collections::HashMap;

    let (scanned, mut kept): (Vec<Scored>, Vec<Scored>) = scored
        .into_iter()
        .partition(|s| matches!(s.entry.source.as_str(), "scan" | "extra"));

    let mut by_dir: HashMap<String, Vec<Scored>> = HashMap::new();
    for s in scanned {
        by_dir.entry(parent_dir(&s.entry.exec)).or_default().push(s);
    }

    for (dir, group) in by_dir {
        if group.len() == 1 {
            kept.extend(group);
            continue;
        }
        let folder = dir_leaf(&dir).to_string();
        let (matched, rest): (Vec<Scored>, Vec<Scored>) = group
            .into_iter()
            .partition(|s| looks_like_main_binary(&s.entry.exec, &folder, &s.product));
        if !matched.is_empty() {
            kept.extend(matched);
        } else if let Some(biggest) = rest.into_iter().max_by_key(|s| s.size) {
            kept.push(biggest);
        }
    }
    kept
}

/// Collapse a lower-ranked entry into a higher-ranked one that shares a non-empty
/// CompanyName *and* ProductName — a raw-scan `FooLauncher.exe` folding into the
/// curated Start Menu `Foo`. Two entries that *tie* on source rank are distinct
/// installs of the same product (Python 3.11 vs 3.12, Chrome vs Chrome Beta) and
/// are both kept — `dedupe` still removes exact-path duplicates afterwards.
/// Entries missing either field pass through.
fn dedupe_by_product(scored: Vec<Scored>) -> Vec<Scored> {
    use std::collections::HashMap;
    let mut out: Vec<Scored> = Vec::new();
    let mut seen: HashMap<(String, String), usize> = HashMap::new();

    for s in scored {
        let key = (norm(&s.company), norm(&s.product));
        if key.0.is_empty() || key.1.is_empty() {
            out.push(s);
            continue;
        }
        match seen.get(&key).copied() {
            None => {
                seen.insert(key, out.len());
                out.push(s);
            }
            Some(i) => {
                use std::cmp::Ordering;
                match source_rank(&s.entry.source).cmp(&source_rank(&out[i].entry.source)) {
                    Ordering::Greater => {
                        // Curated entry supersedes the lower-ranked satellite exe.
                        if out[i].entry.icon.is_some() && s.entry.icon.is_none() {
                            let icon = out[i].entry.icon.take();
                            out[i] = s;
                            out[i].entry.icon = icon;
                        } else {
                            out[i] = s;
                        }
                    }
                    Ordering::Less => {} // lower-ranked duplicate — drop it
                    Ordering::Equal => out.push(s), // distinct install — keep both
                }
            }
        }
    }
    out
}

#[cfg(windows)]
fn run_powershell(script: &str) -> AppResult<String> {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| crate::error::AppError::msg(format!("powershell spawn failed: {e}")))?;

    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(script.as_bytes())
        .map_err(|e| crate::error::AppError::msg(format!("powershell stdin: {e}")))?;

    let out = child
        .wait_with_output()
        .map_err(|e| crate::error::AppError::msg(format!("powershell wait: {e}")))?;

    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_os = "linux")]
pub fn discover(cfg: &Config) -> Vec<AppEntry> {
    if !cfg.apps.enabled {
        return Vec::new();
    }
    linux::discover(cfg)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn discover(_cfg: &Config) -> Vec<AppEntry> {
    Vec::new()
}

// --- Linux: freedesktop .desktop discovery ------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use super::{AppEntry, AppKind};
    use crate::config::Config;
    use crate::error::{AppError, AppResult};
    use crate::rules::which;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    /// Skip embedding an icon file larger than this — keeps `apps.json` bounded
    /// when a theme only ships an oversized PNG. Themed SVGs are far smaller.
    const ICON_MAX_BYTES: u64 = 256 * 1024;

    /// The `Exec` field codes (see the Desktop Entry spec §"The Exec key").
    /// All are dropped — dev-prompt launches apps with no document/URI argument.
    const FIELD_CODES: &[&str] = &[
        "%f", "%F", "%u", "%U", "%i", "%c", "%k", "%d", "%D", "%n", "%N", "%v", "%m",
    ];

    pub fn discover(cfg: &Config) -> Vec<AppEntry> {
        let excludes: Vec<String> = cfg
            .apps
            .exclude
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        // id -> entry; directories are walked in precedence order and the first
        // writer of an id wins (an `~/.local/share` override shadows `/usr`).
        let mut by_id: BTreeMap<String, AppEntry> = BTreeMap::new();
        for dir in app_dirs(cfg) {
            for (id, file) in desktop_files(&dir) {
                if by_id.contains_key(&id) {
                    continue;
                }
                let Some(entry) = parse_entry(&file, &id) else {
                    continue;
                };
                let hay = format!("{}\n{}", entry.name.to_lowercase(), id.to_lowercase());
                if excludes.iter().any(|x| hay.contains(x)) {
                    continue;
                }
                by_id.insert(id, entry);
            }
        }

        let mut out: Vec<AppEntry> = by_id.into_values().collect();
        out.sort_by_key(|a| a.name.to_lowercase());
        out
    }

    /// Launch a `.desktop` entry. Prefers `gtk-launch` (part of gtk3, already a
    /// hard runtime dep) so `Exec` field codes, `Terminal=true`, D-Bus
    /// activation and startup notification are all the platform's problem, not
    /// ours. Falls back to `gio launch`, then to spawning the parsed `Exec`.
    pub fn launch(entry: &AppEntry) -> AppResult<()> {
        // The gtk-launch / gio id is the freedesktop desktop-file ID:
        // the path under `applications/`, `.desktop` stripped, `/` -> `-`
        // (so `kde/systemsettings.desktop` -> `kde-systemsettings`). A plain
        // file_stem drops the subdir and gtk-launch then can't resolve it.
        let id = entry
            .exec
            .rsplit_once("/applications/")
            .map(|(_, rel)| rel.trim_end_matches(".desktop").replace('/', "-"))
            .unwrap_or_else(|| {
                Path::new(&entry.exec)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });

        if !id.is_empty() && which("gtk-launch").is_some() {
            return crate::launch::spawn("gtk-launch", &[id], "");
        }
        if which("gio").is_some() {
            // Under Flatpak entry.exec is a /run/host/... path that host `gio`
            // (via flatpak-spawn --host) can't see — strip the prefix to the
            // real host path. Non-host paths (~/.local/share/...) are unchanged.
            let path = entry.exec.strip_prefix("/run/host").unwrap_or(&entry.exec);
            return crate::launch::spawn("gio", &["launch".into(), path.to_string()], "");
        }

        // Last resort: run the parsed Exec ourselves.
        let (prog, rest) = entry
            .args
            .split_first()
            .ok_or_else(|| AppError::msg("desktop entry has no runnable Exec"))?;
        if entry.terminal {
            let term = std::env::var("TERMINAL")
                .ok()
                .filter(|t| !t.is_empty() && which(t).is_some())
                .or_else(|| {
                    [
                        "x-terminal-emulator",
                        "alacritty",
                        "kitty",
                        "foot",
                        "wezterm",
                        "gnome-terminal",
                        "konsole",
                        "xterm",
                    ]
                    .iter()
                    .find(|t| which(t).is_some())
                    .map(|s| s.to_string())
                })
                .ok_or_else(|| AppError::msg("no terminal emulator for a Terminal=true app"))?;
            let mut a = vec!["-e".to_string()];
            a.push(prog.clone());
            a.extend(rest.iter().cloned());
            crate::launch::spawn(&term, &a, "")
        } else {
            crate::launch::spawn(prog, rest, "")
        }
    }

    /// XDG application directories, highest precedence first: user `extra_dirs`,
    /// then `$XDG_DATA_HOME` (+ its Flatpak exports), then each `$XDG_DATA_DIRS`,
    /// then the system Flatpak and Snap export roots.
    fn app_dirs(cfg: &Config) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();

        for d in &cfg.apps.extra_dirs {
            let d = d.trim();
            if !d.is_empty() {
                push_dir(&mut dirs, crate::config::expand_path(d));
            }
        }

        let home = dirs::home_dir();
        // Under Flatpak, $XDG_DATA_HOME is redirected into
        // ~/.var/app/<id>/data (empty), so honouring it would hide every
        // user-level entry — including `flatpak install --user` apps, the common
        // case. The real ~/.local/share is reachable via --filesystem=home;
        // reach it through $HOME, exactly as autostart.rs does.
        let data_home = if crate::launch::in_flatpak() {
            home.as_ref().map(|h| h.join(".local/share"))
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| home.as_ref().map(|h| h.join(".local/share")))
        };
        if let Some(dh) = &data_home {
            push_dir(&mut dirs, dh.join("applications"));
            push_dir(&mut dirs, dh.join("flatpak/exports/share/applications"));
        }

        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        for base in data_dirs.split(':').filter(|s| !s.is_empty()) {
            push_dir(&mut dirs, Path::new(base).join("applications"));
        }

        // Under Flatpak $XDG_DATA_DIRS points at the runtime's /usr, which has
        // almost nothing. The host's system apps are exposed at /run/host/usr
        // (--filesystem=host-os:ro).
        if crate::launch::in_flatpak() {
            push_dir(&mut dirs, PathBuf::from("/run/host/usr/share/applications"));
            push_dir(
                &mut dirs,
                PathBuf::from("/run/host/usr/local/share/applications"),
            );
        }

        push_dir(
            &mut dirs,
            PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        );
        push_dir(
            &mut dirs,
            PathBuf::from("/var/lib/snapd/desktop/applications"),
        );
        dirs
    }

    fn push_dir(dirs: &mut Vec<PathBuf>, p: PathBuf) {
        if p.is_dir() && !dirs.iter().any(|d| d == &p) {
            dirs.push(p);
        }
    }

    /// `(desktop-file id, path)` for every `*.desktop` under `root`. The id is
    /// the path relative to `root` with `/` turned into `-` (spec §"Desktop File
    /// ID"); subdirectories are walked a few levels deep.
    fn desktop_files(root: &Path) -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        walk(root, root, 0, &mut out);
        out
    }

    fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for ent in rd.flatten() {
            let Ok(ft) = ent.file_type() else { continue };
            let path = ent.path();
            if ft.is_dir() {
                if depth < 3 {
                    walk(root, &path, depth + 1, out);
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("desktop") {
                if let Ok(rel) = path.strip_prefix(root) {
                    let id = rel
                        .to_string_lossy()
                        .strip_suffix(".desktop")
                        .unwrap_or_default()
                        .replace('/', "-");
                    if !id.is_empty() {
                        out.push((id, path));
                    }
                }
            }
        }
    }

    /// Parse one `.desktop` file's `[Desktop Entry]` group into an [`AppEntry`],
    /// or `None` if it isn't a launchable, visible application.
    fn parse_entry(file: &Path, id: &str) -> Option<AppEntry> {
        let text = std::fs::read_to_string(file).ok()?;
        let mut kv: BTreeMap<String, String> = BTreeMap::new();
        let mut in_group = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_group = line == "[Desktop Entry]";
                continue;
            }
            if !in_group {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                kv.insert(k.trim().to_string(), v.trim().to_string());
            }
        }

        if kv.get("Type").map(|s| s.as_str()) != Some("Application") {
            return None;
        }
        if is_true(kv.get("NoDisplay")) || is_true(kv.get("Hidden")) {
            return None;
        }
        let exec_line = kv.get("Exec").map(String::as_str).unwrap_or("").trim();
        if exec_line.is_empty() {
            return None;
        }
        if let Some(te) = kv.get("TryExec").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let ok = if te.contains('/') {
                Path::new(te).is_file()
            } else {
                which(te).is_some()
            };
            if !ok {
                return None;
            }
        }

        let name = localized(&kv, "Name")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.to_string());
        let args = parse_exec(exec_line);
        if args.is_empty() {
            return None;
        }

        Some(AppEntry {
            name,
            exec: file.to_string_lossy().into_owned(),
            kind: AppKind::Desktop,
            args,
            icon: kv
                .get("Icon")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .and_then(resolve_icon),
            terminal: is_true(kv.get("Terminal")),
            source: "desktop".into(),
        })
    }

    fn is_true(v: Option<&String>) -> bool {
        v.map(|s| s.trim().eq_ignore_ascii_case("true")).unwrap_or(false)
    }

    /// `Name`, preferring a `Name[xx]` / `Name[xx_YY]` that matches `$LANG`.
    fn localized(kv: &BTreeMap<String, String>, key: &str) -> Option<String> {
        let lang = std::env::var("LC_MESSAGES")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default();
        let lang = lang.split('.').next().unwrap_or("").trim();
        if !lang.is_empty() {
            if let Some(v) = kv.get(&format!("{key}[{lang}]")) {
                return Some(v.clone());
            }
            if let Some((short, _)) = lang.split_once('_') {
                if let Some(v) = kv.get(&format!("{key}[{short}]")) {
                    return Some(v.clone());
                }
            }
        }
        kv.get(key).cloned()
    }

    /// Split an `Exec` value into argv, honouring the spec's double-quote
    /// quoting (`\\` and `\"` escapes inside quotes) and dropping field codes.
    /// `%%` becomes a literal `%`; any other `%x` is stripped.
    fn parse_exec(s: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut chars = s.chars().peekable();
        let mut in_quote = false;
        let mut has_tok = false;

        while let Some(c) = chars.next() {
            match c {
                '"' => {
                    in_quote = !in_quote;
                    has_tok = true;
                }
                '\\' if in_quote => {
                    if let Some(&n) = chars.peek() {
                        if n == '"' || n == '\\' || n == '`' || n == '$' {
                            cur.push(n);
                            chars.next();
                        } else {
                            cur.push('\\');
                        }
                    } else {
                        cur.push('\\');
                    }
                }
                c if c.is_whitespace() && !in_quote => {
                    if has_tok {
                        out.push(std::mem::take(&mut cur));
                        has_tok = false;
                    }
                }
                _ => {
                    cur.push(c);
                    has_tok = true;
                }
            }
        }
        if has_tok {
            out.push(cur);
        }

        out.into_iter()
            .filter_map(|tok| {
                if FIELD_CODES.contains(&tok.as_str()) {
                    return None;
                }
                let cleaned = strip_field_codes(&tok);
                if cleaned.is_empty() && !tok.is_empty() {
                    None
                } else {
                    Some(cleaned)
                }
            })
            .collect()
    }

    /// Replace `%%` → `%` and remove any remaining `%<char>` inside a token.
    fn strip_field_codes(tok: &str) -> String {
        if !tok.contains('%') {
            return tok.to_string();
        }
        let mut out = String::with_capacity(tok.len());
        let mut chars = tok.chars();
        while let Some(c) = chars.next() {
            if c == '%' {
                // `%%` is a literal percent; any other `%x` is a field code — drop it.
                if let Some('%') = chars.next() {
                    out.push('%');
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    // --- icon resolution --------------------------------------------------

    /// Resolve an `Icon=` value to a `data:` URI: an absolute path is used
    /// directly, otherwise the icon-theme search path is walked (current theme,
    /// then Adwaita / breeze / Papirus, then hicolor, then `pixmaps`).
    fn resolve_icon(name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        let p = Path::new(name);
        if p.is_absolute() {
            return p.is_file().then(|| encode_icon(p)).flatten();
        }

        for cand in icon_candidates(name) {
            if cand.is_file() {
                if let Some(uri) = encode_icon(&cand) {
                    return Some(uri);
                }
            }
        }
        None
    }

    // Both are invariant for the process and were being rebuilt once per app
    // icon — `icon_themes()` in particular forks `gsettings` on any non-GNOME
    // desktop. Memoised; a GTK theme change mid-session needs a restart to
    // re-resolve icons (a rescan alone reuses the cached themes).
    fn icon_roots() -> &'static [PathBuf] {
        static ROOTS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
        ROOTS.get_or_init(|| {
            let mut roots = Vec::new();
            if let Some(h) = dirs::home_dir() {
                roots.push(h.join(".local/share/icons"));
                roots.push(h.join(".icons"));
            }
            let data_dirs = std::env::var("XDG_DATA_DIRS")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
            for base in data_dirs.split(':').filter(|s| !s.is_empty()) {
                roots.push(Path::new(base).join("icons"));
            }
            roots.push(PathBuf::from("/usr/share/pixmaps"));
            // Host icon themes for the /run/host apps (see app_dirs).
            if crate::launch::in_flatpak() {
                roots.push(PathBuf::from("/run/host/usr/share/icons"));
                roots.push(PathBuf::from("/run/host/usr/local/share/icons"));
                roots.push(PathBuf::from("/run/host/usr/share/pixmaps"));
            }
            roots
        })
    }

    fn icon_themes() -> &'static [String] {
        static THEMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        THEMES.get_or_init(|| {
            let mut themes: Vec<String> = Vec::new();
            if let Some(t) = configured_icon_theme() {
                themes.push(t);
            }
            for d in ["Adwaita", "breeze", "Papirus", "hicolor", "gnome"] {
                if !themes.iter().any(|t| t == d) {
                    themes.push(d.to_string());
                }
            }
            themes
        })
    }

    /// The GTK icon theme from `settings.ini`, else `gsettings`, else `None`.
    fn configured_icon_theme() -> Option<String> {
        let cfg = dirs::config_dir()?;
        for f in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
            if let Ok(text) = std::fs::read_to_string(cfg.join(f)) {
                for line in text.lines() {
                    if let Some(v) = line.trim().strip_prefix("gtk-icon-theme-name") {
                        let v = v.trim_start_matches([' ', '=']).trim();
                        if !v.is_empty() {
                            return Some(v.to_string());
                        }
                    }
                }
            }
        }
        let out = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "icon-theme"])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let s = s.trim().trim_matches(['\'', '"']).trim();
        (!s.is_empty()).then(|| s.to_string())
    }

    fn icon_candidates(name: &str) -> Vec<PathBuf> {
        // Mid-size rasters first (crisp at the 20px the row renders without
        // bloating the cache), then scalable SVG, then the extremes.
        const SIZES: &[&str] = &[
            "48x48", "64x64", "96x96", "128x128", "scalable", "256x256", "32x32", "512x512",
            "24x24", "16x16",
        ];
        const EXTS: &[&str] = &["png", "svg"];

        let mut out = Vec::new();
        let roots = icon_roots();
        let themes = icon_themes();

        for root in roots {
            let is_pixmaps = root.ends_with("pixmaps");
            if is_pixmaps {
                for ext in EXTS.iter().chain(std::iter::once(&"xpm")) {
                    out.push(root.join(format!("{name}.{ext}")));
                }
                continue;
            }
            for theme in themes {
                for size in SIZES {
                    for ext in EXTS {
                        // freedesktop / hicolor layout
                        out.push(root.join(theme).join(size).join("apps").join(format!("{name}.{ext}")));
                        // breeze layout
                        out.push(root.join(theme).join("apps").join(size).join(format!("{name}.{ext}")));
                    }
                }
            }
        }
        out
    }

    fn encode_icon(path: &Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        if meta.len() == 0 || meta.len() > ICON_MAX_BYTES {
            return None;
        }
        let mime = match path.extension().and_then(|e| e.to_str()) {
            Some("svg") => "image/svg+xml",
            Some("png") => "image/png",
            _ => return None, // xpm et al. — browsers can't render these
        };
        let bytes = std::fs::read(path).ok()?;
        Some(format!("data:{mime};base64,{}", b64(&bytes)))
    }

    /// Standard-alphabet base64 with padding (no dependency pulled in for this).
    fn b64(data: &[u8]) -> String {
        const T: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            s.push(T[(n >> 18 & 63) as usize] as char);
            s.push(T[(n >> 12 & 63) as usize] as char);
            s.push(if chunk.len() > 1 {
                T[(n >> 6 & 63) as usize] as char
            } else {
                '='
            });
            s.push(if chunk.len() > 2 {
                T[(n & 63) as usize] as char
            } else {
                '='
            });
        }
        s
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct TmpDir(PathBuf);
        impl TmpDir {
            fn new(tag: &str) -> Self {
                let p = std::env::temp_dir().join(format!(
                    "dev-prompt-apps-{tag}-{}-{:?}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                std::fs::create_dir_all(&p).unwrap();
                TmpDir(p)
            }
            fn write(&self, rel: &str, body: &str) -> PathBuf {
                let f = self.0.join(rel);
                std::fs::create_dir_all(f.parent().unwrap()).unwrap();
                std::fs::write(&f, body).unwrap();
                f
            }
        }
        impl Drop for TmpDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn parse_exec_strips_field_codes_and_quotes() {
            assert_eq!(parse_exec("/usr/bin/foo %U"), vec!["/usr/bin/foo"]);
            assert_eq!(
                parse_exec("foo --flag %F --bar"),
                vec!["foo", "--flag", "--bar"]
            );
            assert_eq!(
                parse_exec(r#""/opt/My App/run" --name "a b" %f"#),
                vec!["/opt/My App/run", "--name", "a b"]
            );
            assert_eq!(parse_exec("env A=1 prog %U"), vec!["env", "A=1", "prog"]);
            // %% is a literal percent
            assert_eq!(parse_exec("prog 100%%done"), vec!["prog", "100%done"]);
        }

        #[test]
        fn parse_entry_filters_hidden_nondisplay_wrongtype_and_missing_tryexec() {
            let d = TmpDir::new("filter");
            let good = d.write(
                "ok.desktop",
                "[Desktop Entry]\nType=Application\nName=OK\nExec=/bin/true %U\n",
            );
            assert!(parse_entry(&good, "ok").is_some());

            let nd = d.write(
                "nd.desktop",
                "[Desktop Entry]\nType=Application\nName=ND\nExec=/bin/true\nNoDisplay=true\n",
            );
            assert!(parse_entry(&nd, "nd").is_none());

            let hidden = d.write(
                "h.desktop",
                "[Desktop Entry]\nType=Application\nName=H\nExec=/bin/true\nHidden=true\n",
            );
            assert!(parse_entry(&hidden, "h").is_none());

            let link = d.write(
                "l.desktop",
                "[Desktop Entry]\nType=Link\nName=L\nURL=https://example.com\n",
            );
            assert!(parse_entry(&link, "l").is_none());

            let te = d.write(
                "te.desktop",
                "[Desktop Entry]\nType=Application\nName=TE\nExec=/bin/true\nTryExec=definitely-not-a-real-binary-xyz\n",
            );
            assert!(parse_entry(&te, "te").is_none());
        }

        #[test]
        fn parse_entry_prefers_localized_name_and_reads_terminal() {
            let d = TmpDir::new("name");
            let f = d.write(
                "x.desktop",
                "[Desktop Entry]\nType=Application\nName=Plain\nName[fr]=Français\nExec=htop\nTerminal=true\n",
            );
            std::env::set_var("LANG", "fr_FR.UTF-8");
            let e = parse_entry(&f, "x").unwrap();
            assert_eq!(e.name, "Français");
            assert!(e.terminal);
            assert_eq!(e.kind, AppKind::Desktop);
            std::env::remove_var("LANG");
        }

        #[test]
        fn discover_id_precedence_first_dir_wins() {
            let hi = TmpDir::new("hi");
            let lo = TmpDir::new("lo");
            hi.write(
                "editor.desktop",
                "[Desktop Entry]\nType=Application\nName=HiEditor\nExec=/bin/true\n",
            );
            lo.write(
                "editor.desktop",
                "[Desktop Entry]\nType=Application\nName=LoEditor\nExec=/bin/true\n",
            );
            let mut cfg = crate::config::bundled_defaults();
            cfg.apps.enabled = true;
            cfg.apps.extra_dirs = vec![
                hi.0.to_string_lossy().into_owned(),
                lo.0.to_string_lossy().into_owned(),
            ];
            let apps = discover(&cfg);
            let e = apps.iter().find(|a| a.args == vec!["/bin/true"]).unwrap();
            assert_eq!(e.name, "HiEditor");
            assert_eq!(apps.iter().filter(|a| a.exec.ends_with("editor.desktop")).count(), 1);
        }

        #[test]
        fn discover_honours_exclude() {
            let d = TmpDir::new("excl");
            d.write(
                "keepme.desktop",
                "[Desktop Entry]\nType=Application\nName=KeepMe\nExec=/bin/true\n",
            );
            d.write(
                "zoomy.desktop",
                "[Desktop Entry]\nType=Application\nName=Zoomy\nExec=/bin/true\n",
            );
            let mut cfg = crate::config::bundled_defaults();
            cfg.apps.enabled = true;
            cfg.apps.extra_dirs = vec![d.0.to_string_lossy().into_owned()];
            cfg.apps.exclude = vec!["zoom".into()];
            let names: Vec<_> = discover(&cfg).into_iter().map(|a| a.name).collect();
            assert!(names.contains(&"KeepMe".to_string()));
            assert!(!names.iter().any(|n| n == "Zoomy"));
        }

        #[test]
        fn resolve_icon_takes_absolute_svg_path() {
            let d = TmpDir::new("icon");
            let svg = d.write("a/b/logo.svg", "<svg xmlns='http://www.w3.org/2000/svg'/>");
            let uri = resolve_icon(&svg.to_string_lossy()).unwrap();
            assert!(uri.starts_with("data:image/svg+xml;base64,"));
        }

        #[test]
        fn b64_matches_known_vectors() {
            assert_eq!(b64(b""), "");
            assert_eq!(b64(b"f"), "Zg==");
            assert_eq!(b64(b"fo"), "Zm8=");
            assert_eq!(b64(b"foo"), "Zm9v");
            assert_eq!(b64(b"foob"), "Zm9vYg==");
            assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        }
    }
}

// --- raw JSON from the PowerShell script ----------------------------------

#[derive(Debug, Deserialize)]
struct RawApp {
    name: Option<String>,
    exec: Option<String>,
    kind: Option<String>,
    #[serde(default)]
    args: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    product: Option<String>,
    #[serde(default)]
    company: Option<String>,
    #[serde(default)]
    size: u64,
}

/// An [`AppEntry`] plus the version-resource fields the culling passes need
/// (not persisted to `apps.json`).
struct Scored {
    entry: AppEntry,
    product: String,
    company: String,
    size: u64,
}

impl RawApp {
    fn into_scored(self, cfg: &Config) -> Option<Scored> {
        let name = self.name?.trim().to_string();
        let exec = self.exec?.trim().to_string();
        if name.is_empty() || exec.is_empty() {
            return None;
        }

        let kind = match self.kind.as_deref() {
            Some("aumid") => AppKind::Aumid,
            _ => AppKind::Exe,
        };

        if kind == AppKind::Exe && !keep_entry(&name, &exec) {
            return None;
        }

        // User excludes: case-insensitive substring on the name or the path.
        let hay = format!("{}\n{}", name.to_lowercase(), exec.to_lowercase());
        if cfg
            .apps
            .exclude
            .iter()
            .filter(|x| !x.trim().is_empty())
            .any(|x| hay.contains(&x.to_lowercase()))
        {
            return None;
        }

        let args = self
            .args
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(crate::rules::win_split_args)
            .unwrap_or_default();

        let icon = self
            .icon
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|b64| format!("data:image/png;base64,{b64}"));

        let source = self.source.unwrap_or_else(|| "scan".into());

        Some(Scored {
            entry: AppEntry {
                name,
                exec,
                kind,
                args,
                icon,
                terminal: false,
                source,
            },
            product: self.product.unwrap_or_default().trim().to_string(),
            company: self.company.unwrap_or_default().trim().to_string(),
            size: self.size,
        })
    }
}

// --- launch --------------------------------------------------------------

pub fn launch(entry: &AppEntry) -> AppResult<()> {
    match entry.kind {
        AppKind::Exe => {
            let cwd = std::path::Path::new(&entry.exec)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            crate::launch::spawn(&entry.exec, &entry.args, &cwd)
        }
        AppKind::Aumid => crate::launch::spawn(
            "explorer",
            &[format!("shell:AppsFolder\\{}", entry.exec)],
            "",
        ),
        AppKind::Desktop => {
            #[cfg(target_os = "linux")]
            {
                linux::launch(entry)
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(crate::error::AppError::msg(
                    "desktop-entry launch is Linux-only",
                ))
            }
        }
    }
}

// Windows-only: every case drives the discovery scoring (`keep_entry`,
// `prune_scanned`, `dedupe*`, `into_scored`, `looks_like_main_binary`) with
// `C:\`-style paths and `.exe` semantics. The code under test never runs off
// Windows, and some of the folder/stem parsing assumes `\` separators.
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn entry(name: &str, exec: &str, source: &str, icon: Option<&str>) -> AppEntry {
        AppEntry {
            name: name.into(),
            exec: exec.into(),
            kind: AppKind::Exe,
            args: vec![],
            icon: icon.map(String::from),
            terminal: false,
            source: source.into(),
        }
    }

    fn scored(name: &str, exec: &str, source: &str, product: &str, company: &str, size: u64) -> Scored {
        Scored {
            entry: entry(name, exec, source, None),
            product: product.into(),
            company: company.into(),
            size,
        }
    }

    #[test]
    fn keep_entry_rejects_noise_keeps_real_apps() {
        assert!(!keep_entry("Uninstall DBeaver", r"C:\Program Files\DBeaver\unins000.exe"));
        assert!(!keep_entry(
            "Microsoft Visual C++ 2015 Redistributable",
            r"C:\temp\vc_redist.x64.exe"
        ));
        assert!(!keep_entry("Notepad", r"C:\Windows\System32\notepad.exe"));
        assert!(!keep_entry("Something Setup", r"D:\dl\something-setup.exe"));
        assert!(!keep_entry("Elevate", r"C:\Users\me\AppData\Local\GitHubDesktop\Elevate.exe"));
        assert!(!keep_entry("app", r"D:\tools\foo\squirrel.exe"));

        assert!(keep_entry("DBeaver", r"C:\Program Files\DBeaver\dbeaver.exe"));
        assert!(keep_entry(
            "Visual Studio Code",
            r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe"
        ));
    }

    #[test]
    fn prune_scanned_keeps_only_the_main_binary_per_folder() {
        let got = prune_scanned(vec![
            scored("GitHub Desktop", r"C:\a\GitHubDesktop\GitHubDesktop.exe", "scan", "GitHub Desktop", "GitHub", 40),
            scored("tool", r"C:\a\GitHubDesktop\tool.exe", "scan", "GitHub Desktop", "GitHub", 10),
            // a curated entry in the same dir is never pruned
            scored("Sidecar", r"C:\a\GitHubDesktop\sidecar.exe", "start-menu", "", "", 0),
        ]);
        let names: Vec<&str> = got.iter().map(|s| s.entry.name.as_str()).collect();
        assert!(names.contains(&"GitHub Desktop"));
        assert!(names.contains(&"Sidecar"));
        assert!(!names.contains(&"tool"));
    }

    #[test]
    fn prune_scanned_leaves_single_exe_folders_and_extra_dirs_alone() {
        let got = prune_scanned(vec![scored(
            "mytool",
            r"D:\tools\mytool\mytool.exe",
            "extra",
            "",
            "",
            0,
        )]);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn dedupe_by_product_collapses_same_vendor_and_product() {
        let got = dedupe_by_product(vec![
            scored("FooLauncher", r"C:\a\FooLauncher.exe", "scan", "Foo", "Acme", 0),
            scored("Foo", r"C:\b\Foo.exe", "start-menu", "Foo", "Acme", 0),
            // no metadata -> never collapsed
            scored("bar", r"C:\c\bar.exe", "scan", "", "", 0),
        ]);
        let names: Vec<&str> = got.iter().map(|s| s.entry.name.as_str()).collect();
        assert_eq!(names, vec!["Foo", "bar"]);
    }

    #[test]
    fn dedupe_by_product_keeps_distinct_installs_that_tie_on_source() {
        // Two Python installs: same CompanyName/ProductName, same source rank.
        let got = dedupe_by_product(vec![
            scored("Python", r"C:\Py311\python.exe", "scan", "Python", "PSF", 0),
            scored("Python", r"C:\Py312\python.exe", "scan", "Python", "PSF", 0),
        ]);
        assert_eq!(got.len(), 2, "neither version should be silently dropped");
    }

    #[test]
    fn dedupe_keeps_best_source_and_an_icon() {
        let got = dedupe(vec![
            entry("code", r"C:\x\Code.exe", "scan", None),
            entry("Visual Studio Code", r"C:\x\code.exe", "start-menu", None),
            entry("code", r"C:\x\Code.EXE", "uninstall", Some("ICON")),
        ]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "Visual Studio Code"); // start-menu won the name
        assert_eq!(got[0].source, "start-menu");
        assert_eq!(got[0].icon.as_deref(), Some("ICON")); // icon carried over
    }

    #[test]
    fn dedupe_does_not_merge_aumid_with_exe() {
        let mut a = entry("Terminal", "Microsoft.WindowsTerminal_8we!app", "store", None);
        a.kind = AppKind::Aumid;
        let got = dedupe(vec![a, entry("Terminal", r"C:\x\wt.exe", "start-menu", None)]);
        assert_eq!(got.len(), 2);
    }

    fn raw(name: &str, exec: &str, kind: &str) -> RawApp {
        RawApp {
            name: Some(name.into()),
            exec: Some(exec.into()),
            kind: Some(kind.into()),
            args: None,
            icon: None,
            source: Some("scan".into()),
            product: None,
            company: None,
            size: 0,
        }
    }

    #[test]
    fn into_scored_maps_kind_trims_and_wraps_icon_and_args() {
        let cfg = crate::config::bundled_defaults();

        let mut r = raw(
            "  Windows Terminal ",
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            "aumid",
        );
        r.icon = Some("AAAA".into());
        r.source = Some("store".into());
        let s = r.into_scored(&cfg).unwrap();
        assert_eq!(s.entry.kind, AppKind::Aumid);
        assert_eq!(s.entry.name, "Windows Terminal"); // trimmed
        assert_eq!(s.entry.icon.as_deref(), Some("data:image/png;base64,AAAA"));

        let mut r = raw("VLC", r"C:\Program Files\VLC\vlc.exe", "exe");
        r.args = Some(r#"--fullscreen "--meta-title=My Movie""#.into());
        r.product = Some("  VideoLAN  ".into());
        r.size = 42;
        let s = r.into_scored(&cfg).unwrap();
        assert_eq!(s.entry.kind, AppKind::Exe);
        assert_eq!(s.entry.args, vec!["--fullscreen", "--meta-title=My Movie"]);
        assert_eq!(s.product, "VideoLAN"); // trimmed
        assert_eq!(s.size, 42);
    }

    #[test]
    fn into_scored_rejects_empty_and_exe_noise_but_not_aumids() {
        let cfg = crate::config::bundled_defaults();
        assert!(raw("", r"C:\x\a.exe", "exe").into_scored(&cfg).is_none());
        assert!(raw("Real", "   ", "exe").into_scored(&cfg).is_none());
        // keep_entry applies to exe rows…
        assert!(raw("Updater", r"C:\x\update.exe", "exe")
            .into_scored(&cfg)
            .is_none());
        // …but not to aumids (there's no path to judge).
        assert!(raw("Squirrel Thing", "Squirrel_pkg!App", "aumid")
            .into_scored(&cfg)
            .is_some());
    }

    #[test]
    fn looks_like_main_binary_matches_folder_or_product_either_direction() {
        assert!(looks_like_main_binary(
            r"C:\a\GitHubDesktop\GitHubDesktop.exe",
            "GitHubDesktop",
            ""
        ));
        // folder name carries a version suffix — stem is still contained
        assert!(looks_like_main_binary(r"C:\a\Foo-1.2.3\foo.exe", "Foo-1.2.3", ""));
        // stem echoes the product, not the folder
        assert!(looks_like_main_binary(
            r"C:\a\bin\launcher.exe",
            "bin",
            "Acme Launcher"
        ));
        // a satellite exe matches neither
        assert!(!looks_like_main_binary(
            r"C:\a\GitHubDesktop\elevate.exe",
            "GitHubDesktop",
            "GitHub Desktop"
        ));
        // nothing to compare against
        assert!(!looks_like_main_binary(r"C:\a\x\helper.exe", "", ""));
    }

    #[test]
    fn raw_app_excludes_by_user_pattern() {
        let mut cfg = crate::config::bundled_defaults();
        cfg.apps.exclude = vec!["zoom".into()];
        let raw = RawApp {
            name: Some("Zoom".into()),
            exec: Some(r"C:\Users\me\AppData\Roaming\Zoom\bin\Zoom.exe".into()),
            kind: Some("exe".into()),
            args: None,
            icon: None,
            source: Some("start-menu".into()),
            product: None,
            company: None,
            size: 0,
        };
        assert!(raw.into_scored(&cfg).is_none());
    }
}
