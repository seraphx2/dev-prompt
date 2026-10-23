<script lang="ts">
  import { icons } from "../icons";
  import { middleTruncate } from "../text";
  import { shellCrumb, type ShellSession } from "../shells.svelte";
  import Highlight from "./Highlight.svelte";

  let {
    entries,
    total,
    selected,
    onselect,
    onactivate,
    onclose,
    oncloseall,
    oncontext,
  }: {
    entries: { session: ShellSession; positions: number[] }[];
    /** Every shell, filtered or not — what "Close all" closes. */
    total: number;
    selected: number;
    onselect: (i: number) => void;
    /** Row clicked -> go into that shell. */
    onactivate: (i: number) => void;
    /** A row's ✕ -> close that shell. */
    onclose: (i: number) => void;
    /** Close every shell, not just the filtered ones. */
    oncloseall: () => void;
    oncontext?: (i: number, e: MouseEvent) => void;
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

{#if total}
  <div class="flex items-center gap-3 border-b border-hair px-4 py-1.5 text-[11px] text-white/40">
    <span class="min-w-0 flex-1 truncate">
      {total} shell{total === 1 ? "" : "s"} — they keep running until closed
    </span>
    <button
      type="button"
      class="inline-flex shrink-0 items-center gap-1.5 rounded-sm border border-red-400/30 bg-red-400/8
             px-2 py-0.5 text-[11px] text-red-300/90 transition-colors
             hover:border-red-400/60 hover:bg-red-400/20 hover:text-red-200"
      onclick={oncloseall}
      title="Close every shell and anything running in them (Ctrl+Shift+W)"
    >
      Close all
    </button>
  </div>
{/if}

<div bind:this={container} class="scroll-thin flex-1 overflow-y-auto px-2 py-2">
  {#if total === 0}
    <div class="px-3 py-8 text-center text-[13px] leading-relaxed text-white/45">
      No shells running.<br />Open shell (<kbd class="font-mono">Ctrl+R</kbd> in a repo's actions) starts one.
    </div>
  {:else if entries.length === 0}
    <div class="px-3 py-8 text-center text-[13px] text-white/45">No shells match.</div>
  {:else}
    {#each entries as { session: s, positions }, i (s.cwd)}
      {@const crumb = shellCrumb(s)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        data-idx={i}
        class="group flex w-full items-center gap-3 rounded-lg py-2 pl-1 pr-2 transition-colors
               {i === selected ? 'bg-white/10' : 'hover:bg-white/4'}"
        oncontextmenu={(e) => oncontext?.(i, e)}
        onpointermove={() => {
          // Only real pointer movement selects — see ResultRow.
          if (i !== selected) skipScroll = true;
          onselect(i);
        }}
      >
        <button
          type="button"
          class="flex min-w-0 flex-1 items-center gap-3 text-left"
          onclick={() => onactivate(i)}
        >
          <span class="flex w-3.5 shrink-0 justify-center">
            <svg
              class="h-3.5 w-3.5 {s.state === 'exited' ? 'text-amber-300/70' : 'text-sky-300'}"
              viewBox="0 0 24 24"
              fill="currentColor"
              fill-rule="evenodd"
              aria-hidden="true"
            >
              {@html icons.terminal.raw}
            </svg>
          </span>
          <span class="min-w-0 flex-1">
            <span class="block truncate text-[13px] leading-tight text-white/90">
              <Highlight text={s.repo.name} indices={positions} />
              {#if crumb}
                <span class="text-white/25">›</span>
                <span class="text-orange-300">{crumb}</span>
              {/if}
            </span>
            <span
              class="block overflow-hidden whitespace-nowrap font-mono text-[11px] leading-tight text-white/50"
              title={s.cwd}
            >
              {middleTruncate(s.cwd, 100)}
            </span>
          </span>
        </button>
        {#if s.state === "exited"}
          <span class="shrink-0 text-[11px] text-amber-300/80">
            {s.error ? "couldn't start" : `exited${s.exitCode === null ? "" : ` (${s.exitCode})`}`}
          </span>
        {/if}
        <span class="shrink-0 rounded-sm bg-white/6 px-1.5 py-0.5 font-mono text-[10px] text-white/50">
          {s.label || s.shell || "shell"}
        </span>
        <button
          type="button"
          class="shrink-0 rounded-sm p-1 text-white/45 transition-colors hover:bg-red-400/20 hover:text-red-200"
          onclick={() => onclose(i)}
          title="Close this shell (Ctrl+W)"
          aria-label="Close shell in {s.repo.name}"
        >
          <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6"
            stroke-linecap="round" aria-hidden="true">
            <path d="M3 3l6 6M9 3l-6 6" />
          </svg>
        </button>
      </div>
    {/each}
  {/if}
</div>
