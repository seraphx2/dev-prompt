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

/// Bump `exec`'s hit in `map` (keyed case-insensitively). Pure — the I/O is in
/// [`bump`].
fn apply(mut map: HashMap<String, Hit>, exec: &str, now: u64) -> HashMap<String, Hit> {
    let e = map.entry(exec.to_lowercase()).or_default();
    e.count = e.count.saturating_add(1);
    e.last = now;
    map
}

/// Record one launch of `exec` (keyed case-insensitively).
pub fn bump(exec: &str) {
    let map = apply(read(), exec, now_secs());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_lowercases_the_key_and_inserts_new() {
        let m = apply(HashMap::new(), r"C:\Apps\X.EXE", 100);
        assert_eq!(m.len(), 1);
        let hit = &m[r"c:\apps\x.exe"];
        assert_eq!(hit.count, 1);
        assert_eq!(hit.last, 100);
    }

    #[test]
    fn apply_increments_and_advances_last_on_repeat() {
        let m = apply(HashMap::new(), "a.exe", 10);
        let m = apply(m, "A.exe", 20);
        let m = apply(m, "a.exe", 30);
        assert_eq!(m["a.exe"].count, 3);
        assert_eq!(m["a.exe"].last, 30);
    }

    #[test]
    fn counts_projection_survives_a_json_round_trip() {
        let m = apply(apply(HashMap::new(), "a.exe", 1), "b.exe", 2);
        let back: HashMap<String, Hit> =
            serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        let counts: HashMap<String, u32> =
            back.into_iter().map(|(k, v)| (k, v.count)).collect();
        assert_eq!(counts["a.exe"], 1);
        assert_eq!(counts["b.exe"], 1);
    }
}
