import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type {
  Action,
  AppConfig,
  ConfigSummary,
  RepoListPayload,
  RepoTrace,
  ScoredRepo,
} from "./types";

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

/** Context-aware actions for the repo at `path`. Served from the scan-time cache. */
export function buildActions(path: string): Promise<Action[]> {
  return invoke<Action[]>("build_actions", { path });
}

/**
 * Re-inspect one repo in the background. Fire-and-forget after `buildActions`:
 * if the repo changed since the last scan the backend emits `repo:context-updated`.
 */
export function refreshRepoContext(path: string): Promise<void> {
  return invoke<void>("refresh_repo_context", { path });
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

/** Read-only view of the merged config (program resolution + rule availability). */
export function configSummary(): Promise<ConfigSummary> {
  return invoke<ConfigSummary>("config_summary");
}

/** Rule-by-rule trace of what one repo produces and why (settings debug view). */
export function repoRuleTrace(path: string): Promise<RepoTrace> {
  return invoke<RepoTrace>("repo_rule_trace", { path });
}

/** Re-read config.yaml + rules.yaml from disk and drop cached program lookups. */
export function reloadConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("reload_config");
}

/** Save an editable subset of settings; returns the updated config. */
export function saveConfig(patch: {
  hotkey?: string;
  roots?: string[];
  cache_ttl_secs?: number;
  scan_max_depth?: number;
  collapse_nested?: boolean | "auto";
}): Promise<AppConfig> {
  return invoke<AppConfig>("save_config", { patch });
}

/** Open rules.yaml (the hand-authored overrides file) in the default editor. */
export function openRulesFile(): Promise<void> {
  return invoke<void>("open_rules_file");
}

/** Native folder picker; returns the chosen directories (empty if cancelled). */
export async function pickDirectories(): Promise<string[]> {
  const res = await openDialog({
    directory: true,
    multiple: true,
    title: "Add root directories",
  });
  if (!res) return [];
  return Array.isArray(res) ? res : [res];
}

/** Whether the app starts at login. */
export function getAutostart(): Promise<boolean> {
  return invoke<boolean>("get_autostart");
}

export function setAutostart(enabled: boolean): Promise<void> {
  return invoke<void>("set_autostart", { enabled });
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

/** Fired by every backend hide path so the UI can reset while off-screen. */
export function onOverlayHidden(cb: () => void): Promise<UnlistenFn> {
  return listen("overlay:hidden", () => cb());
}

/** Fired when a background re-inspect found a repo changed since the last scan. */
export function onRepoContextUpdated(
  cb: (path: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("repo:context-updated", (e) => cb(e.payload));
}

/** Fired by the backend when the tray "Settings…" item is chosen. */
export function onGotoSettings(cb: () => void): Promise<UnlistenFn> {
  return listen("overlay:goto-settings", () => cb());
}
