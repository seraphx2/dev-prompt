//! App-launch frecency. A tiny JSON map in the cache dir records how often each
//! app is launched from dev-prompt so the `>` scope can float favourites to the
//! top. `last` is stored for a future recency decay; ranking currently uses
//! `count` alone.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::cache_dir;

const USAGE_FILE: &str = "app-usage.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Hit {
    count: u32,
    last: u64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn path() -> Option<std::path::PathBuf> {
    cache_dir().ok().map(|d| d.join(USAGE_FILE))
}

fn read() -> HashMap<String, Hit> {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Record one launch of `exec` (keyed case-insensitively).
pub fn bump(exec: &str) {
    let key = exec.to_lowercase();
    let mut map = read();
    let e = map.entry(key).or_default();
    e.count = e.count.saturating_add(1);
    e.last = now_secs();
    if let Some(p) = path() {
        if let Ok(json) = serde_json::to_string(&map) {
            let _ = std::fs::write(p, json);
        }
    }
}

/// `exec (lowercased) -> launch count`, for merging into the app list.
pub fn counts() -> HashMap<String, u32> {
    read().into_iter().map(|(k, v)| (k, v.count)).collect()
}
