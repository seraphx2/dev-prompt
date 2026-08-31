<script lang="ts">
  import { onMount } from "svelte";
  import {
    configSummary,
    getConfig,
    openConfigFile,
    reloadConfig,
    saveConfig,
  } from "../ipc";
  import type { ConfigSummary } from "../types";

  let { onback, onsaved }: { onback: () => void; onsaved: () => void } = $props();

  let hotkey = $state("");
  let roots = $state<string[]>([]);
  let ttlMin = $state(15);
  let loaded = $state(false);
  let busy = $state(false);
  let msg = $state("");
  let confirmIdx = $state<number | null>(null);
  let summary = $state<ConfigSummary | null>(null);

  const base = (p: string) => p.split(/[\\/]/).pop() || p;

  async function loadSummary() {
    try {
      summary = await configSummary();
    } catch {
      summary = null;
    }
  }

  onMount(async () => {
    applyConfig(await getConfig());
    loaded = true;
    void loadSummary();
  });

  const addRoot = () => (roots = [...roots, ""]);
  function removeRoot(i: number) {
    roots = roots.filter((_, k) => k !== i);
    confirmIdx = null;
  }

  function applyConfig(c: {
    hotkey: string;
    roots: string[];
    cache_ttl_secs: number;
  }) {
    hotkey = c.hotkey;
    roots = c.roots.length ? [...c.roots] : [""];
    ttlMin = Math.max(1, Math.round(c.cache_ttl_secs / 60));
  }

  async function reload() {
    busy = true;
    msg = "";
    try {
      applyConfig(await reloadConfig());
      await loadSummary();
      onsaved(); // re-scan so marker/rule changes take effect in the repo list
      msg = "Reloaded from disk.";
    } catch (e) {
      msg = `${e}`;
    } finally {
      busy = false;
    }
  }

  async function openFile() {
    try {
      // Backend drops the overlay's always-on-top so the editor comes to the
      // front; the settings screen stays open behind it with your edits intact.
      await openConfigFile();
      msg = "Opened in your default editor.";
    } catch (e) {
      msg = `${e}`;
    }
  }

  async function save() {
    busy = true;
    msg = "";
    try {
      await saveConfig({
        hotkey: hotkey.trim(),
        roots: roots.map((r) => r.trim()).filter(Boolean),
        cache_ttl_secs: Math.max(60, Math.round(ttlMin * 60)),
      });
      msg = "Saved.";
      onsaved();
      void loadSummary();
    } catch (e) {
      msg = `${e}`;
    } finally {
      busy = false;
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
  <span class="text-[13px] font-medium text-white/80">Settings</span>
</div>

<div class="scroll-thin flex-1 space-y-5 overflow-y-auto px-5 py-4 text-[13px]">
  {#if !loaded}
    <div class="text-white/30">Loading…</div>
  {:else}
    <label class="block space-y-1.5">
      <span class="text-white/50">Global hotkey</span>
      <input
        bind:value={hotkey}
        spellcheck="false"
        class="w-full rounded border border-hair bg-white/[0.04] px-2 py-1.5 text-white/90 focus:border-white/25 focus:outline-none"
      />
      <span class="block text-[11px] text-white/25"
        >e.g. <code>CmdOrCtrl+Shift+Space</code>, <code>Alt+Space</code></span
      >
    </label>

    <div class="space-y-1.5">
      <span class="text-white/50">Root directories</span>
      {#each roots as _root, i (i)}
        <div class="flex items-center gap-2">
          <input
            bind:value={roots[i]}
            spellcheck="false"
            placeholder="D:\git"
            class="min-w-0 flex-1 rounded border border-hair bg-white/[0.04] px-2 py-1.5 font-mono text-[12px] text-white/90 focus:border-white/25 focus:outline-none"
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
              onclick={() => (confirmIdx = i)}
              aria-label="Remove from scan list"
              title="Remove from scan list"
              class="shrink-0 rounded border border-hair px-2 text-white/40 hover:bg-white/10 hover:text-white/70"
            >
              ×
            </button>
          {/if}
        </div>
      {/each}
      <button
        type="button"
        onclick={addRoot}
        class="rounded border border-hair px-2 py-1 text-[12px] text-white/50 hover:bg-white/10 hover:text-white/80"
      >
        + Add root
      </button>
      <p class="text-[11px] text-white/25">
        Removing a row only stops dev-prompt from scanning that path — it never
        touches the folder on disk.
      </p>
    </div>

    <label class="block space-y-1.5">
      <span class="text-white/50">Cache lifetime (minutes)</span>
      <input
        type="number"
        min="1"
        bind:value={ttlMin}
        class="w-24 rounded border border-hair bg-white/[0.04] px-2 py-1.5 text-white/90 focus:border-white/25 focus:outline-none"
      />
    </label>

    <div class="flex flex-wrap items-center gap-3 pt-1">
      <button
        type="button"
        onclick={save}
        disabled={busy}
        class="rounded bg-sky-500/80 px-3 py-1.5 text-[12px] font-medium text-white hover:bg-sky-500 disabled:opacity-50"
      >
        Save
      </button>
      <button
        type="button"
        onclick={openFile}
        class="rounded border border-hair px-3 py-1.5 text-[12px] text-white/60 hover:bg-white/10 hover:text-white/90"
      >
        Open config.yaml
      </button>
      <button
        type="button"
        onclick={reload}
        disabled={busy}
        title="Re-read config.yaml from disk"
        class="rounded border border-hair px-3 py-1.5 text-[12px] text-white/60 hover:bg-white/10 hover:text-white/90 disabled:opacity-50"
      >
        Reload config
      </button>
      {#if msg}<span class="text-[12px] text-white/40">{msg}</span>{/if}
    </div>

    <p class="text-[11px] leading-relaxed text-white/25">
      Detection rules — which files map to which launch actions — are edited
      directly in <code>config.yaml</code>. Saving here re-scans your roots.
    </p>

    {#if summary}
      <details open class="rounded border border-hair">
        <summary
          class="cursor-pointer select-none px-3 py-2 text-[12px] text-white/60 hover:text-white/90"
        >
          Active configuration
          <span class="text-white/25"
            >· {summary.rules.length} rules · {summary.programs.length} programs · {summary.markerCount}
            markers</span
          >
        </summary>

        <div class="space-y-3 border-t border-hair px-3 py-3 font-mono text-[11px]">
          <div>
            <div class="mb-1 text-white/35">programs</div>
            <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
              {#each summary.programs as p (p.key)}
                <span class="text-white/70">{p.key}</span>
                {#if p.resolved}
                  <span class="truncate text-emerald-300/70" title={p.resolved}
                    >{base(p.resolved)}</span
                  >
                {:else}
                  <span class="text-white/25">not found</span>
                {/if}
              {/each}
            </div>
          </div>

          <div>
            <div class="mb-1 text-white/35">rules</div>
            <div class="space-y-1">
              {#each summary.rules as r (r.id)}
                <div class="flex flex-wrap items-baseline gap-x-2">
                  <span class={r.available ? "text-white/70" : "text-white/30"}
                    >{r.id}</span
                  >
                  <span class="text-white/30">{r.matches.join(", ")}</span>
                  <span class="text-white/40">→ {r.kind}</span>
                  {#if r.scope === "repo"}<span class="text-white/25">@repo</span>{/if}
                  {#if !r.available}
                    <span class="text-amber-300/70">unmet: {r.missing.join(", ")}</span>
                  {/if}
                </div>
              {/each}
            </div>
          </div>

          <div>
            <div class="mb-1 text-white/35">universal</div>
            <div class="space-y-0.5">
              {#each summary.universal as u (u.id)}
                <div class="flex items-baseline gap-2">
                  <span class={u.available ? "text-white/70" : "text-white/30"}
                    >{u.label}</span
                  >
                  {#if u.default}<span class="text-sky-300/70">default</span>{/if}
                  {#if !u.available}<span class="text-white/25">unavailable</span>{/if}
                </div>
              {/each}
            </div>
          </div>

          <div class="truncate text-white/20" title={summary.configPath}>
            {summary.configPath}
          </div>
        </div>
      </details>
    {/if}
  {/if}
</div>
