<script lang="ts">
  import { onMount, tick } from "svelte";
  import SearchInput from "./lib/components/SearchInput.svelte";
  import ResultList from "./lib/components/ResultList.svelte";
  import ActionMenu from "./lib/components/ActionMenu.svelte";
  import Settings from "./lib/components/Settings.svelte";
  import {
    buildActions,
    copyPath,
    hideOverlay,
    listRepos,
    onOverlayShown,
    onReposUpdated,
    rescanRepos,
    runAction,
    searchRepos,
    setDismissOnBlur,
  } from "./lib/ipc";
  import type { Action, ScoredRepo } from "./lib/types";
  import { fuzzyScore } from "./lib/fuzzy";

  type Mode = "repo-list" | "action-menu" | "settings";

  let query = $state("");
  let results = $state<ScoredRepo[]>([]);
  let selected = $state(0);

  let mode = $state<Mode>("repo-list");
  let actions = $state<Action[]>([]);
  let actionQuery = $state("");
  let actionSel = $state(0);
  let activeRepo = $state<ScoredRepo | null>(null);

  type FilteredAction = { action: Action; positions: number[] };

  // Fuzzy-filter the action list, keeping the original group order. A match on
  // the label carries highlight positions; a match only on the group/hint still
  // includes the row but without highlights (mirrors the repo list's name-vs-path
  // behaviour).
  const filteredActions = $derived.by<FilteredAction[]>(() => {
    const q = actionQuery.trim();
    if (!q) return actions.map((action) => ({ action, positions: [] }));
    const out: FilteredAction[] = [];
    for (const action of actions) {
      const onLabel = fuzzyScore(q, action.label);
      if (onLabel) {
        out.push({ action, positions: onLabel.positions });
      } else if (fuzzyScore(q, `${action.group} ${action.hint}`)) {
        out.push({ action, positions: [] });
      }
    }
    return out;
  });

  // Keep the action selection within the (possibly filtered) list.
  $effect(() => {
    if (actionSel >= filteredActions.length) {
      actionSel = Math.max(0, filteredActions.length - 1);
    }
  });

  // The settings screen is a form — don't let a click-away dismiss it.
  $effect(() => {
    void setDismissOnBlur(mode !== "settings");
  });

  // Footer key hints, per screen. `[key, description]`.
  const hints = $derived<[string, string][]>(
    mode === "repo-list"
      ? [
          ["Up/Down", "move"],
          ["Enter", "launch"],
          ["Tab", "actions"],
          ["Esc", "close"],
        ]
      : mode === "settings"
        ? [["Esc", "back"]]
        : [
            ["Up/Down", "move"],
            ["Enter", "run"],
            ["Esc", "back"],
          ],
  );

  let scanning = $state(false);
  let status = $state("");
  let search: SearchInput | undefined = $state();

  let searchSeq = 0;

  async function refresh() {
    const seq = ++searchSeq;
    const scored = await searchRepos(query);
    if (seq !== searchSeq) return; // a newer query already answered
    results = scored;
    if (selected >= results.length) selected = Math.max(0, results.length - 1);
  }

  // Re-run the fuzzy search whenever the query changes.
  $effect(() => {
    void query;
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

  async function openActions(entry: ScoredRepo) {
    activeRepo = entry;
    actions = await buildActions(entry.repo.path);
    actionQuery = "";
    actionSel = 0;
    mode = "action-menu";
  }

  function backToList() {
    mode = "repo-list";
    activeRepo = null;
    actions = [];
    actionQuery = "";
  }

  // Keep the search box focused whenever the repo list is showing, so typing
  // works. Keyboard *navigation* no longer depends on this — it's handled at the
  // window level below — but focus is still needed to type into the input.
  $effect(() => {
    if (mode === "repo-list") {
      void search; // re-run when the input is (re)mounted after leaving the menu
      tick().then(() => search?.focus());
    }
  });

  async function execute(action: Action, repoPath: string) {
    if (action.clientSide) {
      if (action.id === "copy-path") await copyPath(repoPath);
      await hideOverlay();
      return;
    }
    try {
      await runAction(action.id, repoPath);
      await hideOverlay();
    } catch (e) {
      status = `Launch failed: ${e}`;
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
      selected = Math.min(selected + 1, results.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (e.shiftKey || e.ctrlKey) {
        if (results[selected]) openActions(results[selected]);
      } else {
        activateRepo(selected);
      }
    } else if (e.key === "Tab") {
      // Tab always goes "forward" — into the selected repo's actions.
      e.preventDefault();
      if (results[selected]) openActions(results[selected]);
    } else if (e.key === "Delete") {
      e.preventDefault();
      query = "";
    } else if (e.key === "Escape") {
      e.preventDefault();
      hideOverlay();
    } else if (e.key.toLowerCase() === "r" && e.ctrlKey) {
      e.preventDefault();
      rescan();
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
      actionSel = Math.min(actionSel + 1, filteredActions.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      actionSel = Math.max(actionSel - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (activeRepo && filteredActions[actionSel])
        execute(filteredActions[actionSel].action, activeRepo.repo.path);
    } else if (e.key === "Tab") {
      e.preventDefault(); // nothing "forward" from here; keep focus in the filter
    } else if (e.key === "Delete") {
      e.preventDefault();
      actionQuery = "";
      actionSel = 0;
    } else if (e.key === "Escape") {
      // Esc always steps back a level.
      e.preventDefault();
      backToList();
    }
  }

  // All keyboard handling runs at the window level and is dispatched by mode, so
  // navigation keeps working even when no element is focused (e.g. right after
  // returning from the action menu). Character keys fall through to the focused
  // search input untouched.
  function onWindowKeydown(e: KeyboardEvent) {
    if (mode === "action-menu") {
      onMenuKeydown(e);
    } else if (mode === "settings") {
      if (e.key === "Escape") {
        e.preventDefault();
        backToList();
      }
    } else {
      onListKeydown(e);
    }
  }

  onMount(() => {
    const unlisteners: Array<Promise<() => void>> = [];
    unlisteners.push(
      onOverlayShown(() => {
        query = "";
        selected = 0;
        backToList();
        void tick().then(() => search?.focus());
        void loadInitial();
      }),
    );
    unlisteners.push(onReposUpdated(() => void refresh()));

    search?.focus();
    void loadInitial();

    return () => {
      for (const u of unlisteners) u.then((fn) => fn());
    };
  });
</script>

<svelte:window onkeydown={onWindowKeydown} />

<main
  class="relative mx-auto flex h-[480px] w-[720px] flex-col overflow-hidden rounded
         border border-hair bg-panel/[0.82] backdrop-blur-xl"
>
  {#if mode === "repo-list"}
    <SearchInput bind:this={search} bind:value={query} />
    <ResultList
      entries={results}
      {selected}
      onselect={(i) => (selected = i)}
      onactivate={(i) => activateRepo(i)}
    />
  {:else if mode === "action-menu" && activeRepo}
    <ActionMenu
      repoName={activeRepo.repo.name}
      items={filteredActions}
      bind:filter={actionQuery}
      selected={actionSel}
      onselect={(i) => (actionSel = i)}
      onrun={(i) =>
        activeRepo && filteredActions[i] &&
        execute(filteredActions[i].action, activeRepo.repo.path)}
      onback={backToList}
    />
  {:else if mode === "settings"}
    <Settings onback={backToList} onsaved={() => rescan()} />
  {/if}

  <footer
    class="flex items-center justify-between border-t border-hair px-4 py-2
           text-[11px] text-white/30"
  >
    <span class="flex min-w-0 items-center gap-3">
      <span class="truncate">{status}</span>
      {#if mode === "repo-list"}
        <span class="inline-flex shrink-0 items-center gap-1"
          ><kbd>Ctrl+R</kbd>rescan</span
        >
      {/if}
    </span>
    <span class="flex shrink-0 items-center gap-3.5">
      {#each hints as [key, label] (label)}
        <span class="inline-flex items-center gap-1"><kbd>{key}</kbd>{label}</span>
      {/each}
      {#if mode !== "settings"}
        <button
          type="button"
          onclick={() => (mode = "settings")}
          title="Settings (Ctrl+,)"
          aria-label="Settings"
          class="shrink-0 rounded-[3px] border border-white/15 bg-white/[0.09] p-1
                 text-white/80 transition-colors hover:bg-white/20 hover:text-white"
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
    </span>
  </footer>

  {#if scanning}
    <div
      class="pointer-events-none absolute right-4 top-3 text-[11px] text-sky-300/70"
    >
      scanning…
    </div>
  {/if}
</main>

<style>
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
