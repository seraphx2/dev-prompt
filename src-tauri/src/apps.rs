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
//!
//! **Incremental updates.** Gathering is the slow part (seconds; tens of
//! seconds with a cold disk) and combining is pure in-memory work, so the
//! cache keeps every [`Gathered`] row — tagged with the `origin` it came from
//! (a `.lnk` file, an Uninstall entry, an app folder, a `.desktop` file) — in
//! an [`Index`], and [`combine`] rebuilds the list from it. When the app
//! watcher reports a [`Change`], [`update`] re-gathers only the origins it
//! touched and swaps their rows, so the result is exactly what a full rescan
//! of the same state would give.

use std::collections::BTreeMap;
use std::path::PathBuf;

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

/// One gathered row, before the sources are combined — an [`AppEntry`] plus
/// where it came from and the fields the combining passes need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gathered {
    /// The item this row came from, replaced as a unit: `lnk:<path>`, `store`,
    /// `uninstall:<tag>\<subkey>`, `scan:<app folder>`, `extra:<app folder>`
    /// (Windows) or a `.desktop` path (Linux).
    pub origin: String,
    pub entry: AppEntry,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub product: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub company: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub size: u64,
    /// Linux: the desktop-file id, and the precedence of the directory it was
    /// found in (lower wins).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub rank: u32,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

/// Everything gathered, kept in `apps.json` so a change can be applied
/// without gathering it all again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub gathered: Vec<Gathered>,
    /// Windows: last-write time of each Uninstall entry (`<tag>\<subkey>`),
    /// which is how a registry change is narrowed to the entries it touched.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uninstall_stamps: BTreeMap<String, u64>,
}

/// Something the app watcher saw that may change the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A file or folder under a watched app location.
    Path(PathBuf),
    /// Something under an Uninstall registry key.
    Uninstall,
    /// A Store / MSIX package came or went.
    Store,
}

/// Gather every source in full — what Ctrl+R in `>` and the daily check run.
/// Disabled apps (`apps.enabled: false`) gather nothing.
pub fn gather_all(cfg: &Config, sources: &Sources) -> AppResult<Index> {
    if !cfg.apps.enabled {
        return Ok(Index::default());
    }
    gather_full(cfg, sources)
}

/// Apply `changes` to `index`: re-gather only the origins they touched and
/// swap those rows. `combine` of the result equals `combine` of a full gather
/// of the same state.
pub fn update(cfg: &Config, sources: &Sources, index: &Index, changes: &[Change]) -> AppResult<Index> {
    if !cfg.apps.enabled {
        return Ok(Index::default());
    }
    update_index(cfg, sources, index, changes)
}

/// Rows in a fixed order, so combining doesn't depend on whether they arrived
/// from a full gather or a series of updates.
fn canonical(index: &Index) -> Vec<Gathered> {
    let mut rows = index.gathered.clone();
    rows.sort_by(|a, b| {
        (a.origin.to_lowercase(), a.entry.exec.to_lowercase(), &a.entry.name)
            .cmp(&(b.origin.to_lowercase(), b.entry.exec.to_lowercase(), &b.entry.name))
    });
    rows
}

