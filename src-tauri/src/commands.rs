use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::actions::{build_actions as build_actions_impl, find_action, Action};
use crate::cache;
use crate::config::{self, resolved_roots, Config};
use crate::error::{AppError, AppResult};
use crate::index::{self, ScoredRepo};
use crate::inspect;
use crate::launch;
use crate::scan::{self, Repo};

/// In-memory app state shared across commands.
pub struct AppState {
    pub config: Mutex<Config>,
    pub repos: Mutex<Vec<Repo>>,
    /// Age (secs) of the list currently in `repos`; -1 when never cached.
    pub age_secs: Mutex<i64>,
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
    // Re-read config.yaml so edits to `roots` take effect without an app restart.
    // A YAML syntax error surfaces here instead of being silently ignored.
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
    build_actions_impl(&repo, &ctx)
}

#[tauri::command]
pub fn run_action(
    state: State<'_, AppState>,
    action_id: String,
    path: String,
) -> AppResult<()> {
    let repo = repo_for_path(&state, &path);
    let ctx = inspect::inspect(std::path::Path::new(&repo.path));
    let action = find_action(&repo, &ctx, &action_id)
        .ok_or_else(|| AppError::msg(format!("unknown action: {action_id}")))?;
    launch::launch(&action, &repo)
}

#[tauri::command]
pub fn hide_overlay(window: tauri::WebviewWindow) -> AppResult<()> {
    window
        .hide()
        .map_err(|e| AppError::msg(format!("hide failed: {e}")))
}
