// The Settings screen's shared form state, the save patch each page sends,
// and which saves need a rescan. Pages edit `Form` in place and save their own
// slice of it — every change autosaves, so there's no Save button and nothing
// is ever "pending" past a short typing pause.

import type { AppConfig, ConfigPatch } from "../../types";

export type Page =
  | "general"
  | "repos"
  | "tools"
  | "apps"
  | "rules"
  | "trace"
  | "icons"
  | "about";

export const PAGE_LABEL: Record<Page, string> = {
  general: "General",
  repos: "Repos",
  tools: "Tools",
  apps: "Apps",
  rules: "Active rules",
  trace: "Trace a repo",
  icons: "Icons",
  about: "About",
};

/** How long a text or number field waits after the last keystroke to save. */
export const TYPING_DELAY_MS = 600;

/** Called by a page to save part of the form. `delay` debounces typing;
 *  patches waiting at the same time are merged into one save. */
export type Save = (patch: ConfigPatch, delay?: number) => void;

/** A transient message in the Settings header (errors stay until replaced). */
export type Note = (text: string, isError?: boolean) => void;

export type Dismiss = "always" | "keep_on_blur" | "manual";

/** The editable settings, in the shape the pages bind to. */
export interface Form {
  hotkey: string;
  /** "" = off. */
  appsHotkey: string;
  roots: string[];
  ttlMin: number;
  scanDepth: number;
  /** Bound to a <select>; serialises to `true` / `false` / `"auto"`. */
  collapseNested: "true" | "false" | "auto";
  topLevelFolders: boolean;
  dismiss: Dismiss;
  /** "" = auto, "__custom__" = raw template, else a terminal id. */
  terminalSel: string;
  terminalTemplate: string;
  /** "" = default shell (pwsh -> powershell), else a shell name. */
  shellSel: string;
  /** "" = the default font stack. */
  shellFont: string;
  /** 0 = the default size. */
  shellFontSize: number;
  /** "" = auto, "__custom__" = raw template, else a file manager id. */
  filemanagerSel: string;
  filemanagerTemplate: string;
  appsEnabled: boolean;
  appExtraDirs: string[];
  /** Not editable here, but sent back with every apps save so it survives. */
  appExclude: string[];
}

export const CUSTOM = "__custom__";

export function formFromConfig(c: AppConfig): Form {
  const terminalTemplate = c.terminal_template ?? "";
  const filemanagerTemplate = c.filemanager_template ?? "";
  return {
    hotkey: c.hotkey,
    appsHotkey: c.apps_hotkey ?? "",
    // Always one row to type into.
    roots: c.roots.length ? [...c.roots] : [""],
    ttlMin: Math.max(1, Math.round(c.cache_ttl_secs / 60)),
    scanDepth: Math.max(1, c.scan?.max_depth ?? 4),
    collapseNested:
      c.scan?.collapse_nested === undefined
        ? "true"
        : (String(c.scan.collapse_nested) as Form["collapseNested"]),
    topLevelFolders: c.scan?.top_level_folders ?? true,
    dismiss: c.dismiss ?? "always",
    terminalTemplate,
    terminalSel: terminalTemplate ? CUSTOM : (c.terminal ?? ""),
    shellSel: c.shell ?? "",
    shellFont: c.shell_font ?? "",
    shellFontSize: c.shell_font_size ?? 0,
    filemanagerTemplate,
    filemanagerSel: filemanagerTemplate ? CUSTOM : (c.filemanager ?? ""),
    appsEnabled: c.apps?.enabled ?? true,
    appExtraDirs: [...(c.apps?.extra_dirs ?? [])],
    appExclude: [...(c.apps?.exclude ?? [])],
  };
}

/** Trimmed, blank rows dropped, duplicates dropped (first one wins). */
export function cleanList(xs: string[]): string[] {
  return [...new Set(xs.map((x) => x.trim()).filter(Boolean))];
}

/** Adds picked folders that aren't already listed, dropping blank rows. */
export function addFolders(have: string[], picked: string[]): string[] {
  return cleanList([...have, ...picked]);
}

// One patch per thing a page saves. A number field mid-edit (empty, or not a
// number yet) sends nothing rather than a bogus value.

/** `roots` are the rows that passed [`sortFolders`]. */
export const rootsPatch = (roots: string[]): ConfigPatch => ({ roots: cleanList(roots) });

export function ttlPatch(f: Form): ConfigPatch {
  return Number.isFinite(f.ttlMin) && f.ttlMin >= 1
    ? { cache_ttl_secs: Math.max(60, Math.round(f.ttlMin * 60)) }
    : {};
}

