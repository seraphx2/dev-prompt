<script lang="ts">
  import type { AppEntry } from "../types";
  import { middleTruncate } from "../text";
  import { icons } from "../icons";
  import Highlight from "./Highlight.svelte";

  let {
    app,
    positions,
    active,
    onactivate,
    onhover,
  }: {
    app: AppEntry;
    /** Character indices in `app.name` the query matched. */
    positions: number[];
    active: boolean;
    onactivate: () => void;
    onhover: () => void;
  } = $props();

  const fallback = icons.app;
  const sub = $derived(
    app.kind === "aumid" ? "Store app" : middleTruncate(app.exec, 72),
  );
</script>

<button
  type="button"
  class="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors
         {active ? 'bg-white/10' : 'hover:bg-white/[0.04]'}"
  onclick={onactivate}
  onpointerenter={onhover}
>
  {#if app.icon}
    <img src={app.icon} alt="" class="h-5 w-5 shrink-0 rounded-[3px]" />
  {:else}
    <svg
      class="h-5 w-5 shrink-0 text-white/30"
      viewBox={fallback.vb ?? "0 0 24 24"}
      fill="currentColor"
    >
      {#if fallback.raw}{@html fallback.raw}{:else}<path d={fallback.d} />{/if}
    </svg>
  {/if}
  <div class="min-w-0 flex-1">
    <div class="truncate text-[13px] leading-tight text-white/90">
      <Highlight text={app.name} indices={positions} />
    </div>
    <div
      class="overflow-hidden whitespace-nowrap font-mono text-[11px] leading-tight text-white/35"
      title={app.exec}
    >
      {sub}
    </div>
  </div>
  {#if app.uses > 0}
    <span class="shrink-0 font-mono text-[10px] text-white/25">{app.uses}×</span>
  {/if}
</button>
