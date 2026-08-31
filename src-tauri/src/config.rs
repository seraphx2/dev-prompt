use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const APP_DIR: &str = "dev-prompt";
pub const CONFIG_FILE: &str = "config.yaml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Config {
    /// Accelerator string understood by `tauri-plugin-global-shortcut`.
    pub hotkey: String,
    /// Root directories to scan for repositories. `~`, `%VAR%` and `$VAR` are expanded.
    pub roots: Vec<String>,
    pub scan: ScanConfig,
    /// How long a cached repo list is considered fresh.
    pub cache_ttl_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct ScanConfig {
    pub max_depth: usize,
    /// A directory is treated as a repo if it directly contains any of these names.
    pub sentinels: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: "CmdOrCtrl+Shift+Space".into(),
            roots: default_roots(),
            scan: ScanConfig::default(),
            cache_ttl_secs: 900,
        }
    }
}

impl Default for ScanConfig {
    fn default() -> Self {
        ScanConfig {
            max_depth: 4,
            sentinels: [
                ".git",
                ".sln",
                "package.json",
                "Cargo.toml",
                "pyproject.toml",
                "go.mod",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

fn default_roots() -> Vec<String> {
    // A reasonable first guess per platform; the user edits config.yaml afterwards.
    #[cfg(windows)]
    {
        vec!["C:\\Projects".into(), "%USERPROFILE%\\source\\repos".into()]
    }
    #[cfg(not(windows))]
    {
        vec!["~/src".into(), "~/projects".into()]
    }
}

pub fn config_dir() -> AppResult<PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| AppError::msg("no OS config directory"))?
        .join(APP_DIR);
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

pub fn cache_dir() -> AppResult<PathBuf> {
    let base = dirs::cache_dir()
        .ok_or_else(|| AppError::msg("no OS cache directory"))?
        .join(APP_DIR);
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

pub fn config_path() -> AppResult<PathBuf> {
    Ok(config_dir()?.join(CONFIG_FILE))
}

/// Load `config.yaml`, writing a default file on first run.
pub fn load() -> AppResult<Config> {
    let path = config_path()?;
    if !path.exists() {
        let cfg = Config::default();
        save(&cfg)?;
        return Ok(cfg);
    }
    let text = std::fs::read_to_string(&path)?;
    let cfg: Config = serde_yaml_ng::from_str(&text)?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> AppResult<()> {
    let path = config_path()?;
    let header = "# dev-prompt configuration\n\
                  # roots: directories scanned for repositories (~, %VAR%, $VAR expanded)\n\
                  # hotkey: global accelerator to toggle the overlay\n\n";
    let body = serde_yaml_ng::to_string(cfg)?;
    std::fs::write(&path, format!("{header}{body}"))?;
    Ok(())
}

/// Expand `~`, `%VAR%` (Windows-style) and `$VAR` (POSIX-style) in a path string.
pub fn expand_path(raw: &str) -> PathBuf {
    let mut s = raw.trim().to_string();

    if let Some(rest) = s.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            let rest = rest.trim_start_matches(['/', '\\']);
            return home.join(rest);
        }
    }

    // %VAR%
    while let Some(start) = s.find('%') {
        if let Some(end_rel) = s[start + 1..].find('%') {
            let end = start + 1 + end_rel;
            let name = &s[start + 1..end];
            let val = std::env::var(name).unwrap_or_default();
            s.replace_range(start..=end, &val);
        } else {
            break;
        }
    }

    // $VAR (stops at the first non-alphanumeric/underscore character)
    while let Some(start) = s.find('$') {
        let rest = &s[start + 1..];
        let end_rel = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        if end_rel == 0 {
            break;
        }
        let name = &rest[..end_rel];
        let val = std::env::var(name).unwrap_or_default();
        s.replace_range(start..start + 1 + end_rel, &val);
    }

    PathBuf::from(s)
}

/// Expand + keep only roots that currently exist as directories.
pub fn resolved_roots(cfg: &Config) -> Vec<PathBuf> {
    cfg.roots
        .iter()
        .map(|r| expand_path(r))
        .filter(|p| p.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_path("~/foo"), home.join("foo"));
        assert_eq!(expand_path("~\\foo"), home.join("foo"));
    }

    #[test]
    fn expands_env_windows_style() {
        std::env::set_var("DP_TEST_VAR", "xyz");
        assert_eq!(expand_path("a/%DP_TEST_VAR%/b"), PathBuf::from("a/xyz/b"));
    }

    #[test]
    fn expands_env_posix_style() {
        std::env::set_var("DP_TEST_VAR2", "qq");
        assert_eq!(expand_path("a/$DP_TEST_VAR2/b"), PathBuf::from("a/qq/b"));
    }

    #[test]
    fn unmatched_percent_is_left_alone() {
        assert_eq!(expand_path("100%done"), PathBuf::from("100%done"));
    }

    #[test]
    fn default_config_roundtrips_through_yaml() {
        let cfg = Config::default();
        let text = serde_yaml_ng::to_string(&cfg).unwrap();
        let back: Config = serde_yaml_ng::from_str(&text).unwrap();
        assert_eq!(back.hotkey, cfg.hotkey);
        assert_eq!(back.scan.max_depth, cfg.scan.max_depth);
    }
}
