<script lang="ts">
  import { onMount, tick } from "svelte";
  import { checkNewRepo, createRepo, initRepo, newRepoOptions } from "../ipc";
  import type { NewRepoOptions, ProvisionTool, Repo } from "../types";
  import { middleTruncate } from "../text";

  let {
    /** Set up version control in this existing folder instead of creating one. */
    existing = null,
    oncreated,
    onback,
  }: {
    existing?: { path: string; name: string } | null;
    oncreated: (repo: Repo) => void;
    onback: () => void;
  } = $props();

  // Last location / tool used, so a repeat visit is "type a name, Enter".
  const LAST_ROOT = "newRepo.root";
  const LAST_TOOL = "newRepo.tool";
  function recall(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }
  function remember(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      // Storage unavailable — the form just won't preselect next time.
    }
  }

  let options = $state<NewRepoOptions | null>(null);
  let loadError = $state("");
  let name = $state("");
  let root = $state("");
  let toolId = $state("");
  /** Field values per tool, so switching tools and back keeps what was typed. */
  let values = $state<Record<string, Record<string, string>>>({});
  let nameProblem = $state<string | null>(null);
  let createError = $state("");
  let busy = $state(false);
  /** Set after a Create attempt, so an empty name is only flagged once it matters. */
  let attempted = $state(false);
  let nameInput: HTMLInputElement | undefined = $state();

  const tool = $derived<ProvisionTool | undefined>(
    options?.tools.find((t) => t.id === toolId),
  );
  const toolValues = $derived(values[toolId] ?? {});
  const missingField = $derived(
    tool?.fields.find((f) => !(toolValues[f.id] ?? "").trim()),
  );
  const canCreate = $derived(
    !busy &&
      !!tool &&
      !missingField &&
      (existing !== null ||
        (!!root && name.length > 0 && nameProblem === null)),
  );

  const sep = $derived(root.includes("\\") ? "\\" : "/");
  const target = $derived(
    existing
      ? existing.path
      : root && name
        ? `${root.replace(/[\\/]+$/, "")}${sep}${name}`
        : "",
  );

  function quote(arg: string): string {
    return arg === "" || /\s/.test(arg) ? `"${arg}"` : arg;
  }

  // Mirrors `file_url` in provision.rs, for the preview only.
  function fileUrl(path: string): string {
    const p = path.replace(/\\/g, "/").replace(/\/+$/, "");
    const enc = Array.from(new TextEncoder().encode(p), (b) => {
      const c = String.fromCharCode(b);
      return /[A-Za-z0-9\-._~/:]/.test(c)
        ? c
        : `%${b.toString(16).toUpperCase().padStart(2, "0")}`;
    }).join("");
    return `file://${p.startsWith("/") ? "" : "/"}${enc}`;
  }

  // The exact commands Create will run, with the current values filled in.
  const preview = $derived.by<string[]>(() => {
    if (!tool) return [];
    const vars: Record<string, string> = {
      dir: target || "<folder>",
      name: name || "<name>",
      root,
      root_url: fileUrl(root),
      ...toolValues,
    };
    return tool.run.map((argv) =>
      argv
        .map((a) =>
          quote(a.replace(/\{\{\s*([\w-]+)\s*\}\}/g, (_, k) => vars[k] ?? "")),
        )
        .join(" "),
    );
  });

  onMount(async () => {
    try {
      const o = await newRepoOptions();
      const wantRoot = recall(LAST_ROOT);
      const wantTool = recall(LAST_TOOL);
      if (existing) {
        // Fixed: the folder's own name, and the deepest root it sits in.
        name = existing.name;
        root =
          o.roots
            .filter((r) => isInside(existing!.path, r))
            .sort((a, b) => b.length - a.length)[0] ?? "";
      } else {
        root = o.roots.find((r) => r === wantRoot) ?? o.roots[0] ?? "";
      }
      toolId = o.tools.find((t) => t.id === wantTool)?.id ?? o.tools[0]?.id ?? "";
      values = Object.fromEntries(
        o.tools.map((t) => [
          t.id,
          Object.fromEntries(t.fields.map((f) => [f.id, f.default])),
        ]),
      );
      options = o;
    } catch (e) {
      loadError = `${e}`;
    }
    await tick();
    if (existing) focusFirst();
    else nameInput?.focus();
  });

  function isInside(path: string, root: string): boolean {
    const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
    return norm(path).startsWith(norm(root) + "/");
  }

  let form: HTMLDivElement | undefined = $state();
  // With nothing to type, start on the tool choice (or Create).
  function focusFirst() {
    form
      ?.querySelector<HTMLElement>('[aria-checked="true"], input, button:not(:disabled)')
      ?.focus();
  }

  // As-you-type check of the name against the chosen location. Sequenced so a
  // slow answer for an older name can't overwrite a newer one.
  let checkSeq = 0;
  $effect(() => {
    const r = root;
    const n = name;
    const seq = ++checkSeq;
    if (existing || !r || !n) {
      nameProblem = null;
      return;
    }
    void checkNewRepo(r, n).then((problem) => {
      if (seq === checkSeq) nameProblem = problem;
    });
  });

  // A changed input makes the last failure stale.
  $effect(() => {
    void name;
    void root;
    void toolId;
    createError = "";
  });

  async function submit() {
    attempted = true;
    if (!canCreate || !tool) return;
    busy = true;
    createError = "";
    try {
      const repo = existing
        ? await initRepo(existing.path, tool.id, { ...toolValues })
        : await createRepo(root, name, tool.id, { ...toolValues });
      if (!existing) remember(LAST_ROOT, root);
      remember(LAST_TOOL, tool.id);
      oncreated(repo);
    } catch (e) {
      createError = `${e}`;
      busy = false;
      await tick();
      if (existing) focusFirst();
      else nameInput?.focus();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      // The window-level handler must not see this keystroke too.
      e.stopPropagation();
      void submit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (!busy) onback();
    }
  }

  // Arrow keys move the choice inside a radio group (Tab moves between groups).
  function onGroupKey(e: KeyboardEvent, items: string[], current: string, set: (v: string) => void) {
    const step =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (!step || items.length < 2) return;
    e.preventDefault();
    const i = Math.max(0, items.indexOf(current));
    const next = items[(i + step + items.length) % items.length];
    set(next);
    const group = e.currentTarget as HTMLElement;
    void tick().then(() =>
      group.querySelector<HTMLElement>('[aria-checked="true"]')?.focus(),
    );
  }

  const chip =
    "rounded border px-2.5 py-1 text-[12px] focus:outline-none focus-visible:ring-1 focus-visible:ring-sky-400/70";
  const chipOn = "border-sky-400/50 bg-sky-500/15 text-white";
  const chipOff = "border-hair text-white/55 hover:bg-white/10 hover:text-white/85";
  const field =
    "rounded border border-hair bg-white/[0.04] px-2 py-1.5 font-mono text-[12px] text-white/90 placeholder:text-white/25 focus:border-white/25 focus:outline-none disabled:opacity-60";
