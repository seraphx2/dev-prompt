<script lang="ts">
  import { onMount } from "svelte";
  import { configSummary, openRulesFile } from "../../ipc";
  import type { ConfigSummary } from "../../types";
  import { glyphFor } from "../../glyph";
  import type { Note } from "./model";
  import * as ui from "./ui";

  let {
    note,
    reload,
  }: {
    note: Note;
    /** Re-reads config.yaml + rules.yaml (after any waiting save goes out). */
    reload: () => Promise<void>;
  } = $props();

  let summary = $state<ConfigSummary | null>(null);
  let busy = $state(false);

  const base = (p: string) => p.split(/[\\/]/).pop() || p;

  async function load() {
    try {
      summary = await configSummary();
    } catch (e) {
      summary = null;
      note(`${e}`, true);
    }
  }
  onMount(load);

  async function openRules() {
    try {
      // The backend drops the overlay's always-on-top so the editor comes to
      // the front; Settings stays open behind it.
      await openRulesFile();
      note("Opened rules.yaml in your default editor.");
    } catch (e) {
      note(`${e}`, true);
    }
  }

  async function reloadRules() {
    busy = true;
    try {
      await reload();
      await load();
      note("Reloaded from disk.");
    } catch (e) {
      note(`${e}`, true);
    } finally {
      busy = false;
    }
  }
</script>

<div class="space-y-2">
  <p class={ui.help}>
    Editor, build-tool and launcher mappings, layered over the built-ins. Edit
    <span class="font-mono">rules.yaml</span>, then reload.
  </p>
  <div class="flex flex-wrap items-center gap-3">
    <button type="button" onclick={openRules} class={ui.btn}>Open rules file</button>
    <button
      type="button"
      onclick={reloadRules}
      disabled={busy}
      title="Re-read rules.yaml (and config.yaml) from disk"
      class={ui.btn}
    >
      Reload rules
    </button>
  </div>
  {#if summary?.rulesPath}
    <div class="truncate font-mono text-[11px] text-white/25" title={summary.rulesPath}>
      {summary.rulesPath}
    </div>
  {/if}
</div>

{#if summary}
  <div class="space-y-4 font-mono text-[11px]">
    <p class="font-sans text-[12px] text-white/50">
      {summary.rules.length} rules · {summary.programs.length} programs · {summary.markerCount} markers
    </p>

    <div class="grid grid-cols-2 gap-x-6 gap-y-3">
      <div>
        <div class={ui.subhead}>programs</div>
        <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
          {#each summary.programs as p (p.key)}
            <span class="text-white/70">{p.key}</span>
            {#if p.resolved}
              <span class="truncate text-emerald-300/70" title={p.resolved}>{base(p.resolved)}</span>
            {:else}
              <span class="text-amber-300/70">not found</span>
            {/if}
          {/each}
        </div>
      </div>

      <div>
        <div class={ui.subhead}>universal</div>
        <div class="space-y-0.5">
          {#each summary.universal as u (u.id)}
            {@const g = glyphFor(u.icon)}
            <div
              class="flex items-center gap-2 {u.disabled
                ? 'text-white/40'
                : u.available
                  ? 'text-white/70'
                  : 'text-white/30'}"
            >
              <svg
                class="h-3.5 w-3.5 shrink-0"
                viewBox={g.vb ?? "0 0 24 24"}
                fill="currentColor"
                fill-rule="evenodd"
              >
                {#if g.raw}{@html g.raw}{:else}<path d={g.d} />{/if}
              </svg>
              <span>{u.label}</span>
              {#if u.disabled}
                <span class="text-red-300/70">disabled</span>
              {:else if !u.available}
                <span class="text-amber-300/70">unavailable</span>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </div>

    <div>
      <div class={ui.subhead}>rules</div>
      <div class="space-y-1">
        {#each summary.rules as r (r.id)}
          <div class="flex flex-wrap items-baseline gap-x-2">
            <span class={r.disabled ? "text-white/40" : r.available ? "text-white/70" : "text-white/30"}
              >{r.id}</span
            >
            <span class="text-white/30">{r.matches.join(", ")}</span>
            <span class="text-white/40">→ {r.kind}</span>
            {#if r.scope === "repo"}<span class="text-white/25">@repo</span>{/if}
            {#if r.disabled}
              <span class="text-red-300/70">disabled</span>
            {:else if !r.available}
              <span class="text-amber-300/70">unmet: {r.missing.join(", ")}</span>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}
