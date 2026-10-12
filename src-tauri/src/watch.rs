//! Keeps the repo list current while dev-prompt runs. Each root is watched;
//! when something that can change the list happens — a discovery marker
//! created or deleted, a folder added or removed directly in a root, a listed
//! repo deleted or renamed, a folder moved in — a rescan runs once things go
//! quiet. Everything else (git's own churn inside `.git`, builds writing to
//! `target/`, editing files) is dropped without doing anything.
//!
//! The open-time staleness check (`cache_ttl_secs`) stays as the backstop for
//! whatever a watcher can miss: network shares that don't report changes, the
//! Linux inotify watch limit, events lost while the watcher is rebuilt.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use globset::GlobSet;
use notify::event::{EventKind, ModifyKind, RenameMode};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};

use crate::commands::AppState;
use crate::config::{self, Config};
use crate::scan::HARD_SKIP;

/// Rescan this long after the last relevant change — one rescan per burst
/// (a clone, an unzip, `git init` writing a dozen files).
const QUIET: Duration = Duration::from_secs(1);
/// …but never put it off longer than this while changes keep coming.
const MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Signal {
    /// Linux only: a folder appeared that needs its own inotify watch.
    Rewatch,
    /// The repo list may have changed.
    Rescan,
}

/// What the event handler needs to judge a path, captured per [`sync`].
struct Filter {
    roots: Vec<PathBuf>,
    max_depth: usize,
    top_level_folders: bool,
    discovery: GlobSet,
    /// VCS marker names (`.git`, `.hg`, …): nothing below one matters.
    vcs_dirs: Vec<String>,
}

impl Filter {
    fn new(cfg: &Config) -> Self {
        Filter {
            roots: config::resolved_roots(cfg),
            max_depth: cfg.scan.max_depth,
            top_level_folders: cfg.scan.top_level_folders,
            discovery: config::compile_globset(&config::discovery_globs(cfg)),
            vcs_dirs: config::vcs_markers(cfg).into_iter().map(|(n, _)| n.to_lowercase()).collect(),
        }
    }

    /// A folder whose contents never change the repo list.
    fn is_opaque(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        HARD_SKIP.iter().any(|s| lower == s.to_lowercase()) || self.vcs_dirs.contains(&lower)
    }

    /// What `path` changing in the way `kind` describes means for the list.
    /// `holds_repo(p)` says whether a listed repo is at or below `p`.
    fn classify(&self, path: &Path, kind: &EventKind, holds_repo: &dyn Fn(&Path) -> bool) -> Option<Signal> {
        let root = self.roots.iter().find(|r| path.starts_with(r))?;
        let rel: Vec<String> = path
            .strip_prefix(root)
            .ok()?
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let Some((last, above)) = rel.split_last() else {
            return Some(Signal::Rescan); // the root itself went or moved
        };
        if above.iter().any(|seg| self.is_opaque(seg)) {
            return None; // inside `.git`, `node_modules`, …
        }
        // A marker sits *in* a project folder, so it can be one level below
        // the deepest folder the scan visits.
        if rel.len() > self.max_depth + 1 {
            return None;
        }

        if self.discovery.is_match(last.to_lowercase()) {
            return Some(Signal::Rescan); // a marker came or went
        }

        let removed = matches!(
            kind,
            EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From))
        );
        let added = matches!(
            kind,
            EventKind::Create(_)
                | EventKind::Modify(ModifyKind::Name(RenameMode::To | RenameMode::Both | RenameMode::Any))
        );

        if removed || matches!(kind, EventKind::Modify(ModifyKind::Name(_))) {
            // A listed repo — or a folder holding some — deleted or renamed away.
            if holds_repo(path) {
                return Some(Signal::Rescan);
            }
        }

        if rel.len() == 1 && self.top_level_folders && (removed || (added && path.is_dir())) {
            return Some(Signal::Rescan); // a folder added or removed directly in a root
        }

        if added && rel.len() <= self.max_depth && path.is_dir() && !self.is_opaque(last) {
            // A folder moved or copied in may already be a repo (or hold some).
            if matches!(kind, EventKind::Modify(ModifyKind::Name(_))) {
                return Some(Signal::Rescan);
            }
            // Created in place: a repo only if a marker is already inside.
            if has_marker(path, &self.discovery) {
                return Some(Signal::Rescan);
            }
            if cfg!(target_os = "linux") {
                return Some(Signal::Rewatch);
            }
        }
        None
    }
}

