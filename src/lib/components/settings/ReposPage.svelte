<script lang="ts">
  import { onDestroy } from "svelte";
  import FolderList from "./FolderList.svelte";
  import {
    DEPTH_MAX,
    TYPING_DELAY_MS,
    collapsePatch,
    depthPatch,
    depthProblem,
    rootsPatch,
    ttlPatch,
    ttlProblem,
    type Form,
    type Note,
    type Save,
  } from "./model";
  import * as ui from "./ui";

  let {
    form,
    save,
    note,
    report,
    rescan,
  }: {
    form: Form;
    save: Save;
    note: Note;
    report: (msg: string) => void;
    /** Sends anything waiting, then rescans the roots. */
    rescan: () => void;
  } = $props();

  // A number saves as you type once it's valid. An invalid one is never
  // saved, and leaving the field puts the last good value back.
  // svelte-ignore state_referenced_locally
  let good = { depth: form.scanDepth, ttl: form.ttlMin };
  let depthErr = $state("");
  let ttlErr = $state("");

  function depthInput() {
    depthErr = depthProblem(form.scanDepth) ?? "";
    if (!depthErr) {
      good.depth = form.scanDepth;
      save(depthPatch(form), TYPING_DELAY_MS);
    }
  }
  function ttlInput() {
    ttlErr = ttlProblem(form.ttlMin) ?? "";
    if (!ttlErr) {
      good.ttl = form.ttlMin;
      save(ttlPatch(form), TYPING_DELAY_MS);
    }
  }
  function depthBlur() {
    if (depthErr) {
      form.scanDepth = good.depth;
      depthErr = "";
    }
  }
  function ttlBlur() {
    if (ttlErr) {
      form.ttlMin = good.ttl;
      ttlErr = "";
    }
  }

  onDestroy(() => {
    if (depthErr) report(`Didn't save scan depth — ${depthErr}`);
    if (ttlErr) report(`Didn't save cache lifetime — ${ttlErr}`);
  });
</script>

<div class="space-y-1.5">
  <span class={ui.label}
    >Root directories <span class="text-white/25">— scanned recursively for projects</span></span
  >
  <FolderList
    rows={form.roots}
    what="root"
    placeholder="D:\git"
    addLabel="+ Add root"
    confirmRemove
    keepOneRow
    onsave={(roots) => save(rootsPatch(roots))}
    {note}
    {report}
  >
    <button
      type="button"
      onclick={rescan}
      title="Rescan the root directories now"
      class={ui.smallBtn}
    >
      Rescan
    </button>
  </FolderList>
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
      max={DEPTH_MAX}
      bind:value={form.scanDepth}
      oninput={depthInput}
      onblur={depthBlur}
      aria-invalid={!!depthErr}
      title="How many directory levels deep the scan looks for repos"
      class="w-24 {ui.input} {depthErr ? 'border-red-400/60!' : ''}"
    />
    {#if depthErr}<span class="block text-[11px] text-red-300">{depthErr}</span>{/if}
  </label>
  <label class="block space-y-1.5">
    <span class={ui.label}>Cache lifetime (minutes)</span>
    <input
      type="number"
      min="1"
      bind:value={form.ttlMin}
      oninput={ttlInput}
      onblur={ttlBlur}
      aria-invalid={!!ttlErr}
      title="How old the cached repo list can get before dev-prompt rescans it"
      class="w-24 {ui.input} {ttlErr ? 'border-red-400/60!' : ''}"
    />
    {#if ttlErr}<span class="block text-[11px] text-red-300">{ttlErr}</span>{/if}
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
