<script lang="ts">
  let {
    text,
    indices = [],
  }: {
    text: string;
    /** Character positions in `text` to emphasise. */
    indices?: number[];
  } = $props();

  const hit = $derived(new Set(indices));
</script>

{#each text.split("") as ch, i}<span class={hit.has(i) ? "text-sky-300" : ""}
  >{ch}</span
>{/each}
