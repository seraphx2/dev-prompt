// Shared update state. One poll feeds every surface: the Settings panel, the
// overlay footer chip, the tray tooltip, and a system notification.

import { invoke } from "@tauri-apps/api/core";
import { notify } from "./notify";
import { checkForUpdate, currentVersion, type UpdateInfo } from "./updater";
import { updaterMode, type UpdaterMode } from "./ipc";

export const upd = $state<{
  info: UpdateInfo | null;
  /** The running version, shown in the footer. Filled on the first poll. */
  current: string;
  checking: boolean;
  /** How this build updates. `null` until the first poll resolves it. Only
   *  `"self"` installs updates; `"unmanaged"` (a bare executable, a dev
   *  build) checks and links to the download; `"managed"` leaves it to the
   *  package manager. */
  mode: UpdaterMode | null;
  /** Why the last check asked for in Settings failed ("" = it didn't). */
  error: string;
}>({ info: null, current: "", checking: false, mode: null, error: "" });

/** Version we've already toasted about this session — don't repeat. */
let notified: string | null = null;

/**
 * Check GitHub for a newer release and update `upd.info`. A background check
 * that fails (offline, GitHub down, a malformed `latest.json`) is only logged —
 * the next launch / daily poll retries. A `manual` one (Settings ▸ About ▸
 * Check for updates) reports the failure in `upd.error` instead.
 */
export async function pollUpdates({ manual = false } = {}): Promise<void> {
  if (upd.checking) return;
  upd.checking = true;
  if (manual) upd.error = "";
  try {
    if (!upd.current) upd.current = await currentVersion().catch(() => "");
    if (upd.mode === null)
      upd.mode = await updaterMode().catch(() => "self" as const);
    // Package-manager installs update through the package manager; Settings
    // says so instead of checking.
    if (upd.mode === "managed") {
      upd.info = null;
      return;
    }
    // A dev build's version is the 0.1.0 placeholder, so every release looks
    // newer: check only when asked, or the footer chip would never go away.
    if (upd.mode === "unmanaged" && import.meta.env.DEV && !manual) return;

    const info = await checkForUpdate();
    upd.info = info;

    void invoke("set_update_hint", { version: info?.version ?? null }).catch(
      () => {},
    );
    if (info && info.version !== notified) {
      notified = info.version;
      void toast(info.version, upd.mode === "self");
    }
  } catch (e) {
    if (manual) upd.error = `${e}`;
    console.warn("update check failed:", e);
  } finally {
    upd.checking = false;
  }
}

/** The footer chip and tray show it too, so a missed notification costs nothing. */
function toast(version: string, installable: boolean): Promise<void> {
  return notify(
    "dev-prompt update available",
    installable
      ? `Version ${version} is ready to install — open Settings to apply.`
      : `Version ${version} is out — Settings ▸ About has the download link.`,
  );
}
