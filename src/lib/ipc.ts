import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type {
  Action,
  AppConfig,
  AppEntry,
  AppListPayload,
  BranchList,
  ClonePlan,
  ConfigPatch,
  ConfigSummary,
  FileManagerOption,
  NewRepoOptions,
  RemovalAssessment,
  Repo,
  RepoListPayload,
  RepoStatus,
  RepoTrace,
  ScoredRepo,
  SwitchTarget,
  SyncOp,
  SystemPaths,
  TerminalOption,
  VcsDetails,
  VcsOutcome,
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
 * Universal actions only (terminal / editor / file manager / AI-CLI
 * launchers) — no `requires:` PATH walk, so this resolves instantly even on a
 * cold cache. Use this to render the action menu immediately, then merge in
 * `buildDetectedActions` once it lands.
 */
export function buildUniversalActions(path: string): Promise<Action[]> {
  return invoke<Action[]>("build_universal_actions", { path });
}

/**
 * Per-ecosystem detected actions (npm/cargo/mvn/docker/…). Slower — walks
 * PATH for each `requires:` binary the first time this session checks it
 * (memoized after). Call after the menu is already showing universal actions.
 */
export function buildDetectedActions(path: string): Promise<Action[]> {
  return invoke<Action[]>("build_detected_actions", { path });
}

/**
 * Re-inspect one repo in the background. Fire-and-forget after `buildActions`:
 * if the repo changed since the last scan the backend emits `repo:context-updated`.
 */
export function refreshRepoContext(path: string): Promise<void> {
  return invoke<void>("refresh_repo_context", { path });
}

/** Spawn the action's process detached from the overlay — `elevated`: as
 *  administrator / root, for an action with `elevated` set. */
export function runAction(actionId: string, path: string, elevated = false): Promise<void> {
  return invoke<void>("run_action", { actionId, path, elevated });
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
export function saveConfig(patch: ConfigPatch): Promise<AppConfig> {
  return invoke<AppConfig>("save_config", { patch });
}

/** Installed terminal emulators dev-prompt knows how to drive. */
export function listTerminals(): Promise<TerminalOption[]> {
  return invoke<TerminalOption[]>("list_terminals");
}

/** What "Auto" actually opens right now — names the dropdown's Auto option. */
export function autoTerminal(): Promise<string> {
  return invoke<string>("auto_terminal");
}

/** Installed file managers found via `programs.filemanager`. */
export function listFileManagers(): Promise<FileManagerOption[]> {
  return invoke<FileManagerOption[]>("list_file_managers");
}

/** Shells found on PATH (`pwsh`, `bash`, `nu`, …). */
export function listShells(): Promise<string[]> {
  return invoke<string[]>("list_shells");
}

/** The shell used with nothing pinned. */
export function defaultShell(): Promise<string> {
  return invoke<string>("default_shell");
}

/**
 * Start a built-in shell session in `cwd`. `shell` is a picked shell, or
 * empty for `config.shell` / the platform default. Output arrives on `onData`
 * as raw bytes (UTF-8 + escape codes, possibly split mid-character — feed it
 * straight to xterm.js); `onExit` fires once, after the last of it.
 */
export function shellOpen(
  cwd: string,
  shell: string | null,
  cols: number,
  rows: number,
  onData: (bytes: Uint8Array) => void,
  onExit: (code: number | null) => void,
): Promise<{ id: number; label: string }> {
  const data = new Channel<ArrayBuffer>();
  data.onmessage = (buf) => onData(new Uint8Array(buf));
  const exit = new Channel<number | null>();
  exit.onmessage = onExit;
  return invoke("shell_open", {
    cwd,
    shell: shell || null,
    cols,
    rows,
    onData: data,
    onExit: exit,
  });
}

/** Keystrokes / a paste for a shell session. */
export function shellWrite(id: number, data: Uint8Array): Promise<void> {
  return invoke<void>("shell_write", { id, data: Array.from(data) });
}

export function shellResize(id: number, cols: number, rows: number): Promise<void> {
  return invoke<void>("shell_resize", { id, cols, rows });
}

/** End a shell session — its exit still arrives through `onExit`. */
export function shellClose(id: number): Promise<void> {
  return invoke<void>("shell_close", { id });
}

/** End every shell session (run at startup, for any a reload lost track of). */
export function shellCloseAll(): Promise<void> {
  return invoke<void>("shell_close_all");
}

/** The usual overlay size, or the roomier one while a shell is open. */
export function setOverlaySize(large: boolean): Promise<void> {
  return invoke<void>("set_overlay_size", { large });
}

/** Open an http(s) link in the default browser. */
export function openUrl(url: string): Promise<void> {
  return invoke<void>("open_url", { url });
}

/** Roots + installed version-control setups for the "New repository" screen. */
export function newRepoOptions(): Promise<NewRepoOptions> {
  return invoke<NewRepoOptions>("new_repo_options");
}

/** Whether each path (`~` / environment variables allowed) is an existing folder. */
export function foldersExist(paths: string[]): Promise<boolean[]> {
  return invoke<boolean[]>("folders_exist", { paths });
}

/** Why `name` can't be created under `root`; `null` when it can. */
export function checkNewRepo(root: string, name: string): Promise<string | null> {
  return invoke<string | null>("check_new_repo", { root, name });
}

/**
 * Create `<root>/<name>` and run version-control setup `tool` in it. Resolves to
 * the new repo (already added to the list); rejects with the tool's error.
 */
/** What `url` is (asking the installed tools when its shape doesn't say), its
 *  folder name, and the tools that can clone it. Rejects when nothing can. */
export function clonePlan(url: string): Promise<ClonePlan> {
  return invoke<ClonePlan>("clone_plan", { url });
}

/** Clone `url` into `<root>/<name>` with `tool`. Resolves to the new repo
 *  (already added to the list); rejects with the tool's error. */
export function cloneRepo(root: string, name: string, url: string, tool: string): Promise<Repo> {
  return invoke<Repo>("clone_repo", { root, name, url, tool });
}

export function createRepo(
  root: string,
  name: string,
  tool: string,
  values: Record<string, string>,
): Promise<Repo> {
  return invoke<Repo>("create_repo", { root, name, tool, values });
}

/**
 * Set up version control in an existing folder that has none. Resolves to the
 * versioned repo (already updated in the list); rejects with the tool's error.
 */
export function initRepo(
  path: string,
  tool: string,
  values: Record<string, string>,
): Promise<Repo> {
  return invoke<Repo>("init_repo", { path, tool, values });
}

/** Every repo's last-read working-copy status, by path. */
export function listStatuses(): Promise<Record<string, RepoStatus>> {
  return invoke<Record<string, RepoStatus>>("list_statuses");
}

/** Re-read `paths` now, or (none given) every repo not read in the last few
 *  minutes. Results arrive through `onStatusUpdated`. */
export function refreshStatuses(paths?: string[]): Promise<void> {
  return invoke<void>("refresh_statuses", { paths: paths ?? null });
}

/** One repo's status changed (`status` null: it has none any more). */
export function onStatusUpdated(
  cb: (path: string, status: RepoStatus | null) => void,
): Promise<UnlistenFn> {
  return listen<{ path: string; status: RepoStatus | null }>("status:updated", (e) =>
    cb(e.payload.path, e.payload.status),
  );
}

/** Branches or bookmarks, remote ones nothing local tracks, and other
 *  working copies, for the branch switcher (Subversion's from the server). */
export function vcsBranches(path: string): Promise<BranchList> {
  return invoke<BranchList>("vcs_branches", { path });
}

/** Remotes, the default branch, web links, pull / push locations — local reads only. */
export function vcsDetails(path: string): Promise<VcsDetails> {
  return invoke<VcsDetails>("vcs_details", { path });
}

/** Check out a branch (bookmark); rejects with the reason when it can't. */
export function vcsSwitch(path: string, target: SwitchTarget): Promise<VcsOutcome> {
  return invoke<VcsOutcome>("vcs_switch", { path, target });
}

/** Fetch, pull, push, publish (to `remote`), update or sync. */
export function vcsSync(path: string, op: SyncOp, remote?: string): Promise<VcsOutcome> {
  return invoke<VcsOutcome>("vcs_sync", { path, op, remote: remote ?? null });
}

/** Check a branch out in a second working copy beside the repo; resolves to it. */
export function vcsAddCopy(path: string, target: SwitchTarget): Promise<Repo> {
  return invoke<Repo>("vcs_add_copy", { path, target });
}

/** What moving the folder at `path` to the bin would lose. */
export function assessRemoval(path: string): Promise<RemovalAssessment> {
  return invoke<RemovalAssessment>("assess_removal", { path });
}

/** Move a listed folder to the Recycle Bin / Trash. Resolves to whether Undo
 *  is available. */
export function removeFolder(path: string): Promise<boolean> {
  return invoke<boolean>("remove_folder", { path });
}

/** Put the folder last moved to the bin back. Resolves to its path. */
export function undoRemoval(): Promise<string> {
  return invoke<string>("undo_removal");
}

/** Cache-first list of installed apps for the `>` scope. */
export function listApps(): Promise<AppListPayload> {
  return invoke<AppListPayload>("list_apps");
}

/** Force a fresh enumeration of installed apps. Emits `apps:updated`. */
export function rescanApps(): Promise<AppListPayload> {
  return invoke<AppListPayload>("rescan_apps");
}

/** Launch an installed app (bumps its frecency count) — `elevated`: Run as
 *  administrator, for a Windows desktop app (`kind: "exe"`). */
export function runApp(e: AppEntry, elevated = false): Promise<void> {
  return invoke<void>("run_app", {
    exec: e.exec,
    kind: e.kind,
    args: e.args ?? null,
    terminal: e.terminal ?? null,
    elevated,
  });
}

/** Fired after a background app re-enumeration replaces the cache. */
export function onAppsUpdated(cb: () => void): Promise<UnlistenFn> {
  return listen("apps:updated", () => cb());
}

/** Open rules.yaml (the hand-authored overrides file) in the default editor. */
export function openRulesFile(): Promise<void> {
  return invoke<void>("open_rules_file");
}

/** Open the GitHub releases page in the default browser. */
export function openReleasesPage(): Promise<void> {
  return invoke<void>("open_releases_page");
}

/** Open the folder holding the running executable. */
export function openInstallDir(): Promise<void> {
  return invoke<void>("open_install_dir");
}

/** Open the folder holding config.yaml / rules.yaml. */
export function openConfigDir(): Promise<void> {
  return invoke<void>("open_config_dir");
}

/** Open the folder holding the regenerable caches (repos.json, apps.json, …). */
export function openCacheDir(): Promise<void> {
  return invoke<void>("open_cache_dir");
}

/** Where things live on disk, for the Settings "File locations" panel. */
export function systemPaths(): Promise<SystemPaths> {
  return invoke<SystemPaths>("system_paths");
}

/** How this build receives updates — see the `updater_mode` command.
 *  `"self"`: run the in-app updater. `"managed"`/`"unmanaged"`: don't. */
export type UpdaterMode = "self" | "managed" | "unmanaged";
export function updaterMode(): Promise<UpdaterMode> {
  return invoke<UpdaterMode>("updater_mode");
}

/** True when running inside a Flatpak sandbox — hides controls it can't honour. */
export function isFlatpak(): Promise<boolean> {
  return invoke<boolean>("is_flatpak");
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

/**
 * Flatpak-only autostart via the XDG Background portal (the sandbox can't write
 * ~/.config/autostart). Shows a system consent dialog the first time; resolves
 * to the state the portal actually granted.
 */
export function setAutostartPortal(enabled: boolean): Promise<boolean> {
  return invoke<boolean>("set_autostart_portal", { enabled });
}

/** Toggle whether clicking away dismisses the overlay (off for the settings screen). */
export function setDismissOnBlur(enabled: boolean): Promise<void> {
  return invoke<void>("set_dismiss_on_blur", { enabled });
}

export function copyPath(path: string): Promise<void> {
  return writeText(path);
}

/** Put any text on the clipboard. */
export function copyText(text: string): Promise<void> {
  return writeText(text);
}

/** The clipboard's text, or "" when it holds none (an image, nothing). */
export async function readClipboard(): Promise<string> {
  try {
    return (await readText()) ?? "";
  } catch {
    return "";
  }
}

/** Open the folder holding `path` in the user's file manager. */
export function showFolder(path: string): Promise<void> {
  return invoke<void>("show_folder", { path });
}

/**
 * Fired by the backend when the overlay is shown. `scope` is `"apps"` when it
 * was opened via the app-launcher hotkey, else `"repos"`.
 */
export function onOverlayShown(
  cb: (scope: "repos" | "apps") => void,
): Promise<UnlistenFn> {
  return listen<string>("overlay:shown", (e) =>
    cb(e.payload === "apps" ? "apps" : "repos"),
  );
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

/** The tray's "Close all shells" was clicked. */
export function onCloseAllShells(cb: () => void): Promise<UnlistenFn> {
  return listen("shells:close-all", () => cb());
}
