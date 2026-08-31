<script lang="ts">
  import { onDestroy, onMount } from "svelte";
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
  let msgError = $state(false);
  let confirmIdx = $state<number | null>(null);
  let msgTimer: ReturnType<typeof setTimeout> | undefined;

  /** Errors stay put (red); confirmations flash green and clear after 5s. */
  function note(text: string, isError = false) {
    clearTimeout(msgTimer);
    msg = text;
    msgError = isError;
    if (text && !isError) {
      msgTimer = setTimeout(() => (msg = ""), 5000);
    }
  }

  onDestroy(() => clearTimeout(msgTimer));
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

  // --- hotkey recorder ---
  let capturing = $state(false);
  let captureHint = $state("");
  let typeMode = $state(false);

  function codeToKey(code: string, fallback: string): string {
    if (code.startsWith("Key")) return code.slice(3); // KeyA -> A
    if (code.startsWith("Digit")) return code.slice(5); // Digit1 -> 1
    return code || fallback; // Space, F5, Comma, ArrowUp, Minus, …
  }

  function onCaptureKey(e: KeyboardEvent) {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      capturing = false;
      captureHint = "";
      return;
    }
    if (["Control", "Alt", "Shift", "Meta", "OS"].includes(e.key)) {
      captureHint = "…keep going";
      return;
    }

    const mods: string[] = [];
    if (e.ctrlKey) mods.push("CmdOrCtrl");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    if (e.metaKey) mods.push("Super");

    const key = codeToKey(e.code, e.key);
    if (mods.length === 0 && !/^F\d{1,2}$/.test(key)) {
      captureHint = "needs Ctrl, Alt or Shift";
      return;
    }

    hotkey = [...mods, key].join("+");
    capturing = false;
    captureHint = "";
    void saveHotkey();
  }

  // Recording a hotkey commits immediately (typed edits still use Save).
  async function saveHotkey() {
    busy = true;
    note("");
    try {
      await saveConfig({ hotkey: hotkey.trim() });
      note(`Hotkey saved — ${hotkey.trim()}`);
    } catch (e) {
      note(`${e}`, true);
    } finally {
      busy = false;
    }
  }

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
    note("");
    try {
      applyConfig(await reloadConfig());
      await loadSummary();
      onsaved(); // re-scan so marker/rule changes take effect in the repo list
      note("Reloaded from disk.");
    } catch (e) {
      note(`${e}`, true);
    } finally {
      busy = false;
    }
  }

  async function openFile() {
    try {
      // Backend drops the overlay's always-on-top so the editor comes to the
      // front; the settings screen stays open behind it with your edits intact.
      await openConfigFile();
      note("Opened in your default editor.");
    } catch (e) {
      note(`${e}`, true);
    }
  }

  async function save() {
    busy = true;
    note("");
    try {
      await saveConfig({
        hotkey: hotkey.trim(),
        roots: roots.map((r) => r.trim()).filter(Boolean),
        cache_ttl_secs: Math.max(60, Math.round(ttlMin * 60)),
      });
      note("Saved.");
      onsaved();
      void loadSummary();
    } catch (e) {
      note(`${e}`, true);
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
  {#if msg}
    <span
      class="ml-auto truncate pl-2 text-[11px] {msgError
        ? 'text-red-300'
        : 'text-emerald-300'}">{msg}</span
    >
  {/if}
</div>

<div class="scroll-thin flex-1 space-y-5 overflow-y-auto px-5 py-4 text-[13px]">
  {#if !loaded}
    <div class="text-white/30">Loading…</div>
  {:else}
    <label class="block space-y-1.5">
      <span class="text-orange-400">Global hotkey</span>
      {#if typeMode}
        <input
          bind:value={hotkey}
          spellcheck="false"
          placeholder="CmdOrCtrl+Shift+Space"
          class="w-full rounded border border-hair bg-white/[0.04] px-2 py-1.5 font-mono text-[12px] text-white/90 focus:border-white/25 focus:outline-none"
        />
      {:else}
        <button
          type="button"
          onclick={() => {
            captureHint = "";
            capturing = true;
          }}
          onkeydown={onCaptureKey}
          class="flex w-full items-center justify-between rounded border px-2 py-1.5 text-left transition-colors
                 {capturing
            ? 'border-orange-400/60 bg-orange-400/[0.08]'
            : 'border-hair bg-white/[0.04] hover:border-white/25'}"
        >
          <span
            class="font-mono text-[12px] {capturing ? 'text-white/40' : 'text-white/90'}"
          >
            {capturing ? "Press a combination…" : hotkey || "not set"}
          </span>
          <span class="shrink-0 text-[10px] uppercase tracking-wide text-white/30">
            {capturing ? "Esc cancels" : "click to record"}
          </span>
        </button>
      {/if}
      <span class="block text-[11px] text-white/25">
        {#if captureHint}
          <span class="text-amber-300/70">{captureHint}</span>
        {:else}
          <button
            type="button"
            class="underline decoration-white/20 underline-offset-2 hover:text-white/50"
            onclick={() => {
              typeMode = !typeMode;
              capturing = false;
            }}>{typeMode ? "use the recorder" : "type it manually"}</button
          >
        {/if}
      </span>
    </label>

    <div class="space-y-1.5">
      <span class="text-orange-400"
        >Root directories
        <span class="text-white/25">— scanned recursively for projects</span></span
      >
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
    </div>

    <label class="block space-y-1.5">
      <span class="text-orange-400">Cache lifetime (minutes)</span>
      <input
        type="number"
        min="1"
        bind:value={ttlMin}
        class="w-24 rounded border border-hair bg-white/[0.04] px-2 py-1.5 text-white/90 focus:border-white/25 focus:outline-none"
      />
    </label>

    <div>
      <button
        type="button"
        onclick={save}
        disabled={busy}
        class="rounded bg-sky-500/80 px-3 py-1.5 text-[12px] font-medium text-white hover:bg-sky-500 disabled:opacity-50"
      >
        Save
      </button>
    </div>

    <div class="space-y-2 border-t border-hair pt-4">
      <span class="text-orange-400">Configuration</span>
      <div class="flex flex-wrap items-center gap-3">
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
      </div>
      {#if summary?.configPath}
        <div
          class="truncate font-mono text-[11px] text-white/25"
          title={summary.configPath}
        >
          {summary.configPath}
        </div>
      {/if}
    </div>

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
            <div class="mb-1 font-semibold uppercase tracking-wide text-sky-300/80">
              programs
            </div>
            <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
              {#each summary.programs as p (p.key)}
                <span class="text-white/70">{p.key}</span>
                {#if p.resolved}
                  <span class="truncate text-emerald-300/70" title={p.resolved}
                    >{base(p.resolved)}</span
                  >
                {:else}
                  <span class="text-amber-300/70">not found</span>
                {/if}
              {/each}
            </div>
          </div>

          <div>
            <div class="mb-1 font-semibold uppercase tracking-wide text-sky-300/80">
              rules
            </div>
            <div class="space-y-1">
              {#each summary.rules as r (r.id)}
                <div class="flex flex-wrap items-baseline gap-x-2">
                  <span
                    class={r.disabled
                      ? "text-white/40"
                      : r.available
                        ? "text-white/70"
                        : "text-white/30"}>{r.id}</span
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

          <div>
            <div class="mb-1 font-semibold uppercase tracking-wide text-sky-300/80">
              universal
            </div>
            <div class="space-y-0.5">
              {#each summary.universal as u (u.id)}
                <div class="flex items-baseline gap-2">
                  <span
                    class={u.disabled
                      ? "text-white/40"
                      : u.available
                        ? "text-white/70"
                        : "text-white/30"}>{u.label}</span
                  >
                  {#if u.default}<span class="text-sky-300/70">default</span>{/if}
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
      </details>
    {/if}
  {/if}
</div>
