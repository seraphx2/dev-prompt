//! Keeps the `>` app list current while dev-prompt runs. There's no "an app
//! was installed" event on Windows, but installers leave the same traces every
//! time, and those can be watched:
//!
//! - **Start Menu** shortcuts, `%LOCALAPPDATA%\Programs` and the user's
//!   `extra_dirs` — folders, through `notify`;
//! - the **Uninstall** registry keys every proper installer registers under,
//!   and the per-user **Store package** key — through `RegNotifyChangeKeyValue`.
//!
//! On Linux it's the XDG application folders, where `.desktop` files appear —
//! the same thing desktop environments watch.
//!
//! Changes are collected until things go quiet (installers write over many
//! seconds), then `apps::update` re-gathers only what they touched. The daily
//! staleness check and Ctrl+R in `>` stay full rescans, so anything missed
//! here is still picked up.

use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};

use crate::apps::{Change, Sources};
use crate::commands::AppState;

/// Apply the collected changes this long after the last one…
const QUIET: Duration = Duration::from_secs(3);
/// …but never put it off longer than this while an install keeps writing.
const MAX_WAIT: Duration = Duration::from_secs(30);

/// A running set of watches; dropping it stops them all.
pub struct Watching {
    _fs: Option<RecommendedWatcher>,
    #[cfg(windows)]
    _keys: Vec<reg::KeyWatch>,
}

/// Start watching `sources`, passing every relevant change to `on_change`.
/// A location that can't be watched is skipped; the daily check covers it.
pub fn start(sources: &Sources, on_change: impl Fn(Change) + Send + Sync + 'static) -> Watching {
    let on_change = Arc::new(on_change);

    let fs_cb = on_change.clone();
    let fs = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if matches!(
            ev.kind,
            EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_))
        ) {
            return;
        }
        // A folder reported as modified only means something was written
        // inside it (an app's logs, say); its own create/delete/rename events
        // are what matter. A modified shortcut or program does count.
        let modified = matches!(ev.kind, EventKind::Modify(m) if !matches!(m, ModifyKind::Name(_)));
        for p in ev.paths.iter().filter(|p| relevant(p) && !(modified && p.is_dir())) {
            (*fs_cb)(Change::Path(p.clone()));
        }
    })
    .ok()
    .map(|mut w| {
        for dir in sources.watch_dirs() {
            if let Err(e) = w.watch(&dir, RecursiveMode::Recursive) {
                eprintln!("app watcher: can't watch {}: {e}", dir.display());
            }
        }
        w
    });

    #[cfg(windows)]
    let keys = sources
        .watch_keys()
        .into_iter()
        .filter_map(|(path, change)| {
            let cb = on_change.clone();
            reg::watch(&path, move || (*cb)(change.clone()))
        })
        .collect();

    Watching {
        _fs: fs,
        #[cfg(windows)]
        _keys: keys,
    }
}

/// A path whose change can alter the list: a shortcut, an executable or a
/// `.desktop` file, or a folder (which may hold them, or have just gone).
fn relevant(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "lnk" | "exe" | "desktop") || !path.is_file()
}

/// (Re)start the app watcher for the current config. Called at startup and
/// after every settings save or config reload; does nothing (and stops any
/// watcher) while app indexing is off.
pub fn sync(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    if !cfg.apps.enabled {
        *state.app_watcher.lock().unwrap() = None;
        return;
    }
    let tx = changes(app).clone();
    let watching = start(&Sources::from_config(&cfg), move |c| {
        let _ = tx.send(c);
    });
    *state.app_watcher.lock().unwrap() = Some(watching);
}

/// [`sync`] on its own thread, for callers on the UI thread.
pub fn sync_in_background(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || sync(&app));
}

