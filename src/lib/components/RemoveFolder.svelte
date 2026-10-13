<script lang="ts">
  import { onMount, tick } from "svelte";
  import { assessRemoval, removeFolder } from "../ipc";
  import type { RemovalAssessment } from "../types";

  let {
    name,
    path,
    ondone,
    onback,
  }: {
    name: string;
    path: string;
    /** Moved; `undoable` says whether Undo can bring it back. */
    ondone: (undoable: boolean) => void;
    onback: () => void;
  } = $props();

  const bin =
    document.documentElement.dataset.os === "windows" ? "Recycle Bin" : "Trash";

  let assessment = $state<RemovalAssessment | null>(null);
  let checkError = $state("");
  let typed = $state("");
  let busy = $state(false);
  let error = $state("");
  let typeInput: HTMLInputElement | undefined = $state();
  let confirmButton: HTMLButtonElement | undefined = $state();

  const confirmed = $derived(
    !!assessment && (!assessment.needsTyped || typed === assessment.name),
  );

  onMount(async () => {
    try {
      assessment = await assessRemoval(path);
    } catch (e) {
      checkError = `${e}`;
    }
    await tick();
    if (assessment?.needsTyped) typeInput?.focus();
    else confirmButton?.focus();
  });

  async function confirm() {
    if (!confirmed || busy) return;
    busy = true;
    error = "";
    try {
      ondone(await removeFolder(path));
    } catch (e) {
      error = `${e}`;
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.stopPropagation();
      // Enter on Cancel is a click on Cancel.
      if ((e.target as HTMLElement | null)?.closest("[data-cancel]")) return;
      e.preventDefault();
      void confirm();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (!busy) onback();
    }
  }
</script>

<div class="flex items-center gap-2 border-b border-hair px-3 py-2.5">
  <button
    type="button"
    class="shrink-0 rounded-sm px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
    onclick={() => !busy && onback()}
    aria-label="Back"
  >
    ←
  </button>
  <span class="shrink-0 truncate text-[13px] font-medium text-white/80">{name}</span>
  <span class="shrink-0 text-white/15">/</span>
  <span class="text-[13px] text-white/60">Move to {bin}</span>
</div>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="flex min-h-0 flex-1 flex-col overflow-y-auto px-5 py-4 text-[13px]" onkeydown={onKey}>
  <p class="font-mono text-[12px] text-white/60">{path}</p>

  {#if checkError}
    <p class="mt-4 text-red-300">{checkError}</p>
  {:else if !assessment}
    <p class="mt-4 text-white/40">Checking what's in it…</p>
  {:else}
    {#if assessment.needsTyped}
      <div class="mt-4 space-y-2 rounded-sm border border-amber-400/40 bg-amber-500/10 px-3 py-2.5">
        <p class="font-medium text-amber-200">This has work that exists only here:</p>
        <ul class="list-disc space-y-1 pl-5 text-amber-100/90">
          {#each assessment.risks as r (r)}
            <li>{r}</li>
          {/each}
          {#each assessment.unknown as u (u)}
            <li class="text-amber-100/70">{u}</li>
          {/each}
        </ul>
      </div>
      <label class="mt-4 block space-y-1.5">
        <span class="text-white/70"
          >Type <span class="font-mono text-white">{assessment.name}</span> to confirm</span
        >
        <input
          bind:this={typeInput}
          bind:value={typed}
          disabled={busy}
          spellcheck="false"
          autocomplete="off"
          class="w-72 rounded-sm border border-hair bg-white/4 px-2 py-1.5 font-mono text-[12px]
                 text-white/90 focus:border-white/25 focus:outline-hidden"
        />
      </label>
    {:else}
      <p class="mt-4 text-white/70">
        Everything in it is committed{assessment.vcs ? ` (${assessment.vcs})` : ""} and on a
        remote, so nothing exists only here.
      </p>
    {/if}
    <p class="mt-3 text-[12px] text-white/40">
      It goes to the {bin}, so it can be restored — Undo appears right after.
    </p>
  {/if}

  {#if error}
    <pre
      class="mt-3 max-h-32 overflow-auto whitespace-pre-wrap rounded-sm border border-red-400/40 bg-red-500/10 px-3 py-2 font-mono text-[11px] text-red-200">{error}</pre>
  {/if}

  <div class="mt-auto flex justify-end gap-2 pt-4">
    <button
      type="button"
      data-cancel
      disabled={busy}
      onclick={onback}
      class="rounded-sm border border-hair px-3 py-1.5 text-[12px] text-white/60 hover:bg-white/10 hover:text-white/90"
    >
      Cancel
    </button>
    <button
      bind:this={confirmButton}
      type="button"
      disabled={!confirmed || busy}
      onclick={() => void confirm()}
      class="rounded-sm bg-red-500/80 px-3.5 py-1.5 text-[12px] font-medium text-white hover:bg-red-500
             disabled:opacity-40"
    >
      {busy ? "Moving…" : `Move to ${bin}`}
    </button>
  </div>
</div>
