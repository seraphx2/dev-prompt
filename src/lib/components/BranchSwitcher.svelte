<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import type { Branch, BranchList, Repo, RepoStatus, SwitchTarget, VcsOutcome, Worktree } from "../types";
  import { cancelRead, vcsAddCopy, vcsBranches, vcsSwitch } from "../ipc";
  import { fuzzyScore } from "../fuzzy";
  import { ago, plural, vocab } from "../vcsStatus";
  import { icons } from "../icons";
  import { middleTruncate } from "../text";
  import Highlight from "./Highlight.svelte";
  import ClearButton from "./ClearButton.svelte";

  let {
    repo,
    status,
    onback,
    onswitched,
    onopen,
  }: {
    repo: Repo;
    status: RepoStatus | null | undefined;
    onback: () => void;
    /** A switch went through: what happened, and the repo's new status. */
    onswitched: (outcome: VcsOutcome) => void;
    /** Open another working copy's action menu. */
    onopen: (repo: Repo) => void;
  } = $props();

  /** This system's words: branch or bookmark, worktree / workspace / share /
   *  checkout, and what switching does to uncommitted changes. */
  const v = $derived(vocab(status?.vcs ?? repo.vcs));

  type Row =
    | { kind: "branch"; group: string; branch: Branch; positions: number[] }
    | { kind: "worktree"; group: string; wt: Worktree; positions: number[] }
    | { kind: "create"; group: string; name: string };

  let list = $state<BranchList | null>(null);
  let loadError = $state("");
  let filter = $state("");
  let selected = $state(0);
  /** What's running ("Switching to x…"), or "" when idle. */
  let busy = $state("");
  /** Why the last attempt was refused. */
  let failure = $state("");
  let filterEl: HTMLInputElement | undefined = $state();
  let container: HTMLDivElement | undefined = $state();

  onMount(() => {
    filterEl?.focus();
    vcsBranches(repo.path)
      .then((l) => (list = l))
      .catch((e) => (loadError = `${e}`));
  });
  // Left before the list arrived: stop reading it, so nothing keeps running in
  // the repo.
  onDestroy(() => {
    if (!list && !loadError) void cancelRead("branches");
  });

  const base = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

  /** The heading over each kind of row. */
  const GROUP: Record<Branch["kind"], string> = {
    branch: "Branches",
    bookmark: "Bookmarks",
    remote: "Remote",
  };
  const groupOf = (b: Branch) =>
    b.kind === "remote" ? (v?.noun === "bookmark" ? "Remote bookmarks" : "Remote branches") : GROUP[b.kind];

  const rows = $derived.by<Row[]>(() => {
    if (!list) return [];
    const q = filter.trim();
    const out: Row[] = [];
    const add = (b: Branch) => {
      if (!q) return out.push({ kind: "branch", group: groupOf(b), branch: b, positions: [] });
      const hit = fuzzyScore(q, b.name);
      if (hit) out.push({ kind: "branch", group: groupOf(b), branch: b, positions: hit.positions });
    };
    // Bookmarks, then branches, then remote ones — current first in each.
    for (const kind of ["bookmark", "branch", "remote"] as const) {
      for (const b of list.branches) if (b.kind === kind) add(b);
    }
    // Working copies on a branch show on that branch's row; these are the
    // rest — minus any whose folder is gone.
    for (const wt of list.worktrees) {
      if (wt.current || wt.branch || wt.prunable) continue;
      const label = base(wt.path);
      const hit = q ? fuzzyScore(q, label) : { positions: [] };
      if (hit) out.push({ kind: "worktree", group: `Other ${v?.copy ?? "working copie"}s`, wt, positions: hit.positions });
    }
    // Typed something that isn't a branch: offer to make it one.
    const exact = list.branches.some((b) => b.name === q || (b.remote && (b.name === `${b.remote}/${q}` || b.name === `${q}@${b.remote}`)));
    if (q && !exact && !/\s/.test(q)) {
      out.push({ kind: "create", group: "", name: q });
    }
    return out;
  });

  $effect(() => {
    void filter;
    selected = 0;
  });
  $effect(() => {
    if (selected >= rows.length) selected = Math.max(0, rows.length - 1);
  });

  let skipScroll = false;
  $effect(() => {
    const idx = selected;
    void rows;
    if (skipScroll) {
      skipScroll = false;
      return;
    }
    if (!container) return;
    if (idx === 0) container.scrollTop = 0;
    else container.querySelector<HTMLElement>(`[data-idx="${idx}"]`)?.scrollIntoView({ block: "nearest" });
  });

  function targetOf(row: Row): SwitchTarget | null {
    if (row.kind === "create") return { kind: "new", name: row.name };
    if (row.kind !== "branch") return null;
    const b = row.branch;
    return b.remote ? { kind: "remote", remote: b.remote, name: b.name } : { kind: "local", name: b.name };
  }

  const copyRepo = (path: string): Repo => ({
    name: base(path),
    path,
    vcs: status?.vcs ?? repo.vcs,
    sentinels: [],
    lastSeen: 0,
  });

  /** Uncommitted changes, and whether they stop a switch here. */
  const dirty = $derived((status?.changed ?? 0) + (status?.conflicted ?? 0));
  const blocked = $derived(!!status?.operation || (v?.changes === "block" && dirty > 0));

  /** Enter: check it out here — or, when it's checked out in another working
   *  copy, go to that one instead. */
  async function activate(i: number) {
    const row = rows[i];
    if (!row || busy) return;
    if (row.kind === "worktree") return onopen(copyRepo(row.wt.path));
    if (row.kind === "branch" && row.branch.current && v?.changes !== "stay") return onback();
    if (row.kind === "branch" && row.branch.worktree) return onopen(copyRepo(row.branch.worktree));
    const target = targetOf(row);
    if (!target) return;
    const name = row.kind === "create" ? row.name : row.branch.name;
    busy = row.kind === "create" ? `Creating ${name}…` : `Switching to ${name}…`;
    failure = "";
    try {
      onswitched(await vcsSwitch(repo.path, target));
    } catch (e) {
      failure = `${e}`;
    } finally {
      busy = "";
    }
  }

  /** Ctrl+Enter: check it out in a new working copy beside this one, and go
   *  there. Uncommitted changes here don't matter: they stay here. */
  async function inNewCopy(i: number) {
    const row = rows[i];
    if (!row || busy || !v) return;
    if (row.kind === "branch" && row.branch.worktree) {
      failure = `${row.branch.name} is checked out in ${row.branch.worktree} — Enter goes there.`;
      return;
    }
    if (row.kind === "branch" && row.branch.current && v.changes === "carry") {
      failure = `${row.branch.name} is checked out here already — git won't check it out twice.`;
      return;
    }
    const target = targetOf(row);
    if (!target) return;
    busy = `Making a ${v.copy}…`;
    failure = "";
    try {
      onopen(await vcsAddCopy(repo.path, target));
    } catch (e) {
      failure = `${e}`;
    } finally {
      busy = "";
    }
  }

  /** Keys from the window (App owns dispatch, as for the other screens). */
  export function keydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, rows.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (e.ctrlKey) void inNewCopy(selected);
      else void activate(selected);
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (!busy) onback();
    } else if (e.key === "Delete") {
      e.preventDefault();
      filter = "";
    }
  }
