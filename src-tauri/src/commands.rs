use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::apps::{self, AppEntry};
use crate::cache;
use crate::config::{self, resolved_roots, Config};
use crate::error::{AppError, AppResult};
use crate::index::{self, ScoredRepo};
use crate::inspect::{self, RepoContext};
use crate::launch;
use crate::rules::{
    build_actions as build_actions_impl, find_action, terminal_command, Action, Resolver,
};
use crate::scan::{self, Repo};

/// In-memory app state shared across commands.
pub struct AppState {
    pub config: Mutex<Config>,
    pub repos: Mutex<Vec<Repo>>,
    /// Per-repo project inspection keyed by repo path — captured during the scan
    /// so opening the action menu never walks the disk. See `cache::CacheFile`.
    pub contexts: Mutex<HashMap<String, RepoContext>>,
    /// Age (secs) of the list currently in `repos`; -1 when never cached.
    pub age_secs: Mutex<i64>,
    /// Installed apps for the `>` scope, cached in `apps.json`.
    pub apps: Mutex<Vec<AppEntry>>,
    /// Age (secs) of `apps`; -1 when never cached.
    pub apps_age_secs: Mutex<i64>,
    /// Whether losing focus dismisses the overlay. Off while the settings
    /// screen is open so clicking away to copy a path doesn't nuke edits.
    pub dismiss_on_blur: Mutex<bool>,
    /// True when `config.yaml` did not exist before this launch (first run).
    pub first_run: bool,
    /// First-run latch: while set, focus loss never dismisses the overlay, so
    /// the window `setup` pops on first run stays put until the user explicitly
    /// closes it (Esc, launching something, the hotkey, Alt+F4). Cleared on that
    /// first dismiss, after which `dismiss_on_blur` governs as usual.
    pub sticky_open: Mutex<bool>,
}

impl AppState {
    pub fn load() -> Self {
        let first_run = config::config_path().map(|p| !p.exists()).unwrap_or(false);
        let config = config::load().unwrap_or_default();
        let loaded = cache::load().unwrap_or(cache::LoadedCache {
            repos: Vec::new(),
            contexts: HashMap::new(),
            age_secs: -1,
        });
        let (apps, apps_age) = cache::load_apps().unwrap_or((Vec::new(), -1));
        AppState {
            config: Mutex::new(config),
            repos: Mutex::new(loaded.repos),
            contexts: Mutex::new(loaded.contexts),
            age_secs: Mutex::new(loaded.age_secs),
            apps: Mutex::new(apps),
            apps_age_secs: Mutex::new(apps_age),
            dismiss_on_blur: Mutex::new(true),
            first_run,
            sticky_open: Mutex::new(first_run),
        }
    }
}

/// The cached inspection for `repo`, or a fresh live walk when the cache predates
/// this feature (or the repo was added since the last scan). The cache hit is the
/// common path and touches no disk.
fn context_for(state: &AppState, repo: &Repo) -> RepoContext {
    if let Some(ctx) = state.contexts.lock().unwrap().get(&repo.path).cloned() {
        return ctx;
    }
    let cfg = state.config.lock().unwrap();
    inspect_cold(&repo.path, &cfg)
}

