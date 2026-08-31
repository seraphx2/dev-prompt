// Shapes mirrored from the Rust backend (serde-serialized, camelCase).

export interface Repo {
  name: string;
  path: string;
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
  /** True when this action is handled purely in the frontend (e.g. copy path). */
  clientSide: boolean;
}

export interface AppConfig {
  hotkey: string;
  roots: string[];
  scan: { max_depth: number };
  cache_ttl_secs: number;
}

export interface ConfigSummary {
  configPath: string;
  markerCount: number;
  programs: { key: string; resolved: string | null }[];
  rules: {
    id: string;
    matches: string[];
    kind: string;
    scope: string;
    available: boolean;
    missing: string[];
  }[];
  universal: { id: string; label: string; default: boolean; available: boolean }[];
}

export interface RepoListPayload {
  repos: Repo[];
  /** Age of the cache in seconds; -1 when there was no cache. */
  ageSecs: number;
  stale: boolean;
}
