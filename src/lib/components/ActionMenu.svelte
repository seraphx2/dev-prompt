<script lang="ts">
  import { onMount } from "svelte";
  import type { Action, MenuItem, RepoStatus } from "../types";
  import { glyphColor, glyphFor } from "../glyph";
  import { attention, describe, headLabel, vocab } from "../vcsStatus";
  import { icons } from "../icons";
  import { middleTruncate } from "../text";
  import Highlight from "./Highlight.svelte";
  import ClearButton from "./ClearButton.svelte";

  const glyph = (a: Action) => glyphFor(a.icon);

  let {
    repoName,
    crumb,
    items,
    quickActions,
    filter = $bindable(),
    selected,
    onselect,
    onrun,
    onquickaction,
    onback,
    oncontext,
    status = null,
    onbranches,
    onremove,
    binWord = "Recycle",
    shift = false,
    adminWord = "administrator",
  }: {
    repoName: string;
    /** Sub-project name when drilled into one, else null. */
    crumb: string | null;
    /** Already filtered / grouped by the parent. */
    items: MenuItem[];
    /** Universal actions with a dedicated hotkey, shown as a row above the list. */
    quickActions: { key: string; action: Action }[];
    filter: string;
    selected: number;
    onselect: (i: number) => void;
    /** `e` carries Shift: as administrator / root, where the action can be. */
    onrun: (i: number, e?: MouseEvent) => void;
    onquickaction: (action: Action, e?: MouseEvent) => void;
    onback: () => void;
    /** Row right-clicked -> its context menu. */
    oncontext?: (i: number, e: MouseEvent) => void;
    /** The repo's working-copy state, for the line under the header. */
    status?: RepoStatus | null;
    /** Open the branch switcher (systems that have one). */
    onbranches?: () => void;
    /** Open "Move to Recycle Bin / Trash" — absent where it isn't offered
     *  (a root folder). */
    onremove?: () => void;
    /** "Recycle" (Windows) or "Trash" (Linux), for the header button. */
    binWord?: string;
    /** Shift is held: the selected row and the quick actions that can run
     *  elevated say so. */
    shift?: boolean;
    /** "administrator" (Windows) or "root" (Linux). */
    adminWord?: string;
  } = $props();

  const head = $derived(headLabel(status));
  const lines = $derived(describe(status));
  const level = $derived(attention(status));
  /** How this repo's system talks about branches; null: no switcher. */
  const v = $derived(vocab(status?.vcs));

  let container: HTMLDivElement | undefined = $state();
  let filterEl: HTMLInputElement | undefined = $state();

  onMount(() => {
    filterEl?.focus();
    filterEl?.select();
  });

  const key = (it: MenuItem) =>
    it.kind === "submenu" ? `sub:${it.target}` : it.kind === "loading" ? "loading" : it.action.id;

  // Auto-scroll follows keyboard navigation only — see the note in ResultList:
  // scrolling a hovered row shifts the list under the mouse and loops.
  let skipScroll = false;

  // Re-runs on new items too, pinning a fresh filter to the top — see ResultList.
  $effect(() => {
    const idx = selected; // track the selection
    void items;
    if (skipScroll) {
      skipScroll = false;
      return;
    }
    if (!container) return;
    if (idx === 0) container.scrollTop = 0;
    else
      container
        .querySelector<HTMLElement>(`[data-idx="${idx}"]`)
        ?.scrollIntoView({ block: "nearest" });
  });
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
  <span class="shrink-0 truncate text-[13px] font-medium text-white/80">{repoName}</span>
  {#if crumb}
    <span class="shrink-0 text-white/25">›</span>
    <span class="shrink-0 truncate text-[13px] font-medium text-orange-300">{crumb}</span>
  {/if}
  <span class="shrink-0 text-white/15">/</span>
  <input
    bind:this={filterEl}
    bind:value={filter}
    placeholder="Filter actions…"
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
  {#if onremove}
    <!-- Out of the list, where it sat among the apps; here it's on every
         repo but nowhere you'd land by accident. It only opens the check. -->
    <button
      type="button"
      onclick={onremove}
      title={`Move this folder to the ${binWord === "Trash" ? "Trash" : "Recycle Bin"} (Ctrl+Delete)\nChecks first for anything that exists only here, and offers Undo after`}
      class="inline-flex shrink-0 items-center gap-1 rounded-sm px-1.5 py-0.5 text-[12px] text-white/50
             hover:bg-red-400/12 hover:text-red-300"
    >
      <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd" aria-hidden="true">
        {#if icons.trash.raw}{@html icons.trash.raw}{:else}<path d={icons.trash.d} />{/if}
      </svg>
      {binWord}
    </button>
  {/if}
</div>

{#if status && (head || lines.length)}
  <div class="flex items-center gap-2 border-b border-hair px-3 py-1.5 text-[11px]" title={lines.join("\n")}>
    {#if head}
      <button
        type="button"
        disabled={!v}
        onclick={() => onbranches?.()}
        class="inline-flex max-w-[40%] shrink-0 items-center gap-1 rounded-sm px-1 py-0.5 font-mono text-white/75
               enabled:hover:bg-white/10 enabled:hover:text-white"
        title={v ? `Switch ${v.noun} (Ctrl+B)` : undefined}
      >
        <svg class="h-3 w-3 shrink-0 text-white/40" viewBox="0 0 24 24" fill="currentColor" fill-rule="evenodd" aria-hidden="true">
          {#if icons.branch.raw}{@html icons.branch.raw}{:else}<path d={icons.branch.d} />{/if}
        </svg>
        <span class="truncate">{head}</span>
      </button>
    {/if}
    <span
      class="min-w-0 flex-1 truncate {level >= 3
        ? 'text-red-300'
        : level === 2
          ? 'text-amber-200/80'
          : 'text-white/40'}">{lines.join(" · ")}</span
    >
    {#if v}
      <span class="inline-flex shrink-0 items-center gap-1 text-white/45"><kbd>Ctrl+B</kbd>{v.title.toLowerCase()}</span>
    {/if}
  </div>
{/if}

{#if quickActions.length}
  <div class="grid grid-cols-4 gap-1 border-b border-hair px-2 py-2">
    {#each quickActions as qa (qa.action.id)}
      <button
        type="button"
        class="flex items-center justify-center gap-1.5 rounded-lg px-2 py-1.5
               text-[13px] text-white/70 transition-colors hover:bg-white/6 hover:text-white/90"
        onclick={(e) => onquickaction(qa.action, e)}
      >
        <kbd>{qa.key}</kbd>
        <span class="truncate">{qa.action.label}</span>
        {#if shift && qa.action.elevated}{@render shield()}{/if}
      </button>
    {/each}
  </div>
{/if}

<div bind:this={container} class="scroll-thin flex-1 overflow-y-auto px-2 py-2">
  {#if items.length === 0}
    <div class="px-3 py-8 text-center text-[13px] text-white/45">
      No actions match.
    </div>
  {:else}
    {#each items as item, i (key(item))}
      {@const newGroup = i > 0 && items[i - 1].group !== item.group}
      <button
        type="button"
        data-idx={i}
        disabled={item.kind === "loading"}
        class="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors
               {newGroup ? 'mt-3' : ''}
               {item.kind === 'loading'
          ? 'cursor-default'
          : i === selected
            ? 'bg-white/10'
            : 'hover:bg-white/4'}"
        onclick={(e) => onrun(i, e)}
        oncontextmenu={(e) => oncontext?.(i, e)}
        onpointermove={() => {
          // Not `pointerenter` — see ResultRow: rows reshuffling under a
          // resting cursor mustn't steal the highlight from the top match.
          if (item.kind === "loading") return;
          if (i !== selected) skipScroll = true;
          onselect(i);
        }}
      >
        {#if item.kind === "loading"}
          <svg
            class="h-4 w-4 shrink-0 animate-spin text-white/40"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            stroke-linecap="round"
          >
            <path d="M12 3a9 9 0 1 0 9 9" />
          </svg>
          <span class="flex-1 truncate text-[13px] text-white/40">{item.label}</span>
        {:else if item.kind === "submenu"}
          <svg
            class="h-4 w-4 shrink-0 text-orange-300"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d="M4 6a2 2 0 0 1 2-2h3l2 2h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z" />
          </svg>
          <span class="flex-1 truncate text-[13px] font-medium text-white/90"
            >{item.label}</span
          >
          <span class="shrink-0 font-mono text-[11px] text-white/45"
            >{item.count} action{item.count === 1 ? "" : "s"}</span
          >
          <svg
            class="h-4 w-4 shrink-0 text-white/50"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d="m9 6 6 6-6 6" />
          </svg>
        {:else}
          {@const g = glyph(item.action)}
          {@const color = glyphColor(g, 0.9)}
          <svg
            class="h-4 w-4 shrink-0 {color
              ? 'opacity-90'
              : g.hex
                ? 'text-white/80'
                : 'text-white/40'}"
            viewBox={g.vb ?? "0 0 24 24"}
            fill="currentColor"
            fill-rule="evenodd"
            style={color ? `color:${color}` : ""}
          >
            {#if g.raw}{@html g.raw}{:else}<path d={g.d} />{/if}
          </svg>
          <span
            class="flex-1 truncate text-[13px] {item.action.remove ? 'text-red-300' : 'text-white/90'}"
          >
            <Highlight text={item.action.label} indices={item.positions} />
            {#if shift && i === selected && item.action.elevated}
              {" "}<span class="text-sky-300">as {adminWord}</span>{@render shield()}
            {/if}
          </span>
          <span
            class="shrink-0 overflow-hidden whitespace-nowrap font-mono text-[11px] text-white/50"
            title={item.action.hint}>{middleTruncate(item.action.hint, 46)}</span
          >
        {/if}
      </button>
    {/each}
  {/if}
</div>

{#snippet shield()}
  <svg class="ml-1 inline h-3.5 w-3.5 shrink-0 align-[-2px]" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    {@html icons.shield.raw}
  </svg>
{/snippet}

<style>
  /* Key hints: bright key-cap, dim descriptive label — matches the footer. */
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
