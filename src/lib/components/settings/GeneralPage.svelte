<script lang="ts">
  import { onMount } from "svelte";
  import { getAutostart, isFlatpak, setAutostart, setAutostartPortal } from "../../ipc";
  import type { AppConfig, ConfigPatch } from "../../types";
  import HotkeyRecorder from "./HotkeyRecorder.svelte";
  import type { Form, Note, Save } from "./model";
  import * as ui from "./ui";

  let {
    form,
    save,
    commit,
    note,
  }: {
    form: Form;
    save: Save;
    commit: (patch: ConfigPatch) => Promise<AppConfig | null>;
    note: Note;
  } = $props();

  let autostart = $state(false);
  let flatpak = $state(false);
  let hotkeyBusy = $state(false);

  onMount(async () => {
    flatpak = await isFlatpak().catch(() => false);
    autostart = await getAutostart().catch(() => false);
  });

  // The OS is the source of truth for autostart, not config.yaml. Under
  // Flatpak it goes through the Background portal, which asks for consent the
  // first time and returns the state it actually granted.
  async function toggleAutostart() {
    try {
      if (flatpak) autostart = await setAutostartPortal(autostart);
      else await setAutostart(autostart);
    } catch (e) {
      autostart = !autostart;
      note(`${e}`, true);
    }
  }

  // A hotkey only shows its new value once the OS has registered it.
  async function setHotkey(patch: { hotkey?: string; apps_hotkey?: string }) {
    hotkeyBusy = true;
    const c = await commit(patch);
    hotkeyBusy = false;
    if (c) {
      form.hotkey = c.hotkey;
      form.appsHotkey = c.apps_hotkey ?? "";
    }
  }
</script>

<div class="space-y-1">
  <label class="flex w-fit cursor-pointer items-center gap-2">
    <input
      type="checkbox"
      bind:checked={autostart}
      onchange={toggleAutostart}
      class={ui.checkbox}
    />
    <span class={ui.label}>Start at login</span>
  </label>
  {#if flatpak}
    <p class="pl-6 {ui.help}">Your desktop will ask permission the first time.</p>
  {/if}
</div>

<div class="space-y-2">
  <span class={ui.label}>Global hotkeys</span>
  <div class="grid grid-cols-2 gap-4">
    <HotkeyRecorder
      label="Repo browser"
      value={form.hotkey}
      busy={hotkeyBusy}
      onsave={(a) => setHotkey({ hotkey: a })}
    />
    <HotkeyRecorder
      label="App launcher"
      value={form.appsHotkey}
      clearable
      busy={hotkeyBusy}
      onsave={(a) => setHotkey({ apps_hotkey: a })}
    />
  </div>
  <p class={ui.help}>
    The app launcher hotkey opens the overlay straight into the
    <span class="font-mono">›</span> installed-apps view.
  </p>
</div>

<label class="block w-fit space-y-1.5">
  <span class={ui.label}>Overlay dismissal</span>
  <select
    bind:value={form.dismiss}
    onchange={() => save({ dismiss: form.dismiss })}
    title="When the overlay closes itself"
    class="w-80 {ui.select}"
  >
    <option value="always">On focus loss &amp; after an action (default)</option>
    <option value="keep_on_blur">Keep open on focus loss; close after an action</option>
    <option value="manual">Keep open until Esc</option>
  </select>
</label>