/// Cold-path project inspection for a repo the last scan didn't capture (cache
/// predates the feature, or the repo was added since). Shared by the
/// `spawn_blocking` closures in `build_actions` / `run_action`, which can't call
/// `context_for` because it borrows `AppState`.
fn inspect_cold(repo_path: &str, cfg: &Config) -> RepoContext {
    let discovery = config::compile_globset(&config::discovery_globs(cfg));
    inspect::inspect(Path::new(repo_path), &discovery)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoListPayload {
    pub repos: Vec<Repo>,
    pub age_secs: i64,
    pub stale: bool,
}

fn payload(state: &AppState) -> RepoListPayload {
    let repos = state.repos.lock().unwrap().clone();
    let age = *state.age_secs.lock().unwrap();
    let ttl = state.config.lock().unwrap().cache_ttl_secs;
    RepoListPayload {
        repos,
        age_secs: age,
        stale: cache::is_stale(age, ttl),
    }
}

#[tauri::command]
pub fn list_repos(state: State<'_, AppState>) -> RepoListPayload {
    payload(&state)
}

#[tauri::command]
pub async fn rescan_repos(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<RepoListPayload> {
    // Re-read config.yaml so edits to `roots` / rules take effect without an app
    // restart. A YAML syntax error surfaces here instead of being silently
    // ignored, and stale program lookups are forgotten.
    crate::rules::clear_program_cache();
    let cfg = config::load()?;
    *state.config.lock().unwrap() = cfg.clone();
    let previous = state.repos.lock().unwrap().clone();

    // Filesystem walk + per-repo inspection off the main thread. Doing the
    // inspection here (while the walk's dir entries are still cache-warm) is far
    // cheaper than a cold walk later when the action menu is opened.
    let (fresh, contexts) = tauri::async_runtime::spawn_blocking(move || {
        let roots = resolved_roots(&cfg);
        let repos = scan::scan(&roots, &cfg);
        let discovery = config::compile_globset(&config::discovery_globs(&cfg));
        let contexts = inspect::inspect_all(repos.iter().map(|r| r.path.as_str()), &discovery);
        (repos, contexts)
    })
    .await
    .map_err(|e| AppError::msg(format!("scan task failed: {e}")))?;

    let merged = cache::merge(&previous, fresh);
    cache::save(&merged, &contexts)?;

    *state.repos.lock().unwrap() = merged;
    *state.contexts.lock().unwrap() = contexts;
    *state.age_secs.lock().unwrap() = 0;

    let out = payload(&state);
    let _ = app.emit("repos:updated", ());
    Ok(out)
}

#[tauri::command]
pub fn search_repos(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Vec<ScoredRepo> {
    let repos = state.repos.lock().unwrap();
    index::search(&query, &repos, limit.unwrap_or(200))
}

fn repo_for_path(state: &AppState, path: &str) -> Repo {
    let repos = state.repos.lock().unwrap();
    repos
        .iter()
        .find(|r| r.path == path)
        .cloned()
        .unwrap_or_else(|| Repo {
            name: std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string()),
            path: path.to_string(),
            vcs: None,
            sentinels: Vec::new(),
            last_seen: 0,
        })
}

/// `async` + `spawn_blocking`: rule evaluation resolves every program key, and
/// the first call per session globs the filesystem / scans PATH / runs vswhere
/// (memoized after). A cache miss also walks the repo. None of that belongs on
/// the UI thread — the action menu `await`s this before it renders.
#[tauri::command]
pub async fn build_actions(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<Vec<Action>> {
    let repo = repo_for_path(&state, &path);
    let cached = state.contexts.lock().unwrap().get(&repo.path).cloned();
    let cfg = state.config.lock().unwrap().clone();
    Ok(tauri::async_runtime::spawn_blocking(move || {
        let ctx = cached.unwrap_or_else(|| inspect_cold(&repo.path, &cfg));
        build_actions_impl(&repo, &ctx, &cfg)
    })
    .await
    .unwrap_or_else(|e| {
        // A JoinError here means the closure panicked (poisoned mutex, an unwrap
        // in `inspect`). An empty menu is a softer landing than failing the IPC,
        // but the panic shouldn't vanish silently.
        eprintln!("build_actions task failed: {e}");
        Vec::new()
    }))
}

/// Rule-by-rule explanation of what a single repo produces and why — feeds the
/// settings "trace a repo" view. Uses the scan-time cached context like
/// `build_actions`.
#[tauri::command]
pub fn repo_rule_trace(
    state: State<'_, AppState>,
    path: String,
) -> crate::rules::RepoTrace {
    let repo = repo_for_path(&state, &path);
    let ctx = context_for(&state, &repo);
    let cfg = state.config.lock().unwrap();
    crate::rules::trace(&cfg, &ctx, &repo)
}

/// Re-inspect a single repo off the UI thread. The action menu renders instantly
/// from the cached context, then calls this; if the repo changed on disk since
/// the last scan (new npm script, added `Cargo.toml`, …) the cache is updated
/// and `repo:context-updated` fires so the open menu rebuilds itself.
#[tauri::command]
pub async fn refresh_repo_context(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<()> {
    let walk_path = path.clone();
    let discovery = config::compile_globset(&config::discovery_globs(
        &state.config.lock().unwrap(),
    ));
    let fresh = tauri::async_runtime::spawn_blocking(move || {
        inspect::inspect(Path::new(&walk_path), &discovery)
    })
    .await
    .map_err(|e| AppError::msg(format!("inspect task failed: {e}")))?;

    let changed = {
        let mut map = state.contexts.lock().unwrap();
        if map.get(&path) == Some(&fresh) {
            false
        } else {
            map.insert(path.clone(), fresh);
            true
        }
    };

    if changed {
        let repos = state.repos.lock().unwrap().clone();
        let contexts = state.contexts.lock().unwrap().clone();
        let _ = cache::save(&repos, &contexts);
        let _ = app.emit("repo:context-updated", path);
    }
    Ok(())
}

/// `async` + `spawn_blocking` for the same reason as [`build_actions`], plus the
/// `CreateProcess` in `launch::launch` — the overlay `await`s this before it
/// dismisses.
#[tauri::command]
pub async fn run_action(
    state: State<'_, AppState>,
    action_id: String,
    path: String,
) -> AppResult<()> {
    let repo = repo_for_path(&state, &path);
    let cached = state.contexts.lock().unwrap().get(&repo.path).cloned();
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = cached.unwrap_or_else(|| inspect_cold(&repo.path, &cfg));
        let action = find_action(&repo, &ctx, &cfg, &action_id)
            .ok_or_else(|| AppError::msg(format!("unknown action: {action_id}")))?;
        launch::launch(&action, &repo)
    })
    .await
    .map_err(|e| AppError::msg(format!("launch task failed: {e}")))?
}

#[tauri::command]
pub fn hide_overlay(window: tauri::WebviewWindow, state: State<'_, AppState>) -> AppResult<()> {
    // The first explicit dismiss ends the first-run "stay open" latch.
    *state.sticky_open.lock().unwrap() = false;
    // Mirror the backend hide paths: reset the frontend to the repo list while
    // the window is off-screen so the next show has no visible snap-back.
    let _ = window.emit("overlay:hidden", ());
    window
        .hide()
        .map_err(|e| AppError::msg(format!("hide failed: {e}")))
}

// --- settings ---------------------------------------------------------------

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Config {
    state.config.lock().unwrap().clone()
}

/// Read-only view of the merged config: which programs resolve, which rules are
/// active, which universal actions are available.
#[tauri::command]
pub async fn config_summary(
    state: State<'_, AppState>,
) -> AppResult<crate::rules::ConfigSummary> {
    // `summarize` resolves every program by globbing the filesystem (plus a
    // per-rule PATH search) — seconds on a cold cache. A sync command would run
    // that on the UI thread and stall *all* pending IPC responses behind it
    // (that's what lagged the Settings autostart checkbox by ~6s). Off-thread it.
    let path = config::rules_path()?.to_string_lossy().into_owned();
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || crate::rules::summarize(&cfg, path))
        .await
        .map_err(|e| AppError::msg(format!("config summary task failed: {e}")))
}

/// Re-read `config.yaml` + `rules.yaml` from disk, forget memoized program
/// lookups, and return the fresh merged config (does not re-scan repos).
#[tauri::command]
pub fn reload_config(state: State<'_, AppState>) -> AppResult<Config> {
    crate::rules::clear_program_cache();
    let cfg = config::load()?;
    *state.config.lock().unwrap() = cfg.clone();
    Ok(cfg)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConfigPatch {
    pub hotkey: Option<String>,
    /// `Some("")` turns the second (app-launcher) hotkey off.
    pub apps_hotkey: Option<String>,
    pub roots: Option<Vec<String>>,
    pub cache_ttl_secs: Option<u64>,
    pub scan_max_depth: Option<usize>,
    pub collapse_nested: Option<config::CollapseNested>,
    /// `Some("")` clears the pin / template / shell back to auto.
    pub terminal: Option<String>,
    pub terminal_template: Option<String>,
    pub shell: Option<String>,
    pub apps: Option<config::AppsConfig>,
}

/// Apply an editable subset of settings: write `config.yaml`, update the live
/// config, and re-register the global hotkeys if they changed. Accelerators are
/// parsed up front and the new ones claimed before the file is written; if any
/// step fails the claims are rolled back, so the live shortcuts and `config.yaml`
/// never disagree.
#[tauri::command]
pub fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: ConfigPatch,
) -> AppResult<Config> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let (old_hotkey, old_apps_hotkey) = {
        let cfg = state.config.lock().unwrap();
        (cfg.hotkey.clone(), cfg.apps_hotkey.clone())
    };
    let mut user = config::load_user()?;

    if let Some(h) = patch.hotkey {
        user.hotkey = Some(h.trim().to_string());
    }
    if let Some(h) = patch.apps_hotkey {
        user.apps_hotkey = Some(h.trim().to_string());
    }
    if let Some(roots) = patch.roots {
        user.roots = roots
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(ttl) = patch.cache_ttl_secs {
        user.cache_ttl_secs = Some(ttl);
    }
    if patch.scan_max_depth.is_some() || patch.collapse_nested.is_some() {
        let mut scan = user.scan.clone().unwrap_or_default();
        if let Some(depth) = patch.scan_max_depth {
            scan.max_depth = depth.max(1);
        }
        if let Some(cn) = patch.collapse_nested {
            scan.collapse_nested = cn;
        }
        user.scan = Some(scan);
    }
    if let Some(t) = patch.terminal {
        let t = t.trim();
        user.terminal = (!t.is_empty()).then(|| t.to_string());
    }
    if let Some(t) = patch.terminal_template {
        let t = t.trim();
        user.terminal_template = (!t.is_empty()).then(|| t.to_string());
    }
    if let Some(s) = patch.shell {
        let s = s.trim();
        user.shell = (!s.is_empty()).then(|| s.to_string());
    }
    if let Some(mut a) = patch.apps {
        a.extra_dirs = a
            .extra_dirs
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        a.exclude = a
            .exclude
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        user.apps = Some(a);
    }

    let new_hotkey = user.hotkey.clone().unwrap_or_else(|| old_hotkey.clone());
    // Fall back to the current effective value (like `new_hotkey` does) when the
    // patch omits it — otherwise a Save that doesn't mention `apps_hotkey` reads
    // as "cleared" and tears down the bundled default.
    let new_apps_hotkey = user
        .apps_hotkey
        .clone()
        .or_else(|| old_apps_hotkey.clone())
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty());

    // Reject malformed combos before touching OS state or the config file.
    let parse_ok = |accel: &str| -> AppResult<()> {
        accel
            .parse::<tauri_plugin_global_shortcut::Shortcut>()
            .map(|_| ())
            .map_err(|e| AppError::msg(format!("\"{accel}\" isn't a valid hotkey. ({e})")))
    };
    parse_ok(&new_hotkey)?;
    if let Some(h) = &new_apps_hotkey {
        parse_ok(h)?;
    }

    if let Some(h) = &new_apps_hotkey {
        if crate::same_shortcut(h, &new_hotkey) {
            return Err(AppError::msg(
                "The app launcher hotkey must be different from the repo browser hotkey.",
            ));
        }
    }

    let hotkey_changed = !crate::same_shortcut(&new_hotkey, &old_hotkey);
    let old_apps_nonempty = old_apps_hotkey.as_deref().filter(|s| !s.is_empty());
    let apps_changed = match (new_apps_hotkey.as_deref(), old_apps_nonempty) {
        (Some(a), Some(b)) => !crate::same_shortcut(a, b),
        (None, None) => false,
        _ => true,
    };

    let register = |accel: &str| -> AppResult<()> {
        app.global_shortcut().register(accel).map_err(|e| {
            AppError::msg(format!(
                "Couldn't register {accel} — it may already be in use by another \
                 app, or it isn't a valid combination. ({e})"
            ))
        })
    };
    let unregister = |accel: &str| {
        let _ = app.global_shortcut().unregister(accel);
    };

    // Claim the new accelerators first; on any failure, undo the claims so the
    // live shortcuts still match what's on disk.
    let mut claimed: Vec<&str> = Vec::new();
    if hotkey_changed {
        register(&new_hotkey)?;
        claimed.push(new_hotkey.as_str());
    }
    if apps_changed {
        if let Some(h) = &new_apps_hotkey {
            if let Err(e) = register(h) {
                for a in &claimed {
                    unregister(a);
                }
                return Err(e);
            }
            claimed.push(h.as_str());
        }
    }

    // A changed `terminal` pin must not lose to a memoized `terminal` lookup.
    crate::rules::clear_program_cache();

    if let Err(e) = config::save_user(&user) {
        for a in &claimed {
            unregister(a);
        }
        return Err(e);
    }

    // Persisted — safe to release the superseded accelerators now.
    if hotkey_changed {
        unregister(old_hotkey.as_str());
    }
    if apps_changed {
        if let Some(old) = old_apps_nonempty {
            if new_apps_hotkey.as_deref() != Some(old) {
                unregister(old);
            }
        }
    }

    let merged = config::load()?;
    *state.config.lock().unwrap() = merged.clone();
    Ok(merged)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOption {
    /// Value to store in `config.terminal` (bare name / path).
    pub id: String,
    /// Display name (the binary's basename).
    pub label: String,
}

/// Installed terminal emulators dev-prompt knows how to drive — feeds the
/// Settings dropdown.
#[tauri::command]
pub fn list_terminals(state: State<'_, AppState>) -> Vec<TerminalOption> {
    let cfg = state.config.lock().unwrap();
    crate::rules::terminal_options(&cfg)
        .into_iter()
        .map(|(id, label)| TerminalOption { id, label })
        .collect()
}

/// Shells found on PATH — feeds the Settings "Shell" dropdown and the
/// "Run command…" picker.
#[tauri::command]
pub fn list_shells() -> Vec<String> {
    let mut out: Vec<String> = ["pwsh", "powershell", "cmd", "bash", "zsh", "fish", "nu"]
        .into_iter()
        .filter(|name| crate::rules::which(name).is_some())
        .map(String::from)
        .collect();

    // Git-for-Windows bash usually isn't on PATH.
    #[cfg(windows)]
    if !out.iter().any(|s| s == "bash") {
        let git_bash = ["ProgramFiles", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(|b| std::env::var(b).ok())
            .flat_map(|pf| {
                ["Git\\bin\\bash.exe", "Git\\usr\\bin\\bash.exe"]
                    .map(|rel| Path::new(&pf).join(rel))
            })
            .any(|p| p.is_file());
        if git_bash {
            out.push("bash".to_string());
        }
    }
    out
}

/// Run a free-form command in `path`'s terminal. Blank `command` opens the
/// chosen shell interactively. Feeds the "Run command…" action.
/// `async` + `spawn_blocking`: terminal/shell resolution (`which`, globs) and the
/// spawn — the overlay `await`s this before it dismisses.
#[tauri::command]
pub async fn run_command(
    state: State<'_, AppState>,
    path: String,
    command: String,
    shell: Option<String>,
) -> AppResult<()> {
    let repo = repo_for_path(&state, &path);
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let shell = shell
            .filter(|s| !s.trim().is_empty())
            .or_else(|| cfg.shell.clone());
        let resolver = Resolver::new(&cfg.programs)
            .with_terminal(cfg.terminal.as_deref(), cfg.terminal_template.as_deref())
            .with_shell(shell.as_deref());

        let command = command.trim();
        let (program, args, cwd) = if command.is_empty() {
            let sh = shell.clone().unwrap_or_else(|| {
                if crate::rules::which("pwsh").is_some() {
                    "pwsh".into()
                } else {
                    "powershell".into()
                }
            });
            terminal_command(&sh, &repo.path, &resolver, false)
        } else {
            terminal_command(command, &repo.path, &resolver, true)
        };

        let action = Action {
            id: "run-command".into(),
            label: command.to_string(),
            hint: command.to_string(),
            group: String::new(),
            default: false,
            icon: None,
            program,
            args,
            cwd,
            client_side: false,
            prompt: false,
        };
        launch::launch(&action, &repo)
    })
    .await
    .map_err(|e| AppError::msg(format!("run command task failed: {e}")))?
}

