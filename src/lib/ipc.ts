import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { Action, AppConfig, RepoListPayload, ScoredRepo } from "./types";

/** Cache-first repo list; returns instantly from the on-disk cache when present. */
export function listRepos(): Promise<RepoListPayload> {
  return invoke<RepoListPayload>("list_repos");
}

/** Force a fresh filesystem scan. Emits `repos:updated` on completion. */
export function rescanRepos(): Promise<RepoListPayload> {
  return invoke<RepoListPayload>("rescan_repos");
}

/** Fuzzy-rank the cached repos against `query`. Empty query => recent/alpha order. */
export function searchRepos(query: string, limit = 200): Promise<ScoredRepo[]> {
  return invoke<ScoredRepo[]>("search_repos", { query, limit });
}

/** Context-aware actions for the repo at `path` (M1: universal actions only). */
export function buildActions(path: string): Promise<Action[]> {
  return invoke<Action[]>("build_actions", { path });
}

/** Spawn the action's process detached from the overlay. */
export function runAction(actionId: string, path: string): Promise<void> {
  return invoke<void>("run_action", { actionId, path });
}

export function hideOverlay(): Promise<void> {
  return invoke<void>("hide_overlay");
}

export function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_config");
}

/** Save an editable subset of settings; returns the updated config. */
export function saveConfig(patch: {
  hotkey?: string;
  roots?: string[];
  cache_ttl_secs?: number;
}): Promise<AppConfig> {
  return invoke<AppConfig>("save_config", { patch });
}

export function openConfigFile(): Promise<void> {
  return invoke<void>("open_config_file");
}

/** Toggle whether clicking away dismisses the overlay (off for the settings screen). */
export function setDismissOnBlur(enabled: boolean): Promise<void> {
  return invoke<void>("set_dismiss_on_blur", { enabled });
}

export function copyPath(path: string): Promise<void> {
  return writeText(path);
}

/** Fired by the backend when the overlay is shown via the global hotkey. */
export function onOverlayShown(cb: () => void): Promise<UnlistenFn> {
  return listen("overlay:shown", () => cb());
}

/** Fired by the backend after a background rescan replaces the cache. */
export function onReposUpdated(cb: () => void): Promise<UnlistenFn> {
  return listen("repos:updated", () => cb());
}
