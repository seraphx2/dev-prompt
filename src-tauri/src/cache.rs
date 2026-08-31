use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::cache_dir;
use crate::error::AppResult;
use crate::scan::Repo;

pub const CACHE_FILE: &str = "repos.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CacheFile {
    /// Unix seconds when this list was written.
    written_at: u64,
    repos: Vec<Repo>,
}

pub struct LoadedCache {
    pub repos: Vec<Repo>,
    /// Age in seconds, or -1 when there was no cache file.
    pub age_secs: i64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> AppResult<PathBuf> {
    Ok(cache_dir()?.join(CACHE_FILE))
}

pub fn load() -> AppResult<LoadedCache> {
    let path = cache_path()?;
    if !path.exists() {
        return Ok(LoadedCache {
            repos: Vec::new(),
            age_secs: -1,
        });
    }
    let text = std::fs::read_to_string(&path)?;
    let parsed: CacheFile = serde_json::from_str(&text).unwrap_or_default();
    let age = now_secs().saturating_sub(parsed.written_at) as i64;
    Ok(LoadedCache {
        repos: parsed.repos,
        age_secs: age,
    })
}

pub fn save(repos: &[Repo]) -> AppResult<()> {
    let path = cache_path()?;
    let file = CacheFile {
        written_at: now_secs(),
        repos: repos.to_vec(),
    };
    std::fs::write(&path, serde_json::to_string_pretty(&file)?)?;
    Ok(())
}

pub fn is_stale(age_secs: i64, ttl_secs: u64) -> bool {
    age_secs < 0 || age_secs as u64 >= ttl_secs
}

/// Merge a fresh scan over the previous cache: fresh entries win; anything not in
/// the fresh set is considered gone and dropped.
pub fn merge(previous: &[Repo], fresh: Vec<Repo>) -> Vec<Repo> {
    use std::collections::HashMap;
    let prev_by_path: HashMap<&str, &Repo> =
        previous.iter().map(|r| (r.path.as_str(), r)).collect();

    fresh
        .into_iter()
        .map(|mut r| {
            // Preserve the earliest `last_seen` we know about for stable "recency".
            if let Some(p) = prev_by_path.get(r.path.as_str()) {
                r.last_seen = r.last_seen.max(p.last_seen);
            }
            r
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staleness_rules() {
        assert!(is_stale(-1, 900));
        assert!(is_stale(900, 900));
        assert!(is_stale(1000, 900));
        assert!(!is_stale(10, 900));
    }
}
