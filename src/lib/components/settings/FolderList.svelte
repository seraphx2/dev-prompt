<script lang="ts">
  import { onDestroy, tick, type Snippet } from "svelte";
  import { foldersExist, pickDirectories } from "../../ipc";
  import { addFolders, sortFolders, type Note } from "./model";
  import * as ui from "./ui";

  let {
    rows,
    what,
    placeholder,
    addLabel,
    confirmRemove = false,
    keepOneRow = false,
    onsave,
    note,
    report,
    children,
  }: {
    /** The form's list, edited in place. */
    rows: string[];
    /** What one row is, for messages: "root", "folder". */
    what: string;
    placeholder: string;
    addLabel: string;
    /** Ask before removing a row with a path in it. */
    confirmRemove?: boolean;
    /** Always leave one row to type into. */
    keepOneRow?: boolean;
    /** Saves the rows that name existing folders. */
    onsave: (good: string[]) => void;
    note: Note;
    report: (msg: string) => void;
    /** Extra buttons after Add / Browse. */
    children?: Snippet;
  } = $props();

  // A row is checked — and the list saved — when you leave it or press Enter,
  // not per keystroke. Only folders that exist are saved: a half-typed path
  // would rescan, and watch, a folder that isn't there.
  let problems = $state<(string | null)[]>([]);
  let confirmIdx = $state<number | null>(null);
  let inputs: HTMLInputElement[] = $state([]);
  /** Typed into since the last check. */
  let edited = false;
  /** Paths already saved. They stay saved even while missing — a drive that
   *  isn't plugged in — since only something newly typed has to exist. */
  // svelte-ignore state_referenced_locally
  const known = new Set(rows.map((r) => r.trim()).filter(Boolean));

  /** Checks every row, saves the ones that exist, returns the ones that don't. */
  async function check(): Promise<string[]> {
    edited = false;
    const typed = [...new Set(rows.map((r) => r.trim()).filter(Boolean))];
    let found: boolean[];
    try {
      found = typed.length ? await foldersExist(typed) : [];
    } catch (e) {
      report(`${e}`);
      return [];
    }
    const sorted = sortFolders(rows, new Map(typed.map((t, i) => [t, found[i] || known.has(t)])));
    problems = sorted.problems;
    for (const g of sorted.good) known.add(g);
    onsave(sorted.good);
    return sorted.bad;
  }

  // Leaving the page (or Settings) without leaving the field still saves
  // what's typed — and says what couldn't be.
  onDestroy(() => {
    if (!edited && !problems.some(Boolean)) return;
    void check().then((bad) => {
      for (const b of bad) report(`Didn't save ${what} ${b} — folder not found.`);
    });
  });

  async function add() {
    rows.push("");
    await tick();
    inputs[rows.length - 1]?.focus();
  }

  function remove(i: number) {
    rows.splice(i, 1);
    problems.splice(i, 1);
    if (keepOneRow && !rows.length) rows.push("");
    confirmIdx = null;
    void check();
  }

  async function browse() {
    try {
      const picked = await pickDirectories();
      if (!picked.length) return;
      const next = addFolders(rows, picked);
      rows.splice(0, rows.length, ...(next.length || !keepOneRow ? next : [""]));
      void check();
    } catch (e) {
      note(`${e}`, true);
    }
  }
</script>

{#each rows as _row, i (i)}
  <div>
    <div class="flex items-center gap-2">
      <input
        bind:this={inputs[i]}
        bind:value={rows[i]}
        oninput={() => {
          edited = true;
          problems[i] = null;
        }}
        onchange={() => void check()}
        spellcheck="false"
        {placeholder}
        aria-invalid={!!problems[i]}
        class="min-w-0 flex-1 {ui.mono} {problems[i] ? 'border-red-400/60!' : ''}"
      />
      {#if confirmIdx === i}
        <button
          type="button"
          onclick={() => remove(i)}
          class="shrink-0 rounded-sm border border-red-400/40 bg-red-500/15 px-2 py-1 text-[11px] text-red-300 hover:bg-red-500/25"
        >
          Remove
        </button>
        <button
          type="button"
          onclick={() => (confirmIdx = null)}
          class="shrink-0 rounded-sm border border-hair px-2 py-1 text-[11px] text-white/50 hover:bg-white/10"
        >
          Cancel
        </button>
      {:else}
        <button
          type="button"
          onclick={() => (confirmRemove && rows[i].trim() ? (confirmIdx = i) : remove(i))}
          aria-label="Remove {what}"
          title="Remove {what}"
          class="shrink-0 rounded-sm border border-hair px-2 py-1 text-[11px] text-white/40 hover:bg-white/10 hover:text-white/70"
        >
          ×
        </button>
      {/if}
    </div>
    {#if problems[i]}
      <p class="mt-0.5 text-[11px] text-red-300">{problems[i]} It isn't saved.</p>
    {/if}
  </div>
{/each}
<div class="flex gap-2">
  <button type="button" onclick={add} class={ui.smallBtn}>{addLabel}</button>
  <button type="button" onclick={browse} class={ui.smallBtn}>Browse…</button>
  {@render children?.()}
</div>
