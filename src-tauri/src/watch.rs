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
use std::sync::{Arc, Mutex, OnceLock};
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
    /// The repo list may have changed.
    Rescan,
}

/// A running watcher over the roots; dropping it stops it.
pub struct RepoWatcher {
    /// The config it was built from (see [`watch_key`]), so a sync that
    /// changes nothing it depends on keeps it instead of rebuilding.
    key: String,
    /// Shared with the Linux thread that adds watches for new folders.
    _inner: Arc<Mutex<RecommendedWatcher>>,
}

/// Everything [`Filter`] and the watches depend on.
fn watch_key(cfg: &Config) -> String {
    format!(
        "{:?}",
        (
            config::resolved_roots(cfg),
            cfg.scan.max_depth,
            cfg.scan.top_level_folders,
            config::discovery_globs(cfg),
            config::vcs_markers(cfg),
        )
    )
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

        // Only something appearing, going or moving can change the list. On
        // Windows, writing *inside* a folder also reports the folder itself as
        // modified — so without this, every write in `.git` (each `git status`)
        // would read as "the `.git` marker changed".
        if matches!(kind, EventKind::Modify(m) if !matches!(m, ModifyKind::Name(_))) {
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
        }
        None
    }

    /// Linux: how many levels below `path` — a folder that just appeared —
    /// still need inotify watches, or `None` if it needs none (outside the
    /// scan depth, or inside `.git` / `node_modules` / …).
    fn levels_below(&self, path: &Path) -> Option<usize> {
        let root = self.roots.iter().find(|r| path.starts_with(r))?;
        let rel: Vec<String> = path
            .strip_prefix(root)
            .ok()?
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if rel.is_empty() || rel.len() > self.max_depth || rel.iter().any(|seg| self.is_opaque(seg)) {
            return None;
        }
        path.is_dir().then(|| self.max_depth - rel.len())
    }
}

fn has_marker(dir: &Path, discovery: &GlobSet) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| discovery.is_match(e.file_name().to_string_lossy().to_lowercase()))
    })
}

/// (Re)build the watcher for the current config, unless it was built from the
/// same roots, depth and markers. Called at startup and after every config
/// save, reload or rescan.
pub fn sync(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    // Settings save often; one that changes nothing the watcher depends on
    // keeps it, rather than dropping events while it's rebuilt (and, on
    // Linux, re-walking every root).
    if state.watcher.lock().unwrap().as_ref().is_some_and(|w| w.key == watch_key(&cfg)) {
        return;
    }
    let tx = signals(app).clone();
    let handler_app = app.clone();
    let holds_repo = move |p: &Path| {
        let state = handler_app.state::<AppState>();
        let repos = state.repos.lock().unwrap();
        repos.iter().any(|r| Path::new(&r.path).starts_with(p))
    };
    let watcher = start(&cfg, holds_repo, move |s| {
        let _ = tx.send(s);
    });
    *state.watcher.lock().unwrap() = watcher;
}

