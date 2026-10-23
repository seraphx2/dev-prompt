<script lang="ts">
  import type { ScoredRepo } from "../types";
  import ResultRow from "./ResultRow.svelte";

  let {
    entries,
    selected,
    onselect,
    onactivate,
    oncontext,
    empty = "No repositories match.",
  }: {
    entries: ScoredRepo[];
    selected: number;
    /** Pointer moved onto a row -> move the selection there. */
    onselect: (i: number) => void;
    /** Row clicked -> open that repo's actions. */
    onactivate: (i: number) => void;
    /** Row right-clicked -> its context menu. */
    oncontext?: (i: number, e: MouseEvent) => void;
    /** What an empty list says. */
    empty?: string;
  } = $props();

  let container: HTMLDivElement | undefined = $state();

  // Auto-scroll follows keyboard navigation only. A pointer hover also moves the
  // selection, but that row is already under the cursor — scrolling it would
  // shift the list under the mouse and trigger a hover/scroll feedback loop.
  let skipScroll = false;

  // Also re-runs when the entries change: a fresh search lands with the top
  // match selected, and the list pins to the very top (padding included)
  // rather than wherever the previous result set had been scrolled to.
  $effect(() => {
    const idx = selected; // track the selection
    void entries;
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

<div
  bind:this={container}
  class="scroll-thin flex-1 overflow-y-auto px-2 py-2"
>
  {#if entries.length === 0}
    <div class="px-3 py-8 text-center text-[13px] text-white/45">
      {empty}
    </div>
  {:else}
    {#each entries as entry, i (entry.repo.path)}
      <div data-idx={i} oncontextmenu={(e) => oncontext?.(i, e)} role="presentation">
        <ResultRow
          {entry}
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
