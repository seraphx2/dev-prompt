<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { cancelRead, checkNewRepo, clonePlan, createRepo, initRepo, newRepoOptions } from "../ipc";
  import type { CloneJob, CloneRequest } from "../clone";
  import type { ClonePlan, NewRepoOptions, ProvisionTool, Repo } from "../types";
  import { middleTruncate } from "../text";

  let {
    /** Set up version control in this existing folder instead of creating one. */
    existing = null,
    /** Clone this (a URL, or GitHub's owner/repo) instead of creating a folder. */
    clone = null,
    job = null,
    onclone,
    oncancelclone,
    oncreated,
    onfailed,
    onback,
  }: {
    existing?: { path: string; name: string } | null;
    clone?: string | null;
    /** The clone under way or last failed (App runs it, so it outlives this
     *  form): this form's own when its `source` is `clone`. */
    job?: CloneJob | null;
    /** Start the clone — App runs it in the background. */
    onclone?: (req: CloneRequest) => void;
    /** Stop this form's clone (confirmed here first). */
    oncancelclone?: () => void;
    oncreated: (repo: Repo) => void;
    /** It failed — the form shows why; this is for telling someone who
     *  isn't looking (the overlay was hidden meanwhile). */
    onfailed?: (message: string) => void;
    onback: () => void;
  } = $props();

  // Last location / tool used, so a repeat visit is "type a name, Enter".
  const LAST_ROOT = "newRepo.root";
  const LAST_TOOL = "newRepo.tool";
  const LAST_CLONE_TOOL = "newRepo.cloneTool";
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

  // Clone mode: what's at the URL, worked out as it's typed.
  let url = $state("");
  let plan = $state<ClonePlan | null>(null);
  let planError = $state("");
  let planning = $state(false);
  /** The name was typed by hand — a new plan no longer replaces it. */
  let nameEdited = $state(false);
  /** Enter came before the plan did: clone as soon as it's in. */
  let submitWhenPlanned = false;

  /** The clone this form started is under way: it's shown here, the fields
   *  are locked, and Back leaves it running. */
  const ownJob = $derived(clone !== null && !!job && job.source === clone ? job : null);
  const cloning = $derived(ownJob?.state === "running");
  /** "Cancel cloning" was pressed: ask before stopping it. */
  let confirmingCancel = $state(false);
  $effect(() => {
    if (!cloning) confirmingCancel = false;
  });
  /** Another clone is running — one at a time. */
  const otherClone = $derived(clone !== null && job?.state === "running" && !ownJob ? job : null);
  /** Inputs locked: a create / setup in flight, or this form's clone. */
  const locked = $derived(busy || cloning);
  /** What went wrong: this form's create / setup, or its clone. */
  const shownError = $derived(ownJob?.state === "failed" ? (ownJob.error ?? "") : createError);

  /** The tools on offer: the installed setups, or in clone mode the ones that
   *  can clone this URL. */
  const tools = $derived<ProvisionTool[]>(clone !== null ? (plan?.tools ?? []) : (options?.tools ?? []));
  const tool = $derived<ProvisionTool | undefined>(tools.find((t) => t.id === toolId));
  const toolValues = $derived(values[toolId] ?? {});
  const missingField = $derived(
    tool?.fields.find((f) => !(toolValues[f.id] ?? "").trim()),
  );
  const canCreate = $derived(
    !locked &&
      !otherClone &&
      !!tool &&
      !missingField &&
      (clone === null || (!planning && !!plan)) &&
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

  // Left with an address still being looked into: stop the probes (each new
  // address stops the last one's too).
  onDestroy(() => {
    if (planning) void cancelRead("plan");
  });

  onMount(async () => {
    // The field starts with what was pasted, then it's the user's to edit.
    // Reopened for a clone that ran: its own name and location too.
    if (clone !== null) url = clone;
    if (ownJob) {
      name = ownJob.name;
      nameEdited = true;
    }
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
        const want = ownJob?.root ?? wantRoot;
        root = o.roots.find((r) => r === want) ?? o.roots[0] ?? "";
      }
      if (clone === null) {
        toolId = o.tools.find((t) => t.id === wantTool)?.id ?? o.tools[0]?.id ?? "";
      }
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

  // Clone mode: re-plan a moment after the URL stops changing. Sequenced so a
  // slow answer for an older URL can't overwrite a newer one.
  let planSeq = 0;
  $effect(() => {
    if (clone === null) return;
    const u = url.trim();
    const seq = ++planSeq;
    plan = null;
    planError = "";
    if (!u) {
      planning = false;
      return;
    }
    planning = true;
    const timer = setTimeout(() => {
      void clonePlan(u)
        .then((p) => {
          if (seq !== planSeq) return;
          plan = p;
          if (!nameEdited) name = p.name;
          const want = ownJob?.tool ?? recall(LAST_CLONE_TOOL);
          toolId = p.tools.find((t) => t.id === want)?.id ?? p.tools[0]?.id ?? "";
        })
        .catch((e) => {
          if (seq === planSeq) planError = `${e}`;
        })
        .finally(() => {
          if (seq !== planSeq) return;
          planning = false;
          if (submitWhenPlanned) {
            submitWhenPlanned = false;
            void submit();
          }
        });
    }, 600);
    return () => clearTimeout(timer);
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
    if (clone !== null && planning) {
      submitWhenPlanned = true;
      return;
    }
    if (!canCreate || !tool) return;
    if (clone !== null && plan) {
      // App runs it, so leaving this form doesn't stop it.
      remember(LAST_ROOT, root);
      remember(LAST_CLONE_TOOL, tool.id);
      onclone?.({ root, name, url: plan.url, tool: tool.id, source: clone });
      return;
    }
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
      onfailed?.(createError);
      await tick();
      if (existing) focusFirst();
      else nameInput?.focus();
    }
  }

  /** A create / clone / setup is running — App keeps the form through a hide. */
  export function working(): boolean {
    return locked;
  }

  /** Leave the form, unless a create is in flight. Also App's Escape fallback
   *  for when focus sits outside the form. */
  export function back() {
    // A clone carries on without the form (the banner shows it); a create
    // or setup is quick, and finishes here.
    if (!busy) onback();
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
      back();
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
    "rounded-sm border px-2.5 py-1 text-[12px] focus:outline-hidden focus-visible:ring-1 focus-visible:ring-sky-400/70";
  const chipOn = "border-sky-400/50 bg-sky-500/15 text-white";
  const chipOff = "border-hair text-white/55 hover:bg-white/10 hover:text-white/85";
  const quiet =
    "rounded-sm border border-hair px-3.5 py-1.5 text-[12px] text-white/70 hover:bg-white/10 hover:text-white/90";
  const danger =
    "rounded-sm border border-red-400/50 bg-red-500/15 px-3.5 py-1.5 text-[12px] font-medium text-red-200 hover:bg-red-500/25";
  const field =
    "rounded-sm border border-hair bg-white/4 px-2 py-1.5 font-mono text-[12px] text-white/90 placeholder:text-white/25 focus:border-white/25 focus:outline-hidden disabled:opacity-60";
</script>

<div class="flex items-center gap-2 border-b border-hair px-3 py-2.5">
  <button
    type="button"
    class="shrink-0 rounded-sm px-1.5 py-0.5 text-white/40 hover:bg-white/10 hover:text-white/70"
    onclick={back}
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
  {:else if clone !== null}
    <span class="text-[13px] font-medium text-white/80">Clone repository</span>
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
        <!-- From and Name side by side, headings on one line; they wrap under
             each other when there isn't room. -->
        <div class="flex flex-wrap items-start gap-4">
          {#if clone !== null}
            <label class="block min-w-64 flex-[2] space-y-1.5">
              <span class="block text-orange-400">From</span>
              <input
                bind:value={url}
                disabled={locked}
                spellcheck="false"
                autocomplete="off"
                placeholder="https://github.com/owner/repo"
                class="w-full {field}"
              />
              {#if planning}
                <span class="block text-[12px] text-white/50">Checking what's there…</span>
              {:else if planError}
                <span class="block whitespace-pre-wrap text-[12px] text-red-300">{planError}</span>
              {:else if plan && plan.url !== url.trim()}
                <span class="block font-mono text-[12px] text-white/50">{plan.url}</span>
              {/if}
            </label>
          {/if}

          <label class="block min-w-48 flex-1 space-y-1.5">
            <span class="block text-orange-400">Name</span>
            <input
              bind:this={nameInput}
              bind:value={name}
              oninput={() => (nameEdited = true)}
              disabled={locked}
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
        </div>

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
                  disabled={locked}
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
        {#if clone !== null && !plan}
          <p class="text-white/45">{planning ? "…" : "Enter what to clone."}</p>
        {:else if tools.length === 0}
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
                tools.map((t) => t.id),
                toolId,
                (v) => (toolId = v),
              )}
          >
            {#each tools as t (t.id)}
              <button
                type="button"
                role="radio"
                aria-checked={t.id === toolId}
                tabindex={t.id === toolId ? 0 : -1}
                disabled={locked}
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
                disabled={locked}
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
        <div class="space-y-0.5 font-mono text-[11px] text-white/50">
          <div class="truncate" title={target}>
            → {target ? middleTruncate(target, 96) : "pick a name"}
          </div>
          {#each preview as line, i (i)}
            <div class="truncate" title={line}>$ {line}</div>
          {/each}
        </div>
      {/if}

      {#if shownError}
        <pre
          class="max-h-32 overflow-auto whitespace-pre-wrap rounded-sm border border-red-400/40 bg-red-500/10 px-3 py-2 font-mono text-[11px] text-red-200">{shownError}</pre>
      {/if}
    </div>

    <div class="mt-auto flex items-center justify-end gap-3 pt-4">
      {#if cloning && confirmingCancel}
        <span class="text-[12px] text-white/70">Stop cloning? The half-cloned folder will be removed.</span>
        <button type="button" onclick={() => (confirmingCancel = false)} class={quiet}>Keep cloning</button>
        <button
          type="button"
          onclick={() => {
            confirmingCancel = false;
            oncancelclone?.();
          }}
          class={danger}>Stop cloning</button
        >
      {:else if cloning}
        <span class="text-[12px] text-white/50"
          >A big repository can take a few minutes. Esc goes back — it keeps going.</span
        >
        <button type="button" onclick={() => (confirmingCancel = true)} class={danger}>Cancel cloning</button>
      {:else if otherClone}
        <span class="text-[12px] text-amber-200/80"
          >{otherClone.name} is still cloning. Wait for it, or cancel it below.</span
        >
      {/if}
      <button
        type="button"
        disabled={!canCreate}
        onclick={() => void submit()}
        class="rounded-sm bg-sky-500/80 px-3.5 py-1.5 text-[12px] font-medium text-white hover:bg-sky-500 disabled:opacity-40"
      >
        {#if existing}{busy ? "Setting up…" : "Set up"}{:else if clone !== null}{cloning
            ? "Cloning…"
            : "Clone"}{:else}{busy
            ? "Creating…"
            : "Create"}{/if}
      </button>
    </div>
  {/if}
</div>