fn has_marker(dir: &Path, discovery: &GlobSet) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| discovery.is_match(e.file_name().to_string_lossy().to_lowercase()))
    })
}

/// (Re)build the watcher for the current config. Called at startup and after
/// anything that can change the roots, the scan depth or the markers.
pub fn sync(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    let filter = Filter::new(&cfg);
    let roots = filter.roots.clone();
    let max_depth = filter.max_depth;

    let tx = signals(app).clone();
    let handler_app = app.clone();
    let holds_repo = move |p: &Path| {
        let state = handler_app.state::<AppState>();
        let repos = state.repos.lock().unwrap();
        repos.iter().any(|r| Path::new(&r.path).starts_with(p))
    };
    let made = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let signal = match res {
            // The OS dropped events (buffer overflow): assume the worst.
            Err(_) => Some(Signal::Rescan),
            Ok(ev) if ev.need_rescan() => Some(Signal::Rescan),
            Ok(ev) => match ev.kind {
                EventKind::Access(_)
                | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)) => None,
                ref kind => ev
                    .paths
                    .iter()
                    .filter_map(|p| filter.classify(p, kind, &holds_repo))
                    .max(),
            },
        };
        if let Some(s) = signal {
            let _ = tx.send(s);
        }
    });
    let mut watcher = match made {
        Ok(w) => w,
        Err(e) => {
            eprintln!("repo watcher unavailable: {e}");
            *state.watcher.lock().unwrap() = None;
            return;
        }
    };

    for root in &roots {
        if let Err(e) = watch_root(&mut watcher, root, max_depth, &cfg) {
            // Typically the inotify watch limit; the open-time check covers it.
            eprintln!("watching {} stopped early: {e}", root.display());
            break;
        }
    }
    *state.watcher.lock().unwrap() = Some(watcher);
}

/// [`sync`] on its own thread — for callers on the UI thread, since on Linux
/// it walks the roots.
pub fn sync_in_background(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || sync(&app));
}

/// Windows: one recursive watch per root is a single OS handle.
#[cfg(not(target_os = "linux"))]
fn watch_root(w: &mut RecommendedWatcher, root: &Path, _max_depth: usize, _cfg: &Config) -> notify::Result<()> {
    w.watch(root, RecursiveMode::Recursive)
}

/// Linux: inotify watches one folder at a time, and a recursive watch would
/// also cover every `node_modules` and `.git` below — easily past the
/// per-user watch limit. So watch only the folders the scan visits, down to
/// its depth, skipping the same folders it skips.
#[cfg(target_os = "linux")]
fn watch_root(w: &mut RecommendedWatcher, root: &Path, max_depth: usize, cfg: &Config) -> notify::Result<()> {
    let vcs: Vec<String> = config::vcs_markers(cfg).into_iter().map(|(n, _)| n.to_lowercase()).collect();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        w.watch(&dir, RecursiveMode::NonRecursive)?;
        if depth >= max_depth {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            if HARD_SKIP.iter().any(|s| name == s.to_lowercase()) || vcs.contains(&name) {
                continue;
            }
            stack.push((e.path(), depth + 1));
        }
    }
    Ok(())
}

