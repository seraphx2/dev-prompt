// Shared update state. One poll feeds every surface: the Settings panel, the
// overlay footer chip, the tray tooltip, and a system notification.

import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { checkForUpdate, currentVersion, type UpdateInfo } from "./updater";

export const upd = $state<{
  info: UpdateInfo | null;
  /** The running version, shown in the footer. Filled on the first poll. */
  current: string;
  checking: boolean;
  /** Result text for the manual check ("" when quiet / an update is pending). */
  note: string;
}>({ info: null, current: "", checking: false, note: "" });

/** Version we've already toasted about this session — don't repeat. */
let notified: string | null = null;

/**
 * Check GitHub for a newer release. `loud` surfaces a "you're up to date" /
 * error note for the manual button; the silent path only updates `upd.info`.
 */
export async function pollUpdates(loud = false): Promise<void> {
  if (upd.checking) return;
  upd.checking = true;
  if (loud) upd.note = "";
  try {
    if (!upd.current) upd.current = await currentVersion().catch(() => "");
    const info = await checkForUpdate();
    upd.info = info;
    if (loud) upd.note = info ? "" : "You're on the latest version.";

    void invoke("set_update_hint", { version: info?.version ?? null }).catch(
      () => {},
    );
    if (info && info.version !== notified) {
      notified = info.version;
      void toast(info.version);
    }
  } catch (e) {
    if (loud) upd.note = `${e}`;
  } finally {
    upd.checking = false;
  }
}

async function toast(version: string): Promise<void> {
  try {
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) {
      sendNotification({
        title: "dev-prompt update available",
        body: `Version ${version} is ready to install — open Settings to apply.`,
      });
    }
  } catch {
    /* notifications unavailable — the footer chip and tray still show it */
  }
}
