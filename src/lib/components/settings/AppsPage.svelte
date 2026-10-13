<script lang="ts">
  import FolderList from "./FolderList.svelte";
  import { appsPatch, cleanList, type Form, type Note, type Save } from "./model";
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
    /** Sends anything waiting, then re-gathers the installed apps. */
    rescan: () => void;
  } = $props();

  /** The extra folders as last saved — the checkbox saves these, never a
   *  row that hasn't been checked yet. */
  // svelte-ignore state_referenced_locally
  let dirs = cleanList(form.appExtraDirs);

  function saveDirs(good: string[]) {
    dirs = good;
    save(appsPatch(form, dirs));
  }
</script>

<div class="space-y-1">
  <div class="flex items-center gap-2">
    <label class="flex w-fit cursor-pointer items-center gap-2">
      <input
        type="checkbox"
        bind:checked={form.appsEnabled}
        onchange={() => save(appsPatch(form, dirs))}
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
    <FolderList
      rows={form.appExtraDirs}
      what="folder"
      placeholder="D:\tools"
      addLabel="+ Add folder"
      onsave={saveDirs}
      {note}
      {report}
    >
      <button
        type="button"
        onclick={rescan}
        title="Re-gather the installed apps now"
        class={ui.smallBtn}
      >
        Rescan apps
      </button>
    </FolderList>
  </div>
{/if}
