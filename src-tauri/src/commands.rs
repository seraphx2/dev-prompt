use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::cache;
use crate::config::{self, resolved_roots, Config};
use crate::error::{AppError, AppResult};
use crate::index::{self, ScoredRepo};
use crate::inspect;
use crate::launch;
use crate::rules::{build_actions as build_actions_impl, find_action, Action};
use crate::scan::{self, Repo};

/// In-memory app state shared across commands.
pub struct AppState {
    pub config: Mutex<Config>,
    pub repos: Mutex<Vec<Repo>>,
    /// Age (secs) of the list currently in `repos`; -1 when never cached.
    pub age_secs: Mutex<i64>,
    /// Whether losing focus dismisses the overlay. Off while the settings
    /// screen is open so clicking away to copy a path doesn't nuke edits.
    pub dismiss_on_blur: Mutex<bool>,
}

impl AppState {
    pub fn load() -> Self {
        let config = config::load().unwrap_or_default();
        let loaded = cache::load().unwrap_or(cache::LoadedCache {
            repos: Vec::new(),
            age_secs: -1,
        });
        AppState {
            config: Mutex::new(config),
            repos: Mutex::new(loaded.repos),
            age_secs: Mutex::new(loaded.age_secs),
            dismiss_on_blur: Mutex::new(true),
        }
    }
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

    // Filesystem walk off the main thread.
    let fresh = tauri::async_runtime::spawn_blocking(move || {
        let roots = resolved_roots(&cfg);
        scan::scan(&roots, &cfg)
    })
    .await
    .map_err(|e| AppError::msg(format!("scan task failed: {e}")))?;

    let merged = cache::merge(&previous, fresh);
    cache::save(&merged)?;

    *state.repos.lock().unwrap() = merged;
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
            sentinels: Vec::new(),
            last_seen: 0,
        })
}

#[tauri::command]
pub fn build_actions(state: State<'_, AppState>, path: String) -> Vec<Action> {
    let repo = repo_for_path(&state, &path);
    let ctx = inspect::inspect(std::path::Path::new(&repo.path));
    let cfg = state.config.lock().unwrap();
    build_actions_impl(&repo, &ctx, &cfg)
}

#[tauri::command]
pub fn run_action(
    state: State<'_, AppState>,
    action_id: String,
    path: String,
) -> AppResult<()> {
    let repo = repo_for_path(&state, &path);
    let ctx = inspect::inspect(std::path::Path::new(&repo.path));
    let action = {
        let cfg = state.config.lock().unwrap();
        find_action(&repo, &ctx, &cfg, &action_id)
    }
    .ok_or_else(|| AppError::msg(format!("unknown action: {action_id}")))?;
    launch::launch(&action, &repo)
}

#[tauri::command]
pub fn hide_overlay(window: tauri::WebviewWindow) -> AppResult<()> {
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
pub fn config_summary(state: State<'_, AppState>) -> AppResult<crate::rules::ConfigSummary> {
    let path = config::rules_path()?.to_string_lossy().into_owned();
    let cfg = state.config.lock().unwrap();
    Ok(crate::rules::summarize(&cfg, path))
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
    pub roots: Option<Vec<String>>,
    pub cache_ttl_secs: Option<u64>,
    pub scan_max_depth: Option<usize>,
}

/// Apply an editable subset of settings: write `config.yaml`, update the live
/// config, and re-register the global hotkey if it changed (validated first, so
/// a bad accelerator is rejected before anything is persisted).
#[tauri::command]
pub fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: ConfigPatch,
) -> AppResult<Config> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let old_hotkey = state.config.lock().unwrap().hotkey.clone();
    let mut user = config::load_user()?;

    if let Some(h) = patch.hotkey {
        user.hotkey = Some(h.trim().to_string());
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
    if let Some(depth) = patch.scan_max_depth {
        user.scan = Some(config::ScanConfig {
            max_depth: depth.max(1),
        });
    }

    let new_hotkey = user.hotkey.clone().unwrap_or_else(|| old_hotkey.clone());
    if new_hotkey != old_hotkey {
        let gs = app.global_shortcut();
        gs.register(new_hotkey.as_str())
            .map_err(|e| AppError::msg(format!("invalid hotkey `{new_hotkey}`: {e}")))?;
        let _ = gs.unregister(old_hotkey.as_str());
    }

    config::save_user(&user)?;
    let merged = config::load()?;
    *state.config.lock().unwrap() = merged.clone();
    Ok(merged)
}

/// Toggle whether focus loss dismisses the overlay (frontend turns this off for
/// the settings screen).
#[tauri::command]
pub fn set_dismiss_on_blur(state: State<'_, AppState>, enabled: bool) {
    *state.dismiss_on_blur.lock().unwrap() = enabled;
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
