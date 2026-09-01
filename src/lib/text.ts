/**
 * Shorten `s` to at most `max` characters by cutting out the middle and
 * inserting an ellipsis. Biased toward keeping the end — for a path that's the
 * filename, which is what usually matters. Text in the menu is monospace, so a
 * character budget tracks pixel width closely enough.
 */
export function middleTruncate(s: string, max: number): string {
  if (s.length <= max) return s;
  if (max <= 1) return "…";
  const keep = max - 1; // one char for the ellipsis
  const end = Math.ceil(keep * 0.55);
  const start = keep - end;
  return s.slice(0, start) + "…" + s.slice(s.length - end);
}
