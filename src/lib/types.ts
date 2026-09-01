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
  roots: string[];
  /** `collapse_nested`: `true` collapse, `false` list all, `"auto"` keep independent. */
  scan: { max_depth: number; collapse_nested: boolean | "auto" };
  cache_ttl_secs: number;
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
    default: boolean;
    available: boolean;
    disabled: boolean;
  }[];
}

export interface RepoListPayload {
  repos: Repo[];
  /** Age of the cache in seconds; -1 when there was no cache. */
  ageSecs: number;
  stale: boolean;
}