</script>

<div class="flex items-center gap-2 border-b border-hair px-3 py-2.5">
  <button
    type="button"
    class="shrink-0 rounded-sm px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
    onclick={onback}
    aria-label="Back"
  >
    ←
  </button>
  <span class="shrink-0 truncate text-[13px] font-medium text-white/80">{repo.name}</span>
  <span class="shrink-0 text-white/25">›</span>
  <span class="shrink-0 text-[13px] font-medium text-orange-300">{v?.title ?? "Branches"}</span>
  <span class="shrink-0 text-white/15">/</span>
  <input
    bind:this={filterEl}
    bind:value={filter}
    placeholder={`Filter, or type a new ${v?.create ?? "branch"} name…`}
    spellcheck="false"
    autocomplete="off"
    class="min-w-0 flex-1 bg-transparent text-[13px] text-white/90 placeholder:text-white/25 focus:outline-hidden"
  />
  <ClearButton
    show={!!filter}
    onclear={() => {
      filter = "";
      filterEl?.focus();
    }}
  />
</div>

{#if failure}
  <div class="border-b border-hair bg-red-400/8 px-4 py-2 text-[12px] whitespace-pre-line text-red-200">{failure}</div>
{:else if status?.operation}
  <div class="border-b border-hair bg-red-400/8 px-4 py-2 text-[12px] text-red-200">
    {#if status.operation === "stale"}
      This workspace is stale — run <code>jj workspace update-stale</code> in it before switching.
    {:else if status.operation === "cleanup"}
      An interrupted command left this working copy locked — run <code>svn cleanup</code> before switching.
    {:else}
      A {status.operation} is in progress here. Finish or abort it before switching.
    {/if}
    A new {v?.copy ?? "working copy"} (Ctrl+Enter) still works.
  </div>
{:else if dirty && v?.changes === "block"}
  <div class="border-b border-hair bg-amber-400/8 px-4 py-2 text-[12px] text-amber-100/90">
    Commit or shelve your {plural(dirty, "uncommitted change")} before switching — {status?.vcs} would merge
    {dirty === 1 ? "it" : "them"} into the other {v.noun}. A new {v.copy} (Ctrl+Enter) leaves {dirty === 1 ? "it" : "them"}
    here.
  </div>
{:else if dirty && v?.changes === "carry"}
  <div class="border-b border-hair bg-amber-400/8 px-4 py-2 text-[12px] text-amber-100/90">
    {plural(dirty, "uncommitted change")} come along to the branch you switch to. If one would be overwritten,
    git refuses and nothing changes.
  </div>
{:else if v?.changes === "stay"}
  <div class="border-b border-hair px-4 py-2 text-[12px] text-white/45">
    Switching starts a new change on top of the bookmark; the change you're in stays as it is.
  </div>
{/if}

<div bind:this={container} class="scroll-thin flex-1 overflow-y-auto px-2 py-2">
  {#if loadError}
    <div class="px-3 py-8 text-center text-[13px] text-red-300/80">{loadError}</div>
  {:else if !list}
    <div class="px-3 py-8 text-center text-[13px] text-white/45">
      {v?.server ? "Asking the server for branches…" : `Reading ${v?.title.toLowerCase() ?? "branches"}…`}
    </div>
  {:else if rows.length === 0}
    <div class="px-3 py-8 text-center text-[13px] text-white/45">Nothing matches.</div>
  {:else}
    {#each rows as row, i (row.kind === "branch" ? `${row.branch.kind}:${row.branch.name}` : row.kind === "worktree" ? `w:${row.wt.path}` : "create")}
      {@const newGroup = i === 0 ? !!row.group : rows[i - 1].group !== row.group}
      {#if newGroup && row.group}
        <div class="px-3 pt-2 pb-1 text-[10px] font-medium tracking-wide text-white/45 uppercase {i ? 'mt-2' : ''}">
          {row.group}
        </div>
      {/if}
      <button
        type="button"
        data-idx={i}
        class="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left transition-colors
               {i === selected ? 'bg-white/10' : 'hover:bg-white/4'} {row.kind === 'create' && !newGroup ? 'mt-2' : ''}
               {blocked && row.kind !== 'worktree' && !(row.kind === 'branch' && row.branch.worktree) ? 'opacity-60' : ''}"
        onclick={() => void activate(i)}
        onpointermove={() => {
          if (i !== selected) skipScroll = true;
          selected = i;
        }}
      >
        {#if row.kind === "create"}
          <span class="w-4 shrink-0 text-center text-[15px] leading-none text-emerald-300">+</span>
          <span class="flex-1 truncate text-[13px] text-white/90">
            Create {v?.create ?? "branch"} <span class="font-mono text-emerald-200">{row.name}</span>
            <span class="text-white/40"
              >{v?.server ? `on the server, from ${status?.branch ?? "here"}` : `from ${status?.branch ?? "here"}`}</span
            >
          </span>
        {:else}
          <svg class="h-4 w-4 shrink-0 {row.kind === 'branch' && row.branch.current ? 'text-sky-300' : 'text-white/35'}" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd" aria-hidden="true">
            {#if icons.branch.raw}{@html icons.branch.raw}{:else}<path d={icons.branch.d} />{/if}
          </svg>
          {#if row.kind === "branch"}
            {@const b = row.branch}
            <span class="flex min-w-0 flex-1 items-baseline gap-2">
              <span class="shrink-0 font-mono text-[13px] {b.current ? 'text-sky-200' : 'text-white/90'}">
                <Highlight text={b.name} indices={row.positions} />
              </span>
              <span class="truncate text-[12px] text-white/45">{b.subject}</span>
            </span>
            <span class="flex shrink-0 items-center gap-1.5 font-mono text-[10px] text-white/40">
              {#if b.current}<span class="rounded-sm bg-sky-400/15 px-1 py-0.5 text-sky-200">here</span>{/if}
              {#if b.worktree}
                <span class="rounded-sm bg-white/6 px-1 py-0.5 text-white/55" title={b.worktree}
                  >{v?.copy ?? "copy"} {middleTruncate(base(b.worktree), 24)}</span
                >
              {/if}
              {#if b.gone}<span class="rounded-sm bg-white/6 px-1 py-0.5" title="Its upstream was deleted on the remote">gone</span>{/if}
              {#if b.ahead}<span class="text-sky-200/80" title="Commits its upstream doesn't have">↑{b.ahead}</span>{/if}
              {#if b.behind}<span class="text-sky-200/80" title="Commits on its upstream it doesn't have">↓{b.behind}</span>{/if}
              <span class="w-20 text-right whitespace-nowrap text-white/45">{b.date ? ago(b.date) : ""}</span>
            </span>
          {:else}
            <span class="flex min-w-0 flex-1 items-baseline gap-2">
              <span class="shrink-0 font-mono text-[13px] text-white/90">
                <Highlight text={base(row.wt.path)} indices={row.positions} />
              </span>
              <span class="truncate font-mono text-[11px] text-white/45" title={row.wt.path}>{row.wt.path}</span>
            </span>
            {#if row.wt.head}<span class="shrink-0 font-mono text-[10px] text-white/40">@{row.wt.head}</span>{/if}
          {/if}
        {/if}
      </button>
    {/each}
  {/if}
</div>

{#if busy}
  <div class="border-t border-hair px-4 py-1.5 text-[12px] text-white/60">{busy}</div>
{/if}
