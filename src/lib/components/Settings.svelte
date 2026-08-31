<script lang="ts">
  import { onMount } from "svelte";
  import { getConfig, openConfigFile, saveConfig } from "../ipc";

  let { onback, onsaved }: { onback: () => void; onsaved: () => void } = $props();

  let hotkey = $state("");
  let roots = $state<string[]>([]);
  let ttlMin = $state(15);
  let loaded = $state(false);
  let busy = $state(false);
  let msg = $state("");
  let confirmIdx = $state<number | null>(null);

  onMount(async () => {
    const c = await getConfig();
    hotkey = c.hotkey;
    roots = c.roots.length ? [...c.roots] : [""];
    ttlMin = Math.max(1, Math.round(c.cache_ttl_secs / 60));
    loaded = true;
  });

  const addRoot = () => (roots = [...roots, ""]);
  function removeRoot(i: number) {
    roots = roots.filter((_, k) => k !== i);
    confirmIdx = null;
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
      {#if msg}<span class="text-[12px] text-white/40">{msg}</span>{/if}
    </div>

    <p class="text-[11px] leading-relaxed text-white/25">
      Detection rules — which files map to which launch actions — are edited
      directly in <code>config.yaml</code>. Saving here re-scans your roots.
    </p>
  {/if}
</div>
