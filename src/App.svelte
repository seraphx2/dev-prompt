<script lang="ts">
  import { onMount, tick } from "svelte";
  import SearchInput from "./lib/components/SearchInput.svelte";
  import ResultList from "./lib/components/ResultList.svelte";
  import AppList from "./lib/components/AppList.svelte";
  import ActionMenu from "./lib/components/ActionMenu.svelte";
  import RunCommand from "./lib/components/RunCommand.svelte";
  import Settings from "./lib/components/Settings.svelte";
  import {
    buildActions,
    copyPath,
    hideOverlay,
    listApps,
    listRepos,
    onAppsUpdated,
    onGotoSettings,
    onOverlayHidden,
    onOverlayShown,
    onReposUpdated,
    onRepoContextUpdated,
    refreshRepoContext,
    rescanApps,
    rescanRepos,
    runAction,
    runApp,
    runCommand,
    searchRepos,
    setDismissOnBlur,
  } from "./lib/ipc";
  import type { Action, AppEntry, MenuItem, ScoredRepo } from "./lib/types";
  import { fuzzyScore } from "./lib/fuzzy";
  import { upd, pollUpdates } from "./lib/updateStore.svelte";

  const DAY_MS = 24 * 60 * 60 * 1000;

  type Mode = "repo-list" | "action-menu" | "settings" | "run-command";

  let query = $state("");
  let results = $state<ScoredRepo[]>([]);
  let selected = $state(0);

  // App-launcher scope: a leading ">" in the query switches the repo list for a
  // search over installed apps. Repos stay the default on every overlay open.
  let apps = $state<AppEntry[]>([]);
  let appStatus = $state("");
  let appsScanning = $state(false);
  const appScope = $derived(query.startsWith(">"));
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
  /** When set, the action menu is drilled into this `Detected · <x>` group. */
  let subGroup = $state<string | null>(null);
  /** `run:` template of the prompt action that opened the "Run command…" mode. */
  let promptTemplate = $state("");

  const SUB_PREFIX = "Detected · ";

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
    if (q) return fuzzyItems(actions, q);

    const items: MenuItem[] = [];
    const seen = new Set<string>();
    for (const a of actions) {
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
    if (!it) return;
    if (it.kind === "submenu") {
      subGroup = it.target;
      actionQuery = "";
      actionSel = 0;
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

  // Settings + the run-command input are forms — a click-away mustn't nuke them.
  $effect(() => {
    void setDismissOnBlur(mode !== "settings" && mode !== "run-command");
  });

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
          ["Enter", "launch"],
          ["Tab", "actions"],
          ["Esc", "close"],
        ]
      : mode === "settings"
        ? [["Esc", "back"]]
        : mode === "run-command"
          ? [
              ["Enter", "run"],
              ["Esc", "back"],
            ]
          : [
              ["Up/Down", "move"],
              ["Enter", "run"],
              ["Esc", "back"],
            ],
  );

  let scanning = $state(false);
  let status = $state("");
  let search: SearchInput | undefined = $state();

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
    const seq = ++searchSeq;
    const scored = await searchRepos(query);
    if (seq !== searchSeq) return; // a newer query already answered
    results = scored;
    if (selected >= results.length) selected = Math.max(0, results.length - 1);
  }

  // Re-run the fuzzy search whenever the query changes (repo scope only; the
  // app list filters client-side via `filteredApps`).
  $effect(() => {
    void query;
    if (appScope) return;
    refresh();
  });

  async function loadInitial() {
    const payload = await listRepos();
    status = payload.ageSecs < 0
      ? "No cache — press Ctrl+R to scan"
      : `${payload.repos.length} repos · cache ${fmtAge(payload.ageSecs)} old`;
    await refresh();
    if (payload.stale || payload.ageSecs < 0) void rescan();
  }

  async function rescan() {
    if (scanning) return;
    scanning = true;
    status = "Scanning…";
    try {
      const payload = await rescanRepos();
      status = `${payload.repos.length} repos · just scanned`;
      await refresh();
    } catch (e) {
      status = `Scan failed: ${e}`;
    } finally {
      scanning = false;
    }
  }

  function fmtAge(secs: number): string {
    if (secs < 90) return `${secs}s`;
    if (secs < 5400) return `${Math.round(secs / 60)}m`;
    return `${Math.round(secs / 3600)}h`;
  }

  async function loadApps() {
    try {
      const p = await listApps();
      apps = p.apps;
      appStatus = `${p.apps.length} apps`;
      if (p.stale || p.ageSecs < 0) void rescanAppsNow();
    } catch (e) {
      appStatus = `${e}`;
    }
  }

  async function rescanAppsNow() {
    if (appsScanning) return;
    appsScanning = true;
    appStatus = "Scanning for apps…";
    try {
      const p = await rescanApps();
      apps = p.apps;
      appStatus = `${p.apps.length} apps · just scanned`;
    } catch (e) {
      appStatus = `App scan failed: ${e}`;
    } finally {
      appsScanning = false;
    }
  }

  async function openActions(entry: ScoredRepo) {
    activeRepo = entry;
    actions = await buildActions(entry.repo.path);
    actionQuery = "";
    actionSel = 0;
    subGroup = null;
    mode = "action-menu";
    // Menu is up from the scan-time cache; re-check this one repo off-thread in
    // case it changed since the last scan. `onRepoContextUpdated` rebuilds it.
    void refreshRepoContext(entry.repo.path);
  }

  function backToList() {
    mode = "repo-list";
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
      await hideOverlay();
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
      await hideOverlay();
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
      await hideOverlay();
    } catch (e) {
      appStatus = `Launch failed: ${e}`;
    } finally {
      running = false;
    }
  }

  /** Enter on a repo runs its default action (the terminal), else the first. */
  async function activateRepo(i: number) {
    const entry = results[i];
    if (!entry) return;
    const acts = await buildActions(entry.repo.path);
    const def = acts.find((a) => a.default) ?? acts[0];
    if (def) await execute(def, entry.repo.path);
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
      } else if (e.shiftKey || e.ctrlKey) {
        if (results[selected]) openActions(results[selected]);
      } else {
        activateRepo(selected);
      }
    } else if (e.key === "Tab") {
      // Tab goes "forward" — into the selected repo's actions (repo scope only).
      e.preventDefault();
      if (!appScope && results[selected]) openActions(results[selected]);
    } else if (e.key === "Delete") {
      e.preventDefault();
      query = "";
    } else if (e.key === "Escape") {
      e.preventDefault();
      hideOverlay();
    } else if (e.key.toLowerCase() === "r" && e.ctrlKey) {
      e.preventDefault();
      if (appScope) rescanAppsNow();
      else rescan();
    } else if (
      (e.key === "," || e.code === "Comma") &&
      (e.ctrlKey || e.metaKey)
    ) {
      e.preventDefault();
      mode = "settings";
    }
  }

  function onMenuKeydown(e: KeyboardEvent) {
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

  // All keyboard handling runs at the window level and is dispatched by mode, so
  // navigation keeps working even when no element is focused (e.g. right after
  // returning from the action menu). Character keys fall through to the focused
  // search input untouched.
  function onWindowKeydown(e: KeyboardEvent) {
    if (mode === "action-menu") {
      onMenuKeydown(e);
    } else if (mode === "run-command") {
      // The RunCommand component owns its keys (Enter / Esc).
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
    const back = e.button === 3;
    if (mode === "settings") {
      if (back) backToList();
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
        backToList();
      }),
    );
    unlisteners.push(onReposUpdated(() => void refresh()));
    unlisteners.push(
      onAppsUpdated(() => void listApps().then((p) => (apps = p.apps))),
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
          });
        }
      }),
    );
    unlisteners.push(onGotoSettings(() => (mode = "settings")));

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

