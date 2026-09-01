<script lang="ts">
  import type { ScoredRepo } from "../types";
  import { middleTruncate } from "../text";
  import Highlight from "./Highlight.svelte";

  let {
    entry,
    active,
    onactivate,
    onhover,
  }: {
    entry: ScoredRepo;
    active: boolean;
    /** Row clicked -> open this repo's actions. */
    onactivate: () => void;
    /** Pointer moved onto the row -> move the selection here. */
    onhover: () => void;
  } = $props();
</script>

<button
  type="button"
  class="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors
         {active ? 'bg-white/10' : 'hover:bg-white/[0.04]'}"
  onclick={onactivate}
  onpointerenter={onhover}
>
  <div class="min-w-0 flex-1">
    <div class="truncate text-[13px] leading-tight text-white/90">
      <Highlight text={entry.repo.name} indices={entry.matchIndices} />
    </div>
    <div
      class="overflow-hidden whitespace-nowrap font-mono text-[11px] leading-tight text-white/35"
      title={entry.repo.path}
    >
      {middleTruncate(entry.repo.path, 72)}
    </div>
  </div>
  <div class="flex shrink-0 items-center gap-1">
    {#if entry.repo.vcs}
      <span
        class="rounded border border-emerald-400/25 bg-emerald-400/10 px-1.5 py-0.5 text-[10px] font-medium text-emerald-200/90"
        >{entry.repo.vcs}</span
      >
    {/if}
    {#each entry.repo.sentinels.slice(0, 3) as s}
      <span
        class="rounded bg-white/[0.06] px-1.5 py-0.5 font-mono text-[10px] text-white/40"
        >{s}</span
      >
    {/each}
  </div>
</button>
