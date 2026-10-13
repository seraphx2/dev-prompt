<script lang="ts">
  import { tick } from "svelte";
  import { pickDirectories } from "../../ipc";
  import {
    TYPING_DELAY_MS,
    addFolders,
    collapsePatch,
    depthPatch,
    rootsPatch,
    ttlPatch,
    type Form,
    type Note,
    type Save,
  } from "./model";
  import * as ui from "./ui";

  let {
    form,
    save,
    note,
    rescan,
  }: {
    form: Form;
    save: Save;
    note: Note;
    /** Sends anything waiting, then rescans the roots. */
    rescan: () => void;
  } = $props();

  /** The root row asking "Remove?", if any. */
  let confirmIdx = $state<number | null>(null);
  let rows: HTMLInputElement[] = $state([]);

  // A root saves when you leave its field or press Enter, not per keystroke —
  // a half-typed path would rescan and re-watch a folder that isn't there.
  const saveRoots = () => save(rootsPatch(form));

  async function addRoot() {
    form.roots.push("");
    await tick();
    rows[form.roots.length - 1]?.focus();
  }

  function removeRoot(i: number) {
    form.roots.splice(i, 1);
    if (!form.roots.length) form.roots.push("");
    confirmIdx = null;
    saveRoots();
  }

  async function browse() {
    try {
      const picked = await pickDirectories();
      if (!picked.length) return;
      const next = addFolders(form.roots, picked);
      form.roots = next.length ? next : [""];
      saveRoots();
    } catch (e) {
      note(`${e}`, true);
    }
  }
</script>

<div class="space-y-1.5">
  <span class={ui.label}
    >Root directories <span class="text-white/25">— scanned recursively for projects</span></span
  >
  {#each form.roots as _root, i (i)}
    <div class="flex items-center gap-2">
      <input
        bind:this={rows[i]}
        bind:value={form.roots[i]}
        onchange={saveRoots}
        spellcheck="false"
        placeholder="D:\git"
        class="min-w-0 flex-1 {ui.mono}"
      />
      {#if confirmIdx === i}
        <button
          type="button"
          onclick={() => removeRoot(i)}
          class="shrink-0 rounded border border-red-400/40 bg-red-500/15 px-2 py-1 text-[11px] text-red-300 hover:bg-red-500/25"
        >
          Remove
        </button>
        <button
          type="button"
          onclick={() => (confirmIdx = null)}
          class="shrink-0 rounded border border-hair px-2 py-1 text-[11px] text-white/50 hover:bg-white/10"
        >
          Cancel
        </button>
      {:else}
        <button
          type="button"
          onclick={() => (form.roots[i].trim() ? (confirmIdx = i) : removeRoot(i))}
          aria-label="Remove from scan list"
          title="Remove from scan list"
          class="shrink-0 rounded border border-hair px-2 py-1 text-[11px] text-white/40 hover:bg-white/10 hover:text-white/70"
        >
          ×
        </button>
      {/if}
    </div>
  {/each}
  <div class="flex gap-2">
    <button type="button" onclick={addRoot} class={ui.smallBtn}>+ Add root</button>
    <button type="button" onclick={browse} class={ui.smallBtn}>Browse…</button>
    <button
      type="button"
      onclick={rescan}
      title="Rescan the root directories now"
      class={ui.smallBtn}
    >
      Rescan
    </button>
  </div>
  <p class={ui.help}>
    The list updates on its own when a repo or folder appears or goes away inside a root.
  </p>
</div>

<div class="flex gap-6">
  <label class="block space-y-1.5">
    <span class={ui.label}>Scan depth</span>
    <input
      type="number"
      min="1"
      max="12"
      bind:value={form.scanDepth}
      oninput={() => save(depthPatch(form), TYPING_DELAY_MS)}
      title="How many directory levels deep the scan looks for repos"
      class="w-24 {ui.input}"
    />
  </label>
  <label class="block space-y-1.5">
    <span class={ui.label}>Cache lifetime (minutes)</span>
    <input
      type="number"
      min="1"
      bind:value={form.ttlMin}
      oninput={() => save(ttlPatch(form), TYPING_DELAY_MS)}
      title="How old the cached repo list can get before dev-prompt rescans it"
      class="w-24 {ui.input}"
    />
  </label>
</div>

<label class="block w-fit space-y-1.5">
  <span class={ui.label}>Repo inside another repo</span>
  <select
    bind:value={form.collapseNested}
    onchange={() => save(collapsePatch(form))}
    title="What to do when a discovered repo sits inside another one"
    class="w-80 {ui.select}"
  >
    <option value="true">Collapse into the parent (default)</option>
    <option value="false">List every one separately</option>
    <option value="auto">Auto — keep independent checkouts only</option>
  </select>
</label>

<div class="space-y-1">
  <label class="flex w-fit cursor-pointer items-center gap-2">
    <input
      type="checkbox"
      bind:checked={form.topLevelFolders}
      onchange={() => save({ top_level_folders: form.topLevelFolders })}
      class={ui.checkbox}
    />
    <span class={ui.label}>List every folder directly inside a root</span>
  </label>
  <p class="pl-6 {ui.help}">
    Even with no project files or version control — notes, documents, a project you haven't set
    up yet. Folders that only hold other repos are left out, and deeper folders still need project
    files.
  </p>
</div>