/// Whether `origin` is `target` itself or lies beneath it (a shortcut inside
/// a Start Menu folder that was deleted, say). `/` == `\`; case-insensitive
/// on Windows.
fn origin_within(origin: &str, target: &str) -> bool {
    let norm = |s: &str| {
        let s = s.replace('\\', "/").trim_end_matches('/').to_string();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    let (o, t) = (norm(origin), norm(target));
    o == t || o.starts_with(&format!("{t}/"))
}

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

/// Where the Windows sources live. [`Sources::from_config`] gives the real
/// ones; tests point every field at throwaway folders and registry keys.
#[cfg(windows)]
#[derive(Debug, Clone, Default)]
pub struct Sources {
    /// Start Menu `Programs` folders (all users, then this user).
    pub menu_roots: Vec<PathBuf>,
    /// `%LOCALAPPDATA%\Programs` — per-user installs.
    pub scan_root: Option<PathBuf>,
    /// The user's `apps.extra_dirs`, expanded.
    pub extra_dirs: Vec<PathBuf>,
    /// `(tag, full key path)` of each Uninstall key, e.g.
    /// `("HKCU", r"HKEY_CURRENT_USER\SOFTWARE\…\Uninstall")`.
    pub uninstall_roots: Vec<(String, String)>,
    /// Gather Store apps (`Get-StartApps`).
    pub store: bool,
    /// Registry key that changes when a Store package is (un)installed.
    pub store_key: Option<String>,
}

#[cfg(windows)]
const UNINSTALL_SUFFIX: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

#[cfg(windows)]
impl Sources {
    pub fn from_config(cfg: &Config) -> Self {
        let env_dir = |var: &str, rel: &str| std::env::var_os(var).map(|v| PathBuf::from(v).join(rel));
        let menu = r"Microsoft\Windows\Start Menu\Programs";
        Sources {
            menu_roots: [env_dir("ProgramData", menu), env_dir("APPDATA", menu)]
                .into_iter()
                .flatten()
                .collect(),
            scan_root: env_dir("LOCALAPPDATA", "Programs"),
            extra_dirs: cfg
                .apps
                .extra_dirs
                .iter()
                .map(|d| d.trim())
                .filter(|d| !d.is_empty())
                .map(crate::config::expand_path)
                .collect(),
            uninstall_roots: vec![
                ("HKLM".into(), format!(r"HKEY_LOCAL_MACHINE\{UNINSTALL_SUFFIX}")),
                (
                    "HKLM32".into(),
                    r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
                        .into(),
                ),
                ("HKCU".into(), format!(r"HKEY_CURRENT_USER\{UNINSTALL_SUFFIX}")),
            ],
            store: true,
            store_key: Some(
                r"HKEY_CURRENT_USER\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages"
                    .into(),
            ),
        }
    }

    /// Folders the app watcher watches (recursively).
    pub fn watch_dirs(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.menu_roots.clone();
        out.extend(self.scan_root.clone());
        out.extend(self.extra_dirs.iter().cloned());
        out.retain(|d| d.is_dir());
        out
    }

    /// Registry keys the app watcher watches, and what a change in each means.
    pub fn watch_keys(&self) -> Vec<(String, Change)> {
        let mut out: Vec<(String, Change)> = self
            .uninstall_roots
            .iter()
            .map(|(_, path)| (path.clone(), Change::Uninstall))
            .collect();
        out.extend(self.store_key.clone().map(|k| (k, Change::Store)));
        out
    }
}

/// What the script gathers — see the header of `discover_apps.ps1`. `None`
/// means "all of that source", `Some(list)` only those items.
#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatherParams {
    store: bool,
    menu_roots: Vec<String>,
    menus: Option<Vec<String>>,
    uninstall_roots: Vec<UninstallRoot>,
    uninstall: Option<Vec<String>>,
    scan_root: Option<String>,
    scan: Option<Vec<String>>,
    extra_dirs: Vec<String>,
    extra: Option<Vec<String>>,
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
struct UninstallRoot {
    tag: String,
    path: String,
}

#[cfg(windows)]
impl GatherParams {
    fn new(sources: &Sources) -> Self {
        let s = |p: &PathBuf| p.to_string_lossy().into_owned();
        GatherParams {
            store: sources.store,
            menu_roots: sources.menu_roots.iter().map(s).collect(),
            menus: None,
            uninstall_roots: sources
                .uninstall_roots
                .iter()
                .map(|(tag, path)| UninstallRoot { tag: tag.clone(), path: path.clone() })
                .collect(),
            uninstall: None,
            scan_root: sources.scan_root.as_ref().map(s),
            scan: None,
            extra_dirs: sources.extra_dirs.iter().map(s).collect(),
            extra: None,
        }
    }
}

#[cfg(windows)]
fn gather_full(cfg: &Config, sources: &Sources) -> AppResult<Index> {
    Ok(Index {
        gathered: run_gather(cfg, &GatherParams::new(sources))?,
        uninstall_stamps: uninstall_stamps(&sources.uninstall_roots),
    })
}

/// Run the script with `params` and turn its rows into [`Gathered`] ones,
/// dropping noise and the user's excludes.
#[cfg(windows)]
fn run_gather(cfg: &Config, params: &GatherParams) -> AppResult<Vec<Gathered>> {
    let icon_dir = crate::config::cache_dir()
        .map(|p| p.join("app-icons"))
        .unwrap_or_else(|_| PathBuf::from(r"dev-prompt\cache\app-icons"));
    let script = DISCOVER_PS1
        .replace("__ICON_DIR__", &icon_dir.to_string_lossy())
        .replace("__PARAMS__", &serde_json::to_string(params)?)
        .replace("__ICON_CAP__", "320");

    let json = run_powershell(&script)?;
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return Err(crate::error::AppError::msg("app discovery printed nothing"));
    }
    let raw: Vec<RawApp> = serde_json::from_str(trimmed)?;
    Ok(raw.into_iter().filter_map(|r| r.into_gathered(cfg)).collect())
}