/** The built-in shell's font. Blank family / size go back to the defaults. */
export function shellFontPatch(f: Form): ConfigPatch {
  const size = Number.isFinite(f.shellFontSize) ? Math.round(f.shellFontSize) : 0;
  return { shell_font: f.shellFont.trim(), shell_font_size: size };
}

export function depthPatch(f: Form): ConfigPatch {
  return Number.isFinite(f.scanDepth) && f.scanDepth >= 1
    ? { scan_max_depth: Math.round(f.scanDepth) }
    : {};
}

export const collapsePatch = (f: Form): ConfigPatch => ({
  collapse_nested: f.collapseNested === "auto" ? "auto" : f.collapseNested === "true",
});

export const terminalPatch = (f: Form): ConfigPatch =>
  f.terminalSel === CUSTOM
    ? { terminal: "", terminal_template: f.terminalTemplate.trim() }
    : { terminal: f.terminalSel, terminal_template: "" };

export const filemanagerPatch = (f: Form): ConfigPatch =>
  f.filemanagerSel === CUSTOM
    ? { filemanager: "", filemanager_template: f.filemanagerTemplate.trim() }
    : { filemanager: f.filemanagerSel, filemanager_template: "" };

/** `extraDirs` are the rows that passed [`sortFolders`]. */
export const appsPatch = (f: Form, extraDirs: string[]): ConfigPatch => ({
  apps: {
    enabled: f.appsEnabled,
    extra_dirs: cleanList(extraDirs),
    exclude: cleanList(f.appExclude),
  },
});

/** Folder rows sorted by whether each typed path exists (`exists`, keyed by
 *  the trimmed path): the ones to save, the ones that can't be, and each
 *  row's problem. A row typed since the check is in neither list. */
export function sortFolders(rows: string[], exists: Map<string, boolean>) {
  const state = (r: string) => exists.get(r.trim());
  return {
    good: cleanList(rows.filter((r) => state(r) === true)),
    bad: cleanList(rows.filter((r) => state(r) === false)),
    problems: rows.map((r) => (state(r) === false ? "Folder not found." : null)),
  };
}

// --- validation: an invalid value is never saved -----------------------------

export const DEPTH_MAX = 12;

export function depthProblem(n: number): string | null {
  return Number.isInteger(n) && n >= 1 && n <= DEPTH_MAX ? null : `A whole number from 1 to ${DEPTH_MAX}.`;
}

export function ttlProblem(n: number): string | null {
  return Number.isFinite(n) && n >= 1 ? null : "At least 1 minute.";
}

export const FONT_SIZE_MIN = 6;
export const FONT_SIZE_MAX = 40;

/** Blank (0 / NaN from an emptied box) means "the default size". */
export function fontSizeProblem(n: number): string | null {
  if (!Number.isFinite(n) || n === 0) return null;
  return Number.isInteger(n) && n >= FONT_SIZE_MIN && n <= FONT_SIZE_MAX
    ? null
    : `A whole number from ${FONT_SIZE_MIN} to ${FONT_SIZE_MAX}.`;
}

const PLACEHOLDERS = { terminal: ["dir", "cmd"], filemanager: ["path"] } as const;

/** What's wrong with a custom terminal / file-manager template, or null.
 *  Placeholders are replaced literally, so `{{ dir }}` (with spaces) wouldn't be. */
export function templateProblem(kind: "terminal" | "filemanager", template: string): string | null {
  const t = template.trim();
  const allowed: readonly string[] = PLACEHOLDERS[kind];
  const list = allowed.map((a) => `{{${a}}}`).join(" or ");
  if (!t) return "Type a template, or pick another option.";
  const opens = t.split("{{").length - 1;
  const names = [...t.matchAll(/\{\{([^{}]*)\}\}/g)].map((m) => m[1]);
  if (opens !== names.length || t.split("}}").length - 1 !== names.length) {
    return "A {{placeholder}} isn't closed.";
  }
  const bad = names.find((n) => !allowed.includes(n));
  if (bad !== undefined) return `{{${bad}}} isn't a placeholder here — use ${list}.`;
  if (!names.length) return `It needs ${list} somewhere.`;
  return null;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** What a save from `before` to `after` needs rescanned. Only what changes
 *  discovery counts — a terminal pick or the cache lifetime rescans nothing,
 *  and the app list isn't rebuilt while the app index is off. */
export function rescanNeeds(
  before: AppConfig,
  after: AppConfig,
): { repos: boolean; apps: boolean } {
  const scan = (c: AppConfig) => [
    c.roots,
    c.scan?.max_depth,
    c.scan?.collapse_nested,
    c.scan?.top_level_folders ?? true,
  ];
  const apps = (c: AppConfig) => c.apps ?? { enabled: true, extra_dirs: [], exclude: [] };
  return {
    repos: !same(scan(before), scan(after)),
    apps: apps(after).enabled && !same(apps(before), apps(after)),
  };
}
