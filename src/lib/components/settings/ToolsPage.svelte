<script lang="ts">
  import { onDestroy, onMount } from "svelte";
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
    FONT_SIZE_MAX,
    FONT_SIZE_MIN,
    TYPING_DELAY_MS,
    filemanagerPatch,
    fontSizeProblem,
    shellFontPatch,
    templateProblem,
    terminalPatch,
    type Form,
    type Save,
  } from "./model";
  import * as ui from "./ui";

  let {
    form,
    save,
    report,
  }: { form: Form; save: Save; report: (msg: string) => void } = $props();

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

  type Kind = "terminal" | "filemanager";
  const LABEL: Record<Kind, string> = { terminal: "terminal", filemanager: "file manager" };
  const custom = (k: Kind) => (k === "terminal" ? form.terminalSel : form.filemanagerSel) === CUSTOM;
  const template = (k: Kind) => (k === "terminal" ? form.terminalTemplate : form.filemanagerTemplate);
  const patch = (k: Kind) => (k === "terminal" ? terminalPatch(form) : filemanagerPatch(form));

  /** Why the custom template isn't saved, per kind ("" = it is, or isn't in use). */
  let problem = $state<Record<Kind, string>>({ terminal: "", filemanager: "" });

  // Custom saves only a template that works; until then the previous choice
  // stays in effect. Emptying the box never quietly switches back to Auto.
  function pick(k: Kind) {
    if (!custom(k)) {
      problem[k] = "";
      save(patch(k));
    } else if (!templateProblem(k, template(k))) {
      save(patch(k));
    }
  }

  function typed(k: Kind) {
    problem[k] = templateProblem(k, template(k)) ?? "";
    if (!problem[k]) save(patch(k), TYPING_DELAY_MS);
  }

  /** Why the font size isn't saved ("" = it is). */
  let sizeErr = $state("");
  /** The last size that saved, restored when the box is left invalid. */
  // svelte-ignore state_referenced_locally
  let goodSize = form.shellFontSize;

  function fontTyped() {
    sizeErr = fontSizeProblem(form.shellFontSize) ?? "";
    if (!sizeErr) {
      goodSize = form.shellFontSize;
      save(shellFontPatch(form), TYPING_DELAY_MS);
    }
  }
  function sizeBlur() {
    if (sizeErr) {
      form.shellFontSize = goodSize;
      sizeErr = "";
    }
  }

  onDestroy(() => {
    for (const k of ["terminal", "filemanager"] as const) {
      const p = custom(k) && templateProblem(k, template(k));
      if (p) report(`Didn't save the custom ${LABEL[k]} — ${p}`);
    }
    if (sizeErr) report(`Didn't save the shell font size — ${sizeErr}`);
  });
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
      oninput={() => typed("terminal")}
      aria-invalid={!!problem.terminal}
      spellcheck="false"
      placeholder="wezterm start --cwd {'{{dir}}'} -- {'{{cmd}}'}"
      class="w-full {ui.mono} {problem.terminal ? 'border-red-400/60!' : ''}"
    />
    {#if problem.terminal}
      <span class="block text-[11px] text-red-300">{problem.terminal} It isn't saved.</span>
    {/if}
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
    title="Shell that Open shell starts, and that one-shot terminal commands run inside"
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
  <span class="block {ui.help}">
    Open shell runs this right in the overlay. Its header can switch to another.
  </span>
</label>

<div class="flex gap-6">
  <label class="block space-y-1.5">
    <span class={ui.label}>Shell font</span>
    <input
      bind:value={form.shellFont}
      oninput={fontTyped}
      spellcheck="false"
      placeholder="Default (Cascadia, Nerd Font first)"
      title="Font family for Open shell — a Nerd Font shows prompt icons (oh-my-posh, Starship)"
      class="w-80 {ui.input}"
    />
  </label>
  <label class="block space-y-1.5">
    <span class={ui.label}>Size</span>
    <input
      type="number"
      min={FONT_SIZE_MIN}
      max={FONT_SIZE_MAX}
      bind:value={form.shellFontSize}
      oninput={fontTyped}
      onblur={sizeBlur}
      aria-invalid={!!sizeErr}
      placeholder="13"
      title="Font size for Open shell, in pixels"
      class="w-20 {ui.input} {sizeErr ? 'border-red-400/60!' : ''}"
    />
    {#if sizeErr}<span class="block text-[11px] text-red-300">{sizeErr}</span>{/if}
  </label>
</div>

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
      oninput={() => typed("filemanager")}
      aria-invalid={!!problem.filemanager}
      spellcheck="false"
      placeholder="dopus /cmd Go {'{{path}}'}"
      class="w-full {ui.mono} {problem.filemanager ? 'border-red-400/60!' : ''}"
    />
    {#if problem.filemanager}
      <span class="block text-[11px] text-red-300">{problem.filemanager} It isn't saved.</span>
    {/if}
    <span class="block {ui.help}">
      <span class="font-mono">{"{{path}}"}</span> = the folder to open.
    </span>
  {/if}
</label>