/// The channel into the single collect-and-apply thread, started on first use.
fn changes(app: &AppHandle) -> &'static Sender<Change> {
    static TX: OnceLock<Sender<Change>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Change>();
        let app = app.clone();
        std::thread::spawn(move || loop {
            let Ok(first) = rx.recv() else { return };
            let mut batch = vec![first];
            let started = Instant::now();
            loop {
                let wait = QUIET.min(MAX_WAIT.saturating_sub(started.elapsed()));
                match rx.recv_timeout(wait) {
                    Ok(c) => {
                        if !batch.contains(&c) {
                            batch.push(c);
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            if let Err(e) = crate::commands::update_apps_blocking(&app, &batch) {
                eprintln!("app list update failed: {e}");
            }
        });
        tx
    })
}

/// Registry change notifications (`RegNotifyChangeKeyValue`), one thread per
/// key. Each waits in short slices so dropping the [`KeyWatch`] ends it.
#[cfg(windows)]
mod reg {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Registry::{
        RegNotifyChangeKeyValue, HKEY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    pub struct KeyWatch {
        stop: Arc<AtomicBool>,
    }

    impl Drop for KeyWatch {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    /// Call `on_change` whenever anything under `path` (`HKEY_…\…`) changes.
    /// `None` when the key can't be opened (it doesn't exist, no access).
    pub fn watch(path: &str, on_change: impl Fn() + Send + 'static) -> Option<KeyWatch> {
        // Open here so a missing key is reported now, not on the thread.
        crate::apps::open_key(path)?;
        let path = path.to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            // The notification is tied to the thread that registered it, so the
            // key is opened and watched from here.
            let Some(key) = crate::apps::open_key(&path) else { return };
            let hkey = HKEY(key.raw_handle());
            let Ok(event) = (unsafe { CreateEventW(None, false, false, None) }) else { return };
            'arm: while !stopped.load(Ordering::Relaxed) {
                let armed = unsafe {
                    RegNotifyChangeKeyValue(
                        hkey,
                        true,
                        REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
                        Some(event),
                        true,
                    )
                };
                if armed.is_err() {
                    break;
                }
                loop {
                    if stopped.load(Ordering::Relaxed) {
                        break 'arm;
                    }
                    if unsafe { WaitForSingleObject(event, 250) } == WAIT_OBJECT_0 {
                        on_change();
                        continue 'arm;
                    }
                }
            }
            unsafe {
                let _ = CloseHandle(event);
            }
        });
        Some(KeyWatch { stop })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::mpsc::Receiver;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "dp-appwatch-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn channel_watch(sources: &Sources) -> (Watching, Receiver<Change>) {
        let (tx, rx) = mpsc::channel();
        let w = start(sources, move |c| {
            let _ = tx.send(c);
        });
        // Give the OS watches a moment to arm before the test acts.
        std::thread::sleep(Duration::from_millis(300));
        (w, rx)
    }

    /// Wait until `want` arrives (anything else is skipped) or time runs out.
    fn expect(rx: &Receiver<Change>, want: impl Fn(&Change) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(c) if want(&c) => return true,
                Ok(_) => continue,
                Err(_) => return false,
            }
        }
        false
    }

    fn is_path(c: &Change, p: &Path) -> bool {
        matches!(c, Change::Path(x) if x.ends_with(p.file_name().unwrap()))
    }

    #[test]
    fn relevant_paths_are_shortcuts_programs_desktop_files_and_folders() {
        let dir = tmp("relevant");
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert!(relevant(&dir.join("Foo.lnk")));
        assert!(relevant(&dir.join("foo.EXE")));
        assert!(relevant(&dir.join("foo.desktop")));
        assert!(relevant(&dir)); // a folder
        assert!(relevant(&dir.join("gone-folder"))); // deleted: can't tell, so yes
        assert!(!relevant(&dir.join("notes.txt"))); // an existing ordinary file
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    fn sources_at(menu: &Path, scan: &Path) -> Sources {
        Sources {
            menu_roots: vec![menu.to_path_buf()],
            scan_root: Some(scan.to_path_buf()),
            ..Sources::default()
        }
    }

    #[cfg(windows)]
    #[test]
    fn real_folder_events_reach_the_callback() {
        let (menu, scan) = (tmp("menu"), tmp("scan"));
        let (_w, rx) = channel_watch(&sources_at(&menu, &scan));

        let lnk = menu.join("Some App.lnk");
        std::fs::write(&lnk, "not really a shortcut").unwrap();
        assert!(expect(&rx, |c| is_path(c, &lnk)), "shortcut creation wasn't reported");

        std::fs::remove_file(&lnk).unwrap();
        assert!(expect(&rx, |c| is_path(c, &lnk)), "shortcut removal wasn't reported");

        let app_dir = scan.join("NewApp");
        std::fs::create_dir(&app_dir).unwrap();
        assert!(expect(&rx, |c| is_path(c, &app_dir)), "a new app folder wasn't reported");

        let _ = std::fs::remove_dir_all(&menu);
        let _ = std::fs::remove_dir_all(&scan);
    }

    /// A throwaway registry key under HKCU, removed when dropped.
    #[cfg(windows)]
    struct TestKey(String);

    #[cfg(windows)]
    impl TestKey {
        fn new(tag: &str) -> Self {
            let rel = format!(
                r"Software\dev-prompt-test\{tag}-{}\Uninstall",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
                .create_subkey(&rel)
                .unwrap();
            TestKey(rel)
        }
        fn full(&self) -> String {
            format!(r"HKEY_CURRENT_USER\{}", self.0)
        }
        fn key(&self) -> winreg::RegKey {
            winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
                .open_subkey_with_flags(&self.0, winreg::enums::KEY_ALL_ACCESS)
                .unwrap()
        }
    }

    #[cfg(windows)]
    impl Drop for TestKey {
        fn drop(&mut self) {
            let parent = self.0.trim_end_matches(r"\Uninstall");
            let _ = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).delete_subkey_all(parent);
        }
    }

    #[cfg(windows)]
    #[test]
    fn real_registry_changes_reach_the_callback() {
        let key = TestKey::new("watch");
        let sources = Sources {
            uninstall_roots: vec![("T".into(), key.full())],
            ..Sources::default()
        };
        let (_w, rx) = channel_watch(&sources);

        let (sub, _) = key.key().create_subkey("SomeApp").unwrap();
        assert!(expect(&rx, |c| *c == Change::Uninstall), "a new entry wasn't reported");

        sub.set_value("DisplayName", &"Some App").unwrap();
        assert!(expect(&rx, |c| *c == Change::Uninstall), "a changed entry wasn't reported");

        key.key().delete_subkey_all("SomeApp").unwrap();
        assert!(expect(&rx, |c| *c == Change::Uninstall), "a removed entry wasn't reported");
    }

    #[cfg(windows)]
    #[test]
    fn a_missing_registry_key_is_skipped() {
        assert!(reg::watch(r"HKEY_CURRENT_USER\Software\dev-prompt-test\no-such-key", || {}).is_none());
    }

    /// Everything the watcher reports until it's been quiet for a while.
    fn drain(rx: &Receiver<Change>) -> Vec<Change> {
        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(c) => {
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
                Err(_) => break,
            }
        }
        out
    }

    /// The incremental path must land exactly where a full gather does.
    fn assert_same(cfg: &crate::config::Config, sources: &Sources, updated: &crate::apps::Index) {
        let full = crate::apps::gather_all(cfg, sources).unwrap();
        assert_eq!(crate::apps::combine(updated), crate::apps::combine(&full));
        let sorted = |i: &crate::apps::Index| {
            let mut g = i.gathered.clone();
            g.sort_by(|a, b| (&a.origin, &a.entry.exec).cmp(&(&b.origin, &b.entry.exec)));
            g
        };
        assert_eq!(sorted(updated), sorted(&full));
    }

    #[cfg(windows)]
    fn powershell(cmd: &str) {
        let ok = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "powershell failed: {cmd}");
    }

    /// Installs and uninstalls, made the way installers make them, in
    /// throwaway locations: what the watcher reports, fed to `apps::update`,
    /// must give the same list as a full gather every time.
    #[cfg(windows)]
    #[test]
    fn watched_installs_and_uninstalls_update_the_list_like_a_full_rescan() {
        let (menu, scan, extra, bin) = (tmp("e2e-menu"), tmp("e2e-scan"), tmp("e2e-extra"), tmp("e2e-bin"));
        let key = TestKey::new("e2e");
        let mut cfg = crate::config::bundled_defaults();
        cfg.apps.enabled = true;
        cfg.apps.extra_dirs.clear();
        cfg.apps.exclude.clear();
        let sources = Sources {
            menu_roots: vec![menu.clone()],
            scan_root: Some(scan.clone()),
            extra_dirs: vec![extra.clone()],
            uninstall_roots: vec![("T".into(), key.full())],
            ..Sources::default()
        };

        let index0 = crate::apps::gather_all(&cfg, &sources).unwrap();
        assert!(crate::apps::combine(&index0).is_empty());
        let (_w, rx) = channel_watch(&sources);

        // "Install" four apps, one per source. Shortcut and Uninstall targets
        // only need to be `.exe` files; the scanned one needs a real version
        // resource (a FileDescription), so it's a copy of cmd.exe.
        let shortcut_target = bin.join("shortcut-app.exe");
        let uninstall_target = bin.join("registry-app.exe");
        std::fs::write(&shortcut_target, "stub").unwrap();
        std::fs::write(&uninstall_target, "stub").unwrap();
        let lnk = menu.join("Test Shortcut App.lnk");
        powershell(&format!(
            "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath = '{}'; $s.Save()",
            lnk.display(),
            shortcut_target.display()
        ));
        std::fs::create_dir(scan.join("ScannedApp")).unwrap();
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        std::fs::copy(
            Path::new(&system).join(r"System32\cmd.exe"),
            scan.join(r"ScannedApp\scannedapp.exe"),
        )
        .unwrap();
        // (Not "…Uninstall…": `keep_entry` rightly drops anything named like
        // an uninstaller.)
        // An extra folder takes any `.exe`, named after the file.
        std::fs::create_dir(extra.join("Portable")).unwrap();
        std::fs::write(extra.join(r"Portable\portable-tool.exe"), "stub").unwrap();
        let (entry, _) = key.key().create_subkey("TestRegistryApp").unwrap();
        entry.set_value("DisplayName", &"Test Registry App").unwrap();
        entry
            .set_value("DisplayIcon", &uninstall_target.to_string_lossy().into_owned())
            .unwrap();

        let changes = drain(&rx);
        assert!(changes.contains(&Change::Uninstall), "{changes:?}");
        let index1 = crate::apps::update(&cfg, &sources, &index0, &changes).unwrap();
        let names: Vec<String> = crate::apps::combine(&index1).into_iter().map(|a| a.name).collect();
        for want in ["Test Shortcut App", "Test Registry App", "Windows Command Processor", "portable-tool"] {
            assert!(names.iter().any(|n| n == want), "{want} missing from {names:?}");
        }
        assert_same(&cfg, &sources, &index1);

        // Uninstall them all again.
        std::fs::remove_file(&lnk).unwrap();
        std::fs::remove_dir_all(scan.join("ScannedApp")).unwrap();
        std::fs::remove_dir_all(extra.join("Portable")).unwrap();
        key.key().delete_subkey_all("TestRegistryApp").unwrap();

        let changes = drain(&rx);
        let index2 = crate::apps::update(&cfg, &sources, &index1, &changes).unwrap();
        assert!(crate::apps::combine(&index2).is_empty(), "{:?}", crate::apps::combine(&index2));
        assert_same(&cfg, &sources, &index2);

        for d in [&menu, &scan, &extra, &bin] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// The same on Linux: `.desktop` files appearing, being overridden by a
    /// higher-precedence folder, and going away.
    #[cfg(target_os = "linux")]
    #[test]
    fn watched_desktop_files_update_the_list_like_a_full_rescan() {
        let (user, system) = (tmp("e2e-user"), tmp("e2e-system"));
        let mut cfg = crate::config::bundled_defaults();
        cfg.apps.enabled = true;
        let sources = Sources { app_dirs: vec![user.clone(), system.clone()], excludes: Vec::new() };
        let entry = |name: &str| format!("[Desktop Entry]\nType=Application\nName={name}\nExec=/bin/true\n");

        let index0 = crate::apps::gather_all(&cfg, &sources).unwrap();
        let (_w, rx) = channel_watch(&sources);

        std::fs::write(system.join("editor.desktop"), entry("System Editor")).unwrap();
        std::fs::create_dir(system.join("kde")).unwrap();
        std::fs::write(system.join("kde/tool.desktop"), entry("KDE Tool")).unwrap();
        let index1 = crate::apps::update(&cfg, &sources, &index0, &drain(&rx)).unwrap();
        assert_same(&cfg, &sources, &index1);

        // A user-level override shadows the system entry with the same id.
        std::fs::write(user.join("editor.desktop"), entry("My Editor")).unwrap();
        let index2 = crate::apps::update(&cfg, &sources, &index1, &drain(&rx)).unwrap();
        let names: Vec<String> = crate::apps::combine(&index2).into_iter().map(|a| a.name).collect();
        assert!(names.contains(&"My Editor".to_string()) && !names.contains(&"System Editor".to_string()));
        assert_same(&cfg, &sources, &index2);

        std::fs::remove_file(user.join("editor.desktop")).unwrap();
        std::fs::remove_dir_all(system.join("kde")).unwrap();
        let index3 = crate::apps::update(&cfg, &sources, &index2, &drain(&rx)).unwrap();
        assert_same(&cfg, &sources, &index3);

        let _ = std::fs::remove_dir_all(&user);
        let _ = std::fs::remove_dir_all(&system);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn real_desktop_file_events_reach_the_callback() {
        let dir = tmp("apps");
        let sources = Sources { app_dirs: vec![dir.clone()], excludes: Vec::new() };
        let (_w, rx) = channel_watch(&sources);

        let file = dir.join("thing.desktop");
        std::fs::write(&file, "[Desktop Entry]\nType=Application\nName=Thing\nExec=/bin/true\n").unwrap();
        assert!(expect(&rx, |c| is_path(c, &file)), "a new .desktop file wasn't reported");

        std::fs::remove_file(&file).unwrap();
        assert!(expect(&rx, |c| is_path(c, &file)), "a removed .desktop file wasn't reported");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