/// Watch `cfg`'s roots, passing what each relevant event means to
/// `on_signal`. `holds_repo(p)` says whether a listed repo is at or below `p`.
/// `None` when no watcher could be created at all.
fn start(
    cfg: &Config,
    holds_repo: impl Fn(&Path) -> bool + Send + 'static,
    on_signal: impl Fn(Signal) + Send + Sync + 'static,
) -> Option<RepoWatcher> {
    let filter = Arc::new(Filter::new(cfg));
    let on_signal = Arc::new(on_signal);
    // Linux: a folder that appears needs watches of its own. They're added on
    // another thread — the watcher can't be changed from inside its own
    // event handler.
    let (new_tx, new_rx) = mpsc::channel::<(PathBuf, usize)>();

    let f = filter.clone();
    let signal = on_signal.clone();
    let made = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let ev = match res {
            // The OS dropped events (buffer overflow): assume the worst.
            Err(_) => return signal(Signal::Rescan),
            Ok(ev) if ev.need_rescan() => return signal(Signal::Rescan),
            Ok(ev) => ev,
        };
        if matches!(
            ev.kind,
            EventKind::Access(_) | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))
        ) {
            return;
        }
        if let Some(s) = ev.paths.iter().filter_map(|p| f.classify(p, &ev.kind, &holds_repo)).max() {
            signal(s);
        }
        let added = matches!(
            ev.kind,
            EventKind::Create(_)
                | EventKind::Modify(ModifyKind::Name(RenameMode::To | RenameMode::Both | RenameMode::Any))
        );
        if cfg!(target_os = "linux") && added {
            for p in &ev.paths {
                if let Some(levels) = f.levels_below(p) {
                    let _ = new_tx.send((p.clone(), levels));
                }
            }
        }
    });
    let mut watcher = match made {
        Ok(w) => w,
        Err(e) => {
            eprintln!("repo watcher unavailable: {e}");
            return None;
        }
    };

    for root in &filter.roots {
        match watch_root(&mut watcher, root, &filter) {
            Ok(()) => {}
            // Out of inotify watches: the rest can't be watched either. The
            // open-time check covers it.
            Err(e) if matches!(e.kind, notify::ErrorKind::MaxFilesWatch) => {
                eprintln!("watching {} stopped early: {e}", root.display());
                break;
            }
            // A missing drive or a typo affects this root only.
            Err(e) => eprintln!("not watching {}: {e}", root.display()),
        }
    }

    let inner = Arc::new(Mutex::new(watcher));
    if cfg!(target_os = "linux") {
        let weak = Arc::downgrade(&inner);
        std::thread::spawn(move || {
            // Ends when the watcher (and with it the sender) is dropped.
            while let Ok((dir, levels)) = new_rx.recv() {
                let Some(inner) = weak.upgrade() else { return };
                let mut marker = false;
                let res = watch_tree(&mut inner.lock().unwrap(), &dir, levels, &filter, &mut |d| {
                    marker |= has_marker(d, &filter.discovery);
                });
                if let Err(e) = res {
                    eprintln!("not watching {}: {e}", dir.display());
                }
                // A marker written before its folder was watched went unseen.
                if marker {
                    on_signal(Signal::Rescan);
                }
            }
        });
    }
    Some(RepoWatcher { key: watch_key(cfg), _inner: inner })
}

/// [`sync`] on its own thread — for callers on the UI thread, since on Linux
/// it walks the roots.
pub fn sync_in_background(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || sync(&app));
}

/// Windows: one recursive watch per root is a single OS handle.
#[cfg(not(target_os = "linux"))]
fn watch_root(w: &mut RecommendedWatcher, root: &Path, _filter: &Filter) -> notify::Result<()> {
    w.watch(root, RecursiveMode::Recursive)
}

/// Linux: inotify watches one folder at a time, and a recursive watch would
/// also cover every `node_modules` and `.git` below — easily past the
/// per-user watch limit. So watch only the folders the scan visits, down to
/// its depth, skipping the same folders it skips.
#[cfg(target_os = "linux")]
fn watch_root(w: &mut RecommendedWatcher, root: &Path, filter: &Filter) -> notify::Result<()> {
    watch_tree(w, root, filter.max_depth, filter, &mut |_| {})
}

/// Not needed off Linux; never called there.
#[cfg(not(target_os = "linux"))]
fn watch_tree(
    _w: &mut RecommendedWatcher,
    _top: &Path,
    _levels: usize,
    _filter: &Filter,
    _on_dir: &mut dyn FnMut(&Path),
) -> notify::Result<()> {
    Ok(())
}

