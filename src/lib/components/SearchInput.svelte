<script lang="ts">
  import ClearButton from "./ClearButton.svelte";

  let {
    value = $bindable(),
    placeholder = "Search repositories…",
    onkeydown,
  }: {
    value: string;
    placeholder?: string;
    onkeydown?: (e: KeyboardEvent) => void;
  } = $props();

  let el: HTMLInputElement | undefined = $state();

  export function focus() {
    el?.focus();
    el?.select();
  }
</script>

<div class="flex items-center gap-3 border-b border-hair px-4 py-3">
  <svg
    class="h-4 w-4 shrink-0 text-white/30"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
  >
    <circle cx="11" cy="11" r="7" />
    <path d="m21 21-4.3-4.3" />
  </svg>
  <input
    bind:this={el}
    bind:value
    {placeholder}
    {onkeydown}
    spellcheck="false"
    autocomplete="off"
    autocapitalize="off"
    class="min-w-0 flex-1 text-[15px] text-white/90 placeholder:text-white/25"
  />
  <ClearButton
    show={!!value}
    onclear={() => {
      value = "";
      focus();
    }}
  />
</div>