#[cfg(windows)]
fn update_index(cfg: &Config, sources: &Sources, index: &Index, changes: &[Change]) -> AppResult<Index> {
    let mut params = GatherParams::new(sources);
    params.store = false;
    params.menus = Some(Vec::new());
    params.uninstall = Some(Vec::new());
    params.scan = Some(Vec::new());
    params.extra = Some(Vec::new());

    // The origins whose rows get replaced.
    let mut replace: Vec<String> = Vec::new();
    let mut stamps = index.uninstall_stamps.clone();

    for change in changes {
        match change {
            Change::Store => {
                if sources.store && !params.store {
                    params.store = true;
                    replace.push("store".into());
                }
            }
            Change::Uninstall => {
                // Registry notifications don't say which entry changed:
                // compare last-write times to find the ones that did.
                let now = uninstall_stamps(&sources.uninstall_roots);
                let ids = params.uninstall.get_or_insert_with(Vec::new);
                for (id, stamp) in &now {
                    if stamps.get(id) != Some(stamp) && !ids.contains(id) {
                        ids.push(id.clone());
                        replace.push(format!("uninstall:{id}"));
                    }
                }
                for id in stamps.keys().filter(|id| !now.contains_key(*id)) {
                    replace.push(format!("uninstall:{id}"));
                }
                stamps = now;
            }
            Change::Path(path) => {
                let Some((kind, item)) = locate(sources, path) else { continue };
                let item_s = item.to_string_lossy().into_owned();
                let list = match kind {
                    "lnk" => params.menus.get_or_insert_with(Vec::new),
                    "scan" => params.scan.get_or_insert_with(Vec::new),
                    _ => params.extra.get_or_insert_with(Vec::new),
                };
                if !list.iter().any(|x| x.eq_ignore_ascii_case(&item_s)) {
                    list.push(item_s.clone());
                    replace.push(format!("{kind}:{item_s}"));
                }
            }
        }
    }

    if replace.is_empty() {
        return Ok(Index { gathered: index.gathered.clone(), uninstall_stamps: stamps });
    }
    let fresh = run_gather(cfg, &params)?;
    let mut gathered: Vec<Gathered> = index
        .gathered
        .iter()
        .filter(|g| !replace.iter().any(|r| origin_within(&g.origin, r)))
        .cloned()
        .collect();
    gathered.extend(fresh);
    Ok(Index { gathered, uninstall_stamps: stamps })
}

