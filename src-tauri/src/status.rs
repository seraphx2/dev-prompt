//! Keeps every listed repository's working-copy state (`vcs_state`) current
//! for the repo list's flags. Reads run on a few background threads — never
//! on the UI thread, never in the scan — and are asked for three ways:
//!
//! - the watcher, when something in a repo changes (debounced per repo, so a
//!   build writing a thousand files is one read);
//! - the overlay opening, for every repo not read in the last [`SWEEP_AGE`] —
//!   the backstop for whatever a watcher misses (a network share, Linux's
//!   unwatched depths);
//! - a repo's action menu opening, or a git operation finishing, right away.
//!
//! Each read that changes something is pushed to the frontend
//! (`status:updated`), and the whole set is saved to `status.json` whenever
//! the queue runs dry, so the next launch shows the last-known flags at once.
//! A folder without version control, or whose tool isn't installed, has no
//! status at all — nothing is shown for it, nothing errors.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;
use crate::rules::which;
use crate::vcs_state::{self, RepoStatus};

/// Reads running at once. Each is a few short git processes; more than this
/// only competes with whatever the user is doing.
const WORKERS: usize = 3;
/// The overlay opening re-reads repos older than this.
pub const SWEEP_AGE: u64 = 300;
/// A watched change waits this long for the burst it's part of to finish…
const QUIET: Duration = Duration::from_millis(750);
/// …but never longer than this while changes keep coming.
const MAX_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusEvent {
    pub path: String,
    /// `None`: the repo has no status any more.
    pub status: Option<RepoStatus>,
}

#[derive(Default)]
struct Queue {
    /// Path → (when it may run, when it was first asked for).
    pending: HashMap<String, (Instant, Instant)>,
    running: HashSet<String>,
}

#[derive(Default)]
pub struct Statuses {
    map: Mutex<HashMap<String, RepoStatus>>,
    queue: Mutex<Queue>,
    wake: Condvar,
    started: OnceLock<()>,
    /// Something changed since the last save.
    dirty: AtomicBool,
}

impl Statuses {
    pub fn load() -> Self {
        let map = crate::cache::load_statuses().unwrap_or_default();
        Statuses { map: Mutex::new(map), ..Default::default() }
    }

    pub fn snapshot(&self) -> HashMap<String, RepoStatus> {
        self.map.lock().unwrap().clone()
    }

    pub fn get(&self, path: &str) -> Option<RepoStatus> {
        self.map.lock().unwrap().get(path).cloned()
    }

    /// Each repo's branch, for matching a search against.
    pub fn branches(&self) -> HashMap<String, String> {
        self.map
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(p, s)| s.branch.clone().map(|b| (p.clone(), b)))
            .collect()
    }

    /// Store `status` for `path` (or drop it) and tell the frontend when
    /// anything besides the read time changed.
    fn put(&self, app: &AppHandle, path: &str, status: Option<RepoStatus>) {
        let changed = {
            let mut map = self.map.lock().unwrap();
            let before = map.get(path).cloned();
            let same = match (&before, &status) {
                (Some(a), Some(b)) => RepoStatus { checked_at: 0, ..a.clone() } == RepoStatus { checked_at: 0, ..b.clone() },
                (None, None) => true,
                _ => false,
            };
            match &status {
                Some(s) => map.insert(path.to_string(), s.clone()),
                None => map.remove(path),
            };
            !same
        };
        self.dirty.store(true, Ordering::Relaxed);
        if changed {
            let _ = app.emit("status:updated", StatusEvent { path: path.to_string(), status });
        }
    }

    /// Queue `paths` for a read: `debounce` for a watched change (it waits for
    /// the burst to end), otherwise as soon as a worker is free.
    pub fn request(&self, app: &AppHandle, paths: impl IntoIterator<Item = String>, debounce: bool) {
        self.start(app);
        let now = Instant::now();
        let mut q = self.queue.lock().unwrap();
        for p in paths {
            let entry = q.pending.entry(p).or_insert((now, now));
            entry.0 = due_at(now, entry.1, debounce);
        }
        drop(q);
        self.wake.notify_all();
    }

    /// Start the workers, once.
    fn start(&self, app: &AppHandle) {
        self.started.get_or_init(|| {
            for _ in 0..WORKERS {
                let app = app.clone();
                std::thread::spawn(move || worker(&app));
            }
        });
    }

    /// Wait for the next path that's due and not already being read.
    fn next(&self) -> String {
        let mut q = self.queue.lock().unwrap();
        loop {
            let now = Instant::now();
            let due = q
                .pending
                .iter()
                .filter(|(p, _)| !q.running.contains(*p))
                .min_by_key(|(_, (ready, _))| *ready)
                .map(|(p, (ready, _))| (p.clone(), *ready));
            match due {
                None => q = self.wake.wait(q).unwrap(),
                Some((_, ready)) if ready > now => q = self.wake.wait_timeout(q, ready - now).unwrap().0,
                Some((p, _)) => {
                    q.pending.remove(&p);
                    q.running.insert(p.clone());
                    return p;
                }
            }
        }
    }

    /// Mark `path` read; true when that leaves nothing queued or running.
    fn finish(&self, path: &str) -> bool {
        let mut q = self.queue.lock().unwrap();
        q.running.remove(path);
        let idle = q.pending.is_empty() && q.running.is_empty();
        drop(q);
        self.wake.notify_all();
        idle
    }

    fn save_if_dirty(&self) {
        if self.dirty.swap(false, Ordering::Relaxed) {
            if let Err(e) = crate::cache::save_statuses(&self.snapshot()) {
                eprintln!("saving status.json failed: {e}");
            }
        }
    }
}

