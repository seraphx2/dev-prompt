<script lang="ts">
  import { onMount } from "svelte";
  import {
    openCacheDir,
    openConfigDir,
    openInstallDir,
    openReleasesPage,
    systemPaths,
  } from "../../ipc";
  import type { SystemPaths } from "../../types";
  import { installUpdate } from "../../updater";
  import { pollUpdates, upd } from "../../updateStore.svelte";
  import type { Note } from "./model";
  import * as ui from "./ui";

  let { note }: { note: Note } = $props();

  let paths = $state<SystemPaths | null>(null);
  let installing = $state(false);
  let installError = $state("");
  /** A check the user asked for finished; says so when nothing new turned up. */
  let checked = $state(false);

  onMount(async () => {
    void pollUpdates();
    paths = await systemPaths().catch(() => null);
  });

  async function check() {
    checked = false;
    await pollUpdates();
    checked = true;
  }

  async function install() {
    installing = true;
    installError = "";
    try {
      await installUpdate(); // relaunches on success
    } catch (e) {
      installError = `${e}`;
      installing = false;
    }
  }

  async function openFolder(open: () => Promise<void>, label: string) {
    try {
      await open();
    } catch (e) {
      note(`Could not open ${label}: ${e}`, true);
    }
  }

  const folders = $derived(
    paths
      ? [
          { label: "Open install folder", path: paths.installDir, open: openInstallDir, what: "the install folder" },
          { label: "Open config folder", path: paths.configDir, open: openConfigDir, what: "the config folder" },
          { label: "Open cache folder", path: paths.cacheDir, open: openCacheDir, what: "the cache folder" },
        ]
      : [],
  );
</script>

<div class="space-y-2">
  <p class="text-[13px] text-white/80">
    dev-prompt {#if upd.current}<span class="font-mono text-sky-300">v{upd.current}</span>{/if}
  </p>

  {#if upd.info}
    <div class="flex items-center gap-3 rounded-sm border border-sky-400/40 bg-sky-500/8 px-3 py-2">
      <button
        type="button"
        onclick={install}
        disabled={installing}
        class="shrink-0 rounded-sm bg-sky-500/80 px-3 py-1.5 text-[12px] font-medium text-white hover:bg-sky-500 disabled:opacity-50"
      >
        {installing ? "Installing…" : "Install"}
      </button>
      <div class="min-w-0 flex-1">
        <div class="flex items-baseline justify-between gap-3">
          <span class="text-[12px] text-white/80">
            Version <span class="font-mono text-sky-300">{upd.info.version}</span> is available.
          </span>
          <button
            type="button"
            onclick={() => openReleasesPage()}
            class="shrink-0 text-[11px] text-sky-300/70 hover:text-sky-300 hover:underline"
          >
            What's changed ↗
          </button>
        </div>
        {#if installError}
          <div class="mt-1 text-[11px] text-red-300">{installError}</div>
        {/if}
      </div>
    </div>
  {:else if upd.mode === "managed"}
    <p class={ui.help}>
      Updates come through your package manager (<span class="font-mono">apt</span> /
      <span class="font-mono">dnf</span> / <span class="font-mono">pacman -Syu</span>).
    </p>
  {:else if upd.mode === "unmanaged"}
    <p class={ui.help}>
      This build doesn't auto-update.
      <button
        type="button"
        onclick={() => openReleasesPage()}
        class="text-sky-300/70 hover:text-sky-300 hover:underline">Get the latest ↗</button
      >
    </p>
  {:else}
    <div class="flex items-center gap-3">
      <button type="button" onclick={check} disabled={upd.checking} class={ui.btn}>
        {upd.checking ? "Checking…" : "Check for updates"}
      </button>
      {#if checked && !upd.checking}
        <span class="text-[11px] text-white/40">You're on the latest version.</span>
      {/if}
    </div>
  {/if}
</div>

{#if paths}
  <div class="space-y-2">
    <span class={ui.label}>File locations</span>
    <p class={ui.help}>
      Where dev-prompt keeps things — settings, hand-authored overrides, and the caches it rebuilds
      automatically.
    </p>
    <div class="space-y-1.5">
      {#each folders as f (f.label)}
        <div class="flex items-center gap-3">
          <button
            type="button"
            onclick={() => openFolder(f.open, f.what)}
            class="w-40 shrink-0 {ui.btn}"
          >
            {f.label}
          </button>
          <div class="min-w-0 truncate font-mono text-[11px] text-white/35" title={f.path}>
            {f.path}
          </div>
        </div>
      {/each}
    </div>
    <p class="text-[11px] text-white/25">
      Config: <span class="font-mono">config.yaml</span>,
      <span class="font-mono">rules.yaml</span>, and usage history. Cache: regenerable repo / app
      indexes — safe to delete, dev-prompt rebuilds them.
    </p>
  </div>
{/if}