/// Which source a changed path belongs to, and the unit to re-gather: the
/// shortcut (or Start Menu folder) itself, or the app folder directly under a
/// scanned root. `None` for a path no source covers.
#[cfg(windows)]
fn locate(sources: &Sources, path: &std::path::Path) -> Option<(&'static str, PathBuf)> {
    let under = |root: &std::path::Path| -> Option<PathBuf> {
        let rel = strip_prefix_ci(path, root)?;
        let first = rel.components().next()?;
        Some(root.join(first))
    };
    if sources.menu_roots.iter().any(|r| strip_prefix_ci(path, r).is_some()) {
        // A shortcut, or a folder of them. Other files (desktop.ini) don't matter.
        let is_lnk = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
        return (is_lnk || !path.is_file()).then(|| ("lnk", path.to_path_buf()));
    }
    if let Some(item) = sources.scan_root.as_deref().and_then(under) {
        return Some(("scan", item));
    }
    sources.extra_dirs.iter().find_map(|d| under(d)).map(|item| ("extra", item))
}

/// `path` relative to `root`, comparing components without regard to case —
/// Windows paths from different APIs disagree on it.
#[cfg(windows)]
fn strip_prefix_ci(path: &std::path::Path, root: &std::path::Path) -> Option<PathBuf> {
    let lower = |c: std::path::Component| c.as_os_str().to_string_lossy().to_lowercase();
    let mut rest = path.components();
    for rc in root.components() {
        if lower(rest.next()?) != lower(rc) {
            return None;
        }
    }
    Some(rest.as_path().to_path_buf())
}

/// Last-write time of every subkey of each Uninstall key, by `<tag>\<subkey>`.
#[cfg(windows)]
pub fn uninstall_stamps(roots: &[(String, String)]) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for (tag, path) in roots {
        let Some(key) = open_key(path) else { continue };
        for name in key.enum_keys().flatten() {
            let Ok(sub) = key.open_subkey(&name) else { continue };
            let Ok(info) = sub.query_info() else { continue };
            let ft = &*info.last_write_time;
            let stamp = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
            out.insert(format!(r"{tag}\{name}"), stamp);
        }
    }
    out
}

/// Open `HKEY_…\sub\key` for reading (and change notification).
#[cfg(windows)]
pub fn open_key(path: &str) -> Option<winreg::RegKey> {
    use winreg::enums::*;
    let (hive, sub) = path.split_once('\\')?;
    let root = match hive.to_ascii_uppercase().as_str() {
        "HKEY_LOCAL_MACHINE" | "HKLM" => HKEY_LOCAL_MACHINE,
        "HKEY_CURRENT_USER" | "HKCU" => HKEY_CURRENT_USER,
        _ => return None,
    };
    winreg::RegKey::predef(root)
        .open_subkey_with_flags(sub, KEY_READ | KEY_NOTIFY)
        .ok()
}

/// Combine every gathered row into the list the `>` scope shows: cull the
/// raw-scan noise, fold same-product duplicates, merge same-executable ones.
#[cfg(windows)]
pub fn combine(index: &Index) -> Vec<AppEntry> {
    let rows = prune_scanned(canonical(index));
    let rows = dedupe_by_product(rows);
    dedupe(rows.into_iter().map(|s| s.entry).collect())
}

