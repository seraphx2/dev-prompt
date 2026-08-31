<script lang="ts">
  import type { Action } from "../types";

  let {
    repoName,
    actions,
    selected,
    onselect,
    onrun,
    onback,
  }: {
    repoName: string;
    actions: Action[];
    selected: number;
    onselect: (i: number) => void;
    onrun: (i: number) => void;
    onback: () => void;
  } = $props();
</script>

<div class="flex items-center gap-2 border-b border-hair px-4 py-3">
  <button
    type="button"
    class="rounded px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
    onclick={onback}
    aria-label="Back to repository list"
  >
    ←
  </button>
  <span class="text-[13px] font-medium text-white/80">{repoName}</span>
  <span class="text-[12px] text-white/30">— choose an action</span>
</div>

<div class="scroll-thin flex-1 overflow-y-auto px-2 py-2">
  {#each actions as action, i (action.id)}
    <button
      type="button"
      data-idx={i}
      class="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors
             {i === selected ? 'bg-white/10' : 'hover:bg-white/[0.04]'}"
      onclick={() => onselect(i)}
      ondblclick={() => onrun(i)}
    >
      <span class="flex-1 truncate text-[13px] text-white/90">{action.label}</span>
      <span class="shrink-0 truncate font-mono text-[11px] text-white/35"
        >{action.hint}</span
      >
    </button>
  {/each}
</div>
