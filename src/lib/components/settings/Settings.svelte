<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { getConfig, reloadConfig, saveConfig } from "../../ipc";
  import type { AppConfig, ConfigPatch } from "../../types";
  import { icons } from "../../icons";
  import { upd } from "../../updateStore.svelte";
  import {
    PAGE_LABEL,
    formFromConfig,
    getLastPage,
    rescanNeeds,
    setLastPage,
    type Form,
    type Page,
  } from "./model";
  import GeneralPage from "./GeneralPage.svelte";
  import ReposPage from "./ReposPage.svelte";
  import ToolsPage from "./ToolsPage.svelte";
  import AppsPage from "./AppsPage.svelte";
  import RulesPage from "./RulesPage.svelte";
  import TracePage from "./TracePage.svelte";
  import IconsPage from "./IconsPage.svelte";
  import AboutPage from "./AboutPage.svelte";

  let {
    initialPage,
    onback,
    onsaved,
    onrescanrepos,
    onrescanapps,
  }: {
    /** Open on this page instead of the one last shown. */
    initialPage?: Page | null;
    onback: () => void;
    /** Fires after every successful save — App.svelte re-syncs the config
     *  fields it caches (dismiss mode, apps enabled). Never rescans on its
     *  own; that's `onrescanrepos` / `onrescanapps`, called only when a save
     *  changed what they'd find. */
    onsaved: () => void;
    onrescanrepos: () => void;
    onrescanapps: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let page = $state<Page>(initialPage ?? getLastPage());
  $effect(() => setLastPage(page));

  let content: HTMLDivElement | undefined = $state();
  $effect(() => {
    void page;
    if (content) content.scrollTop = 0;
  });

  let rulesOpen = $state(true);

  let form = $state<Form | null>(null);
  /** The config as last saved — what a save compares against to decide on a rescan. */
  let saved: AppConfig | null = null;

  // --- autosave -------------------------------------------------------------
  // Every edit saves itself: toggles and dropdowns at once, typing after a
  // pause. Patches that wait together go out as one save, and saves run one at
  // a time, so a slow one can't land after a newer one.
  let pending: ConfigPatch = {};
  let waiting = $state(false);
  let inflight = $state(0);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let chain: Promise<AppConfig | null> = Promise.resolve(null);

  function save(patch: ConfigPatch, delay = 0) {
    if (!Object.keys(patch).length) return;
    Object.assign(pending, patch);
    waiting = true;
    clearTimeout(timer);
    if (delay > 0) timer = setTimeout(() => void flush(), delay);
    else void flush();
  }

  /** Sends whatever is waiting now. Resolves to the saved config, or null if
   *  the save failed (the error is shown in the header). */
  function flush(): Promise<AppConfig | null> {
    clearTimeout(timer);
    if (!waiting) return chain;
    waiting = false;
    const patch = pending;
    pending = {};
    inflight++;
    chain = chain.then(async () => {
      try {
        const c = await saveConfig(patch);
        const need = saved ? rescanNeeds(saved, c) : { repos: false, apps: false };
        saved = c;
        onsaved();
        if (need.repos) onrescanrepos();
        if (need.apps) onrescanapps();
        if (inflight === 1) note("Saved.");
        return c;
      } catch (e) {
        note(`${e}`, true);
        return null;
      } finally {
        inflight--;
      }
    });
    return chain;
  }

  /** Saves now and waits for the result — for the hotkeys, which only take
   *  their new value once the OS has accepted it. */
  function commit(patch: ConfigPatch): Promise<AppConfig | null> {
    Object.assign(pending, patch);
    waiting = true;
    return flush();
  }

  // Leaving Settings sends anything still waiting on a typing pause.
  onDestroy(() => {
    void flush();
    clearTimeout(msgTimer);
  });

  /** Rules ▸ Reload: re-read both files from disk and take them as-is. */
  async function reload() {
    await flush();
    const c = await reloadConfig();
    const need = saved ? rescanNeeds(saved, c) : { repos: false, apps: false };
    saved = c;
    form = formFromConfig(c);
    onsaved();
    // Markers may have changed even if no setting did.
    onrescanrepos();
    if (need.apps) onrescanapps();
  }

  // --- header message ---------------------------------------------------------
  let msg = $state("");
  let msgError = $state(false);
  let msgTimer: ReturnType<typeof setTimeout> | undefined;

  /** Errors stay put (red); confirmations show briefly. */
  function note(text: string, isError = false) {
    clearTimeout(msgTimer);
    msg = text;
    msgError = isError;
    if (text && !isError) msgTimer = setTimeout(() => (msg = ""), 2500);
  }

  const saving = $derived(waiting || inflight > 0);

  onMount(async () => {
    try {
      const c = await getConfig();
      saved = c;
      form = formFromConfig(c);
    } catch (e) {
      note(`${e}`, true);
    }
  });

  type Entry = { page: Page; icon: string };
  const TOP: Entry[] = [
    { page: "general", icon: "sliders" },
    { page: "repos", icon: "branch" },
    { page: "tools", icon: "terminal" },
    { page: "apps", icon: "app" },
  ];
  const RULES: Entry[] = [
    { page: "rules", icon: "rules" },
    { page: "trace", icon: "trace" },
    { page: "icons", icon: "shapes" },
  ];
</script>

{#snippet row(e: Entry, indent: boolean)}
  {@const g = icons[e.icon]}
  {@const active = page === e.page}
  <button
    type="button"
    onclick={() => (page = e.page)}
    aria-current={active ? "page" : undefined}
    class="flex w-full items-center gap-2 border-l-2 py-1.5 pr-2 text-left text-[12.5px]
           {indent ? 'pl-6' : 'pl-3'}
           {active
      ? 'border-sky-400 bg-white/[0.07] text-white'
      : 'border-transparent text-white/55 hover:bg-white/[0.04] hover:text-white/85'}"
  >
    <svg
      class="h-3.5 w-3.5 shrink-0 {active ? 'text-white/85' : 'text-white/45'}"
      viewBox={g?.vb ?? "0 0 24 24"}
      fill="currentColor"
      fill-rule="evenodd"
      aria-hidden="true"
    >
      {#if g?.raw}{@html g.raw}{:else if g?.d}<path d={g.d} />{/if}
    </svg>
    <span class="truncate">{PAGE_LABEL[e.page]}</span>
    {#if e.page === "about" && upd.info}
      <span class="ml-auto h-1.5 w-1.5 shrink-0 rounded-full bg-sky-400" title="Update available"
      ></span>
    {/if}
  </button>
{/snippet}

<div class="flex items-center gap-2 border-b border-hair px-3 py-2.5">
  <button
    type="button"
    class="shrink-0 rounded px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
    onclick={onback}
    aria-label="Back"
  >
    ←
  </button>
  <span class="shrink-0 text-[13px] font-medium text-white/80">Settings</span>
  <span class="shrink-0 text-white/15">/</span>
  <span class="shrink-0 text-[13px] text-white/60">{PAGE_LABEL[page]}</span>
  <span
    class="ml-auto truncate pl-2 text-[11px] {msgError
      ? 'text-red-300'
      : saving
        ? 'text-white/35'
        : 'text-emerald-300'}"
    title={msgError ? msg : ""}
  >
    {msgError ? msg : saving ? "Saving…" : msg}
  </span>
</div>

<div class="flex min-h-0 flex-1">
  <nav
    class="scroll-thin flex w-[152px] shrink-0 flex-col overflow-y-auto border-r border-hair bg-black/15 py-2"
    aria-label="Settings pages"
  >
    {#each TOP as e (e.page)}
      {@render row(e, false)}
    {/each}
    <button
      type="button"
      onclick={() => (rulesOpen = !rulesOpen)}
      aria-expanded={rulesOpen}
      class="mt-2 flex w-full items-center gap-2 border-l-2 border-transparent py-1.5 pl-3 pr-2 text-left
             text-[12.5px] font-medium text-white/55 hover:bg-white/[0.04] hover:text-white/85"
    >
      <svg
        class="h-3.5 w-3.5 shrink-0 text-white/45"
        viewBox={icons.code?.vb ?? "0 0 24 24"}
        fill="currentColor"
        fill-rule="evenodd"
        aria-hidden="true"
      >
        {#if icons.code?.raw}{@html icons.code.raw}{/if}
      </svg>
      <span class="truncate">Rules</span>
      <span
        class="ml-auto text-[9px] text-white/30 transition-transform {rulesOpen ? '' : '-rotate-90'}"
        >▾</span
      >
    </button>
    {#if rulesOpen || RULES.some((e) => e.page === page)}
      {#each RULES as e (e.page)}
        {@render row(e, true)}
      {/each}
    {/if}
    <div class="mt-auto pt-2">
      {@render row({ page: "about", icon: "info" }, false)}
    </div>
  </nav>

  <div
    bind:this={content}
    class="scroll-thin min-w-0 flex-1 space-y-5 overflow-y-auto px-5 py-4 text-[13px]"
  >
    {#if page === "about"}
      <AboutPage {note} />
    {:else if page === "icons"}
      <IconsPage {note} />
    {:else if page === "trace"}
      <TracePage {note} />
    {:else if !form}
      <div class="text-white/30">{msgError ? "" : "Loading…"}</div>
    {:else if page === "general"}
      <GeneralPage {form} {save} {commit} {note} />
    {:else if page === "repos"}
      <ReposPage {form} {save} {note} rescan={() => void flush().then(onrescanrepos)} />
    {:else if page === "tools"}
      <ToolsPage {form} {save} />
    {:else if page === "apps"}
      <AppsPage {form} {save} {note} rescan={() => void flush().then(onrescanapps)} />
    {:else if page === "rules"}
      <RulesPage {note} {reload} />
    {/if}
  </div>
</div>
