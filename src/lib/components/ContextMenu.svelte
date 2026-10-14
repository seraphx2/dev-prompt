<script lang="ts">
  import { onMount, tick } from "svelte";
  import { icons } from "../icons";
  import type { CtxItem } from "../types";

  let {
    x,
    y,
    items,
    onclose,
  }: {
    /** Where the pointer was, in window coordinates. */
    x: number;
    y: number;
    items: CtxItem[];
    onclose: () => void;
  } = $props();

  let menu: HTMLDivElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);
  /** Keyboard highlight: an index into `items`, -1 for none yet. */
  let sel = $state(-1);

  // A pick-one menu (any item with `checked`) reserves a check column on every
  // row, so the icons and labels line up whichever one is ticked.
  const checkable = $derived(items.some((it) => it.kind === "item" && it.checked !== undefined));

  const selectable = (i: number) => {
    const it = items[i];
    return it?.kind === "item" && !it.disabled;
  };

  // Open at the pointer, flipped left / up when it would run off the window.
  async function place() {
    await tick();
    if (!menu) return;
    const pad = 6;
    const { width, height } = menu.getBoundingClientRect();
    left = x + width + pad > window.innerWidth ? Math.max(pad, x - width) : x;
    top = y + height + pad > window.innerHeight ? Math.max(pad, y - height) : y;
  }
  onMount(place);
  // The menu can grow when fetched items arrive; keep it on screen.
  $effect(() => {
    void items.length;
    void place();
  });

  function run(i: number) {
    const it = items[i];
    if (it?.kind !== "item" || it.disabled) return;
    onclose();
    it.run();
  }

  function step(dir: 1 | -1) {
    for (let k = 1; k <= items.length; k++) {
      const i = (sel + dir * k + items.length * 2) % items.length;
      if (selectable(i)) {
        sel = i;
        return;
      }
    }
  }

  /** Keys while the menu is open — called by the window-level handler, which
   *  hands everything to the menu until it closes. */
  export function keydown(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") onclose();
    else if (e.key === "ArrowDown") step(1);
    else if (e.key === "ArrowUp") step(-1);
    else if (e.key === "Enter" && sel >= 0) run(sel);
  }
</script>

<svelte:window onblur={onclose} onresize={onclose} />

<!-- Any click outside closes the menu, without reaching what's underneath. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 z-40"
  onmousedown={(e) => {
    e.preventDefault();
    onclose();
  }}
  oncontextmenu={(e) => {
    e.preventDefault();
    onclose();
  }}
  onwheel={onclose}
></div>

<div
  bind:this={menu}
  role="menu"
  tabindex="-1"
  class="fixed z-50 min-w-52 max-w-88 rounded-lg border border-white/10
         bg-[#1c1f26]/95 p-1 shadow-xl shadow-black/50 backdrop-blur-xl"
  style="left:{left}px; top:{top}px"
>
  {#each items as item, i (i)}
    {#if item.kind === "separator"}
      <div class="mx-2 my-1 border-t border-white/10"></div>
    {:else if item.kind === "header"}
      <div class="truncate px-2.5 pb-1 pt-1.5 text-[11px] text-white/40" title={item.label}>
        {item.label}
      </div>
    {:else if item.kind === "loading"}
      <div class="flex items-center gap-2.5 px-2.5 py-1.5 text-[13px] text-white/35">
        <svg class="h-3.5 w-3.5 shrink-0 animate-spin" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" stroke-width="2.5">
          <path d="M12 3a9 9 0 1 0 9 9" stroke-linecap="round" />
        </svg>
        {item.label}
      </div>
    {:else}
      {@const g = item.icon ? icons[item.icon] : undefined}
      <button
        type="button"
        role={checkable ? "menuitemradio" : "menuitem"}
        aria-checked={checkable ? !!item.checked : undefined}
        disabled={item.disabled}
        class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-[13px]
               {item.disabled
          ? 'cursor-default text-white/25'
          : item.danger
            ? i === sel
              ? 'bg-red-500/20 text-red-200'
              : 'text-red-300 hover:bg-red-500/20 hover:text-red-200'
            : i === sel
              ? 'bg-white/10 text-white'
              : 'text-white/85 hover:bg-white/10'}"
        onpointerenter={() => (sel = item.disabled ? -1 : i)}
        onclick={() => run(i)}
      >
        {#if checkable}
          <span class="-mr-1 flex w-3.5 shrink-0 justify-center">
            {#if item.checked}
              <svg class="h-3.5 w-3.5 text-sky-300" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M5 12.5l4.5 4.5L19 7.5" />
              </svg>
            {/if}
          </span>
        {/if}
        <svg
          class="h-3.5 w-3.5 shrink-0 {item.disabled
            ? 'opacity-40'
            : item.danger
              ? 'text-red-300'
              : 'text-white/60'}"
          viewBox={g?.vb ?? "0 0 24 24"}
          fill="currentColor"
          fill-rule="evenodd"
          aria-hidden="true"
          style={g?.hex ? `color:${g.hex}` : ""}
        >
          {#if g?.raw}{@html g.raw}{:else if g?.d}<path d={g.d} />{/if}
        </svg>
        <span class="truncate">{item.label}</span>
      </button>
    {/if}
  {/each}
</div>
