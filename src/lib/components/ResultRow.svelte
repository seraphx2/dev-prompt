<script lang="ts">
  import type { ScoredRepo } from "../types";

  let {
    entry,
    active,
    onclick,
    ondblclick,
  }: {
    entry: ScoredRepo;
    active: boolean;
    onclick: () => void;
    ondblclick: () => void;
  } = $props();

  const hit = $derived(new Set(entry.matchIndices));
  // Highlight matched characters in the repo name.
  const name = $derived(entry.repo.name);
</script>

<button
  type="button"
  class="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors
         {active ? 'bg-white/10' : 'hover:bg-white/[0.04]'}"
  {onclick}
  {ondblclick}
>
  <div class="min-w-0 flex-1">
    <div class="truncate text-[13px] leading-tight text-white/90">
      {#each name.split("") as ch, i}<span
          class={hit.has(i) ? "text-sky-300" : ""}>{ch}</span
        >{/each}
    </div>
    <div class="truncate font-mono text-[11px] leading-tight text-white/35">
      {entry.repo.path}
    </div>
  </div>
  <div class="flex shrink-0 gap-1">
    {#each entry.repo.sentinels.slice(0, 3) as s}
      <span
        class="rounded bg-white/[0.06] px-1.5 py-0.5 font-mono text-[10px] text-white/40"
        >{s}</span
      >
    {/each}
  </div>
</button>
