<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    configSummary,
    getAutostart,
    getConfig,
    listRepos,
    listShells,
    listTerminals,
    openReleasesPage,
    openRulesFile,
    pickDirectories,
    reloadConfig,
    repoRuleTrace,
    rescanApps,
    saveConfig,
    setAutostart,
  } from "../ipc";
  import type { ConfigSummary, RepoTrace, TerminalOption } from "../types";
  import HotkeyRecorder from "./HotkeyRecorder.svelte";
  import { icons, iconKeys } from "../icons";
  import { glyphFor, glyphDim } from "../glyph";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { installUpdate } from "../updater";
  import { upd, pollUpdates } from "../updateStore.svelte";

  async function copyIcon(k: string) {
    await writeText(`icon: ${k}`);
    note(`Copied "icon: ${k}"`);
  }

  let { onback, onsaved }: { onback: () => void; onsaved: () => void } = $props();

  let hotkey = $state("");
  let roots = $state<string[]>([]);
  let ttlMin = $state(15);
  let scanDepth = $state(4);
  // Bound to the <select>; serialises to `true` / `false` / `"auto"` on save.
  let collapseNested = $state<"true" | "false" | "auto">("true");
  // "" = auto, "__custom__" = raw template, else a terminal id.
  let terminalSel = $state("");
  let terminalTemplate = $state("");
  let terminals = $state<TerminalOption[]>([]);
  // "" = default shell (pwsh -> powershell), else a shell name.
  let shellSel = $state("");
  let shells = $state<string[]>([]);
  // Installed-app launcher (the ">" scope).
  let appsEnabled = $state(true);
  let appExtraDirs = $state<string[]>([]);
  let appExclude = $state<string[]>([]);
  let appsSnapshot = "";
  let autostart = $state(false);
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

  // "Trace a repo" — pick a repo, see rule-by-rule why it does / doesn't resolve.
  let traceRepos = $state<{ name: string; path: string }[]>([]);
  let tracePath = $state("");
  let repoTrace = $state<RepoTrace | null>(null);
  let traceBusy = $state(false);
  let traceAll = $state(false);

  async function runTrace() {
    if (!tracePath) {
      repoTrace = null;
      return;
    }
    traceBusy = true;
    try {
      repoTrace = await repoRuleTrace(tracePath);
    } catch (e) {
      repoTrace = null;
      note(`${e}`, true);
    } finally {
      traceBusy = false;
    }
  }

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
    try {
      autostart = await getAutostart();
    } catch {
      autostart = false;
    }
    try {
      traceRepos = (await listRepos()).repos.map((r) => ({
        name: r.name,
        path: r.path,
      }));
    } catch {
      traceRepos = [];
    }
    try {
      terminals = await listTerminals();
    } catch {
      terminals = [];
    }
    try {
      shells = await listShells();
    } catch {
      shells = [];
    }
    void pollUpdates();
  });

  // Autostart applies immediately (the OS is the source of truth), not via Save.
  async function toggleAutostart() {
    try {
      await setAutostart(autostart);
    } catch (e) {
      autostart = !autostart; // revert on failure
      note(`${e}`, true);
    }
  }

  async function browseRoots() {
    const picked = await pickDirectories();
    if (!picked.length) return;
    const have = new Set(roots.map((r) => r.trim()).filter(Boolean));
    const add = picked.filter((p) => !have.has(p));
    if (add.length) {
      roots = [...roots.map((r) => r.trim()).filter(Boolean), ...add];
    }
  }

  // --- software update (shared store; see lib/updateStore) ---
  let installing = $state(false);
  let installError = $state("");

  async function applyUpdate() {
    installing = true;
    installError = "";
    try {
      await installUpdate(); // relaunches on success
    } catch (e) {
      installError = `${e}`;
      installing = false;
    }
  }

  // --- global hotkeys (recorder logic lives in HotkeyRecorder.svelte) ---
  let appsHotkey = $state("");

  // A recorded/typed combo commits immediately; the big Save is for the rest.
  async function persistHotkey(patch: { hotkey?: string; apps_hotkey?: string }) {
    busy = true;
    note("");
    try {
      const c = await saveConfig(patch);
      hotkey = c.hotkey;
      appsHotkey = c.apps_hotkey ?? "";
      note("Hotkey saved.");
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
    apps_hotkey?: string | null;
    roots: string[];
    scan: { max_depth: number; collapse_nested?: boolean | "auto" };
    cache_ttl_secs: number;
    terminal?: string | null;
    terminal_template?: string | null;
    shell?: string | null;
    apps?: { enabled: boolean; extra_dirs: string[]; exclude: string[] };
  }) {
    hotkey = c.hotkey;
    appsHotkey = c.apps_hotkey ?? "";
    roots = c.roots.length ? [...c.roots] : [""];
    ttlMin = Math.max(1, Math.round(c.cache_ttl_secs / 60));
    scanDepth = Math.max(1, c.scan?.max_depth ?? 4);
    collapseNested =
      c.scan?.collapse_nested === undefined
        ? "true"
        : (String(c.scan.collapse_nested) as "true" | "false" | "auto");
    terminalTemplate = c.terminal_template ?? "";
    terminalSel = terminalTemplate ? "__custom__" : (c.terminal ?? "");
    shellSel = c.shell ?? "";
    appsEnabled = c.apps?.enabled ?? true;
    appExtraDirs = [...(c.apps?.extra_dirs ?? [])];
    appExclude = [...(c.apps?.exclude ?? [])];
    appsSnapshot = JSON.stringify([appsEnabled, appExtraDirs, appExclude]);
  }

  const addAppDir = () => (appExtraDirs = [...appExtraDirs, ""]);
  function removeAppDir(i: number) {
    appExtraDirs = appExtraDirs.filter((_, k) => k !== i);
  }
  async function browseAppDirs() {
    const picked = await pickDirectories();
    if (!picked.length) return;
    const have = new Set(appExtraDirs.map((d) => d.trim()).filter(Boolean));
    const add = picked.filter((p) => !have.has(p));
    if (add.length) {
      appExtraDirs = [
        ...appExtraDirs.map((d) => d.trim()).filter(Boolean),
        ...add,
      ];
    }
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

  async function openRules() {
    try {
      // Backend drops the overlay's always-on-top so the editor comes to the
      // front; the settings screen stays open behind it with your edits intact.
      await openRulesFile();
      note("Opened rules.yaml in your default editor.");
    } catch (e) {
      note(`${e}`, true);
    }
  }

  /** Persist every editable field. Returns whether the app settings changed. */
  async function persist(): Promise<{ ok: boolean; appsChanged: boolean }> {
    busy = true;
    note("");
    try {
      const appExtra = appExtraDirs.map((d) => d.trim()).filter(Boolean);
      const appExcl = appExclude.map((d) => d.trim()).filter(Boolean);
      await saveConfig({
        roots: roots.map((r) => r.trim()).filter(Boolean),
        cache_ttl_secs: Math.max(60, Math.round(ttlMin * 60)),
        scan_max_depth: Math.max(1, Math.round(scanDepth)),
        collapse_nested: collapseNested === "auto" ? "auto" : collapseNested === "true",
        terminal: terminalSel === "__custom__" ? "" : terminalSel,
        terminal_template:
          terminalSel === "__custom__" ? terminalTemplate.trim() : "",
        shell: shellSel,
        apps: { enabled: appsEnabled, extra_dirs: appExtra, exclude: appExcl },
      });
      const next = JSON.stringify([appsEnabled, appExtra, appExcl]);
      const appsChanged = next !== appsSnapshot;
      appsSnapshot = next;
      void loadSummary();
      return { ok: true, appsChanged };
    } catch (e) {
      note(`${e}`, true);
      return { ok: false, appsChanged: false };
    } finally {
      busy = false;
    }
  }

  async function save() {
    const { ok, appsChanged } = await persist();
    if (!ok) return;
    note("Saved.");
    onsaved(); // App.svelte re-scans repos
    if (appsChanged) void rescanApps();
  }

  // The contextual "Rescan" buttons save first, so they act on what's on screen
  // rather than whatever was last saved.
  async function saveAndRescanRepos() {
    const { ok } = await persist();
    if (!ok) return;
    note("Saved — rescanning repositories…");
    onsaved();
  }

  async function saveAndRescanApps() {
    const { ok } = await persist();
    if (!ok) return;
    note("Saved — rescanning apps…");
    await rescanApps();
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
    {#if upd.info}
      <!-- Pending update sits at the top of the screen so the footer chip lands
           straight on the Install button. The manual "Check for updates" button
           stays in its own section further down. -->
      <div
        class="flex items-center gap-3 rounded border border-sky-400/40 bg-sky-500/[0.08] px-3 py-2"
      >
        <button
          type="button"
          onclick={applyUpdate}
          disabled={installing}
          class="shrink-0 rounded bg-sky-500/80 px-3 py-1.5 text-[12px] font-medium text-white hover:bg-sky-500 disabled:opacity-50"
        >
          {installing ? "Installing…" : "Install"}
        </button>
        <div class="min-w-0 flex-1">
          <div class="flex items-baseline justify-between gap-3">
            <span class="text-[12px] text-white/80">
              Version <span class="font-mono text-sky-300">{upd.info.version}</span>
              is available.
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
    {/if}

    <!-- Save floats top-right while the fields it controls are on screen, then
         scrolls away with this wrapper past the Rules section (which has its
         own buttons and doesn't use Save). The sticky strip is zero-height so
         it overlays the first row instead of pushing content down. -->
    <div class="relative">
      <div class="pointer-events-none sticky top-0 z-20 h-0">
        <div class="flex justify-end">
          <button
            type="button"
            onclick={save}
            disabled={busy}
            class="pointer-events-auto -mt-1 rounded-md border border-sky-400/40 bg-sky-500/90 px-3.5 py-1.5 text-[12px] font-medium text-white shadow-lg shadow-black/40 backdrop-blur hover:bg-sky-500 disabled:opacity-50"
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </div>

      <div class="space-y-5">
        <label class="flex items-center gap-2">
          <input
            type="checkbox"
            bind:checked={autostart}
            onchange={toggleAutostart}
            class="h-3.5 w-3.5 accent-sky-500"
          />
          <span class="text-orange-400">Start at login</span>
        </label>

        <div class="space-y-2">
          <span class="text-orange-400">Global hotkeys</span>
      <div class="grid grid-cols-2 gap-4">
        <HotkeyRecorder
          label="Repo browser"
          value={hotkey}
          {busy}
          onsave={(a) => persistHotkey({ hotkey: a })}
        />
        <HotkeyRecorder
          label="App launcher"
          value={appsHotkey}
          clearable
          {busy}
          onsave={(a) => persistHotkey({ apps_hotkey: a })}
        />
      </div>
      <p class="text-[11px] text-white/25">
        The app launcher hotkey opens the overlay straight into the
        <span class="font-mono">›</span> installed-apps view.
      </p>
    </div>

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
              class="shrink-0 rounded border border-hair px-2 py-1 text-[11px] text-white/40 hover:bg-white/10 hover:text-white/70"
            >
              ×
            </button>
          {/if}
        </div>
      {/each}
      <div class="flex gap-2">
        <button
          type="button"
          onclick={addRoot}
          class="rounded border border-hair px-2 py-1 text-[12px] text-white/50 hover:bg-white/10 hover:text-white/80"
        >
          + Add root
        </button>
        <button
          type="button"
          onclick={browseRoots}
          class="rounded border border-hair px-2 py-1 text-[12px] text-white/50 hover:bg-white/10 hover:text-white/80"
        >
          Browse…
        </button>
        <button
          type="button"
          disabled={busy}
          onclick={saveAndRescanRepos}
          title="Save settings and rescan the root directories now"
          class="rounded border border-hair px-2 py-1 text-[12px] text-white/50 hover:bg-white/10 hover:text-white/80 disabled:opacity-50"
        >
          Rescan
        </button>
      </div>
    </div>

    <div class="flex gap-6">
      <label class="block space-y-1.5">
        <span class="text-orange-400">Cache lifetime (minutes)</span>
        <input
          type="number"
          min="1"
          bind:value={ttlMin}
          class="w-24 rounded border border-hair bg-white/[0.04] px-2 py-1.5 text-white/90 focus:border-white/25 focus:outline-none"
        />
      </label>
      <label class="block space-y-1.5">
        <span class="text-orange-400">Scan depth</span>
        <input
          type="number"
          min="1"
          max="12"
          bind:value={scanDepth}
          title="How many directory levels deep the scan looks for repos"
          class="w-24 rounded border border-hair bg-white/[0.04] px-2 py-1.5 text-white/90 focus:border-white/25 focus:outline-none"
        />
      </label>
    </div>

    <label class="block space-y-1.5">
      <span class="text-orange-400">Repo inside another repo</span>
      <select
        bind:value={collapseNested}
        title="What to do when a discovered repo sits inside another one"
        class="w-72 rounded border border-hair bg-white/[0.04] py-1.5 pl-2 pr-7 text-white/90 focus:border-white/25 focus:outline-none"
      >
        <option value="true">Collapse into the parent (default)</option>
        <option value="false">List every one separately</option>
        <option value="auto">Auto — keep independent checkouts only</option>
      </select>
    </label>

    <div class="flex flex-wrap gap-6">
      <label class="block space-y-1.5">
        <span class="text-orange-400">Terminal</span>
        <select
          bind:value={terminalSel}
          title="Which terminal emulator dev-prompt opens for terminal actions"
          class="w-72 rounded border border-hair bg-white/[0.04] py-1.5 pl-2 pr-7 text-white/90 focus:border-white/25 focus:outline-none"
        >
          <option value="">Auto (first available)</option>
          {#each terminals as t (t.id)}
            <option value={t.id}>{t.label}</option>
          {/each}
          {#if terminalSel && terminalSel !== "__custom__" && !terminals.some((t) => t.id === terminalSel)}
            <option value={terminalSel}>{terminalSel}</option>
          {/if}
          <option value="__custom__">Custom…</option>
        </select>
        {#if terminalSel === "__custom__"}
          <input
            bind:value={terminalTemplate}
            spellcheck="false"
            placeholder="wezterm start --cwd {'{{dir}}'} -- {'{{cmd}}'}"
            class="w-full rounded border border-hair bg-white/[0.04] px-2 py-1.5 font-mono text-[12px] text-white/90 focus:border-white/25 focus:outline-none"
          />
          <span class="block text-[11px] text-white/25">
            <span class="font-mono">{"{{dir}}"}</span> = working directory,
            <span class="font-mono">{"{{cmd}}"}</span> = the command to run.
          </span>
        {/if}
      </label>

      <label class="block space-y-1.5">
        <span class="text-orange-400">Shell</span>
        <select
          bind:value={shellSel}
          title="Shell a one-shot terminal command runs inside"
          class="w-56 rounded border border-hair bg-white/[0.04] py-1.5 pl-2 pr-7 text-white/90 focus:border-white/25 focus:outline-none"
        >
          <option value="">Default (PowerShell)</option>
          {#each shells as s (s)}
            <option value={s}>{s}</option>
          {/each}
          {#if shellSel && !shells.includes(shellSel)}
            <option value={shellSel}>{shellSel}</option>
          {/if}
        </select>
      </label>
    </div>

    <div class="space-y-2">
      <label class="flex items-center gap-2">
        <input
          type="checkbox"
          bind:checked={appsEnabled}
          class="h-3.5 w-3.5 accent-sky-500"
        />
        <span class="text-orange-400">Index installed apps</span>
        <span class="text-white/25">— type <span class="font-mono">›</span> in the search bar</span>
      </label>
      {#if appsEnabled}
        <div class="space-y-1.5 pl-5">
          <span class="text-[11px] text-white/30"
            >Extra folders to scan for portable executables (Start Menu, Store
            apps and installed programs are found automatically).</span
          >
          {#each appExtraDirs as _d, i (i)}
            <div class="flex items-center gap-2">
              <input
                bind:value={appExtraDirs[i]}
                spellcheck="false"
                placeholder="D:\tools"
                class="min-w-0 flex-1 rounded border border-hair bg-white/[0.04] px-2 py-1.5 font-mono text-[12px] text-white/90 focus:border-white/25 focus:outline-none"
              />
              <button
                type="button"
                onclick={() => removeAppDir(i)}
                aria-label="Remove folder"
                title="Remove folder"
                class="shrink-0 rounded border border-hair px-2 py-1 text-[11px] text-white/40 hover:bg-white/10 hover:text-white/70"
              >
                ×
              </button>
            </div>
          {/each}
          <div class="flex gap-2">
            <button
              type="button"
              onclick={addAppDir}
              class="rounded border border-hair px-2 py-1 text-[12px] text-white/50 hover:bg-white/10 hover:text-white/80"
            >
              + Add folder
            </button>
            <button
              type="button"
              onclick={browseAppDirs}
              class="rounded border border-hair px-2 py-1 text-[12px] text-white/50 hover:bg-white/10 hover:text-white/80"
            >
              Browse…
            </button>
            <button
              type="button"
              disabled={busy}
              onclick={saveAndRescanApps}
              title="Save settings and re-enumerate installed apps now"
              class="rounded border border-hair px-2 py-1 text-[12px] text-white/50 hover:bg-white/10 hover:text-white/80 disabled:opacity-50"
            >
              Rescan apps
            </button>
          </div>
        </div>
      {/if}
    </div>

      </div>
    </div>
    <!-- /save-relevant wrapper -->

    <div class="space-y-2 border-t border-hair pt-4">
      <span class="text-orange-400">Rules</span>
      <p class="text-[11px] text-white/30">
        Editor / build-tool / launcher mappings, layered over the built-ins. Edit
        <span class="font-mono">rules.yaml</span>, then reload.
      </p>
      <div class="flex flex-wrap items-center gap-3">
        <button
          type="button"
          onclick={openRules}
          class="rounded border border-hair px-3 py-1.5 text-[12px] text-white/60 hover:bg-white/10 hover:text-white/90"
        >
          Open rules file
        </button>
        <button
          type="button"
          onclick={reload}
          disabled={busy}
          title="Re-read rules.yaml (and config.yaml) from disk"
          class="rounded border border-hair px-3 py-1.5 text-[12px] text-white/60 hover:bg-white/10 hover:text-white/90 disabled:opacity-50"
        >
          Reload rules
        </button>
      </div>
      {#if summary?.rulesPath}
        <div
          class="truncate font-mono text-[11px] text-white/25"
          title={summary.rulesPath}
        >
          {summary.rulesPath}
        </div>
      {/if}
    </div>

    {#if summary}
      <details open class="rounded border border-hair">
        <summary
          class="cursor-pointer select-none px-3 py-2 text-[12px] text-white/60 hover:text-white/90"
        >
          Active rules
          <span class="text-white/25"
            >· {summary.rules.length} rules · {summary.programs.length} programs · {summary.markerCount}
            markers</span
          >
        </summary>

        <div class="space-y-3 border-t border-hair px-3 py-3 font-mono text-[11px]">
          <div class="grid grid-cols-2 gap-x-6 gap-y-3">
            <div>
              <div
                class="mb-1 font-semibold uppercase tracking-wide text-sky-300/80"
              >
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
              <div
                class="mb-1 font-semibold uppercase tracking-wide text-sky-300/80"
              >
                universal
              </div>
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
        </div>
      </details>
    {/if}

    <details class="rounded border border-hair">
      <summary
        class="cursor-pointer select-none px-3 py-2 text-[12px] text-white/60 hover:text-white/90"
      >
        Trace a repo
        <span class="text-white/25">· why each rule does or doesn't resolve</span>
      </summary>

      <div class="space-y-3 border-t border-hair px-3 py-3 text-[11px]">
        {#if !traceRepos.length}
          <p class="text-white/40">
            No repos indexed yet — add roots above and rescan, then reopen
            Settings.
          </p>
        {:else}
          <div class="flex items-center gap-3">
            <select
              bind:value={tracePath}
              onchange={runTrace}
              class="min-w-0 flex-1 rounded border border-hair bg-white/[0.04] py-1.5 pl-2 pr-7 text-white/90 focus:border-white/25 focus:outline-none"
            >
              <option value="">Pick a repo…</option>
              {#each traceRepos as r (r.path)}
                <option value={r.path}>{r.name}</option>
              {/each}
            </select>
            <label class="flex shrink-0 items-center gap-1.5 text-white/50">
              <input
                type="checkbox"
                bind:checked={traceAll}
                class="h-3.5 w-3.5 accent-sky-500"
              />
              show idle rules
            </label>
          </div>

          {#if traceBusy}
            <p class="text-white/40">Evaluating…</p>
          {:else if repoTrace}
            {@const shown = traceAll
              ? repoTrace.rules
              : repoTrace.rules.filter((r) => r.gate === "")}
            {#if repoTrace.universal.length}
              <div class="font-mono">
                <span
                  class="font-semibold uppercase tracking-wide text-sky-300/80"
                  >general</span
                >
                <span class="text-white/50"
                  >{repoTrace.universal.join(", ")}</span
                >
              </div>
            {/if}
            <div class="space-y-1.5 font-mono">
              {#each shown as r (r.id)}
                <div>
                  <div class="flex flex-wrap items-baseline gap-x-2">
                    <span class={r.gate === "" ? "text-white/70" : "text-white/30"}
                      >{r.id}</span
                    >
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
                      → {h.produced.length
                        ? h.produced.join(", ")
                        : "(no actions)"}
                    </div>
                  {/each}
                </div>
              {:else}
                <p class="text-white/40">
                  No rules resolved for this repo — only the general actions
                  above.
                </p>
              {/each}
            </div>
          {/if}
        {/if}
      </div>
    </details>

    <details class="rounded border border-hair">
      <summary
        class="cursor-pointer select-none px-3 py-2 text-[12px] text-white/60 hover:text-white/90"
      >
        Icons
        <span class="text-white/25"
          >· {iconKeys.length} keys for <span class="font-mono">icon:</span> in rules.yaml</span
        >
      </summary>
      <div
        class="grid grid-cols-[repeat(auto-fill,minmax(76px,1fr))] gap-1 border-t border-hair p-2"
      >
        {#each iconKeys as k (k)}
          <button
            type="button"
            onclick={() => copyIcon(k)}
            title={`Copy "icon: ${k}"`}
            class="flex flex-col items-center gap-1.5 rounded px-1 py-2.5 hover:bg-white/[0.06]"
          >
            <svg
              class="h-5 w-5 {icons[k].hex && !glyphDim(icons[k]) ? '' : 'text-white/45'}"
              viewBox={icons[k].vb ?? "0 0 24 24"}
              fill="currentColor"
              fill-rule="evenodd"
              style={icons[k].hex && !glyphDim(icons[k]) ? `color:${icons[k].hex}` : ""}
            >
              {#if icons[k].raw}
                {@html icons[k].raw}
              {:else}
                <path d={icons[k].d} />
              {/if}
            </svg>
            <span
              class="w-full truncate text-center font-mono text-[9px] text-white/40"
              >{k}</span
            >
          </button>
        {/each}
      </div>
    </details>
  {/if}
</div>