/// The channel into the single debounce thread, started on first use.
fn signals(app: &AppHandle) -> &'static Sender<Signal> {
    static TX: OnceLock<Sender<Signal>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Signal>();
        let app = app.clone();
        std::thread::spawn(move || loop {
            let Ok(mut signal) = rx.recv() else { return };
            let first = Instant::now();
            loop {
                let wait = QUIET.min(MAX_WAIT.saturating_sub(first.elapsed()));
                match rx.recv_timeout(wait) {
                    Ok(s) => signal = signal.max(s),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            if signal == Signal::Rescan {
                if let Err(e) = crate::commands::rescan_blocking(&app, false) {
                    eprintln!("background rescan failed: {e}");
                }
            }
            // New folders need watches of their own on Linux.
            if cfg!(target_os = "linux") {
                sync(&app);
            }
        });
        tx
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, RemoveKind};

    const CREATE_DIR: EventKind = EventKind::Create(CreateKind::Folder);
    const CREATE_FILE: EventKind = EventKind::Create(CreateKind::File);
    const REMOVE: EventKind = EventKind::Remove(RemoveKind::Any);
    const RENAME_TO: EventKind = EventKind::Modify(ModifyKind::Name(RenameMode::To));

    fn filter(root: &Path) -> Filter {
        let mut cfg = config::bundled_defaults();
        cfg.roots = vec![root.to_string_lossy().into_owned()];
        Filter::new(&cfg)
    }

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "dp-watch-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn markers_coming_or_going_rescan_and_vcs_churn_does_not() {
        let root = tmp("markers");
        let f = filter(&root);
        let none = |_: &Path| false;
        let at = |rel: &str| root.join(rel);

        assert_eq!(f.classify(&at("proj/.git"), &CREATE_DIR, &none), Some(Signal::Rescan));
        assert_eq!(f.classify(&at("a/b/package.json"), &CREATE_FILE, &none), Some(Signal::Rescan));
        assert_eq!(f.classify(&at("proj/.hg"), &REMOVE, &none), Some(Signal::Rescan));
        // Inside a VCS folder or a build folder: dropped.
        assert_eq!(f.classify(&at("proj/.git/index.lock"), &CREATE_FILE, &none), None);
        assert_eq!(f.classify(&at("proj/node_modules/x/package.json"), &CREATE_FILE, &none), None);
        // Below the scan depth (4) plus the marker's own level: dropped.
        assert_eq!(f.classify(&at("a/b/c/d/e/.git"), &CREATE_DIR, &none), None);
        // An ordinary file edit inside a project: dropped.
        assert_eq!(f.classify(&at("proj/src/main.rs"), &CREATE_FILE, &none), None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folders_directly_in_a_root_and_listed_repos_rescan() {
        let root = tmp("folders");
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::create_dir_all(root.join("group/moved-in")).unwrap();
        let f = filter(&root);
        let none = |_: &Path| false;

        assert_eq!(f.classify(&root.join("notes"), &CREATE_DIR, &none), Some(Signal::Rescan));
        assert_eq!(f.classify(&root.join("gone"), &REMOVE, &none), Some(Signal::Rescan));
        // A loose file directly in the root isn't a folder: dropped.
        std::fs::write(root.join("loose.txt"), "x").unwrap();
        assert_eq!(f.classify(&root.join("loose.txt"), &CREATE_FILE, &none), None);

        // A nested repo deleted: only known because it was listed.
        let listed = root.join("group/acme");
        let holds = |p: &Path| listed.starts_with(p);
        assert_eq!(f.classify(&listed, &REMOVE, &holds), Some(Signal::Rescan));
        assert_eq!(f.classify(&root.join("group/other"), &REMOVE, &none), None);

        // A folder moved in deeper may already be a repo.
        assert_eq!(f.classify(&root.join("group/moved-in"), &RENAME_TO, &none), Some(Signal::Rescan));

        // The root itself going away.
        assert_eq!(f.classify(&root, &REMOVE, &none), Some(Signal::Rescan));
        // Somewhere else entirely.
        assert_eq!(f.classify(&std::env::temp_dir().join("elsewhere"), &CREATE_DIR, &none), None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn without_top_level_folders_a_bare_new_folder_is_ignored() {
        let root = tmp("no-top");
        std::fs::create_dir_all(root.join("plain")).unwrap();
        let mut cfg = config::bundled_defaults();
        cfg.roots = vec![root.to_string_lossy().into_owned()];
        cfg.scan.top_level_folders = false;
        let f = Filter::new(&cfg);
        let got = f.classify(&root.join("plain"), &CREATE_DIR, &|_| false);
        // Linux still needs a watch on it; nothing else does.
        let want = if cfg!(target_os = "linux") { Some(Signal::Rewatch) } else { None };
        assert_eq!(got, want);
        let _ = std::fs::remove_dir_all(&root);
    }
}
