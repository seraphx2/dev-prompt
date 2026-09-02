<script lang="ts">
  import { classifyHotkey } from "../hotkeys";

  let {
    label,
    value,
    clearable = false,
    busy = false,
    onsave,
  }: {
    label: string;
    /** The currently-stored accelerator (for display). */
    value: string;
    /** Show a "turn off" affordance — passes "" to `onsave`. */
    clearable?: boolean;
    busy?: boolean;
    /** Called with a validated accelerator (or "" to clear). */
    onsave: (accel: string) => void | Promise<void>;
  } = $props();

  let capturing = $state(false);
  let hint = $state("");
  let pending = $state("");
  let pendingReason = $state("");

  function codeToKey(code: string, fallback: string): string {
    if (code.startsWith("Key")) return code.slice(3); // KeyA -> A
    if (code.startsWith("Digit")) return code.slice(5); // Digit1 -> 1
    return code || fallback; // Space, F5, Comma, ArrowUp, Minus, …
  }

  function start() {
    hint = "";
    pending = "";
    pendingReason = "";
    capturing = true;
  }

  function apply(accel: string) {
    const v = classifyHotkey(accel);
    if (v.level === "block") {
      capturing = false;
      hint = `${v.reason} Pick another.`;
      return;
    }
    if (v.level === "warn") {
      capturing = false;
      hint = "";
      pending = accel;
      pendingReason = v.reason ?? "";
      return;
    }
    capturing = false;
    hint = "";
    void onsave(accel);
  }

  function onKey(e: KeyboardEvent) {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      capturing = false;
      hint = "";
      return;
    }
    if (["Control", "Alt", "Shift", "Meta", "OS"].includes(e.key)) {
      hint = "…keep going";
      return;
    }

    const mods: string[] = [];
    if (e.ctrlKey) mods.push("CmdOrCtrl");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    if (e.metaKey) mods.push("Super");
    apply([...mods, codeToKey(e.code, e.key)].join("+"));
  }

  function confirmPending() {
    const p = pending;
    pending = "";
    pendingReason = "";
    void onsave(p);
  }
</script>

<div class="space-y-1.5">
  <div class="flex items-baseline justify-between">
    <span class="text-[12px] text-white/70">{label}</span>
    {#if clearable && value}
      <button
        type="button"
        disabled={busy}
        onclick={() => onsave("")}
        class="text-[11px] text-white/30 underline decoration-white/20 underline-offset-2 hover:text-white/60 disabled:opacity-50"
        >turn off</button
      >
    {/if}
  </div>

  <button
    type="button"
    onclick={start}
    onkeydown={onKey}
    class="flex w-full items-center justify-between rounded border px-2 py-1.5 text-left transition-colors
           {capturing
      ? 'border-orange-400/60 bg-orange-400/[0.08]'
      : 'border-hair bg-white/[0.04] hover:border-white/25'}"
  >
    <span
      class="font-mono text-[12px] {capturing ? 'text-white/40' : 'text-white/90'}"
    >
      {capturing ? "Press a combination…" : value || (clearable ? "off" : "not set")}
    </span>
    <span class="shrink-0 text-[10px] uppercase tracking-wide text-white/30">
      {capturing ? "Esc cancels" : "click to record"}
    </span>
  </button>

  {#if pending}
    <div
      class="rounded border border-amber-400/40 bg-amber-400/[0.07] px-2.5 py-2 text-[11px]"
    >
      <div class="text-amber-200/90">
        <span class="font-mono text-white/90">{pending}</span> — {pendingReason}
      </div>
      <div class="mt-1.5 flex gap-2">
        <button
          type="button"
          disabled={busy}
          onclick={confirmPending}
          class="rounded border border-amber-400/50 bg-amber-400/15 px-2 py-1 text-amber-100 hover:bg-amber-400/25 disabled:opacity-50"
          >Use it anyway</button
        >
        <button
          type="button"
          onclick={start}
          class="rounded border border-hair px-2 py-1 text-white/50 hover:bg-white/10"
          >Pick another</button
        >
      </div>
    </div>
  {/if}

  {#if hint}
    <span class="block text-[11px] text-amber-300/70">{hint}</span>
  {/if}
</div>