// --- installed-app launcher ( > scope ) -----------------------------------

/// Apps change rarely — a day-long freshness window keeps re-enumeration cheap.
const APPS_TTL_SECS: u64 = 86_400;

/// An `AppEntry` plus its launch count for the frontend. `uses` is derived per
/// read from `app-usage.json` and lives only here — it's never on `AppEntry`, so
/// it can't leak into `apps.json`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    #[serde(flatten)]
    entry: AppEntry,
    uses: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppsPayload {
    pub apps: Vec<AppView>,
    pub age_secs: i64,
    pub stale: bool,
}

/// Attach each app's current launch count.
fn apps_with_usage(apps: Vec<AppEntry>) -> Vec<AppView> {
    let counts = crate::usage::counts();
    apps.into_iter()
        .map(|entry| {
            let uses = counts.get(&entry.exec.to_lowercase()).copied().unwrap_or(0);
            AppView { entry, uses }
        })
        .collect()
}

/// Cache-first installed-app list for the `>` scope.
#[tauri::command]
pub fn list_apps(state: State<'_, AppState>) -> AppsPayload {
    let apps = apps_with_usage(state.apps.lock().unwrap().clone());
    let age = *state.apps_age_secs.lock().unwrap();
    AppsPayload {
        apps,
        age_secs: age,
        stale: cache::is_stale(age, APPS_TTL_SECS),
    }
}

