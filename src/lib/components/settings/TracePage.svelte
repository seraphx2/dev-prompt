<script lang="ts">
  import { onMount } from "svelte";
  import { listRepos, repoRuleTrace } from "../../ipc";
  import type { RepoTrace } from "../../types";
  import type { Note } from "./model";
  import * as ui from "./ui";

  let { note }: { note: Note } = $props();

  let repos = $state<{ name: string; path: string }[] | null>(null);
  let path = $state("");
  let trace = $state<RepoTrace | null>(null);
  let busy = $state(false);
  let showIdle = $state(false);

  onMount(async () => {
    try {
      repos = (await listRepos()).repos
        .map((r) => ({ name: r.name, path: r.path }))
        .sort((a, b) => a.name.localeCompare(b.name));
    } catch (e) {
      repos = [];
      note(`${e}`, true);
    }
  });

  async function run() {
    if (!path) {
      trace = null;
      return;
    }
    busy = true;
    try {
      trace = await repoRuleTrace(path);
    } catch (e) {
      trace = null;
      note(`${e}`, true);
    } finally {
      busy = false;
    }
  }

  const shown = $derived(
    trace ? (showIdle ? trace.rules : trace.rules.filter((r) => r.gate === "")) : [],
  );
</script>

<p class={ui.help}>Why each rule does or doesn't produce actions for one repo.</p>

{#if repos === null}
  <p class="text-white/30">Loading…</p>
{:else if !repos.length}
  <p class="text-white/40">No repos found yet — add a root directory under Repos first.</p>
{:else}
  <div class="flex items-center gap-3">
    <select bind:value={path} onchange={run} class="min-w-0 flex-1 {ui.select}">
      <option value="">Pick a repo…</option>
      {#each repos as r (r.path)}
        <option value={r.path}>{r.name}</option>
      {/each}
    </select>
    <label class="flex shrink-0 cursor-pointer items-center gap-1.5 text-[12px] text-white/50">
      <input type="checkbox" bind:checked={showIdle} class={ui.checkbox} />
      show idle rules
    </label>
  </div>

  {#if busy}
    <p class="text-[11px] text-white/40">Evaluating…</p>
  {:else if trace}
    <div class="space-y-3 text-[11px]">
      {#if trace.universal.length}
        <div class="font-mono">
          <span class="font-semibold uppercase tracking-wide text-sky-300/80">general</span>
          <span class="text-white/50">{trace.universal.join(", ")}</span>
        </div>
      {/if}
      <div class="space-y-1.5 font-mono">
        {#each shown as r (r.id)}
          <div>
            <div class="flex flex-wrap items-baseline gap-x-2">
              <span class={r.gate === "" ? "text-white/70" : "text-white/30"}>{r.id}</span>
              <span class="text-white/25">{r.globs.join(", ")}</span>
              {#if r.gate}
                <span class="text-amber-300/70">{r.gate}</span>
              {:else}
                <span class="text-emerald-300/70">✓ resolved</span>
              {/if}
            </div>
            {#each r.hits as h (h.project)}
              <div class="ml-3 text-white/45">
                {h.project || "root"}:
                <span class="text-white/30">{h.matched.join(", ")}</span>
                → {h.produced.length ? h.produced.join(", ") : "(no actions)"}
              </div>
            {/each}
          </div>
        {:else}
          <p class="text-white/40">
            No rules resolved for this repo — only the general actions above.
          </p>
        {/each}
      </div>
    </div>
  {/if}
{/if}
