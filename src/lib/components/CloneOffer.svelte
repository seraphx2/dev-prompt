<script lang="ts">
  import { cloneLabel } from "../clone";
  import { icons } from "../icons";

  // What the repo list shows instead of results when the search box holds
  // something to clone. Same frame as ResultList, so the footer stays put.
  let {
    source,
    fromGitHub,
    onclone,
  }: {
    /** The URL (or owner/repo) as typed. */
    source: string;
    /** It's the owner/repo shorthand, so GitHub is assumed. */
    fromGitHub: boolean;
    onclone: () => void;
  } = $props();
</script>

<div class="scroll-thin flex-1 overflow-y-auto px-2 py-2">
  <button
    type="button"
    onclick={onclone}
    class="flex w-full items-center gap-3 rounded-lg bg-white/10 px-3 py-2 text-left"
  >
    <svg class="h-5 w-5 shrink-0" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      {@html icons.branch.raw}
    </svg>
    <span class="min-w-0 flex-1">
      <span class="block truncate text-[15px] text-white/90">Clone {cloneLabel(source)}</span>
      <span class="block truncate text-[12px] text-white/50"
        >{fromGitHub ? "From GitHub. " : ""}Pick where it goes and what clones it next.</span
      >
    </span>
  </button>
</div>