/// Watch `top` and its folders `levels` deep, calling `on_dir` for each one
/// watched. A subfolder that can't be watched (gone already, unreadable) is
/// skipped; failing on `top` itself, or running out of watches, is an error.
#[cfg(target_os = "linux")]
fn watch_tree(
    w: &mut RecommendedWatcher,
    top: &Path,
    levels: usize,
    filter: &Filter,
    on_dir: &mut dyn FnMut(&Path),
) -> notify::Result<()> {
    let mut stack = vec![(top.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if let Err(e) = w.watch(&dir, RecursiveMode::NonRecursive) {
            if depth == 0 || matches!(e.kind, notify::ErrorKind::MaxFilesWatch) {
                return Err(e);
            }
            continue;
        }
        on_dir(&dir);
        if depth >= levels {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) && !filter.is_opaque(&e.file_name().to_string_lossy()) {
                stack.push((e.path(), depth + 1));
            }
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
            let Ok(Signal::Rescan) = rx.recv() else { return };
            let first = Instant::now();
            loop {
                let wait = QUIET.min(MAX_WAIT.saturating_sub(first.elapsed()));
                match rx.recv_timeout(wait) {
                    Ok(_) => {}
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            if let Err(e) = crate::commands::rescan_blocking(&app, false) {
                eprintln!("background rescan failed: {e}");
            }
        });
        tx
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, RemoveKind};
    use std::sync::mpsc;

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
        // Windows reporting `.git` itself as modified after a write inside it,
        // or a manifest being edited: neither changes the list.
        let modified = EventKind::Modify(ModifyKind::Any);
        assert_eq!(f.classify(&at("proj/.git"), &modified, &none), None);
        assert_eq!(f.classify(&at("proj/package.json"), &modified, &none), None);

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
        assert_eq!(got, None);
        // Linux still watches it, and below it down to the scan depth.
        assert_eq!(f.levels_below(&root.join("plain")), Some(cfg.scan.max_depth - 1));
        assert_eq!(f.levels_below(&root.join("plain/node_modules")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Real OS events, end to end through the filter: what a user does in
    /// Explorer or a terminal must come out as a rescan, and git's own
    /// churn inside `.git` must not.
    #[test]
    fn real_events_signal_a_rescan_and_vcs_churn_does_not() {
        let root = tmp("real");
        std::fs::create_dir_all(root.join("listed/.git")).unwrap();
        let mut cfg = config::bundled_defaults();
        cfg.roots = vec![root.to_string_lossy().into_owned()];
        let listed = root.join("listed");
        let (tx, rx) = mpsc::channel();
        let _w = start(&cfg, move |p| listed.starts_with(p), move |s| {
            let _ = tx.send(s);
        })
        .expect("a watcher");
        std::thread::sleep(Duration::from_millis(300));

        let next = |what: &str| {
            let got = rx.recv_timeout(Duration::from_secs(10));
            assert_eq!(got, Ok(Signal::Rescan), "{what}");
            // Let the burst finish, then empty the queue for the next step.
            std::thread::sleep(Duration::from_millis(500));
            while rx.try_recv().is_ok() {}
        };

        // `git init` in a new project, two levels down.
        std::fs::create_dir_all(root.join("group/proj")).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        while rx.try_recv().is_ok() {}
        std::fs::create_dir(root.join("group/proj/.git")).unwrap();
        next("a new .git");

        // Work inside a repo: nothing.
        std::fs::write(root.join("listed/.git/index.lock"), "x").unwrap();
        std::fs::write(root.join("listed/.git/HEAD"), "ref: refs/heads/main").unwrap();
        assert!(
            rx.recv_timeout(Duration::from_secs(2)).is_err(),
            "changes inside .git must not rescan"
        );

        // A listed repo deleted.
        std::fs::remove_dir_all(root.join("listed")).unwrap();
        next("a deleted repo");

        // A folder added directly in the root.
        std::fs::create_dir(root.join("notes")).unwrap();
        next("a new top-level folder");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A root that can't be watched (an unplugged drive, a typo) mustn't
    /// leave the roots after it unwatched.
    #[test]
    fn a_missing_root_does_not_stop_the_others() {
        let root = tmp("after-missing");
        let mut cfg = config::bundled_defaults();
        cfg.roots = vec![
            root.join("not-there").to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
        ];
        let (tx, rx) = mpsc::channel();
        let _w = start(&cfg, |_| false, move |s| {
            let _ = tx.send(s);
        })
        .expect("a watcher");
        std::thread::sleep(Duration::from_millis(300));
        std::fs::create_dir(root.join("notes")).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(Signal::Rescan));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Only what the watcher depends on rebuilds it.
    #[test]
    fn the_watch_key_follows_roots_depth_and_markers_only() {
        let (a, b) = (tmp("key-a"), tmp("key-b"));
        let mut cfg = config::bundled_defaults();
        cfg.roots = vec![a.to_string_lossy().into_owned()];
        let key = watch_key(&cfg);
        let mut other = cfg.clone();
        other.terminal = Some("wezterm".into());
        other.cache_ttl_secs = 60;
        assert_eq!(watch_key(&other), key);
        other.scan.max_depth += 1;
        assert_ne!(watch_key(&other), key);
        let mut other = cfg.clone();
        other.roots.push(b.to_string_lossy().into_owned());
        assert_ne!(watch_key(&other), key);
        let mut other = cfg.clone();
        other.scan.top_level_folders = !other.scan.top_level_folders;
        assert_ne!(watch_key(&other), key);
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
}
