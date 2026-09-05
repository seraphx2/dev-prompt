<script lang="ts">
  import { onMount } from "svelte";
  import type { Action, MenuItem } from "../types";
  import { glyphFor, glyphDim } from "../glyph";
  import { middleTruncate } from "../text";
  import Highlight from "./Highlight.svelte";
  import ClearButton from "./ClearButton.svelte";

  const glyph = (a: Action) => glyphFor(a.icon);

  let {
    repoName,
    crumb,
    items,
    filter = $bindable(),
    selected,
    onselect,
    onrun,
    onback,
  }: {
    repoName: string;
    /** Sub-project name when drilled into one, else null. */
    crumb: string | null;
    /** Already filtered / grouped by the parent. */
    items: MenuItem[];
    filter: string;
    selected: number;
    onselect: (i: number) => void;
    onrun: (i: number) => void;
    onback: () => void;
  } = $props();

  let container: HTMLDivElement | undefined = $state();
  let filterEl: HTMLInputElement | undefined = $state();

  onMount(() => {
    filterEl?.focus();
    filterEl?.select();
  });

  const key = (it: MenuItem) =>
    it.kind === "submenu" ? `sub:${it.target}` : it.action.id;

  // Auto-scroll follows keyboard navigation only — see the note in ResultList:
  // scrolling a hovered row shifts the list under the mouse and loops.
  let skipScroll = false;

  $effect(() => {
    const idx = selected; // track the selection
    if (skipScroll) {
      skipScroll = false;
      return;
    }
    container
      ?.querySelector<HTMLElement>(`[data-idx="${idx}"]`)
      ?.scrollIntoView({ block: "nearest" });
  });
</script>

<div class="flex items-center gap-2 border-b border-hair px-3 py-2.5">
  <button
    type="button"
    class="shrink-0 rounded px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
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
    class="min-w-0 flex-1 bg-transparent text-[13px] text-white/90 placeholder:text-white/25 focus:outline-none"
  />
  <ClearButton
    show={!!filter}
    onclear={() => {
      filter = "";
      filterEl?.focus();
    }}
  />
</div>

<div bind:this={container} class="scroll-thin flex-1 overflow-y-auto px-2 py-2">
  {#if items.length === 0}
    <div class="px-3 py-8 text-center text-[13px] text-white/30">
      No actions match.
    </div>
  {:else}
    {#each items as item, i (key(item))}
      {#if i === 0 || items[i - 1].group !== item.group}
        {#if item.group}
          <div
            class="truncate px-3 pb-1 pt-3 text-[10px] font-semibold uppercase tracking-wider text-orange-400"
          >
            {item.group}
          </div>
        {:else}
          <div class="my-1 border-t border-hair"></div>
        {/if}
      {/if}
      <button
        type="button"
        data-idx={i}
        class="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors
               {i === selected ? 'bg-white/10' : 'hover:bg-white/[0.04]'}"
        onclick={() => onrun(i)}
        onpointerenter={() => {
          if (i !== selected) skipScroll = true;
          onselect(i);
        }}
      >
        {#if item.kind === "submenu"}
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
          <span class="shrink-0 font-mono text-[11px] text-white/30"
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
          {@const dim = glyphDim(g)}
          <svg
            class="h-4 w-4 shrink-0 {g.hex && !dim
              ? 'opacity-90'
              : dim
                ? 'text-white/80'
                : 'text-white/30'}"
            viewBox={g.vb ?? "0 0 24 24"}
            fill="currentColor"
            fill-rule="evenodd"
            style={g.hex && !dim ? `color:${g.hex}` : ""}
          >
            {#if g.raw}{@html g.raw}{:else}<path d={g.d} />{/if}
          </svg>
          <span class="flex-1 truncate text-[13px] text-white/90">
            <Highlight text={item.action.label} indices={item.positions} />
          </span>
          <span
            class="shrink-0 overflow-hidden whitespace-nowrap font-mono text-[11px] text-white/35"
            title={item.action.hint}>{middleTruncate(item.action.hint, 46)}</span
          >
        {/if}
      </button>
    {/each}
  {/if}
</div>
