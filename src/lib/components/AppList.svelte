<script lang="ts">
  import type { AppEntry } from "../types";
  import AppRow from "./AppRow.svelte";

  let {
    entries,
    selected,
    scanning = false,
    onselect,
    onactivate,
  }: {
    entries: { app: AppEntry; positions: number[] }[];
    selected: number;
    /** True while a background enumeration is running and the list is empty. */
    scanning?: boolean;
    onselect: (i: number) => void;
    onactivate: (i: number) => void;
  } = $props();

  let container: HTMLDivElement | undefined = $state();

  // Auto-scroll follows keyboard navigation only — see the note in ResultList.
  let skipScroll = false;

  $effect(() => {
    const idx = selected;
    if (skipScroll) {
      skipScroll = false;
      return;
    }
    container
      ?.querySelector<HTMLElement>(`[data-idx="${idx}"]`)
      ?.scrollIntoView({ block: "nearest" });
  });
</script>

<div bind:this={container} class="scroll-thin flex-1 overflow-y-auto px-2 py-2">
  {#if entries.length === 0}
    <div class="px-3 py-8 text-center text-[13px] text-white/30">
      {scanning ? "Scanning for installed apps…" : "No apps match."}
    </div>
  {:else}
    {#each entries as e, i (e.app.exec)}
      <div data-idx={i}>
        <AppRow
          app={e.app}
          positions={e.positions}
          active={i === selected}
          onactivate={() => onactivate(i)}
          onhover={() => {
            if (i !== selected) skipScroll = true;
            onselect(i);
          }}
        />
      </div>
    {/each}
  {/if}
</div>
