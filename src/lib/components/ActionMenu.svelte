<script lang="ts">
  import { onMount } from "svelte";
  import type { Action } from "../types";
  import Highlight from "./Highlight.svelte";
  import ClearButton from "./ClearButton.svelte";

  type Item = { action: Action; positions: number[] };

  let {
    repoName,
    items,
    filter = $bindable(),
    selected,
    onselect,
    onrun,
    onback,
  }: {
    repoName: string;
    /** Already filtered by the parent; headers for empty groups fall away. */
    items: Item[];
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

  // Keep the selected row in view as the user arrows through a long list.
  $effect(() => {
    if (!container) return;
    container
      .querySelector<HTMLElement>(`[data-idx="${selected}"]`)
      ?.scrollIntoView({ block: "nearest" });
  });
</script>

<div class="flex items-center gap-2 border-b border-hair px-3 py-2.5">
  <button
    type="button"
    class="shrink-0 rounded px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
    onclick={onback}
    aria-label="Back to repository list"
  >
    ←
  </button>
  <span class="shrink-0 truncate text-[13px] font-medium text-white/80">{repoName}</span>
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
    {#each items as item, i (item.action.id)}
      {#if i === 0 || items[i - 1].action.group !== item.action.group}
        {#if item.action.group}
          <div
            class="truncate px-3 pb-1 pt-3 text-[10px] font-semibold uppercase tracking-wider text-orange-400"
          >
            {item.action.group}
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
        onpointerenter={() => onselect(i)}
      >
        <span class="flex-1 truncate text-[13px] text-white/90">
          <Highlight text={item.action.label} indices={item.positions} />
        </span>
        <span class="shrink-0 truncate font-mono text-[11px] text-white/35"
          >{item.action.hint}</span
        >
      </button>
    {/each}
  {/if}
</div>
