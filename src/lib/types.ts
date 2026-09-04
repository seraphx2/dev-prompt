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
  /** Enter on a repo runs the action flagged `default` (falls back to first). */
  default: boolean;
  /** Icon key resolved against `lib/icons.ts`; absent -> fallback glyph. */
  icon?: string | null;
  /** True when this action is handled purely in the frontend (e.g. copy path). */
  clientSide: boolean;
  /** Opens the "Run command…" input instead of running; `hint` is the template. */
  prompt?: boolean;
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
    };

export interface AppConfig {
  hotkey: string;
  /** Optional second hotkey that opens straight into the `>` app scope. */
  apps_hotkey?: string | null;
  roots: string[];
  /** `collapse_nested`: `true` collapse, `false` list all, `"auto"` keep independent. */
  scan: { max_depth: number; collapse_nested: boolean | "auto" };
  cache_ttl_secs: number;
  /** Pinned terminal emulator (name / path); absent = auto-probe. */
  terminal?: string | null;
  /** Raw `{{dir}}` / `{{cmd}}` invocation for an unknown terminal. */
  terminal_template?: string | null;
  /** Shell a one-shot terminal command runs inside; absent = pwsh/powershell. */
  shell?: string | null;
  /** Installed-app launcher (`>` scope) settings. */
  apps?: { enabled: boolean; extra_dirs: string[]; exclude: string[] };
}

/** An installed terminal emulator dev-prompt can drive — for the Settings dropdown. */
export interface TerminalOption {
  id: string;
  label: string;
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
    default: boolean;
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
  /** Executable path (`exe`) or AppUserModelID (`aumid`). */
  exec: string;
  kind: "exe" | "aumid";
  args?: string[];
  /** `data:image/png;base64,…` when an icon was extracted. */
  icon?: string | null;
  /** "start-menu" | "store" | "uninstall" | "scan". */
  source: string;
  /** Times launched from dev-prompt (frecency). */
  uses: number;
}

export interface AppListPayload {
  apps: AppEntry[];
  ageSecs: number;
  stale: boolean;
}
