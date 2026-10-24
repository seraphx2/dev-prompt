// The app with its backend replaced by demo-data.ts — loaded by shots.html,
// driven by capture.mjs. Tauri's own mocks stand in for the IPC layer, so the
// frontend runs unmodified: every `invoke` lands in `backend` below, events go
// through the mocked event plugin, and shell output streams down the same
// Channel the real pty.rs uses.

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fuzzyScore } from "../../src/lib/fuzzy";
import type { AppConfig, ScoredRepo } from "../../src/lib/types";
import * as demo from "./demo-data";

declare global {
  interface Window {
    /** Set by capture.mjs: resize the page the way the real window grows. */
    shotsResize?: (large: boolean) => Promise<void>;
    /** Set by capture.mjs for the demo GIF: stream shell output line by line. */
    shotsStreamSlowly?: boolean;
  }
}

type ChannelRef = { id: number };
type Args = Record<string, unknown>;

const internals = () =>
  (window as unknown as { __TAURI_INTERNALS__: { runCallback(id: number, data: unknown): void } })
    .__TAURI_INTERNALS__;

const config: AppConfig = {
  hotkey: "CmdOrCtrl+Backslash",
  apps_hotkey: "CmdOrCtrl+Period",
  roots: [demo.ROOT],
  scan: { max_depth: 4, collapse_nested: true, top_level_folders: true },
  cache_ttl_secs: 900,
  dismiss: "manual",
  apps: { enabled: true, extra_dirs: [], exclude: [] },
};

function search(query: string): ScoredRepo[] {
  const q = query.trim();
  const all = [...demo.repos].sort((a, b) => a.name.localeCompare(b.name));
  if (!q) return all.map((repo) => ({ repo, score: 0, matchIndices: [] }));
  const hits: ScoredRepo[] = [];
  for (const repo of all) {
    const hit = fuzzyScore(q, repo.name);
    if (hit) hits.push({ repo, score: hit.score, matchIndices: hit.positions });
  }
  return hits.sort((a, b) => b.score - a.score);
}

let nextShell = 1;
const encoder = new TextEncoder();

function backend(cmd: string, args: Args): unknown {
  switch (cmd) {
    case "get_config":
      return config;
    case "list_repos":
      return { repos: demo.repos, ageSecs: 20, stale: false };
    case "search_repos":
      return search(String(args.query ?? ""));
    case "build_universal_actions":
      return demo.universal(String(args.path));
    case "build_detected_actions":
      return demo.detected(String(args.path));
    case "build_actions":
      return [...demo.universal(String(args.path)), ...demo.detected(String(args.path))];
    case "list_apps":
      return { apps: demo.apps, ageSecs: 20, stale: false };
    case "list_shells":
      return ["bash", "fish", "zsh"];
    case "default_shell":
      return "zsh";
    case "shell_open": {
      const id = nextShell++;
      const out = args.onData as ChannelRef;
      const text = demo.transcript(String(args.cwd));
      // Stills get it all at once; the demo GIF a line at a time, the way a
      // command prints. Either way only after `shell_open` returns — the real
      // backend's first bytes arrive once the shell has started.
      const chunks = window.shotsStreamSlowly ? text.split(/(?<=\r\n)/) : [text];
      chunks.forEach((chunk, index) => {
        setTimeout(() => {
          const bytes = encoder.encode(chunk);
          internals().runCallback(out.id, { index, message: bytes.buffer });
        }, 30 + index * 70);
      });
      return { id, label: "zsh" };
    }
    case "set_overlay_size":
      return window.shotsResize?.(Boolean(args.large)) ?? null;
    case "new_repo_options":
      return demo.newRepoOptions;
    case "check_new_repo":
      return null;
    case "folders_exist":
      return (args.paths as string[]).map(() => true);
    case "assess_removal":
      return demo.assessment(String(args.path));
    case "list_statuses":
      return demo.statuses;
    case "vcs_details":
      return demo.gitDetails(String(args.path));
    case "vcs_branches":
      return demo.branches(String(args.path));
    case "updater_mode":
      // "managed": no update check, no update chip in the footer.
      return "managed";
    case "is_flatpak":
      return false;
    default:
      // Everything else (launches, hide, dismiss-on-blur, writes to the
      // shell, …) has no visible result to fake.
      return null;
  }
}

mockWindows("overlay");
mockIPC((cmd, args) => backend(cmd, (args ?? {}) as Args), { shouldMockEvents: true });

await import("../../src/main");