/// For the raw-scan tiers (`scan` / `extra`), keep only the executable that
/// looks like the app in each folder — the one whose name echoes the folder or
/// product name. When none do, keep just the largest. Folders with a single
/// candidate, and all curated sources, pass through untouched.
fn prune_scanned(scored: Vec<Gathered>) -> Vec<Gathered> {
    use std::collections::HashMap;

    let (scanned, mut kept): (Vec<Gathered>, Vec<Gathered>) = scored
        .into_iter()
        .partition(|s| matches!(s.entry.source.as_str(), "scan" | "extra"));

    let mut by_dir: HashMap<String, Vec<Gathered>> = HashMap::new();
    for s in scanned {
        by_dir.entry(parent_dir(&s.entry.exec)).or_default().push(s);
    }

    for (dir, group) in by_dir {
        if group.len() == 1 {
            kept.extend(group);
            continue;
        }
        let folder = dir_leaf(&dir).to_string();
        let (matched, rest): (Vec<Gathered>, Vec<Gathered>) = group
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
fn dedupe_by_product(scored: Vec<Gathered>) -> Vec<Gathered> {
    use std::collections::HashMap;
    let mut out: Vec<Gathered> = Vec::new();
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
pub use linux::{combine, Sources};
#[cfg(target_os = "linux")]
use linux::{gather_full, update_index};

/// Nothing to discover elsewhere.
#[cfg(not(any(windows, target_os = "linux")))]
#[derive(Debug, Clone, Default)]
pub struct Sources;

#[cfg(not(any(windows, target_os = "linux")))]
impl Sources {
    pub fn from_config(_cfg: &Config) -> Self {
        Sources
    }
    pub fn watch_dirs(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn gather_full(_cfg: &Config, _sources: &Sources) -> AppResult<Index> {
    Ok(Index::default())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn update_index(_cfg: &Config, _sources: &Sources, index: &Index, _changes: &[Change]) -> AppResult<Index> {
    Ok(index.clone())
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn combine(index: &Index) -> Vec<AppEntry> {
    index.gathered.iter().map(|g| g.entry.clone()).collect()
}

// --- Linux: freedesktop .desktop discovery ------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use super::{origin_within, AppEntry, AppKind, Change, Gathered, Index};
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

    /// The XDG application directories, highest precedence first, and the
    /// user's excludes. Tests point `app_dirs` at throwaway folders.
    #[derive(Debug, Clone, Default)]
    pub struct Sources {
        pub app_dirs: Vec<PathBuf>,
        pub excludes: Vec<String>,
    }

    impl Sources {
        pub fn from_config(cfg: &Config) -> Self {
            Sources {
                app_dirs: app_dirs(cfg),
                excludes: cfg
                    .apps
                    .exclude
                    .iter()
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect(),
            }
        }

        /// Folders the app watcher watches (recursively).
        pub fn watch_dirs(&self) -> Vec<PathBuf> {
            self.app_dirs.clone()
        }
    }

    /// The combined list for `cfg` — a full gather, then [`combine`].
    pub fn discover(cfg: &Config) -> Vec<AppEntry> {
        super::gather_all(cfg, &Sources::from_config(cfg))
            .map(|i| combine(&i))
            .unwrap_or_default()
    }

    /// One `.desktop` file as a row, unless it isn't a launchable app or the
    /// user excluded it.
    fn gathered(sources: &Sources, rank: usize, id: &str, file: &Path) -> Option<Gathered> {
        let entry = parse_entry(file, id)?;
        let hay = format!("{}\n{}", entry.name.to_lowercase(), id.to_lowercase());
        if sources.excludes.iter().any(|x| hay.contains(x)) {
            return None;
        }
        Some(Gathered {
            origin: file.to_string_lossy().into_owned(),
            entry,
            product: String::new(),
            company: String::new(),
            size: 0,
            id: id.to_string(),
            rank: rank as u32,
        })
    }

    pub(super) fn gather_full(_cfg: &Config, sources: &Sources) -> AppResult<Index> {
        let mut out = Vec::new();
        for (rank, dir) in sources.app_dirs.iter().enumerate() {
            for (id, file) in desktop_files(dir) {
                out.extend(gathered(sources, rank, &id, &file));
            }
        }
        Ok(Index { gathered: out, ..Index::default() })
    }

    /// Re-read just the `.desktop` files (or folders of them) that changed.
    pub(super) fn update_index(
        _cfg: &Config,
        sources: &Sources,
        index: &Index,
        changes: &[Change],
    ) -> AppResult<Index> {
        let mut rows = index.gathered.clone();
        for change in changes {
            let Change::Path(path) = change else { continue };
            let Some((rank, root)) = sources
                .app_dirs
                .iter()
                .enumerate()
                .find(|(_, d)| path.starts_with(d))
            else {
                continue;
            };
            let target = path.to_string_lossy();
            rows.retain(|g| !origin_within(&g.origin, &target));

            let mut found = Vec::new();
            if path.is_dir() {
                let depth = path.strip_prefix(root).map(|r| r.components().count()).unwrap_or(0);
                walk(root, path, depth, &mut found);
            } else if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("desktop") {
                if let Some(id) = desktop_id(root, path) {
                    found.push((id, path.clone()));
                }
            }
            for (id, file) in found {
                rows.extend(gathered(sources, rank, &id, &file));
            }
        }
        Ok(Index { gathered: rows, ..Index::default() })
    }

    /// One entry per desktop-file id — the one from the highest-precedence
    /// directory (an `~/.local/share` override shadows `/usr`) — by name.
    pub fn combine(index: &Index) -> Vec<AppEntry> {
        let mut rows = super::canonical(index);
        rows.sort_by_key(|g| g.rank); // stable: ties keep the canonical order
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<AppEntry> = rows
            .into_iter()
            .filter(|g| seen.insert(g.id.clone()))
            .map(|g| g.entry)
            .collect();
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
                if let Some(id) = desktop_id(root, &path) {
                    out.push((id, path));
                }
            }
        }
    }

    /// The desktop-file id: `path` relative to `root`, `.desktop` dropped,
    /// `/` turned into `-` (spec §"Desktop File ID").
    fn desktop_id(root: &Path, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(root).ok()?;
        let id = rel
            .to_string_lossy()
            .strip_suffix(".desktop")
            .unwrap_or_default()
            .replace('/', "-");
        (!id.is_empty()).then_some(id)
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
    #[serde(default)]
    origin: Option<String>,
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

impl RawApp {
    fn into_gathered(self, cfg: &Config) -> Option<Gathered> {
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

        Some(Gathered {
            origin: self.origin.unwrap_or_default(),
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
            id: String::new(),
            rank: 0,
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

    fn scored(name: &str, exec: &str, source: &str, product: &str, company: &str, size: u64) -> Gathered {
        Gathered {
            origin: String::new(),
            entry: entry(name, exec, source, None),
            product: product.into(),
            company: company.into(),
            size,
            id: String::new(),
            rank: 0,
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
            origin: None,
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
        let s = r.into_gathered(&cfg).unwrap();
        assert_eq!(s.entry.kind, AppKind::Aumid);
        assert_eq!(s.entry.name, "Windows Terminal"); // trimmed
        assert_eq!(s.entry.icon.as_deref(), Some("data:image/png;base64,AAAA"));

        let mut r = raw("VLC", r"C:\Program Files\VLC\vlc.exe", "exe");
        r.args = Some(r#"--fullscreen "--meta-title=My Movie""#.into());
        r.product = Some("  VideoLAN  ".into());
        r.size = 42;
        let s = r.into_gathered(&cfg).unwrap();
        assert_eq!(s.entry.kind, AppKind::Exe);
        assert_eq!(s.entry.args, vec!["--fullscreen", "--meta-title=My Movie"]);
        assert_eq!(s.product, "VideoLAN"); // trimmed
        assert_eq!(s.size, 42);
    }

    #[test]
    fn into_scored_rejects_empty_and_exe_noise_but_not_aumids() {
        let cfg = crate::config::bundled_defaults();
        assert!(raw("", r"C:\x\a.exe", "exe").into_gathered(&cfg).is_none());
        assert!(raw("Real", "   ", "exe").into_gathered(&cfg).is_none());
        // keep_entry applies to exe rows…
        assert!(raw("Updater", r"C:\x\update.exe", "exe")
            .into_gathered(&cfg)
            .is_none());
        // …but not to aumids (there's no path to judge).
        assert!(raw("Squirrel Thing", "Squirrel_pkg!App", "aumid")
            .into_gathered(&cfg)
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
            origin: None,
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
        assert!(raw.into_gathered(&cfg).is_none());
    }
}
