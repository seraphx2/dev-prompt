// Is what's in the search box something to clone? A URL is unmistakable, so
// it replaces the search results; GitHub's `owner/repo` could also be a
// search, so it's only offered when no repo matches.

const URL_RE = /^(?:(?:https?|ssh|git|svn|svn\+ssh|bzr|bzr\+ssh|file):\/\/\S+|[\w.-]+@[\w.-]+:\S+|lp:\S+)$/i;
const SHORTHAND_RE = /^[A-Za-z0-9_-][A-Za-z0-9_.-]*\/[A-Za-z0-9_.-]+$/;

export type CloneSource = "url" | "shorthand";

export function cloneSource(text: string): CloneSource | null {
  const t = text.trim();
  if (URL_RE.test(t)) return "url";
  if (SHORTHAND_RE.test(t)) return "shorthand";
  return null;
}

/** A short form for the row: no scheme, no `.git`. */
export function cloneLabel(text: string): string {
  return text
    .trim()
    .replace(/^[a-z+]+:\/\//i, "")
    .replace(/\.git\/?$/, "")
    .replace(/\/$/, "");
}

/** What the clone form asks for: clone `url` into `<root>/<name>` with `tool`.
 *  `source` is the address as typed — the form it came from is reopened by it. */
export interface CloneRequest {
  root: string;
  name: string;
  url: string;
  tool: string;
  source: string;
}

/** The clone under way (one at a time), or the last one that failed. */
export interface CloneJob extends CloneRequest {
  state: "running" | "failed";
  error?: string;
  /** Stopped on request — "failed" only because its folder couldn't go. */
  cancelled?: boolean;
}
