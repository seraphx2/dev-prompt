//! Installed-application discovery for the `>` app-launcher scope.
//!
//! Windows only. Enumeration is done by one embedded PowerShell script
//! (`DISCOVER_PS1`) that unions four sources — `Get-StartApps` (Win32 + Store),
//! Start Menu `.lnk` targets, the three Uninstall registry hives, and a bounded
//! `*.exe` scan of `%LOCALAPPDATA%\Programs` plus any user `extra_dirs`. The
//! script also extracts and disk-caches each app's icon. Rust then filters
//! (`keep_entry` + `config.apps.exclude`) and dedupes by executable path.
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
    /// `start-menu` | `store` | `uninstall` | `scan`.
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
        _ => 1, // "scan"
    }
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
        "crashpad", "crash handler", "crashhandler", "helper", "vc_redist",
        "vcredist", "redistributable", "web installer", "repair",
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
    let entries = raw
        .into_iter()
        .filter_map(|r| r.into_entry(cfg))
        .collect::<Vec<_>>();
    dedupe(entries)
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
}

impl RawApp {
    fn into_entry(self, cfg: &Config) -> Option<AppEntry> {
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

        Some(AppEntry {
            name,
            exec,
            kind,
            args,
            icon,
            source,
            uses: 0,
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

    #[test]
    fn keep_entry_rejects_noise_keeps_real_apps() {
        assert!(!keep_entry("Uninstall DBeaver", r"C:\Program Files\DBeaver\unins000.exe"));
        assert!(!keep_entry(
            "Microsoft Visual C++ 2015 Redistributable",
            r"C:\temp\vc_redist.x64.exe"
        ));
        assert!(!keep_entry("Notepad", r"C:\Windows\System32\notepad.exe"));
        assert!(!keep_entry("Something Setup", r"D:\dl\something-setup.exe"));

        assert!(keep_entry("DBeaver", r"C:\Program Files\DBeaver\dbeaver.exe"));
        assert!(keep_entry(
            "Visual Studio Code",
            r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe"
        ));
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
        };
        assert!(raw.into_entry(&cfg).is_none());
    }
}
