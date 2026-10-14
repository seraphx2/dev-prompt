<script lang="ts">
  import type { ShellSession } from "../shells.svelte";

  let {
    session,
    visible,
    repoName,
    crumb,
    onback,
    onshellmenu,
    oncontext,
    onend,
  }: {
    session: ShellSession | undefined;
    visible: boolean;
    repoName: string;
    /** The sub-folder the shell runs in, when it isn't the repo root. */
    crumb: string | null;
    onback: () => void;
    /** The shell name was clicked — offer the other shells. */
    onshellmenu: (e: MouseEvent) => void;
    oncontext: (e: MouseEvent) => void;
    onend: () => void;
  } = $props();

  let host: HTMLDivElement | undefined = $state();

  /** Put the cursor back in the shell (after a menu closes, say). */
  export function focus() {
    session?.focus();
  }

  // Mount the active session's terminal whenever this screen shows or the
  // session changes. Each session keeps its own element, so switching never
  // loses scrollback.
  $effect(() => {
    if (!visible || !session || !host) return;
    const s = session;
    const el = host;
    // After layout: the terminal sizes itself from the host's box.
    requestAnimationFrame(() => s.show(el));
  });

  // Refit as the window grows or shrinks (the overlay resizes around a shell).
  $effect(() => {
    if (!host) return;
    let frame = 0;
    const ro = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => session?.fit());
    });
    ro.observe(host);
    return () => {
      cancelAnimationFrame(frame);
      ro.disconnect();
    };
  });
</script>

<div class="min-h-0 flex-1 flex-col" class:flex={visible} class:hidden={!visible}>
  <div class="flex items-center gap-2 border-b border-hair px-3 py-2.5">
    <button
      type="button"
      class="shrink-0 rounded-sm px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
      onclick={onback}
      aria-label="Back"
      title="Back (Shift+Esc)"
    >
      ←
    </button>
    <span class="shrink-0 truncate text-[13px] font-medium text-white/80">{repoName}</span>
    {#if crumb}
      <span class="shrink-0 text-white/25">›</span>
      <span class="shrink-0 truncate text-[13px] font-medium text-orange-300">{crumb}</span>
    {/if}
    <span class="shrink-0 text-white/15">/</span>
    <button
      type="button"
      class="inline-flex shrink-0 items-center gap-1 rounded-sm border border-hair bg-white/4 px-1.5 py-0.5
             font-mono text-[12px] text-white/80 hover:bg-white/10"
      onclick={onshellmenu}
      title="Switch shell"
    >
      {session?.label || session?.shell || "shell"}
      <svg class="h-3 w-3 text-white/45" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M3 4.5 6 7.5 9 4.5" />
      </svg>
    </button>
    <span class="min-w-0 flex-1 truncate text-right text-[11px] text-white/35">
      {#if session?.state === "starting"}
        starting…
      {:else if session?.state === "exited"}
        <span class="text-amber-300/80">
          {session.error ? "couldn't start" : `exited${session.exitCode === null ? "" : ` (${session.exitCode})`}`}
        </span>
      {/if}
    </span>
    <!-- Stops the shell — unlike ← / Shift+Esc, which leave it running. -->
    <button
      type="button"
      class="inline-flex shrink-0 items-center gap-1.5 rounded-sm border border-red-400/30 bg-red-400/8
             px-2 py-0.5 text-[12px] text-red-300/90 transition-colors
             hover:border-red-400/60 hover:bg-red-400/20 hover:text-red-200"
      onclick={onend}
      title="Stop this shell and anything running in it (a dev server, a build). ← or Shift+Esc leaves it running instead."
    >
      <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6"
        stroke-linecap="round" aria-hidden="true">
        <path d="M3 3l6 6M9 3l-6 6" />
      </svg>
      Close shell
    </button>
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={host}
    class="min-h-0 flex-1 py-1.5 pl-3 pr-1"
    oncontextmenu={(e) => {
      e.preventDefault();
      oncontext(e);
    }}
  ></div>
</div>