/// Re-enumerate installed apps off the UI thread. Emits `apps:updated`.
#[tauri::command]
pub async fn rescan_apps(app: AppHandle, state: State<'_, AppState>) -> AppResult<AppsPayload> {
    let cfg = state.config.lock().unwrap().clone();

    let fresh = if cfg.apps.enabled {
        tauri::async_runtime::spawn_blocking(move || apps::discover(&cfg))
            .await
            .map_err(|e| AppError::msg(format!("app scan task failed: {e}")))?
    } else {
        Vec::new()
    };

    cache::save_apps(&fresh)?;
    *state.apps.lock().unwrap() = fresh.clone();
    *state.apps_age_secs.lock().unwrap() = 0;

    let _ = app.emit("apps:updated", ());
    Ok(AppsPayload {
        apps: apps_with_usage(fresh),
        age_secs: 0,
        stale: false,
    })
}

/// Launch an installed app, bumping its frecency count only if the launch
/// actually starts (a stale entry whose exe is gone shouldn't climb the list).
/// `async` + `spawn_blocking`: keeps the spawn and the frecency file write off
/// the UI thread, which is about to dismiss the overlay.
#[tauri::command]
pub async fn run_app(
    exec: String,
    kind: String,
    args: Option<Vec<String>>,
) -> AppResult<()> {
    let kind = match kind.as_str() {
        "aumid" => crate::apps::AppKind::Aumid,
        _ => crate::apps::AppKind::Exe,
    };
    let entry = AppEntry {
        name: String::new(),
        exec: exec.clone(),
        kind,
        args: args.unwrap_or_default(),
        icon: None,
        source: String::new(),
    };
    tauri::async_runtime::spawn_blocking(move || apps::launch(&entry))
        .await
        .map_err(|e| AppError::msg(format!("launch task failed: {e}")))??;
    // Frecency write stays off the UI thread but is still awaited: it's a
    // sub-millisecond JSON rewrite, and letting it run detached loses the
    // increment whenever the process exits (tray quit, updater relaunch) before
    // the task gets scheduled.
    let _ = tauri::async_runtime::spawn_blocking(move || crate::usage::bump(&exec)).await;
    Ok(())
}

