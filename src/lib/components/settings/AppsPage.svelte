<script lang="ts">
  import { tick } from "svelte";
  import { pickDirectories } from "../../ipc";
  import { addFolders, appsPatch, type Form, type Note, type Save } from "./model";
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
    /** Sends anything waiting, then re-gathers the installed apps. */
    rescan: () => void;
  } = $props();

  let rows: HTMLInputElement[] = $state([]);

  // Like the roots: a folder saves when you leave its field, not per keystroke.
  const saveApps = () => save(appsPatch(form));

  async function addDir() {
    form.appExtraDirs.push("");
    await tick();
    rows[form.appExtraDirs.length - 1]?.focus();
  }

  function removeDir(i: number) {
    form.appExtraDirs.splice(i, 1);
    saveApps();
  }

  async function browse() {
    try {
      const picked = await pickDirectories();
      if (!picked.length) return;
      form.appExtraDirs = addFolders(form.appExtraDirs, picked);
      saveApps();
    } catch (e) {
      note(`${e}`, true);
    }
  }
</script>

<div class="space-y-1">
  <div class="flex items-center gap-2">
    <label class="flex w-fit cursor-pointer items-center gap-2">
      <input
        type="checkbox"
        bind:checked={form.appsEnabled}
        onchange={saveApps}
        class={ui.checkbox}
      />
      <span class={ui.label}>Index installed apps</span>
    </label>
    <span class="text-white/25">— type <span class="font-mono">›</span> in the search bar</span>
  </div>
  <p class="pl-6 {ui.help}">
    Unchecking this turns the <span class="font-mono">›</span> scope off entirely — typing it does
    nothing and the app launcher hotkey won't open it either.
  </p>
</div>

{#if form.appsEnabled}
  <div class="space-y-1.5">
    <span class={ui.label}>Extra folders</span>
    <p class={ui.help}>
      Scanned for portable executables. Start Menu, Store apps and installed programs are found
      automatically, and the list updates on its own when one is installed or removed.
    </p>
    {#each form.appExtraDirs as _d, i (i)}
      <div class="flex items-center gap-2">
        <input
          bind:this={rows[i]}
          bind:value={form.appExtraDirs[i]}
          onchange={saveApps}
          spellcheck="false"
          placeholder="D:\tools"
          class="min-w-0 flex-1 {ui.mono}"
        />
        <button
          type="button"
          onclick={() => removeDir(i)}
          aria-label="Remove folder"
          title="Remove folder"
          class="shrink-0 rounded border border-hair px-2 py-1 text-[11px] text-white/40 hover:bg-white/10 hover:text-white/70"
        >
          ×
        </button>
      </div>
    {/each}
    <div class="flex gap-2">
      <button type="button" onclick={addDir} class={ui.smallBtn}>+ Add folder</button>
      <button type="button" onclick={browse} class={ui.smallBtn}>Browse…</button>
      <button
        type="button"
        onclick={rescan}
        title="Re-gather the installed apps now"
        class={ui.smallBtn}
      >
        Rescan apps
      </button>
    </div>
  </div>
{/if}
