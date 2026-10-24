/**
 * Every repo's working-copy status, by path, kept in step with the backend
 * (status.rs): loaded once, then updated as each background read lands.
 */
import { SvelteMap } from "svelte/reactivity";
import { listStatuses, onStatusUpdated } from "./ipc";
import type { RepoStatus } from "./types";

export const statuses = new SvelteMap<string, RepoStatus>();

/** Replace the whole set with the backend's (startup, after a rescan). */
export async function loadStatuses(): Promise<void> {
  const all = await listStatuses();
  for (const p of [...statuses.keys()]) if (!(p in all)) statuses.delete(p);
  for (const [p, s] of Object.entries(all)) statuses.set(p, s);
}

/** Follow the backend's reads; returns the unlistener. */
export function followStatuses() {
  return onStatusUpdated((path, s) => {
    if (s) statuses.set(path, s);
    else statuses.delete(path);
  });
}

/** Put a status the caller already has (a git operation's result). */
export function setStatus(path: string, s: RepoStatus | null | undefined) {
  if (s) statuses.set(path, s);
  else statuses.delete(path);
}
