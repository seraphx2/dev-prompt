use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::config::{self, Config};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub name: String,
    pub path: String,
    /// VCS label ("Git", "Mercurial", …) when a `kind: vcs` marker matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vcs: Option<String>,
    /// Non-VCS discovery markers found here (e.g. `sln`, `package.json`).
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

/// Compile the discovery globs, keeping the successfully-added patterns in
/// add-order so `GlobSet::matches` indices line up.
fn build_globset(globs: &[String]) -> (GlobSet, Vec<String>) {
    let mut builder = GlobSetBuilder::new();
    let mut valid = Vec::new();
    for g in globs {
        if let Ok(glob) = Glob::new(&g.to_lowercase()) {
            builder.add(glob);
            valid.push(g.clone());
        }
    }
    (
        builder.build().unwrap_or_else(|_| GlobSet::empty()),
        valid,
    )
}

/// Which markers are present directly in `dir` (deduped, display-cleaned).
fn dir_hits(dir: &Path, set: &GlobSet, globs: &[String]) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut hits: Vec<String> = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name();
        let Some(name) = name.to_str() else { continue };
        for idx in set.matches(name.to_lowercase()) {
            let label = globs[idx].trim_start_matches("*.").to_string();
            if !hits.contains(&label) {
                hits.push(label);
            }
        }
    }
    hits.sort();
    hits
}

/// Walk every configured root and return the discovered repositories, sorted by
/// name. Nested repos are collapsed to their outermost match.
pub fn scan(roots: &[PathBuf], cfg: &Config) -> Vec<Repo> {
    let (set, globs) = build_globset(&config::discovery_globs(cfg));
    let vcs: HashMap<String, String> = config::vcs_markers(cfg).into_iter().collect();
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
            let hits = dir_hits(dir, &set, &globs);
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

        // Split the hits: the first VCS marker becomes the badge, the rest are
        // plain sentinel chips.
        let mut repo_vcs = None;
        let mut sentinels = Vec::new();
        for hit in found.get(path).cloned().unwrap_or_default() {
            match vcs.get(&hit) {
                Some(label) if repo_vcs.is_none() => repo_vcs = Some(label.clone()),
                _ => sentinels.push(hit),
            }
        }

        repos.push(Repo {
            name,
            path: path.to_string_lossy().into_owned(),
            vcs: repo_vcs,
            sentinels,
            last_seen: now,
        });
    }

    repos.sort_by_key(|r| r.name.to_lowercase());
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
    fn globset_matches_names_and_extensions() {
        let (set, globs) = build_globset(&[
            ".git".into(),
            "package.json".into(),
            "*.sln".into(),
        ]);
        assert_eq!(dir_hits_from(&set, &globs, &[".git"]), vec![".git"]);
        assert_eq!(dir_hits_from(&set, &globs, &["MyApp.sln"]), vec!["sln"]);
        assert!(dir_hits_from(&set, &globs, &["package-lock.json"]).is_empty());
    }

    fn dir_hits_from(set: &GlobSet, globs: &[String], names: &[&str]) -> Vec<String> {
        let mut hits: Vec<String> = Vec::new();
        for name in names {
            for idx in set.matches(name.to_lowercase()) {
                let label = globs[idx].trim_start_matches("*.").to_string();
                if !hits.contains(&label) {
                    hits.push(label);
                }
            }
        }
        hits.sort();
        hits
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

        let cfg = config::bundled_defaults();
        let repos = scan(std::slice::from_ref(&tmp), &cfg);
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);

        // `.git` becomes the VCS badge, not a plain sentinel.
        let alpha = &repos[0];
        assert_eq!(alpha.vcs.as_deref(), Some("Git"));
        assert!(!alpha.sentinels.iter().any(|s| s == ".git"));
        assert!(alpha.sentinels.iter().any(|s| s == "Cargo.toml"));
        assert_eq!(repos[1].vcs, None); // beta has only package.json

        let _ = fs::remove_dir_all(&tmp);
    }
}
