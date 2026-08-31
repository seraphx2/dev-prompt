// Thin wrapper over the Tauri updater plugin. The overlay stays out of the way;
// update status surfaces only in the Settings screen.

import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

export interface UpdateInfo {
  version: string;
  notes: string;
  date: string | null;
}

let pending: Update | null = null;

export function currentVersion(): Promise<string> {
  return getVersion();
}

/** Returns details of an available update, or null when up to date. */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  const update = await check();
  if (!update) {
    pending = null;
    return null;
  }
  pending = update;
  return {
    version: update.version,
    notes: update.body ?? "",
    date: update.date ?? null,
  };
}

/** Download + install the update found by the last checkForUpdate(), then
 *  relaunch into the new version. */
export async function installUpdate(): Promise<void> {
  if (!pending) {
    const update = await check();
    if (!update) return;
    pending = update;
  }
  await pending.downloadAndInstall();
  await relaunch();
}