/// When a repo first asked for at `first` may be read, asked for again `now`.
fn due_at(now: Instant, first: Instant, debounce: bool) -> Instant {
    if debounce { (now + QUIET).min(first + MAX_WAIT) } else { now }
}

/// The status `dir` should show: `None` for no version control, or when its
/// tool isn't installed (it just shows nothing, like any missing tool).
fn read_for(dir: &Path) -> Option<RepoStatus> {
    let label = vcs_state::detect(dir)?;
    which(vcs_state::tool_for(label))?;
    Some(vcs_state::read_background(dir))
}

fn worker(app: &AppHandle) {
    let state = app.state::<AppState>();
    loop {
        let path = state.statuses.next();
        // Only listed repos: the watcher or a sweep may name one that's gone.
        let listed = state.repos.lock().unwrap().iter().any(|r| r.path == path);
        let status = if listed { read_for(Path::new(&path)) } else { None };
        state.statuses.put(app, &path, status);
        if state.statuses.finish(&path) {
            state.statuses.save_if_dirty();
        }
    }
}

/// Read `path` now, on this thread, and store the result — after a git
/// operation, whose caller wants the new state before it returns.
pub fn refresh_now(app: &AppHandle, path: &str) -> Option<RepoStatus> {
    let state = app.state::<AppState>();
    let status = read_for(Path::new(path));
    state.statuses.put(app, path, status.clone());
    state.statuses.save_if_dirty();
    status
}

/// Queue every listed repo not read in the last `max_age` seconds, and drop
/// statuses of repos no longer listed.
pub fn sweep(app: &AppHandle, max_age: u64) {
    let state = app.state::<AppState>();
    let repos: Vec<String> = state.repos.lock().unwrap().iter().map(|r| r.path.clone()).collect();
    let listed: HashSet<&str> = repos.iter().map(String::as_str).collect();
    let gone: Vec<String> = state.statuses.map.lock().unwrap().keys().filter(|p| !listed.contains(p.as_str())).cloned().collect();
    for p in gone {
        state.statuses.put(app, &p, None);
    }
    let now = vcs_state::now_secs();
    let map = state.statuses.snapshot();
    let due: Vec<String> = repos
        .into_iter()
        .filter(|p| !map.get(p).is_some_and(|s| now.saturating_sub(s.checked_at) < max_age))
        .collect();
    if due.is_empty() {
        state.statuses.save_if_dirty();
    } else {
        state.statuses.request(app, due, false);
    }
}

/// The watcher saw something change in `repo`.
pub fn changed(app: &AppHandle, repo: String) {
    app.state::<AppState>().statuses.request(app, [repo], true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_debounce_but_never_past_the_cap() {
        let now = Instant::now();
        assert_eq!(due_at(now, now, false), now);
        assert_eq!(due_at(now, now, true), now + QUIET);
        // A repo that keeps changing is still read once the cap is reached.
        let first = now - MAX_WAIT;
        assert_eq!(due_at(now, first, true), now);
    }

    #[test]
    fn next_hands_out_due_paths_once() {
        let s = Statuses::default();
        let now = Instant::now();
        {
            let mut q = s.queue.lock().unwrap();
            q.pending.insert("x".into(), (now, now));
        }
        assert_eq!(s.next(), "x");
        assert!(s.queue.lock().unwrap().running.contains("x"));
        assert!(s.finish("x"));
    }

    #[test]
    fn a_folder_without_version_control_has_no_status() {
        let dir = vcs_state::tests::tmp("status-plain");
        assert_eq!(read_for(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
