<script lang="ts">
  let {
    show,
    onclear,
  }: {
    /** Render only when the field has content worth clearing. */
    show: boolean;
    onclear: () => void;
  } = $props();
</script>

{#if show}
  <button
    type="button"
    tabindex="-1"
    title="Clear (Del)"
    onclick={onclear}
    class="shrink-0 rounded-[3px] border border-white/15 bg-white/[0.09] px-1.5 py-0.5
           text-[10px] font-medium leading-none text-white/90 transition-colors
           hover:border-white/25 hover:bg-white/20"
  >
    Del
  </button>
{/if}
