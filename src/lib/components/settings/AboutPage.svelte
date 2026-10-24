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
    await pollUpdates({ manual: true });
    checked = true;
  }

  /** Installs itself (the installer, the AppImage); anything else gets the
   *  release page instead. */
  const installable = $derived(upd.mode === "self");

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
      {#if installable}
        <button
          type="button"
          onclick={install}
          disabled={installing}
          class="shrink-0 rounded-sm bg-sky-500/80 px-3 py-1.5 text-[12px] font-medium text-white hover:bg-sky-500 disabled:opacity-50"
        >
          {installing ? "Installing…" : "Install"}
        </button>
      {:else}
        <button
          type="button"
          onclick={() => openReleasesPage()}
          class="shrink-0 rounded-sm bg-sky-500/80 px-3 py-1.5 text-[12px] font-medium text-white hover:bg-sky-500"
        >
          Download ↗
        </button>
      {/if}
      <div class="min-w-0 flex-1">
        <div class="flex items-baseline justify-between gap-3">
          <span class="text-[12px] text-white/80">
            Version <span class="font-mono text-sky-300">{upd.info.version}</span> is available.
          </span>
          {#if installable}
            <button
              type="button"
              onclick={() => openReleasesPage()}
              class="shrink-0 text-[11px] text-sky-300/70 hover:text-sky-300 hover:underline"
            >
              What's changed ↗
            </button>
          {/if}
        </div>
        {#if !installable}
          <div class="mt-0.5 text-[11px] text-white/40">
            This copy doesn't install updates itself — the release page has the installer.
          </div>
        {/if}
        {#if installError}
          <div class="mt-1 text-[11px] text-red-300">{installError}</div>
        {/if}
      </div>
    </div>
  {:else if upd.mode === "managed"}
    <p class={ui.help}>
      Updates come through your package manager (<span class="font-mono">apt</span> /
      <span class="font-mono">dnf</span> / <span class="font-mono">pacman -Syu</span> /
      <span class="font-mono">flatpak update</span>).
    </p>
  {:else}
    <div class="flex items-center gap-3">
      <button type="button" onclick={check} disabled={upd.checking} class={ui.btn}>
        {upd.checking ? "Checking…" : "Check for updates"}
      </button>
      {#if checked && !upd.checking}
        {#if upd.error}
          <span class="min-w-0 truncate text-[11px] text-red-300" title={upd.error}
            >Couldn't check: {upd.error}</span
          >
        {:else}
          <span class="text-[11px] text-white/40">You're on the latest version.</span>
        {/if}
      {/if}
    </div>
    {#if upd.mode === "unmanaged"}
      <p class={ui.help}>
        This copy doesn't install updates itself; a newer version links to its download.
      </p>
    {/if}
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
          <div class="min-w-0 truncate font-mono text-[11px] text-white/50" title={f.path}>
            {f.path}
          </div>
        </div>
      {/each}
    </div>
    <p class="text-[11px] text-white/45">
      Config: <span class="font-mono">config.yaml</span>,
      <span class="font-mono">rules.yaml</span>, and usage history. Cache: regenerable repo / app
      indexes — safe to delete, dev-prompt rebuilds them.
    </p>
  </div>
{/if}
