<script lang="ts">
  import type { AppEntry } from "../types";
  import AppRow from "./AppRow.svelte";

  let {
    entries,
    selected,
    scanning = false,
    onselect,
    onactivate,
    oncontext,
    shift = false,
    adminWord = "administrator",
  }: {
    entries: { app: AppEntry; positions: number[] }[];
    selected: number;
    /** True while a background enumeration is running and the list is empty. */
    scanning?: boolean;
    onselect: (i: number) => void;
    /** `e` carries Shift: Run as administrator, where the app can be. */
    onactivate: (i: number, e?: MouseEvent) => void;
    /** Row right-clicked -> its context menu. */
    oncontext?: (i: number, e: MouseEvent) => void;
    /** Shift is held: the selected row says it will run as administrator. */
    shift?: boolean;
    adminWord?: string;
  } = $props();

  let container: HTMLDivElement | undefined = $state();

  // Auto-scroll follows keyboard navigation only — see the note in ResultList.
  let skipScroll = false;

  // Re-runs on new entries too, pinning a fresh filter to the top — see ResultList.
  $effect(() => {
    const idx = selected;
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

<div bind:this={container} class="scroll-thin flex-1 overflow-y-auto px-2 py-2">
  {#if entries.length === 0}
    <div class="px-3 py-8 text-center text-[13px] text-white/45">
      {scanning ? "Scanning apps…" : "No apps match."}
    </div>
  {:else}
    {#each entries as e, i (e.app.exec)}
      <div data-idx={i} oncontextmenu={(e) => oncontext?.(i, e)} role="presentation">
        <AppRow
          app={e.app}
          positions={e.positions}
          active={i === selected}
          elevating={shift && i === selected && e.app.kind === "exe"}
          {adminWord}
          onactivate={(ev) => onactivate(i, ev)}
          onhover={() => {
            if (i !== selected) skipScroll = true;
            onselect(i);
          }}
        />
      </div>
    {/each}
  {/if}
</div>