/// Toggle whether focus loss dismisses the overlay (frontend turns this off for
/// the settings screen).
#[tauri::command]
pub fn set_dismiss_on_blur(state: State<'_, AppState>, enabled: bool) {
    *state.dismiss_on_blur.lock().unwrap() = enabled;
}

/// Whether the app is registered to start at login (OS is the source of truth).
/// Sync: a few registry reads, and kept in the same ordering domain as
/// [`set_autostart`] so a read never races ahead of a pending write.
#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// Sync on purpose: it must stay sync so rapid checkbox toggles apply in click
/// order. As an async command the enable/disable calls race on the runtime pool
/// and the last click can lose. The write is a couple of registry ops — fast on
/// the UI thread now that `config_summary` no longer hogs it.
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> AppResult<()> {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    let r = if enabled { al.enable() } else { al.disable() };
    r.map_err(|e| AppError::msg(format!("autostart: {e}")))?;
    #[cfg(windows)]
    if !enabled {
        clear_startup_approved();
    }
    Ok(())
}

/// Delete the Task-Manager toggle-state entry Windows keeps beside a `Run` value.
/// `auto-launch`'s `disable()` removes the `Run` value but leaves this behind,
/// and once we're uninstalled nothing else owns it. The value name matches
/// `productName` — what `tauri-plugin-autostart` registers the `Run` entry under.
#[cfg(windows)]
fn clear_startup_approved() {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    const APPROVED_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    if let Ok(key) =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(APPROVED_KEY, KEY_SET_VALUE)
    {
        let _ = key.delete_value("dev-prompt");
    }
}

