//! Installed-application discovery for the `>` app-launcher scope.
//!
//! Windows only. Enumeration is done by one embedded PowerShell script
//! (`DISCOVER_PS1`) that unions four sources — `Get-StartApps` (Win32 + Store),
//! Start Menu `.lnk` targets, the three Uninstall registry hives, and a bounded
//! `*.exe` scan of `%LOCALAPPDATA%\Programs` plus any user `extra_dirs`. The
//! script also extracts and disk-caches each app's icon.
//!
//! Rust does the culling: [`keep_entry`] drops installer/updater/helper noise,
//! [`prune_scanned`] keeps only the "main binary" per folder for the raw-scan
//! tiers, [`dedupe_by_product`] collapses same-vendor duplicates, and [`dedupe`]
//! merges entries that point at the same executable.
//!
//! On non-Windows [`discover`] returns an empty list — the `>` scope simply
//! shows nothing.

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    pub name: String,
    /// Executable path (`Exe`) or AppUserModelID (`Aumid`).
    pub exec: String,
    pub kind: AppKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// `data:image/png;base64,…` when an icon was extracted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// `start-menu` | `store` | `uninstall` | `scan` | `extra`.
    pub source: String,
    /// Times launched from dev-prompt (merged from `app-usage.json`).
    #[serde(default)]
    pub uses: u32,
}

/// Source ranking for dedupe — a curated Start Menu entry beats a raw scan hit.
fn source_rank(source: &str) -> u8 {
    match source {
        "start-menu" => 4,
        "store" => 3,
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

    let script = DISCOVER_PS1
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

/// Collapse entries that share a non-empty CompanyName *and* ProductName — the
/// per-vendor satellite exes (`FooLauncher.exe` next to `Foo.exe`). Keeps the
/// highest-ranked source. Entries missing either field pass through.
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
                if source_rank(&s.entry.source) > source_rank(&out[i].entry.source) {
                    if out[i].entry.icon.is_some() && s.entry.icon.is_none() {
                        let icon = out[i].entry.icon.take();
                        out[i] = s;
                        out[i].entry.icon = icon;
                    } else {
                        out[i] = s;
                    }
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

#[cfg(not(windows))]
pub fn discover(_cfg: &Config) -> Vec<AppEntry> {
    Vec::new()
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
            .map(crate::rules::shell_split)
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
                source,
                uses: 0,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, exec: &str, source: &str, icon: Option<&str>) -> AppEntry {
        AppEntry {
            name: name.into(),
            exec: exec.into(),
            kind: AppKind::Exe,
            args: vec![],
            icon: icon.map(String::from),
            source: source.into(),
            uses: 0,
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
