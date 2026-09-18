//! Per-repo action frecency. A JSON store records a continuously-decaying
//! score per `(repo path, action id)` so the universal action list can float
//! favourites to the top — scoped per repo, since which action wins in one
//! repo (say, opening it in Visual Studio) says nothing about which one wins
//! in another (see `docs/rules-engine.md` — this backs the universal tier's
//! usage-based reorder, not the `Detected` group).
//!
//! Decay, not a raw lifetime count: a score halves every `HALF_LIFE_SECS` of
//! disuse, so an old favourite picked 100 times last year doesn't permanently
//! outrank something picked 10 times this week. `bump` folds the decay owed
//! since the last hit into the stored score before adding the new one;
//! `scores` projects every stored score forward to "now" the same way, without
//! writing anything, so ranking reflects the moment it's read rather than the
//! moment of the last selection.
//!
//! Lives in the *config* dir alongside `app-usage.json`, not the cache dir:
//! accumulated user history, not a regenerable index — see `usage.rs`.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::config::config_dir;

const USAGE_FILE: &str = "action-usage.json";

/// A score halves every this many seconds of disuse.
const HALF_LIFE_SECS: f64 = 14.0 * 24.0 * 60.0 * 60.0;

/// Serialises the read-modify-write in [`bump`] — see the matching lock in
/// `usage.rs` for why this is defensive rather than load-bearing today.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Hit {
    score: f64,
    last: u64,
}

/// repo path -> action id -> Hit.
type Store = HashMap<String, HashMap<String, Hit>>;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn path() -> Option<std::path::PathBuf> {
    config_dir().ok().map(|d| d.join(USAGE_FILE))
}

fn read() -> Store {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Write `json` to `path` atomically — a sibling temp file plus a rename, same
/// as `usage::write_atomic`.
fn write_atomic(path: &std::path::Path, json: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// `score` as decayed from `last` up to `now`.
fn decayed(score: f64, last: u64, now: u64) -> f64 {
    let elapsed = now.saturating_sub(last) as f64;
    score * 0.5f64.powf(elapsed / HALF_LIFE_SECS)
}

/// Bump `action_id`'s score for `repo_path`. Pure — the I/O is in [`bump`].
fn apply(mut store: Store, repo_path: &str, action_id: &str, now: u64) -> Store {
    let hit = store
        .entry(repo_path.to_string())
        .or_default()
        .entry(action_id.to_string())
        .or_default();
    hit.score = decayed(hit.score, hit.last, now) + 1.0;
    hit.last = now;
    store
}

/// Record one selection of `action_id` in `repo_path`.
pub fn bump(repo_path: &str, action_id: &str) {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let store = apply(read(), repo_path, action_id, now_secs());
    if let Some(p) = path() {
        if let Ok(json) = serde_json::to_string(&store) {
            let _ = write_atomic(&p, &json);
        }
    }
}

/// `action id -> current (decayed-to-now) score` for one repo. Empty when the
/// repo has no history yet.
pub fn scores(repo_path: &str) -> HashMap<String, f64> {
    let now = now_secs();
    read()
        .remove(repo_path)
        .unwrap_or_default()
        .into_iter()
        .map(|(id, hit)| (id, decayed(hit.score, hit.last, now)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_inserts_new_and_accumulates_on_repeat() {
        let s = apply(Store::new(), "/repo", "vscode", 100);
        assert_eq!(s["/repo"]["vscode"].score, 1.0);
        assert_eq!(s["/repo"]["vscode"].last, 100);

        let s = apply(s, "/repo", "vscode", 100); // no elapsed time — pure add
        assert_eq!(s["/repo"]["vscode"].score, 2.0);
    }

    #[test]
    fn apply_scopes_by_repo() {
        let s = apply(Store::new(), "/a", "vscode", 100);
        let s = apply(s, "/b", "idea", 100);
        assert!(!s["/a"].contains_key("idea"));
        assert!(!s["/b"].contains_key("vscode"));
    }

    #[test]
    fn decayed_halves_after_one_half_life() {
        let half_life = HALF_LIFE_SECS as u64;
        assert!((decayed(10.0, 0, half_life) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn decayed_is_unchanged_with_no_elapsed_time() {
        assert_eq!(decayed(4.0, 100, 100), 4.0);
    }

    #[test]
    fn score_projection_survives_a_json_round_trip_and_decays() {
        let store = apply(Store::new(), "/repo", "vscode", 0);
        let back: Store = serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
        let half_life = HALF_LIFE_SECS as u64;
        let hit = &back["/repo"]["vscode"];
        assert!((decayed(hit.score, hit.last, half_life) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn write_atomic_replaces_the_file_and_leaves_no_temp() {
        let dir =
            std::env::temp_dir().join(format!("dp-action-usage-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("action-usage.json");
        std::fs::write(&p, "OLD").unwrap();

        write_atomic(&p, "NEW").unwrap();

        assert_eq!(std::fs::read_to_string(&p).unwrap(), "NEW");
        assert!(
            !p.with_extension("json.tmp").exists(),
            "temp file should be renamed away, not left behind"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
