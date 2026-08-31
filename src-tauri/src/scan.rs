use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub name: String,
    pub path: String,
    /// Sentinel names/extensions found in this directory (e.g. `.git`, `.sln`).
    pub sentinels: Vec<String>,
    /// Unix seconds when this repo was last observed on disk.
    pub last_seen: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Directory names we never descend into — they never *contain* a project root
/// worth surfacing and they dominate walk time.
const HARD_SKIP: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
    ".gradle",
    ".next",
    ".svelte-kit",
];

/// Does `entry_name` satisfy `sentinel`?
///
/// - exact match on the file/dir name (`.git`, `package.json`, `Cargo.toml`)
/// - or, when the sentinel looks like a bare extension (`.sln`), an extension match
fn sentinel_matches(sentinel: &str, entry_name: &str) -> bool {
    if sentinel.eq_ignore_ascii_case(entry_name) {
        return true;
    }
    let looks_like_ext = sentinel.starts_with('.')
        && sentinel.len() > 1
        && sentinel[1..].chars().all(|c| c.is_ascii_alphanumeric());
    if looks_like_ext {
        if let Some(ext) = Path::new(entry_name).extension().and_then(|e| e.to_str()) {
            return sentinel[1..].eq_ignore_ascii_case(ext);
        }
    }
    false
}

fn dir_sentinels(dir: &Path, sentinels: &[String]) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut hits: Vec<String> = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name();
        let Some(name) = name.to_str() else { continue };
        for s in sentinels {
            if sentinel_matches(s, name) && !hits.contains(s) {
                hits.push(s.clone());
            }
        }
    }
    hits.sort();
    hits
}

/// Walk every configured root and return the discovered repositories, sorted by
/// name. Nested repos are collapsed to their outermost match.
pub fn scan(roots: &[PathBuf], cfg: &Config) -> Vec<Repo> {
    let mut found: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    let now = now_secs();

    for root in roots {
        let mut builder = WalkBuilder::new(root);
        builder
            .max_depth(Some(cfg.scan.max_depth))
            .follow_links(false)
            .hidden(false) // we want to see `.git`, `.svelte-kit`, etc.
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .parents(false)
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !HARD_SKIP.iter().any(|s| name.eq_ignore_ascii_case(s))
            });

        for dent in builder.build().flatten() {
            if dent.file_type().map(|t| t.is_dir()) != Some(true) {
                continue;
            }
            let dir = dent.path();
            let hits = dir_sentinels(dir, &cfg.scan.sentinels);
            if !hits.is_empty() {
                found.entry(dir.to_path_buf()).or_insert(hits);
            }
        }
    }

    // Collapse nested repos: drop any path whose ancestor is also a repo.
    let all: Vec<PathBuf> = found.keys().cloned().collect();
    let mut repos: Vec<Repo> = Vec::new();
    for path in &all {
        let nested = all
            .iter()
            .any(|other| other != path && path.starts_with(other));
        if nested {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        repos.push(Repo {
            name,
            path: path.to_string_lossy().into_owned(),
            sentinels: found.get(path).cloned().unwrap_or_default(),
            last_seen: now,
        });
    }

    repos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    repos
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn sentinel_exact_and_extension() {
        assert!(sentinel_matches(".git", ".git"));
        assert!(sentinel_matches("Cargo.toml", "Cargo.toml"));
        assert!(sentinel_matches(".sln", "MyApp.sln"));
        assert!(!sentinel_matches(".sln", "MyApp.csproj"));
        assert!(!sentinel_matches("package.json", "package-lock.json"));
    }

    #[test]
    fn finds_repos_and_collapses_nested() {
        let tmp = std::env::temp_dir().join(format!("dp-scan-{}", now_secs()));
        let _ = fs::remove_dir_all(&tmp);
        touch(&tmp.join("alpha/.git/HEAD"));
        touch(&tmp.join("alpha/Cargo.toml"));
        touch(&tmp.join("beta/package.json"));
        // nested repo inside alpha — should be collapsed away
        touch(&tmp.join("alpha/vendor/inner/.git/HEAD"));

        let cfg = Config::default();
        let repos = scan(&[tmp.clone()], &cfg);
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);

        let _ = fs::remove_dir_all(&tmp);
    }
}
