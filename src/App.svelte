<script lang="ts">
  import { onMount, tick } from "svelte";
  import SearchInput from "./lib/components/SearchInput.svelte";
  import ResultList from "./lib/components/ResultList.svelte";
  import ActionMenu from "./lib/components/ActionMenu.svelte";
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
  } from "./lib/ipc";
  import type { Action, ScoredRepo } from "./lib/types";

  type Mode = "repo-list" | "action-menu";

  let query = $state("");
  let results = $state<ScoredRepo[]>([]);
  let selected = $state(0);

  let mode = $state<Mode>("repo-list");
  let actions = $state<Action[]>([]);
  let actionSel = $state(0);
  let activeRepo = $state<ScoredRepo | null>(null);

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
    actionSel = 0;
    mode = "action-menu";
  }

  function backToList() {
    mode = "repo-list";
    activeRepo = null;
    actions = [];
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

  /** Enter on a repo runs its first (default) action directly. */
  async function activateRepo(i: number) {
    const entry = results[i];
    if (!entry) return;
    const acts = await buildActions(entry.repo.path);
    if (acts.length > 0) await execute(acts[0], entry.repo.path);
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
    } else if (e.key === "Tab" || (e.key === "ArrowRight" && atInputEnd(e))) {
      e.preventDefault();
      if (results[selected]) openActions(results[selected]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      hideOverlay();
    } else if (e.key.toLowerCase() === "r" && e.ctrlKey) {
      e.preventDefault();
      rescan();
    }
  }

  function atInputEnd(e: KeyboardEvent): boolean {
    const t = e.target;
    return t instanceof HTMLInputElement && t.selectionStart === t.value.length;
  }

  function onMenuKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      actionSel = Math.min(actionSel + 1, actions.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      actionSel = Math.max(actionSel - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (activeRepo && actions[actionSel])
        execute(actions[actionSel], activeRepo.repo.path);
    } else if (e.key === "Escape" || e.key === "ArrowLeft" || e.key === "Backspace") {
      e.preventDefault();
      backToList();
    }
  }

  // All keyboard handling runs at the window level and is dispatched by mode, so
  // navigation keeps working even when no element is focused (e.g. right after
  // returning from the action menu). Character keys fall through to the focused
  // search input untouched.
  function onWindowKeydown(e: KeyboardEvent) {
    if (mode === "action-menu") onMenuKeydown(e);
    else onListKeydown(e);
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
  {:else if activeRepo}
    <ActionMenu
      repoName={activeRepo.repo.name}
      {actions}
      selected={actionSel}
      onselect={(i) => (actionSel = i)}
      onrun={(i) => activeRepo && execute(actions[i], activeRepo.repo.path)}
      onback={backToList}
    />
  {/if}

  <footer
    class="flex items-center justify-between border-t border-hair px-4 py-2
           text-[11px] text-white/30"
  >
    <span>{status}</span>
    <span class="flex gap-3">
      {#if mode === "repo-list"}
        <kbd>Up/Down</kbd> move
        <kbd>Enter</kbd> launch
        <kbd>Tab</kbd> actions
        <kbd>Ctrl+R</kbd> rescan
      {:else}
        <kbd>Enter</kbd> run
        <kbd>Esc</kbd> back
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
  kbd {
    font-family: inherit;
  }
</style>
