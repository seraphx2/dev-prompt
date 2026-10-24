<script module lang="ts">
  export interface BannerAction {
    label: string;
    /** A key that does the same, shown on the button ("Ctrl+Z"). */
    kbd?: string;
    onclick: () => void;
    /** The one to reach for: sky; the rest are quiet. */
    primary?: boolean;
    /** It stops or throws something away: red. */
    danger?: boolean;
  }
</script>

<script lang="ts">
  import type { Snippet } from "svelte";

  // A bar above the footer for something going on in the background: what it
  // is, how it's coming along, and what can be done about it. Undo after a
  // move to the bin counts down; a clone in progress shows a moving bar.
  let {
    tone = "info",
    countdown = null,
    busy = false,
    actions = [],
    children,
  }: {
    /** "error": something went wrong — red, not sky. */
    tone?: "info" | "error";
    /** Counting down to `until` (a Date.now() time) over `total` ms: the
     *  seconds left, and a line draining along the top edge. */
    countdown?: { until: number; total: number } | null;
    /** Under way with no end in sight: a line sweeping along the top edge. */
    busy?: boolean;
    actions?: BannerAction[];
    children: Snippet;
  } = $props();

  let left = $state(0);
  $effect(() => {
    const c = countdown;
    if (!c) return;
    const tick = () => (left = Math.max(0, Math.ceil((c.until - Date.now()) / 1000)));
    tick();
    const every = setInterval(tick, 250);
    return () => clearInterval(every);
  });
</script>

<div
  class="relative flex items-center gap-3 border-t border-hair px-4 py-2 text-[12px]
         {tone === 'error' ? 'bg-red-500/10 text-red-200' : 'bg-white/4 text-white/70'}"
>
  <span class="min-w-0 flex-1 truncate">{@render children()}</span>
  {#if countdown}
    <span class="shrink-0 tabular-nums text-white/50">{left}s</span>
    <!-- Keyed so a new countdown restarts the line. -->
    {#key countdown.until}
      <span
        class="drain absolute top-0 left-0 h-px bg-sky-400/60"
        style="animation-duration: {Math.max(0, countdown.until - Date.now())}ms"
        aria-hidden="true"
      ></span>
    {/key}
  {:else if busy}
    <span class="absolute inset-x-0 top-0 h-px overflow-hidden" aria-hidden="true">
      <span class="sweep absolute top-0 h-px w-1/4 bg-sky-400/70"></span>
    </span>
  {/if}
  {#each actions as a (a.label)}
    <button
      type="button"
      onclick={a.onclick}
      class="inline-flex shrink-0 items-center gap-1.5 rounded-sm px-2 py-0.5 text-[12px]
             {a.danger
        ? 'border border-red-400/50 bg-red-500/15 text-red-200 hover:bg-red-500/25'
        : a.primary
          ? 'border border-sky-400/50 bg-sky-400/10 text-sky-200 hover:bg-sky-400/20'
          : 'border border-hair text-white/60 hover:bg-white/10 hover:text-white/85'}"
    >
      {a.label}{#if a.kbd}<kbd>{a.kbd}</kbd>{/if}
    </button>
  {/each}
</div>

<style>
  kbd {
    font-family: inherit;
    font-size: 10px;
    line-height: 1;
    color: rgb(255 255 255 / 0.9);
    background: rgb(255 255 255 / 0.09);
    border: 1px solid rgb(255 255 255 / 0.12);
    border-radius: 3px;
    padding: 2px 4px;
  }

  /* Full width to nothing over the countdown. */
  @keyframes drain {
    from {
      width: 100%;
    }
    to {
      width: 0%;
    }
  }
  .drain {
    animation-name: drain;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }

  /* A short segment travelling left to right, again and again. */
  @keyframes sweep {
    from {
      left: -25%;
    }
    to {
      left: 100%;
    }
  }
  .sweep {
    animation: sweep 1.4s ease-in-out infinite;
  }

  /* Reduced motion: the seconds left say it; a still line for "busy". */
  @media (prefers-reduced-motion: reduce) {
    .drain {
      display: none;
    }
    .sweep {
      animation: none;
      left: 0;
      width: 100%;
      opacity: 0.4;
    }
  }
</style>
