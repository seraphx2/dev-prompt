<script lang="ts">
  import type { ScoredRepo } from "../types";
  import { middleTruncate } from "../text";
  import Highlight from "./Highlight.svelte";
  import { hasShellUnder } from "../shells.svelte";
  import { icons } from "../icons";

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

  /** A shell is still running in this repo (Open shell, left in the background). */
  const shellLive = $derived(hasShellUnder(entry.repo.path));
</script>

<!-- `pointermove`, not `pointerenter`: the latter also fires when a row slides
     under a resting cursor as the results reshuffle while typing, which would
     drag the highlight off the top match. Only real pointer movement selects. -->
<button
  type="button"
  class="flex w-full items-center gap-3 rounded-lg py-2 pl-1 pr-3 text-left transition-colors
         {active ? 'bg-white/10' : 'hover:bg-white/4'}"
  onclick={onactivate}
  onpointermove={onhover}
>
  <!-- Live state gets its own column, apart from the badges on the right
       (which describe what the repo is). Every row reserves it, so names stay
       lined up and running shells read as one vertical column. The row's
       slim left padding (pl-1) plus the list's px-2 puts 12px before the icon,
       matching the 12px gap after it, so it sits centred in that margin. -->
  <!-- Native tooltip: plain text only, but a newline breaks the line. -->
  <span
    class="flex w-3.5 shrink-0 justify-center"
    title={shellLive ? "A shell is running in this repo\nOpen shell resumes the session" : undefined}
  >
    {#if shellLive}
      <svg
        class="h-3.5 w-3.5 text-sky-300"
        viewBox="0 0 24 24"
        fill="currentColor"
        fill-rule="evenodd"
        role="img"
        aria-label="A shell is running in this repo"
      >
        {@html icons.terminal.raw}
      </svg>
    {/if}
  </span>
  <div class="min-w-0 flex-1">
    <div class="truncate text-[13px] leading-tight text-white/90">
      <Highlight text={entry.repo.name} indices={entry.matchIndices} />
    </div>
    <div
      class="overflow-hidden whitespace-nowrap font-mono text-[11px] leading-tight text-white/35"
      title={entry.repo.path}
    >
      {middleTruncate(entry.repo.path, 100)}
    </div>
  </div>
  <div class="flex shrink-0 items-center gap-1">
    {#if entry.repo.vcs}
      <span
        class="rounded-sm border border-emerald-400/25 bg-emerald-400/10 px-1.5 py-0.5 text-[10px] font-medium text-emerald-200/90"
        >{entry.repo.vcs}</span
      >
    {/if}
    {#each entry.repo.sentinels.slice(0, 5) as s}
      <span
        class="rounded-sm bg-white/6 px-1.5 py-0.5 font-mono text-[10px] text-white/40"
        >{s}</span
      >
    {/each}
  </div>
</button>