<svelte:window
  onkeydown={onWindowKeydown}
  onmousedown={onWindowMouse}
  onmouseup={(e) => {
    if (e.button === 3 || e.button === 4) e.preventDefault();
  }}
/>

<main
  class="relative mx-auto flex h-[480px] w-[720px] flex-col overflow-hidden rounded
         border border-hair bg-panel/[0.82] backdrop-blur-xl"
>
  {#if mode === "repo-list"}
    <SearchInput
      bind:this={search}
      bind:value={query}
      placeholder={appScope ? "Search apps…" : "Search repos…    › for apps"}
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
      />
    {/if}
  {:else if mode === "action-menu" && activeRepo}
    <ActionMenu
      repoName={activeRepo.repo.name}
      crumb={subGroup ? subGroup.slice(SUB_PREFIX.length) : null}
      items={menuItems}
      bind:filter={actionQuery}
      selected={actionSel}
      onselect={(i) => (actionSel = i)}
      onrun={(i) => activateMenuItem(i)}
      onback={menuBack}
    />
  {:else if mode === "run-command" && activeRepo}
    <RunCommand
      repoName={activeRepo.repo.name}
      repoPath={activeRepo.repo.path}
      template={promptTemplate}
      onrun={(cmd, sh) =>
        activeRepo && runCommandAndHide(activeRepo.repo.path, cmd, sh)}
      onback={() => (mode = "action-menu")}
    />
  {:else if mode === "settings"}
    <Settings onback={backToList} onsaved={() => rescan()} />
  {/if}

  <footer
    class="flex h-9 items-center justify-between border-t border-hair px-4
           text-[11px] text-white/30"
  >
    <span class="flex min-w-0 items-center gap-3">
      {#if mode !== "settings"}
        <button
          type="button"
          onclick={() => (mode = "settings")}
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
      <span class="truncate"
        >{mode === "repo-list" && appScope ? appStatus : status}</span
      >
      {#if mode === "repo-list"}
        <span class="inline-flex shrink-0 items-center gap-1"
          ><kbd>Ctrl+R</kbd>rescan</span
        >
      {/if}
      {#if upd.info && mode !== "settings"}
        <button
          type="button"
          onclick={() => (mode = "settings")}
          title={`Update ${upd.info.version} available`}
          class="inline-flex shrink-0 items-center rounded-[3px] border border-sky-400/50
                 bg-sky-400/10 px-1 py-0.5 text-[10px] font-medium leading-none text-sky-200
                 hover:bg-sky-400/20"
        >
          ↑ {upd.info.version}
        </button>
      {/if}
    </span>
    <span class="flex shrink-0 items-center gap-3.5">
      {#each hints as [key, label] (label)}
        <span class="inline-flex items-center gap-1"><kbd>{key}</kbd>{label}</span>
      {/each}
    </span>
  </footer>
</main>

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
