// Shapes mirrored from the Rust backend (serde-serialized, camelCase).

export interface Repo {
  name: string;
  path: string;
  /** VCS label ("Git", "Mercurial", …) when a `kind: vcs` marker matched. */
  vcs?: string | null;
  sentinels: string[];
  /** Unix seconds when this repo was last observed on disk. */
  lastSeen: number;
}

export interface ScoredRepo {
  repo: Repo;
  score: number;
  /** Character indices in the matched haystack that the query hit. */
  matchIndices: number[];
}

export interface Action {
  id: string;
  label: string;
  /** Short hint shown on the right of the row, e.g. the resolved command. */
  hint: string;
  /** Section header to show above this action; "" means "just a divider". */
  group: string;
  /** Curated sort group within the universal tier (`ai-cli`, `ide`, …); "" when unclustered. */
  cluster: string;
  /** Per-repo frecency (0 = no history yet); only meaningful when `group` is "General". */
  usageScore: number;
  /** Icon key resolved against `lib/icons.ts`; absent -> fallback glyph. */
  icon?: string | null;
  /** Fixed keyboard shortcut (e.g. "Ctrl+C"); set only on the handful of
   *  built-in universal actions rendered as quick-action buttons. */
  hotkey?: string | null;
  /** True when this action is handled purely in the frontend (e.g. copy path). */
  clientSide: boolean;
  /** Opens the "Run command…" input instead of running; `hint` is the template. */
  prompt?: boolean;
  /** Opens the "Set up version control" form for this folder instead of running. */
  provision?: boolean;
  /** Opens the "Move to Recycle Bin / Trash" confirmation instead of running. */
  remove?: boolean;
}

/** A row in the action menu — a runnable action, or a drill-in to a
 *  sub-project's own action list. */
export type MenuItem =
  | { kind: "action"; group: string; action: Action; positions: number[] }
  | {
      kind: "submenu";
      group: string;
      /** The `Detected · <x>` group this row opens. */
      target: string;
      label: string;
      count: number;
    }
  | {
      /** Placeholder shown while `buildDetectedActions` is still walking PATH. */
      kind: "loading";
      group: string;
      label: string;
    };

export interface AppConfig {
  hotkey: string;
  /** Optional second hotkey that opens straight into the `>` app scope. */
  apps_hotkey?: string | null;
  roots: string[];
  /** `collapse_nested`: `true` collapse, `false` list all, `"auto"` keep independent.
   *  `top_level_folders`: list every folder directly in a root, marker or not. */
  scan: {
    max_depth: number;
    collapse_nested: boolean | "auto";
    top_level_folders?: boolean;
  };
  cache_ttl_secs: number;
  /** When the overlay closes itself. */
  dismiss?: "always" | "keep_on_blur" | "manual";
  /** Pinned terminal emulator (name / path); absent = auto-probe. */
  terminal?: string | null;
  /** Raw `{{dir}}` / `{{cmd}}` invocation for an unknown terminal. */
  terminal_template?: string | null;
  /** Shell a one-shot terminal command runs inside; absent = pwsh/powershell. */
  shell?: string | null;
  /** Pinned file manager (name / path); absent = auto-probe. */
  filemanager?: string | null;
  /** Raw `{{path}}` invocation for a file manager that wants more than a bare path. */
  filemanager_template?: string | null;
  /** Installed-app launcher (`>` scope) settings. */
  apps?: { enabled: boolean; extra_dirs: string[]; exclude: string[] };
}

/** An installed terminal emulator dev-prompt can drive — for the Settings dropdown. */
export interface TerminalOption {
  id: string;
  label: string;
}

/** An installed file manager dev-prompt found — for the Settings dropdown. */
export interface FileManagerOption {
  id: string;
  label: string;
}

/** Where things live on disk — Settings ▸ "File locations". */
export interface SystemPaths {
  installDir: string;
  configDir: string;
  cacheDir: string;
}

export interface ConfigSummary {
  rulesPath: string;
  markerCount: number;
  programs: { key: string; resolved: string | null }[];
  rules: {
    id: string;
    matches: string[];
    kind: string;
    scope: string;
    available: boolean;
    missing: string[];
    disabled: boolean;
  }[];
  universal: {
    id: string;
    label: string;
    icon: string | null;
    available: boolean;
    disabled: boolean;
  }[];
}

/** Rule-by-rule explanation of what one repo produces — settings "trace a repo". */
export interface RepoTrace {
  repoName: string;
  repoPath: string;
  /** Universal action ids that resolve for this repo. */
  universal: string[];
  rules: RuleTrace[];
}

export interface RuleTrace {
  id: string;
  globs: string[];
  /** "" when the rule resolved; otherwise why it produced nothing. */
  gate: string;
  /** Per-project results, populated only when `gate` is "". */
  hits: ProjectHit[];
}

export interface ProjectHit {
  /** "" = repo root, else the sub-project's relative path. */
  project: string;
  matched: string[];
  produced: string[];
}

export interface RepoListPayload {
  repos: Repo[];
  /** Age of the cache in seconds; -1 when there was no cache. */
  ageSecs: number;
  stale: boolean;
}

/** An installed application for the `>` launcher scope. */
export interface AppEntry {
  name: string;
  /**
   * Executable path (`exe`), AppUserModelID (`aumid`), or absolute `.desktop`
   * file path (`desktop`, Linux).
   */
  exec: string;
  kind: "exe" | "aumid" | "desktop";
  args?: string[];
  /** `desktop` only: the entry declared `Terminal=true`. */
  terminal?: boolean;
  /** `data:image/png;base64,…` / `data:image/svg+xml;base64,…` icon. */
  icon?: string | null;
  /** "start-menu" | "store" | "uninstall" | "scan" | "desktop". */
  source: string;
  /** Times launched from dev-prompt (frecency). */
  uses: number;
}

export interface AppListPayload {
  apps: AppEntry[];
  ageSecs: number;
  stale: boolean;
}

/** What the "New repository" screen (Ctrl+Shift+N) can offer. */
export interface NewRepoOptions {
  /** Configured roots that exist right now, expanded. */
  roots: string[];
  /** Version-control setups whose programs are all installed. */
  tools: ProvisionTool[];
}

export interface ProvisionTool {
  id: string;
  label: string;
  fields: { id: string; label: string; default: string }[];
  /** Raw argv templates (`{{dir}}`, `{{name}}`, `{{root}}`, field ids). */
  run: string[][];
}

/** A line in the right-click menu (`ContextMenu.svelte`). */
export type CtxItem =
  | {
      kind: "item";
      label: string;
      /** Icon key from `lib/icons.ts`. */
      icon?: string | null;
      disabled?: boolean;
      /** Destructive: drawn in red. */
      danger?: boolean;
      run: () => void;
    }
  | { kind: "separator" }
  /** A dim, non-interactive title line (the repo / app name). */
  | { kind: "header"; label: string }
  /** Shown while the rest of the menu is still being fetched. */
  | { kind: "loading"; label: string };

/** What moving a folder to the Recycle Bin / Trash would lose. */
export interface RemovalAssessment {
  name: string;
  path: string;
  /** The version-control system found ("Git", "Mercurial", …), if any. */
  vcs?: string | null;
  /** What would be lost, worded for the dialog. */
  risks: string[];
  /** What couldn't be checked, and why — treated like a risk. */
  unknown: string[];
  /** Typing the folder's name is required to confirm. */
  needsTyped: boolean;
}
