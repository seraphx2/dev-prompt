<script lang="ts">
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { iconKeys, icons } from "../../icons";
  import { glyphColor } from "../../glyph";
  import type { Note } from "./model";
  import * as ui from "./ui";

  let { note }: { note: Note } = $props();

  async function copy(k: string) {
    try {
      await writeText(`icon: ${k}`);
      note(`Copied "icon: ${k}"`);
    } catch (e) {
      note(`${e}`, true);
    }
  }
</script>

<p class={ui.help}>
  {iconKeys.length} keys for <span class="font-mono">icon:</span> in rules.yaml. Click one to copy it.
</p>

<div class="grid grid-cols-[repeat(auto-fill,minmax(76px,1fr))] gap-1">
  {#each iconKeys as k (k)}
    {@const g = icons[k]}
    {@const color = glyphColor(g)}
    <button
      type="button"
      onclick={() => copy(k)}
      title={`Copy "icon: ${k}"`}
      class="flex flex-col items-center gap-1.5 rounded-sm px-1 py-2.5 hover:bg-white/6"
    >
      <svg
        class="h-5 w-5 {color ? '' : 'text-white/45'}"
        viewBox={g.vb ?? "0 0 24 24"}
        fill="currentColor"
        fill-rule="evenodd"
        style={color ? `color:${color}` : ""}
      >
        {#if g.raw}{@html g.raw}{:else}<path d={g.d} />{/if}
      </svg>
      <span class="w-full truncate text-center font-mono text-[9px] text-white/40">{k}</span>
    </button>
  {/each}
</div>
