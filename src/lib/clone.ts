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
