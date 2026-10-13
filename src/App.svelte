<script lang="ts">
  import { onMount, tick } from "svelte";
  import SearchInput from "./lib/components/SearchInput.svelte";
  import ResultList from "./lib/components/ResultList.svelte";
  import AppList from "./lib/components/AppList.svelte";
  import ActionMenu from "./lib/components/ActionMenu.svelte";
  import RunCommand from "./lib/components/RunCommand.svelte";
  import NewRepo from "./lib/components/NewRepo.svelte";
  import ContextMenu from "./lib/components/ContextMenu.svelte";
  import RemoveFolder from "./lib/components/RemoveFolder.svelte";
  import Settings from "./lib/components/settings/Settings.svelte";
  import type { Page as SettingsPage } from "./lib/components/settings/model";
  import {
    buildActions,
    buildDetectedActions,
    buildUniversalActions,
    copyPath,
    copyText,
    getConfig,
    hideOverlay,
    listApps,
    listRepos,
    onAppsUpdated,
    onGotoSettings,
    onOverlayHidden,
    onOverlayShown,
    onReposUpdated,
    onRepoContextUpdated,
    readClipboard,
    refreshRepoContext,
    rescanApps,
    rescanRepos,
    runAction,
    runApp,
    runCommand,
    searchRepos,
    setDismissOnBlur,
    showFolder,
    undoRemoval,
  } from "./lib/ipc";
  import type {
    Action,
    AppEntry,
    CtxItem,
    MenuItem,
    Repo,
    ScoredRepo,
  } from "./lib/types";
  import { fuzzyScore } from "./lib/fuzzy";
  import { icons } from "./lib/icons";
  import { upd, pollUpdates } from "./lib/updateStore.svelte";

  const DAY_MS = 24 * 60 * 60 * 1000;

  type Mode =
    | "repo-list"
    | "action-menu"
    | "settings"
    | "run-command"
    | "new-repo"
    | "remove";

  /** The page Settings opens on; null for the one it showed last. */
  let settingsPage = $state<SettingsPage | null>(null);

  function openSettings(page: SettingsPage | null = null) {
    settingsPage = page;
    mode = "settings";
  }

  let query = $state("");
  let results = $state<ScoredRepo[]>([]);
  let selected = $state(0);

  // App-launcher scope: a leading ">" in the query switches the repo list for a
  // search over installed apps. Repos stay the default on every overlay open.
  let apps = $state<AppEntry[]>([]);
  /** A message about the app list (scanning, an error); "" shows the count. */
  let appStatus = $state("");
  let appCount = $state<number | null>(null);
  let appsScanning = $state(false);
  // Whether the "> apps" scope is usable at all — loaded on mount and
  // refreshed after a settings save. Unchecking "Index installed apps" turns
  // the whole scope off rather than leaving it reachable but always empty.
  let appsEnabled = $state(true);
  getConfig().then((c) => (appsEnabled = c.apps?.enabled ?? true));
  const appScope = $derived(appsEnabled && query.startsWith(">"));
  const term = $derived(
    appScope ? query.slice(1).replace(/^\s+/, "") : query.trim(),
  );

  type AppHit = { app: AppEntry; positions: number[] };
  const filteredApps = $derived.by<AppHit[]>(() => {
    if (!appScope) return [];
    const q = term;
    if (!q) {
      return [...apps]
        .sort((a, b) => b.uses - a.uses || a.name.localeCompare(b.name))
        .map((app) => ({ app, positions: [] }));
    }
    const scored: { app: AppEntry; positions: number[]; score: number }[] = [];
    for (const app of apps) {
      const onName = fuzzyScore(q, app.name);
      if (onName) {
        scored.push({ app, positions: onName.positions, score: onName.score });
      } else if (fuzzyScore(q, app.exec)) {
        // Path-only match: always ranks below any name match.
        scored.push({ app, positions: [], score: -1e6 });
      }
    }
    scored.sort(
      (a, b) =>
        b.score - a.score ||
        b.app.uses - a.app.uses ||
        a.app.name.localeCompare(b.app.name),
    );
    return scored.map(({ app, positions }) => ({ app, positions }));
  });

  const activeCount = $derived(
    appScope ? filteredApps.length : results.length,
  );

  // Keep the selection inside whichever list is on screen.
  $effect(() => {
    if (selected >= activeCount) selected = Math.max(0, activeCount - 1);
  });

  let mode = $state<Mode>("repo-list");
  let actions = $state<Action[]>([]);
  let actionQuery = $state("");
  let actionSel = $state(0);
  let activeRepo = $state<ScoredRepo | null>(null);
  /** True while `buildDetectedActions` is still walking PATH for `activeRepo`. */
  let detectedLoading = $state(false);
  /** When set, the action menu is drilled into this `Detected · <x>` group. */
  let subGroup = $state<string | null>(null);
  /** `run:` template of the prompt action that opened the "Run command…" mode. */
  let promptTemplate = $state("");
  /** Set while the new-repo screen is setting up an existing folder (opened
   *  from the action menu) rather than creating a new one. */
  let setupTarget = $state<ScoredRepo | null>(null);
  /** The run-command / setup screen was opened from a repo row's right-click
   *  menu, so Back returns to the list rather than to an action menu. */
  let openedFromList = $state(false);
  /** The folder the "Move to Recycle Bin / Trash" screen is for. */
  let removeTarget = $state<ScoredRepo | null>(null);
  /** The folder just moved to the bin, while Undo is offered. */
  let undoToast = $state<{ name: string } | null>(null);
  let undoTimer: ReturnType<typeof setTimeout> | undefined;
  const UNDO_MS = 20_000;
  const bin =
    document.documentElement.dataset.os === "windows" ? "Recycle Bin" : "Trash";

  function openRemove(entry: ScoredRepo, fromList: boolean) {
    removeTarget = entry;
    openedFromList = fromList;
    mode = "remove";
  }

  function removed(name: string, undoable: boolean) {
    backToList();
    clearTimeout(undoTimer);
    if (undoable) {
      undoToast = { name };
      undoTimer = setTimeout(() => (undoToast = null), UNDO_MS);
    } else {
      status = `Moved ${name} to the ${bin}. Restore it from there if you need it.`;
    }
  }

  async function undoRemove() {
    if (!undoToast) return;
    const { name } = undoToast;
    undoToast = null;
    clearTimeout(undoTimer);
    try {
      await undoRemoval();
      status = `Restored ${name}.`;
    } catch (e) {
      status = `Couldn't restore ${name}: ${e}`;
    }
  }

  const SUB_PREFIX = "Detected · ";

  // Actions with a `hotkey` (set in default_config.yaml, see docs/rules-engine.md)
  // get a dedicated row at the top of the action menu instead of taking up
  // arrow-key slots in the list below. Sorted by key so the row reads C, E, R, T.
  const quickActions = $derived(
    actions
      .filter((a): a is Action & { hotkey: string } => !!a.hotkey)
      .map((action) => ({ key: action.hotkey, action }))
      .sort((a, b) => a.key.localeCompare(b.key)),
  );

  // "Set up version control…": the new-repo form, pointed at this folder.
  function openSetup() {
    if (!activeRepo) return;
    setupTarget = activeRepo;
    mode = "new-repo";
  }

  function runQuickAction(action: Action) {
    if (action.provision) {
      openSetup();
    } else if (action.prompt) {
      promptTemplate = action.hint ?? "";
      mode = "run-command";
    } else if (activeRepo) {
      execute(action, activeRepo.repo.path);
    }
  }

  // Curated cluster order for the universal tier — matches the hand-alphabetized
  // sections in default_config.yaml. An action's `cluster` sorts it into one of
  // these alphabetically by label; "" (unclustered) stays up front in its
  // original order — nothing built-in is unclustered today since `terminal`
  // moved into the quick-action row, but a custom universal action can still
  // leave `cluster:` unset. A cluster name outside this list (a custom rule's
  // typo, or one left unset on a `pin: true` action) sorts after all of them,
  // grouped by first appearance. This is what lets a `pin: true` rule action
  // (e.g. "Open in Visual Studio") land in its correct alphabetical spot in
  // the `ide` cluster even though it only arrives once the slower detected
  // pass resolves, well after the rest of the universal list is on screen.
  const KNOWN_CLUSTERS = ["ai-cli", "ai-editor", "ide", "git"];

  function clusterSort(items: Action[]): Action[] {
    const passthrough: Action[] = [];
    const known = new Map<string, Action[]>(KNOWN_CLUSTERS.map((c) => [c, []]));
    const unknownOrder: string[] = [];
    const unknown = new Map<string, Action[]>();

    for (const a of items) {
      if (!a.cluster) {
        passthrough.push(a);
      } else if (known.has(a.cluster)) {
        known.get(a.cluster)!.push(a);
      } else {
        if (!unknown.has(a.cluster)) {
          unknown.set(a.cluster, []);
          unknownOrder.push(a.cluster);
        }
        unknown.get(a.cluster)!.push(a);
      }
    }

    const byLabel = (a: Action, b: Action) =>
      a.label.localeCompare(b.label, undefined, { sensitivity: "base" });

    return [
      ...passthrough,
      ...KNOWN_CLUSTERS.flatMap((c) => known.get(c)!.sort(byLabel)),
      ...unknownOrder.flatMap((c) => unknown.get(c)!.sort(byLabel)),
    ];
  }

  // Per-repo usage (see action_usage.rs) floats a picked-often action above the
  // curated cluster order entirely — used actions sort by decayed score, above
  // every unused one, which keeps falling back to clusterSort(). An old
  // favorite with a stale score naturally decays back down over disuse instead
  // of sitting on top forever by lifetime count alone.
  function orderGeneral(items: Action[]): Action[] {
    const used = items
      .filter((a) => a.usageScore > 0)
      .sort((a, b) => b.usageScore - a.usageScore);
    const unused = items.filter((a) => a.usageScore <= 0);
    return [...used, ...clusterSort(unused)];
  }

  function fuzzyItems(list: Action[], q: string, blankGroup = false): MenuItem[] {
    const grp = (a: Action) => (blankGroup ? "" : a.group);
    if (!q) {
      return list.map((action) => ({
        kind: "action" as const,
        group: grp(action),
        action,
        positions: [],
      }));
    }
    const out: MenuItem[] = [];
    for (const action of list) {
      const onLabel = fuzzyScore(q, action.label);
      if (onLabel) {
        out.push({ kind: "action", group: grp(action), action, positions: onLabel.positions });
      } else if (fuzzyScore(q, `${action.group} ${action.hint}`)) {
        out.push({ kind: "action", group: grp(action), action, positions: [] });
      }
    }
    return out;
  }

  // The rows shown in the action menu.
  //  - drilled into a sub-project: just its actions (filtered)
  //  - top level while typing: every action, flattened, so search reaches
  //    into sub-projects
  //  - top level idle: each sub-project collapses to one drill-in row
  const menuItems = $derived.by<MenuItem[]>(() => {
    const q = actionQuery.trim();

    if (subGroup) {
      return fuzzyItems(
        actions.filter((a) => a.group === subGroup),
        q,
        true,
      );
    }
    const rest = actions.filter((a) => !a.hotkey);
    if (q) return fuzzyItems(rest, q);

    // Universal-tier ("General") actions are cluster-sorted as a block up
    // front — this is also what re-slots a `pin: true` rule action into place
    // once it streams in from the (slower) detected pass. Everything else
    // (plain "Detected" root actions, sub-project drill-ins) keeps the
    // original array order, unaffected.
    const generalActions: Action[] = [];
    const others: Action[] = [];
    for (const a of rest) {
      (a.group === "General" ? generalActions : others).push(a);
    }

    const items: MenuItem[] = orderGeneral(generalActions).map((action) => ({
      kind: "action" as const,
      group: action.group,
      action,
      positions: [],
    }));

    const seen = new Set<string>();
    for (const a of others) {
      if (a.group.startsWith(SUB_PREFIX)) {
        if (!seen.has(a.group)) {
          seen.add(a.group);
          items.push({
            kind: "submenu",
            group: "Detected",
            target: a.group,
            label: a.group.slice(SUB_PREFIX.length),
            count: actions.filter((x) => x.group === a.group).length,
          });
        }
        continue;
      }
      items.push({ kind: "action", group: a.group, action: a, positions: [] });
    }
    // Detected actions haven't landed yet — show a placeholder in their spot
    // rather than leaving the menu looking like this repo has nothing.
    if (detectedLoading && !seen.size) {
      items.push({ kind: "loading", group: "Detected", label: "Checking…" });
    }
    return items;
  });

  // Keep the action selection within the current list.
  $effect(() => {
    if (actionSel >= menuItems.length) {
      actionSel = Math.max(0, menuItems.length - 1);
    }
  });

  function activateMenuItem(i: number) {
    const it = menuItems[i];
    if (!it || it.kind === "loading") return;
    if (it.kind === "submenu") {
      subGroup = it.target;
      actionQuery = "";
      actionSel = 0;
    } else if (it.action.remove) {
      if (activeRepo) openRemove(activeRepo, false);
    } else if (it.action.provision) {
      openSetup();
    } else if (it.action.prompt) {
      promptTemplate = it.action.hint ?? "";
      mode = "run-command";
    } else if (activeRepo) {
      execute(it.action, activeRepo.repo.path);
    }
  }

  function menuBack() {
    if (subGroup) {
      subGroup = null;
      actionQuery = "";
      actionSel = 0;
    } else {
      backToList();
    }
  }

  // `dismiss` from config: "always" | "keep_on_blur" | "manual". Loaded on mount
  // and refreshed after a settings save.
  let dismissMode = $state<"always" | "keep_on_blur" | "manual">("always");
  getConfig().then((c) => (dismissMode = c.dismiss ?? "always"));

  // Blur only dismisses in "always" mode. Settings, the run-command input and
  // the new-repository form are forms, so a click-away mustn't nuke them
  // regardless.
  $effect(() => {
    void setDismissOnBlur(
      dismissMode === "always" &&
        mode !== "settings" &&
        mode !== "run-command" &&
        mode !== "new-repo" &&
        mode !== "remove",
    );
  });

  /** Hide after a successful launch — unless the user pinned the overlay open. */
  async function dismissAfterLaunch() {
    if (dismissMode !== "manual") await hideOverlay();
  }

  // Footer key hints, per screen. `[key, description]`.
  const hints = $derived<[string, string][]>(
    mode === "repo-list" && appScope
      ? [
          ["Up/Down", "move"],
          ["Enter", "launch"],
          ["Esc", "close"],
        ]
      : mode === "repo-list"
      ? [
          ["Up/Down", "move"],
          ["Enter", "actions"],
          ["Esc", "close"],
        ]
      : mode === "settings"
        ? [["Esc", "back"]]
        : mode === "run-command"
          ? [
              ["Enter", "run"],
              ["Esc", "back"],
            ]
          : mode === "new-repo"
            ? [
                ["Tab", "next field"],
                ["Enter", setupTarget ? "set up" : "create"],
                ["Esc", "back"],
              ]
          : mode === "remove"
            ? [
                ["Enter", "move"],
                ["Esc", "back"],
              ]
          : [
              ["Up/Down", "move"],
              ["Enter", "run"],
              ["Esc", "back"],
            ],
  );

  let scanning = $state(false);
  /** A message for the footer (scanning, an error, …); "" shows the counts. */
  let status = $state("");
  /** Repos (under version control) and the other listed folders, for the
   *  footer. The two don't overlap — together they're the whole list. */
  let counts = $state<{ repos: number; folders: number } | null>(null);
  let search: SearchInput | undefined = $state();

  // The footer status line, per screen: the app count in ">" scope, both counts
  // on the settings screen, the repo counts (or a message) otherwise.
  const footerStatus = $derived(
    mode === "repo-list" && appScope
      ? appStatus
      : mode === "settings"
        ? [status, appStatus].filter(Boolean).join(" · ")
        : status,
  );
  const showCounts = $derived(
    !!counts && !status && !(mode === "repo-list" && appScope),
  );
  // The app count shows in the ">" scope and on the settings screen.
  const showAppCount = $derived(
    appsEnabled &&
      appCount !== null &&
      !appStatus &&
      ((mode === "repo-list" && appScope) || mode === "settings"),
  );

  // Nothing indexed and no scan running — the user hasn't pointed dev-prompt at
  // any folders yet. Show setup guidance and glow the settings gear.
  const noRepos = $derived(
    mode === "repo-list" &&
      !appScope &&
      !scanning &&
      query.trim() === "" &&
      results.length === 0,
  );

  let searchSeq = 0;

  async function refresh() {
    // In `>` app scope the list comes from `filteredApps`, not `results`; a repo
    // search here matches nothing and would blank `results` + reset `selected`,
    // yanking the app highlight mid-navigation. Guard here so every caller
    // (the query $effect, loadInitial, rescan, onReposUpdated) is covered.
    if (appScope) return;
    const seq = ++searchSeq;
    // The list can change underneath the user (a background rescan after the
    // watcher saw a repo come or go) — keep the highlight on the same repo.
    const keep = results[selected]?.repo.path;
    const scored = await searchRepos(query);
    if (seq !== searchSeq) return; // a newer query already answered
    results = scored;
    const same = keep ? results.findIndex((r) => r.repo.path === keep) : -1;
    if (same >= 0) selected = same;
    else if (selected >= results.length) selected = Math.max(0, results.length - 1);
  }

  // Re-run the fuzzy search whenever the query changes (repo scope only; the
  // app list filters client-side via `filteredApps`).
  $effect(() => {
    void query;
    if (appScope) return;
    refresh();
  });

  // Repositories are the entries under version control; folders are the rest
  // (plain top-level folders and marker-only projects).
  function setCounts(repos: Repo[]) {
    const versioned = repos.filter((r) => r.vcs).length;
    counts = { repos: versioned, folders: repos.length - versioned };
    status = "";
  }

  async function loadInitial() {
    const payload = await listRepos();
    if (payload.ageSecs < 0) status = "No cache — press Ctrl+R to scan";
    else setCounts(payload.repos);
    await refresh();
    if (payload.stale || payload.ageSecs < 0) void rescan();
  }

  // A rescan asked for mid-scan runs once more afterwards: the settings it
  // should see may have been saved after the running one started.
  let rescanAgain = false;
  async function rescan() {
    if (scanning) {
      rescanAgain = true;
      return;
    }
    scanning = true;
    status = "Scanning repos…";
    try {
      const payload = await rescanRepos();
      setCounts(payload.repos);
      await refresh();
    } catch (e) {
      status = `Scan failed: ${e}`;
    } finally {
      scanning = false;
      if (rescanAgain) {
        rescanAgain = false;
        void rescan();
      }
    }
  }

  async function loadApps() {
    try {
      const p = await listApps();
      apps = p.apps;
      appCount = p.apps.length;
      appStatus = "";
      if (p.stale || p.ageSecs < 0) void rescanAppsNow();
    } catch (e) {
      appStatus = `${e}`;
    }
  }

  let appsRescanAgain = false;
  async function rescanAppsNow() {
    if (appsScanning) {
      appsRescanAgain = true;
      return;
    }
    appsScanning = true;
    appStatus = "Scanning apps…";
    try {
      const p = await rescanApps();
      apps = p.apps;
      appCount = p.apps.length;
      appStatus = "";
    } catch (e) {
      appStatus = `App scan failed: ${e}`;
    } finally {
      appsScanning = false;
      if (appsRescanAgain) {
        appsRescanAgain = false;
        void rescanAppsNow();
      }
    }
  }

  async function openActions(entry: ScoredRepo) {
    activeRepo = entry;
    openedFromList = false;
    // Universal actions only — no `requires:` PATH walk, so this is fast even
    // on a cold cache. The menu opens on this alone; detected actions (npm /
    // cargo / docker / …) stream in afterward instead of blocking the open.
    actions = await buildUniversalActions(entry.repo.path);
    actionQuery = "";
    actionSel = 0;
    subGroup = null;
    mode = "action-menu";
    detectedLoading = true;
    void buildDetectedActions(entry.repo.path).then((detected) => {
      // The user may have backed out and opened a different repo already.
      if (activeRepo?.repo.path !== entry.repo.path) return;
      actions = [...actions, ...detected];
      detectedLoading = false;
    });
    // Menu is up from the scan-time cache; re-check this one repo off-thread in
    // case it changed since the last scan. `onRepoContextUpdated` rebuilds it.
    void refreshRepoContext(entry.repo.path);
  }

  // A repo the "New repository" screen just made — already in the list and
  // cache backend-side, so go straight to its action menu.
  function openCreated(repo: Repo) {
    // The user may have left the form (mouse back, hidden overlay) while it ran.
    if (mode !== "new-repo") return;
    if (!setupTarget) query = "";
    setupTarget = null;
    void openActions({ repo, score: 0, matchIndices: [] });
  }

  // Back from the remove screen: to the list or action menu it came from.
  function removeBack() {
    if (openedFromList) backToList();
    else {
      removeTarget = null;
      mode = "action-menu";
    }
  }

  // Back from the new-repo screen: to the action menu it was opened from, or
  // to the list.
  function newRepoBack() {
    if (setupTarget && !openedFromList) {
      setupTarget = null;
      mode = "action-menu";
    } else {
      backToList();
    }
  }

  function backToList() {
    mode = "repo-list";
    setupTarget = null;
    removeTarget = null;
    openedFromList = false;
    activeRepo = null;
    actions = [];
    actionQuery = "";
    subGroup = null;
    promptTemplate = "";
  }

  // Keep the search box focused whenever the repo list is showing, so typing
  // works. Keyboard *navigation* no longer depends on this — it's handled at the
  // window level below — but focus is still needed to type into the input.
  $effect(() => {
    if (mode === "repo-list") {
      void search; // re-run when the input is (re)mounted after leaving the menu
      // Don't re-select an existing query (e.g. the ">" prefix on an app-scope
      // open) — just ensure focus.
      tick().then(() => search?.focus(false));
    }
  });

  let running = false;

  async function execute(action: Action, repoPath: string) {
    if (running) return; // guard against a double-click spawning twice
    running = true;
    try {
      if (action.clientSide) {
        if (action.id === "copy-path") await copyPath(repoPath);
      } else {
        await runAction(action.id, repoPath);
      }
      await dismissAfterLaunch();
      // Pinned open: reset the menu so the next action in this repo is one keypress.
      if (dismissMode === "manual" && mode === "action-menu") {
        actionQuery = "";
        actionSel = 0;
      }
    } catch (e) {
      status = `Launch failed: ${e}`;
    } finally {
      running = false;
    }
  }

  async function runCommandAndHide(path: string, cmd: string, shell: string) {
    if (running) return;
    running = true;
    try {
      await runCommand(path, cmd, shell);
      if (dismissMode === "manual") backToList();
      else await hideOverlay();
    } catch (e) {
      status = `Run failed: ${e}`;
    } finally {
      running = false;
    }
  }

  async function runAppAndHide(app: AppEntry) {
    if (running) return;
    running = true;
    try {
      await runApp(app);
      await dismissAfterLaunch();
    } catch (e) {
      appStatus = `Launch failed: ${e}`;
    } finally {
      running = false;
    }
  }

  function onListKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, activeCount - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (appScope) {
        const hit = filteredApps[selected];
        if (hit) runAppAndHide(hit.app);
      } else if (results[selected]) {
        openActions(results[selected]);
      }
    } else if (e.key === "Delete") {
      e.preventDefault();
      query = "";
    } else if (e.key === "Escape") {
      e.preventDefault();
      hideOverlay();
    } else if (undoToast && e.key.toLowerCase() === "z" && e.ctrlKey && !e.shiftKey) {
      e.preventDefault();
      void undoRemove();
    } else if (e.key.toLowerCase() === "r" && e.ctrlKey) {
      e.preventDefault();
      if (appScope) rescanAppsNow();
      else rescan();
    } else if (
      !appScope &&
      e.key.toLowerCase() === "n" &&
      e.ctrlKey &&
      e.shiftKey &&
      !e.altKey
    ) {
      // Same keys as Explorer's "New folder". In-app only — never a global hotkey.
      e.preventDefault();
      mode = "new-repo";
    } else if (
      (e.key === "," || e.code === "Comma") &&
      (e.ctrlKey || e.metaKey)
    ) {
      e.preventDefault();
      openSettings();
    }
  }

  function onMenuKeydown(e: KeyboardEvent) {
    if (e.ctrlKey && !e.altKey && !e.shiftKey) {
      const hit = quickActions.find(
        (q) => q.key.toLowerCase() === `ctrl+${e.key.toLowerCase()}`,
      );
      if (hit) {
        e.preventDefault();
        runQuickAction(hit.action);
        return;
      }
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      actionSel = Math.min(actionSel + 1, menuItems.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      actionSel = Math.max(actionSel - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      activateMenuItem(actionSel);
    } else if (e.key === "Tab") {
      // Tab goes "forward" — into a sub-project, if one is selected.
      e.preventDefault();
      if (menuItems[actionSel]?.kind === "submenu") activateMenuItem(actionSel);
    } else if (e.key === "Delete") {
      e.preventDefault();
      actionQuery = "";
      actionSel = 0;
    } else if (e.key === "Escape") {
      // Esc steps back one level: sub-project → menu → repo list.
      e.preventDefault();
      menuBack();
    }
  }

  // --- right-click menus ---------------------------------------------------

  /** The open right-click menu, if any. */
  let ctx = $state<{ x: number; y: number; items: CtxItem[] } | null>(null);
  let ctxMenu: ContextMenu | undefined = $state();
  /** Bumped per menu, so a slow fetch can't fill in a menu that's been replaced. */
  let ctxSeq = 0;

  function closeCtx() {
    ctx = null;
  }

  // A screen change takes any open menu with it.
  $effect(() => {
    void mode;
    ctx = null;
  });

  /** Dev builds keep the webview's own menu (and its Inspect) on Shift+right-click. */
  function wantsNative(e: MouseEvent): boolean {
    return import.meta.env.DEV && e.shiftKey;
  }

  /** Open `items` at the pointer; `more` (if given) replaces them once it
   *  resolves — for menus whose contents have to be fetched. */
  function showCtx(e: MouseEvent, items: CtxItem[], more?: Promise<CtxItem[]>) {
    e.preventDefault();
    const seq = ++ctxSeq;
    ctx = { x: e.clientX, y: e.clientY, items };
    void more?.then((next) => {
      if (ctx && seq === ctxSeq) ctx = { ...ctx, items: next };
    });
  }

  // Anything without a menu of its own: text fields get Cut / Copy / Paste,
  // everything else gets nothing — never the browser's Back / Reload / Print.
  function onWindowContextMenu(e: MouseEvent) {
    if (e.defaultPrevented || wantsNative(e)) return; // a row opened its own
    const field = (e.target as HTMLElement | null)?.closest<
      HTMLInputElement | HTMLTextAreaElement
    >("textarea, input:not([type='checkbox']):not([type='radio'])");
    if (field) openTextMenu(e, field);
    else {
      e.preventDefault();
      closeCtx();
    }
  }

  function openTextMenu(e: MouseEvent, el: HTMLInputElement | HTMLTextAreaElement) {
    el.focus();
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? start;
    const picked = el.value.slice(start, end);
    const editable = !el.readOnly && !el.disabled;
    // Replace the selection the way typing would, so `bind:value` sees it.
    const insert = (text: string) => {
      el.focus();
      el.setRangeText(text, start, end, "end");
      el.dispatchEvent(new Event("input", { bubbles: true }));
    };
    const items = (clip: string): CtxItem[] => [
      {
        kind: "item",
        label: "Cut",
        disabled: !picked || !editable,
        run: () => {
          void copyText(picked);
          insert("");
        },
      },
      { kind: "item", label: "Copy", icon: "copy", disabled: !picked, run: () => void copyText(picked) },
      {
        kind: "item",
        label: "Paste",
        disabled: !clip || !editable,
        // A single-line field takes one line, as a native paste does.
        run: () => insert(el instanceof HTMLInputElement ? clip.replace(/[\r\n]+/g, " ") : clip),
      },
      { kind: "separator" },
      {
        kind: "item",
        label: "Select all",
        disabled: !el.value,
        run: () => {
          el.focus();
          el.select();
        },
      },
    ];
    // Paste stays greyed out until the clipboard has been read.
    showCtx(e, items(""), readClipboard().then(items));
  }

  /** Run one of a repo's actions straight from its row in the list. */
  function runFromList(entry: ScoredRepo, a: Action) {
    if (a.remove) {
      openRemove(entry, true);
    } else if (a.provision) {
      activeRepo = entry;
      setupTarget = entry;
      openedFromList = true;
      mode = "new-repo";
    } else if (a.prompt) {
      activeRepo = entry;
      promptTemplate = a.hint ?? "";
      openedFromList = true;
      mode = "run-command";
    } else {
      void execute(a, entry.repo.path);
    }
  }

  // A repo row: the action page's most useful parts, a level up — open its
  // actions, set up version control when it has none, the quick actions, and
  // whatever this repo's history says gets used most.
  function openRepoMenu(e: MouseEvent, entry: ScoredRepo) {
    if (wantsNative(e)) return;
    const head: CtxItem[] = [
      { kind: "header", label: entry.repo.name },
      { kind: "item", label: "Open actions", icon: "run", run: () => void openActions(entry) },
    ];
    const item = (a: Action): Extract<CtxItem, { kind: "item" }> => ({
      kind: "item",
      label: a.label,
      icon: a.icon,
      run: () => runFromList(entry, a),
    });
    const more = buildUniversalActions(entry.repo.path).then((acts) => {
      const out = [...head];
      const setup = acts.find((a) => a.provision);
      if (setup) out.push({ kind: "separator" }, item(setup));
      const quick = acts.filter((a) => a.hotkey);
      if (quick.length) out.push({ kind: "separator" }, ...quick.map(item));
      const used = acts
        .filter((a) => !a.hotkey && !a.provision && !a.remove && a.usageScore > 0)
        .sort((a, b) => b.usageScore - a.usageScore)
        .slice(0, 4);
      if (used.length) {
        out.push({ kind: "separator" }, { kind: "header", label: "Used most here" }, ...used.map(item));
      }
      const rm = acts.find((a) => a.remove);
      if (rm) out.push({ kind: "separator" }, { ...item(rm), danger: true });
      return out;
    });
    showCtx(e, [...head, { kind: "loading", label: "Loading…" }], more);
  }

  // An app row: launch it, or get at the program behind it.
  function openAppMenu(e: MouseEvent, app: AppEntry) {
    if (wantsNative(e)) return;
    const items: CtxItem[] = [
      { kind: "header", label: app.name },
      { kind: "item", label: "Launch", icon: "run", run: () => void runAppAndHide(app) },
    ];
    // Only a real executable has a folder and a path worth copying.
    if (app.kind === "exe") {
      items.push(
        { kind: "separator" },
        {
          kind: "item",
          label: "Show in file manager",
          icon: "folder",
          run: () =>
            void showFolder(app.exec)
              .then(dismissAfterLaunch)
              .catch((err) => (appStatus = `Couldn't open the folder: ${err}`)),
        },
        {
          kind: "item",
          label: "Copy path",
          icon: "copy",
          run: () => void copyText(app.exec).then(dismissAfterLaunch),
        },
      );
    }
    showCtx(e, items);
  }

  // An action-menu row: run it, or copy the command it would run.
  function openActionRowMenu(e: MouseEvent, i: number) {
    if (wantsNative(e)) return;
    const it = menuItems[i];
    if (!it || it.kind === "loading") {
      e.preventDefault();
      return;
    }
    actionSel = i;
    if (it.kind === "submenu") {
      showCtx(e, [
        { kind: "header", label: it.label },
        { kind: "item", label: "Open", icon: "run", run: () => activateMenuItem(i) },
      ]);
      return;
    }
    const a = it.action;
    const items: CtxItem[] = [
      {
        kind: "item",
        label: a.prompt || a.provision || a.remove ? "Open…" : "Run",
        icon: "run",
        run: () => activateMenuItem(i),
      },
    ];
    // `hint` is the resolved command line, except for the form-opening and
    // frontend-only actions, and a bare terminal (whose hint is just a label).
    if (a.hint && !a.clientSide && !a.prompt && !a.provision && !a.remove && a.hint !== "terminal") {
      items.push({
        kind: "item",
        label: "Copy command",
        icon: "copy",
        run: () => void copyText(a.hint),
      });
    }
    showCtx(e, items);
  }

  // All keyboard handling runs at the window level and is dispatched by mode, so
  // navigation keeps working even when no element is focused (e.g. right after
  // returning from the action menu). Character keys fall through to the focused
  // search input untouched.
  function onWindowKeydown(e: KeyboardEvent) {
    // An open menu takes every key until it closes.
    if (ctx) {
      ctxMenu?.keydown(e);
      return;
    }
    if (mode === "action-menu") {
      onMenuKeydown(e);
    } else if (mode === "run-command" || mode === "new-repo" || mode === "remove") {
      // These components own their keys (Enter / Esc).
    } else if (mode === "settings") {
      if (e.key === "Escape") {
        e.preventDefault();
        backToList();
      }
    } else {
      onListKeydown(e);
    }
  }

  // Mouse back (button 3) / forward (button 4) navigate the screen stack like a
  // browser: back == Escape for the current mode, forward == Tab (go deeper).
  function onWindowMouse(e: MouseEvent) {
    if (e.button !== 3 && e.button !== 4) return;
    e.preventDefault();
    if (ctx) {
      closeCtx(); // back/forward first just dismisses an open menu
      return;
    }
    const back = e.button === 3;
    if (mode === "settings") {
      if (back) backToList();
    } else if (mode === "new-repo") {
      if (back) newRepoBack();
    } else if (mode === "remove") {
      if (back) removeBack();
    } else if (mode === "action-menu") {
      if (back) menuBack();
      else if (menuItems[actionSel]?.kind === "submenu")
        activateMenuItem(actionSel);
    } else if (appScope) {
      if (back) hideOverlay();
      else if (filteredApps[selected]) runAppAndHide(filteredApps[selected].app);
    } else {
      if (back) hideOverlay();
      else if (results[selected]) openActions(results[selected]);
    }
  }

  onMount(() => {
    const unlisteners: Array<Promise<() => void>> = [];
    unlisteners.push(
      onOverlayShown((scope) => {
        // The app-launcher hotkey opens straight into the ">" scope, caret
        // after the prefix so the first keystroke filters rather than replaces.
        query = scope === "apps" ? ">" : "";
        selected = 0;
        backToList();
        void tick().then(() => search?.focus(scope !== "apps"));
        void loadInitial();
        if (scope === "apps") void loadApps();
      }),
    );
    unlisteners.push(
      onOverlayHidden(() => {
        // Reset to the repo list while the window is off-screen. WebView2 keeps
        // running scripts when hidden, so by the next show the home view is
        // already rendered — no flicker from the previous screen on slow
        // machines. The user is fine seeing this snap happen during the hide.
        query = "";
        selected = 0;
        actionQuery = "";
        actionSel = 0;
        closeCtx();
        backToList();
      }),
    );
    unlisteners.push(
      onReposUpdated(() => {
        void refresh();
        // Background rescans (the folder watcher) don't go through `rescan()`,
        // so keep the count in the footer honest here.
        if (!scanning) {
          void listRepos().then((p) => setCounts(p.repos));
        }
      }),
    );
    unlisteners.push(
      onAppsUpdated(
        () =>
          void listApps().then((p) => {
            apps = p.apps;
            appCount = p.apps.length;
          }),
      ),
    );
    unlisteners.push(
      onRepoContextUpdated((path) => {
        // A background re-inspect found this repo stale — rebuild the menu in
        // place if it's the one on screen.
        if (
          mode === "action-menu" &&
          activeRepo?.repo.path === path
        ) {
          void buildActions(path).then((a) => {
            actions = a;
            detectedLoading = false;
          });
        }
      }),
    );
    unlisteners.push(onGotoSettings(() => openSettings()));

    search?.focus();
    void loadInitial();
    void loadApps();

    // Update check: once now, then daily while the app runs.
    void pollUpdates();
    const updTimer = setInterval(() => void pollUpdates(), DAY_MS);

    return () => {
      clearInterval(updTimer);
      for (const u of unlisteners) u.then((fn) => fn());
    };
  });
</script>

{#snippet glyph(key: string)}
  <svg
    class="h-3 w-3 shrink-0 text-white/50"
    viewBox="0 0 24 24"
    fill="currentColor"
    fill-rule="evenodd"
    aria-hidden="true"
  >
    {#if icons[key].raw}{@html icons[key].raw}{:else}<path d={icons[key].d} />{/if}
  </svg>
{/snippet}

<svelte:window
  onkeydown={onWindowKeydown}
  oncontextmenu={onWindowContextMenu}
  onmousedown={onWindowMouse}
  onmouseup={(e) => {
    if (e.button === 3 || e.button === 4) e.preventDefault();
  }}
/>

<main
  class="panel-surface relative mx-auto flex h-[480px] w-[760px] flex-col overflow-hidden
         rounded border border-hair bg-panel/[0.82] backdrop-blur-xl"
>
  {#if mode === "repo-list"}
    <SearchInput
      bind:this={search}
      bind:value={query}
      placeholder={appScope
        ? "Search apps…"
        : appsEnabled
          ? "Search repos…    › for apps"
          : "Search repos…"}
    />
    {#if appScope}
      <AppList
        entries={filteredApps}
        {selected}
        scanning={appsScanning}
        onselect={(i) => (selected = i)}
        onactivate={(i) => {
          selected = i;
          if (filteredApps[i]) runAppAndHide(filteredApps[i].app);
        }}
        oncontext={(i, e) => {
          selected = i;
          if (filteredApps[i]) openAppMenu(e, filteredApps[i].app);
        }}
      />
    {:else if noRepos}
      <div
        class="flex flex-1 flex-col items-center justify-center gap-2.5 px-10 text-center"
      >
        <p class="text-[15px] font-medium text-white/80">No project folders yet</p>
        <p class="text-[13px] leading-relaxed text-white/40">
          Open <span class="text-orange-400">Settings</span> — the gear at the
          bottom-left — and add the directories your repositories live in.
        </p>
      </div>
    {:else}
      <ResultList
        entries={results}
        {selected}
        onselect={(i) => (selected = i)}
        onactivate={(i) => {
          selected = i;
          if (results[i]) openActions(results[i]);
        }}
        oncontext={(i, e) => {
          selected = i;
          if (results[i]) openRepoMenu(e, results[i]);
        }}
      />
    {/if}
  {:else if mode === "action-menu" && activeRepo}
    <ActionMenu
      repoName={activeRepo.repo.name}
      crumb={subGroup ? subGroup.slice(SUB_PREFIX.length) : null}
      items={menuItems}
      {quickActions}
      bind:filter={actionQuery}
      selected={actionSel}
      onselect={(i) => (actionSel = i)}
      onrun={(i) => activateMenuItem(i)}
      onquickaction={runQuickAction}
      onback={menuBack}
      oncontext={(i, e) => openActionRowMenu(e, i)}
    />
  {:else if mode === "run-command" && activeRepo}
    <RunCommand
      repoName={activeRepo.repo.name}
      repoPath={activeRepo.repo.path}
      template={promptTemplate}
      onrun={(cmd, sh) =>
        activeRepo && runCommandAndHide(activeRepo.repo.path, cmd, sh)}
      onback={() => (openedFromList ? backToList() : (mode = "action-menu"))}
    />
  {:else if mode === "new-repo"}
    <NewRepo
      existing={setupTarget
        ? { path: setupTarget.repo.path, name: setupTarget.repo.name }
        : null}
      oncreated={openCreated}
      onback={newRepoBack}
    />
  {:else if mode === "remove" && removeTarget}
    <RemoveFolder
      name={removeTarget.repo.name}
      path={removeTarget.repo.path}
      ondone={(undoable) => removeTarget && removed(removeTarget.repo.name, undoable)}
      onback={removeBack}
    />
  {:else if mode === "settings"}
    <Settings
      initialPage={settingsPage}
      onback={backToList}
      onsaved={() => {
        void getConfig().then((c) => {
          dismissMode = c.dismiss ?? "always";
          appsEnabled = c.apps?.enabled ?? true;
        });
      }}
      onrescanrepos={rescan}
      onrescanapps={rescanAppsNow}
    />
  {/if}

  {#if undoToast && mode === "repo-list"}
    <div
      class="flex items-center gap-3 border-t border-hair bg-white/[0.04] px-4 py-2 text-[12px] text-white/70"
    >
      <span class="min-w-0 flex-1 truncate">
        Moved <span class="text-white/90">{undoToast.name}</span> to the {bin}.
      </span>
      <button
        type="button"
        onclick={() => void undoRemove()}
        class="inline-flex shrink-0 items-center gap-1.5 rounded border border-sky-400/50 bg-sky-400/10
               px-2 py-0.5 text-[12px] text-sky-200 hover:bg-sky-400/20"
      >
        Undo <kbd>Ctrl+Z</kbd>
      </button>
    </div>
  {/if}

  <footer
    class="flex h-9 items-center justify-between border-t border-hair px-4
           text-[11px] text-white/30"
  >
    <span class="flex min-w-0 items-center gap-3">
      {#if mode !== "settings"}
        <button
          type="button"
          onclick={() => openSettings(noRepos ? "repos" : null)}
          title="Settings (Ctrl+,)"
          aria-label="Settings"
          class:settings-glow={noRepos}
          class="shrink-0 rounded-[3px] border p-1 text-white/80 transition-colors
                 {noRepos
            ? 'border-sky-400/60 bg-sky-400/10 text-white'
            : 'border-white/15 bg-white/[0.09] hover:bg-white/20 hover:text-white'}"
        >
          <svg
            class="h-3 w-3"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <circle cx="12" cy="12" r="3" />
            <path
              d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
            />
          </svg>
        </button>
      {/if}
      {#if mode === "settings" && upd.current}
        <span class="shrink-0 tabular-nums text-sky-400">v{upd.current}</span>
      {/if}
      {#if showCounts && counts}
        <span class="inline-flex shrink-0 items-center gap-2.5 tabular-nums">
          <span
            class="inline-flex items-center gap-1"
            title="Repositories — folders under version control"
          >
            {@render glyph("branch")}{counts.repos}
          </span>
          <span
            class="inline-flex items-center gap-1"
            title="Folders without version control"
          >
            {@render glyph("folder")}{counts.folders}
          </span>
        </span>
      {/if}
      {#if showAppCount}
        <span
          class="inline-flex shrink-0 items-center gap-1 tabular-nums"
          title="Installed apps"
        >
          {@render glyph("app")}{appCount}
        </span>
      {/if}
      <span class="truncate">{footerStatus}</span>
      {#if mode === "repo-list"}
        <span class="inline-flex shrink-0 items-center gap-1"
          ><kbd>Ctrl+R</kbd>rescan</span
        >
        {#if !appScope}
          <span class="inline-flex shrink-0 items-center gap-1"
            ><kbd>Ctrl+Shift+N</kbd>new repo</span
          >
        {/if}
      {/if}
    </span>
    <span class="flex shrink-0 items-center gap-3.5">
      {#each hints as [key, label] (label)}
        <span class="inline-flex items-center gap-1"><kbd>{key}</kbd>{label}</span>
      {/each}
      {#if upd.info && mode !== "settings"}
        <button
          type="button"
          onclick={() => openSettings("about")}
          title={`Update ${upd.info.version} available — open Settings`}
          class="inline-flex shrink-0 items-center rounded-[3px] border border-sky-400/50
                 bg-sky-400/10 px-1.5 py-0.5 text-[10px] font-medium leading-none text-sky-200
                 hover:bg-sky-400/20"
        >
          ↑ {upd.info.version}
        </button>
      {/if}
    </span>
  </footer>
</main>

{#if ctx}
  <ContextMenu
    bind:this={ctxMenu}
    x={ctx.x}
    y={ctx.y}
    items={ctx.items}
    onclose={closeCtx}
  />
{/if}

<style>
  /* Pulsing ring on the settings gear while no folders are configured. */
  @keyframes settings-glow {
    0%,
    100% {
      box-shadow: 0 0 0 0 rgb(56 189 248 / 0);
    }
    50% {
      box-shadow: 0 0 0 4px rgb(56 189 248 / 0.35);
    }
  }
  .settings-glow {
    animation: settings-glow 1.8s ease-in-out infinite;
  }
  @media (prefers-reduced-motion: reduce) {
    .settings-glow {
      animation: none;
    }
  }

  /* Key hints: bright key-cap, dim descriptive label (inherited from footer). */
  kbd {
    font-family: inherit;
    font-size: 10px;
    line-height: 1;
    color: rgb(255 255 255 / 0.9);
    background: rgb(255 255 255 / 0.09);
    border: 1px solid rgb(255 255 255 / 0.12);
    border-radius: 3px;
    padding: 2px 4px;
  }
</style>
