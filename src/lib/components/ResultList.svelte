<script lang="ts">
  import type { ScoredRepo } from "../types";
  import ResultRow from "./ResultRow.svelte";

  let {
    entries,
    selected,
    onselect,
    onactivate,
  }: {
    entries: ScoredRepo[];
    selected: number;
    /** Pointer moved onto a row -> move the selection there. */
    onselect: (i: number) => void;
    /** Row clicked -> open that repo's actions. */
    onactivate: (i: number) => void;
  } = $props();

  let container: HTMLDivElement | undefined = $state();

  // Keep the selected row scrolled into view as the user arrows through.
  $effect(() => {
    if (!container) return;
    const el = container.querySelector<HTMLElement>(`[data-idx="${selected}"]`);
    el?.scrollIntoView({ block: "nearest" });
  });
</script>

<div
  bind:this={container}
  class="scroll-thin flex-1 overflow-y-auto px-2 py-2"
>
  {#if entries.length === 0}
    <div class="px-3 py-8 text-center text-[13px] text-white/30">
      No repositories match.
    </div>
  {:else}
    {#each entries as entry, i (entry.repo.path)}
      <div data-idx={i}>
        <ResultRow
          {entry}
          active={i === selected}
          onactivate={() => onactivate(i)}
          onhover={() => onselect(i)}
        />
      </div>
    {/each}
  {/if}
</div>
