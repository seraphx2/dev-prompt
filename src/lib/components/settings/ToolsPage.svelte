<script lang="ts">
  import { onMount } from "svelte";
  import {
    autoTerminal,
    defaultShell,
    listFileManagers,
    listShells,
    listTerminals,
  } from "../../ipc";
  import type { FileManagerOption, TerminalOption } from "../../types";
  import {
    CUSTOM,
    TYPING_DELAY_MS,
    filemanagerPatch,
    terminalPatch,
    type Form,
    type Save,
  } from "./model";
  import * as ui from "./ui";

  let { form, save }: { form: Form; save: Save } = $props();

  let terminals = $state<TerminalOption[]>([]);
  /** What "Auto" opens right now — names the dropdown's Auto option. */
  let autoTerminalLabel = $state("");
  let shells = $state<string[]>([]);
  /** What "Default" runs right now. */
  let defaultShellLabel = $state("");
  let filemanagers = $state<FileManagerOption[]>([]);

  onMount(() => {
    void listTerminals().then((t) => (terminals = t), () => {});
    void autoTerminal().then((t) => (autoTerminalLabel = t), () => {});
    void listShells().then((s) => (shells = s), () => {});
    void defaultShell().then((s) => (defaultShellLabel = s), () => {});
    void listFileManagers().then((f) => (filemanagers = f), () => {});
  });

  // Picking Custom keeps the current choice until there's a template to save.
  function pick(which: "terminal" | "filemanager") {
    if (which === "terminal") {
      if (form.terminalSel !== CUSTOM || form.terminalTemplate.trim()) save(terminalPatch(form));
    } else if (form.filemanagerSel !== CUSTOM || form.filemanagerTemplate.trim()) {
      save(filemanagerPatch(form));
    }
  }
</script>

<label class="block space-y-1.5">
  <span class={ui.label}>Terminal</span>
  <select
    bind:value={form.terminalSel}
    onchange={() => pick("terminal")}
    title="Which terminal emulator dev-prompt opens for terminal actions"
    class="w-80 {ui.select}"
  >
    <option value="">Auto{autoTerminalLabel ? ` (${autoTerminalLabel})` : ""}</option>
    {#each terminals as t (t.id)}
      <option value={t.id}>{t.label}</option>
    {/each}
    {#if form.terminalSel && form.terminalSel !== CUSTOM && !terminals.some((t) => t.id === form.terminalSel)}
      <option value={form.terminalSel}>{form.terminalSel}</option>
    {/if}
    <option value={CUSTOM}>Custom…</option>
  </select>
  {#if form.terminalSel === CUSTOM}
    <input
      bind:value={form.terminalTemplate}
      oninput={() => save(terminalPatch(form), TYPING_DELAY_MS)}
      spellcheck="false"
      placeholder="wezterm start --cwd {'{{dir}}'} -- {'{{cmd}}'}"
      class="w-full {ui.mono}"
    />
    <span class="block {ui.help}">
      <span class="font-mono">{"{{dir}}"}</span> = working directory,
      <span class="font-mono">{"{{cmd}}"}</span> = the command to run.
    </span>
  {/if}
</label>

<label class="block space-y-1.5">
  <span class={ui.label}>Shell</span>
  <select
    bind:value={form.shellSel}
    onchange={() => save({ shell: form.shellSel })}
    title="Shell a one-shot terminal command runs inside"
    class="w-80 {ui.select}"
  >
    <option value="">Default{defaultShellLabel ? ` (${defaultShellLabel})` : ""}</option>
    {#each shells as s (s)}
      <option value={s}>{s}</option>
    {/each}
    {#if form.shellSel && !shells.includes(form.shellSel)}
      <option value={form.shellSel}>{form.shellSel}</option>
    {/if}
  </select>
</label>

<label class="block space-y-1.5">
  <span class={ui.label}>File manager</span>
  <select
    bind:value={form.filemanagerSel}
    onchange={() => pick("filemanager")}
    title="Which file manager 'Show in file manager' opens"
    class="w-80 {ui.select}"
  >
    <option value="">Auto{filemanagers[0] ? ` (${filemanagers[0].label})` : ""}</option>
    {#each filemanagers as f (f.id)}
      <option value={f.id}>{f.label}</option>
    {/each}
    {#if form.filemanagerSel && form.filemanagerSel !== CUSTOM && !filemanagers.some((f) => f.id === form.filemanagerSel)}
      <option value={form.filemanagerSel}>{form.filemanagerSel}</option>
    {/if}
    <option value={CUSTOM}>Custom…</option>
  </select>
  {#if form.filemanagerSel === CUSTOM}
    <input
      bind:value={form.filemanagerTemplate}
      oninput={() => save(filemanagerPatch(form), TYPING_DELAY_MS)}
      spellcheck="false"
      placeholder="dopus /cmd Go {'{{path}}'}"
      class="w-full {ui.mono}"
    />
    <span class="block {ui.help}">
      <span class="font-mono">{"{{path}}"}</span> = the folder to open.
    </span>
  {/if}
</label>
