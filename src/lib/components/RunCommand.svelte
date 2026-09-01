<script lang="ts">
  import { onMount } from "svelte";
  import { getConfig, listShells } from "../ipc";

  let {
    repoName,
    /** `run:` template of the chosen prompt action; may contain `{{input}}`. */
    template,
    onrun,
    onback,
  }: {
    repoName: string;
    template: string;
    onrun: (command: string, shell: string) => void;
    onback: () => void;
  } = $props();

  let value = $state("");
  let shellSel = $state("");
  let shells = $state<string[]>([]);
  let input: HTMLInputElement | undefined = $state();

  // Split the template around the {{input}} placeholder, if present.
  const parts = $derived.by(() => {
    const i = template.indexOf("{{input}}");
    return i === -1
      ? { parameterized: false, before: "", after: "" }
      : {
          parameterized: true,
          before: template.slice(0, i),
          after: template.slice(i + "{{input}}".length),
        };
  });

  const command = $derived(
    parts.parameterized ? parts.before + value + parts.after : value,
  );

  onMount(async () => {
    input?.focus();
    try {
      shells = await listShells();
    } catch {
      shells = [];
    }
    try {
      shellSel = (await getConfig()).shell ?? "";
    } catch {
      shellSel = "";
    }
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      onrun(command.trim(), shellSel);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onback();
    }
  }
</script>

<div class="flex items-center gap-2 border-b border-hair px-3 py-2.5">
  <button
    type="button"
    class="shrink-0 rounded px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
    onclick={onback}
    aria-label="Back"
  >
    ←
  </button>
  <span class="shrink-0 truncate text-[13px] font-medium text-white/80"
    >{repoName}</span
  >
  <span class="shrink-0 text-white/15">/</span>
  <select
    bind:value={shellSel}
    title="Shell to run in"
    class="shrink-0 rounded border border-hair bg-white/[0.04] py-1 pl-1.5 pr-6 text-[12px] text-white/80 focus:outline-none"
  >
    <option value="">default</option>
    {#each shells as s (s)}
      <option value={s}>{s}</option>
    {/each}
  </select>
</div>

<div class="flex flex-1 flex-col justify-center px-4">
  <div class="flex items-baseline font-mono text-[14px] text-white/90">
    {#if parts.parameterized}
      <span class="whitespace-pre text-white/40">{parts.before}</span>
    {/if}
    <input
      bind:this={input}
      bind:value
      onkeydown={onKey}
      spellcheck="false"
      autocomplete="off"
      placeholder={parts.parameterized ? "" : "command to run…"}
      class="min-w-0 flex-1 bg-transparent text-white/90 placeholder:text-white/25 focus:outline-none"
    />
    {#if parts.parameterized}
      <span class="whitespace-pre text-white/40">{parts.after}</span>
    {/if}
  </div>
  <div class="mt-1 truncate text-[11px] text-white/25">
    {command.trim() ||
      `opens ${shellSel || "a shell"} in a terminal at ${repoName}`}
  </div>
</div>