</script>

<div class="flex items-center gap-2 border-b border-hair px-3 py-2.5">
  <button
    type="button"
    class="shrink-0 rounded px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
    onclick={() => !busy && onback()}
    aria-label="Back"
  >
    ←
  </button>
  {#if existing}
    <span class="shrink-0 truncate text-[13px] font-medium text-white/80"
      >{existing.name}</span
    >
    <span class="shrink-0 text-white/15">/</span>
    <span class="text-[13px] text-white/60">Set up version control</span>
  {:else}
    <span class="text-[13px] font-medium text-white/80">New repository</span>
  {/if}
</div>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  bind:this={form}
  class="flex min-h-0 flex-1 flex-col overflow-y-auto px-5 py-4 text-[13px]"
  onkeydown={onKey}
>
  {#if loadError}
    <p class="text-red-300">Couldn't load the options: {loadError}</p>
  {:else if !options}
    <p class="text-white/40">Checking which version-control tools are installed…</p>
  {:else}
    <div class="space-y-4">
      {#if existing}
        <div class="space-y-1.5">
          <span class="block text-orange-400">Folder</span>
          <p class="font-mono text-[12px] text-white/70">{existing.path}</p>
          <p class="text-[12px] text-white/40">
            Nothing already in it is changed. If setup fails, only what it
            added is removed.
          </p>
        </div>
      {:else}
        <label class="block space-y-1.5">
          <span class="text-orange-400">Name</span>
          <input
            bind:this={nameInput}
            bind:value={name}
            disabled={busy}
            spellcheck="false"
            autocomplete="off"
            placeholder="my-new-thing"
            class="w-full {field}"
          />
          {#if nameProblem && (name || attempted)}
            <span class="block text-[12px] text-red-300">{nameProblem}</span>
          {:else if attempted && !name}
            <span class="block text-[12px] text-red-300">Enter a folder name.</span>
          {/if}
        </label>

        <div class="space-y-1.5">
          <span class="block text-orange-400">Location</span>
          {#if options.roots.length === 0}
            <p class="text-white/45">
              No root folders are set up yet. Add one in
              <span class="text-orange-400">Settings</span> first.
            </p>
          {:else if options.roots.length === 1}
            <p class="font-mono text-[12px] text-white/70">{options.roots[0]}</p>
          {:else}
            <div
              role="radiogroup"
              aria-label="Location"
              tabindex="-1"
              class="flex flex-wrap gap-1.5"
              onkeydown={(e) => onGroupKey(e, options!.roots, root, (v) => (root = v))}
            >
              {#each options.roots as r (r)}
                <button
                  type="button"
                  role="radio"
                  aria-checked={r === root}
                  tabindex={r === root ? 0 : -1}
                  disabled={busy}
                  onclick={() => (root = r)}
                  class="{chip} font-mono {r === root ? chipOn : chipOff}">{r}</button
                >
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <div class="space-y-1.5">
        <span class="block text-orange-400">Version control</span>
        {#if options.tools.length === 0}
          <p class="text-white/45">
            None of the supported tools (Git, Jujutsu, Mercurial, Breezy, Darcs)
            is on PATH. Install one, or add your own under
            <span class="font-mono">provision:</span> in rules.yaml.
          </p>
        {:else}
          <div
            role="radiogroup"
            aria-label="Version control"
            tabindex="-1"
            class="flex flex-wrap gap-1.5"
            onkeydown={(e) =>
              onGroupKey(
                e,
                options!.tools.map((t) => t.id),
                toolId,
                (v) => (toolId = v),
              )}
          >
            {#each options.tools as t (t.id)}
              <button
                type="button"
                role="radio"
                aria-checked={t.id === toolId}
                tabindex={t.id === toolId ? 0 : -1}
                disabled={busy}
                onclick={() => (toolId = t.id)}
                class="{chip} {t.id === toolId ? chipOn : chipOff}">{t.label}</button
              >
            {/each}
          </div>
        {/if}
      </div>

      {#if tool && tool.fields.length}
        <div class="flex flex-wrap gap-4">
          {#each tool.fields as f (f.id)}
            <label class="block w-56 space-y-1.5">
              <span class="text-orange-400">{f.label}</span>
              <input
                bind:value={values[toolId][f.id]}
                disabled={busy}
                spellcheck="false"
                autocomplete="off"
                class="w-full {field}"
              />
              {#if attempted && missingField?.id === f.id}
                <span class="block text-[12px] text-red-300">Fill this in.</span>
              {/if}
            </label>
          {/each}
        </div>
      {/if}

      {#if tool && (root || existing)}
        <div class="space-y-0.5 font-mono text-[11px] text-white/35">
          <div class="truncate" title={target}>
            → {target ? middleTruncate(target, 96) : "pick a name"}
          </div>
          {#each preview as line, i (i)}
            <div class="truncate" title={line}>$ {line}</div>
          {/each}
        </div>
      {/if}

      {#if createError}
        <pre
          class="max-h-32 overflow-auto whitespace-pre-wrap rounded border border-red-400/40 bg-red-500/10 px-3 py-2 font-mono text-[11px] text-red-200">{createError}</pre>
      {/if}
    </div>

    <div class="mt-auto flex justify-end pt-4">
      <button
        type="button"
        disabled={!canCreate}
        onclick={() => void submit()}
        class="rounded bg-sky-500/80 px-3.5 py-1.5 text-[12px] font-medium text-white hover:bg-sky-500 disabled:opacity-40"
      >
        {#if existing}{busy ? "Setting up…" : "Set up"}{:else}{busy
            ? "Creating…"
            : "Create"}{/if}
      </button>
    </div>
  {/if}
</div>