/// Reflect update availability in the tray tooltip. `version` = `None` resets it.
#[tauri::command]
pub fn set_update_hint(app: AppHandle, version: Option<String>) {
    if let Some(tray) = app.tray_by_id("main") {
        let tip = match version {
            Some(v) => format!("dev-prompt — v{v} available"),
            None => "dev-prompt".to_string(),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}

/// Open `rules.yaml` with the OS default handler for `.yaml` (or the "Open
/// with" picker when nothing is associated), and drop the overlay's
/// always-on-top so the editor is actually visible in front of it.
#[tauri::command]
pub fn open_rules_file(window: tauri::WebviewWindow) -> AppResult<()> {
    let path = config::rules_path()?;
    if !path.exists() {
        let _ = config::load()?; // writes the scaffold
    }

    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    std::process::Command::new(program)
        .arg(&path)
        .spawn()
        .map_err(|e| AppError::msg(format!("could not open {}: {e}", path.display())))?;

    let _ = window.set_always_on_top(false);
    Ok(())
}

/// Open the GitHub releases page in the user's browser. The update box links
/// here rather than carrying in-app release notes.
#[tauri::command]
pub fn open_releases_page(window: tauri::WebviewWindow) -> AppResult<()> {
    const URL: &str = "https://github.com/seraphx2/dev-prompt/releases";

    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    std::process::Command::new(program)
        .arg(URL)
        .spawn()
        .map_err(|e| AppError::msg(format!("could not open {URL}: {e}")))?;

    let _ = window.set_always_on_top(false);
    Ok(())
}
