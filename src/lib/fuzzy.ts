export interface FuzzyHit {
  score: number;
  /** Indices into the haystack that the needle matched, ascending. */
  positions: number[];
}

/**
 * Lightweight fzf-style subsequence scorer for small in-memory lists (the repo
 * list uses the Rust `nucleo` matcher instead). Whitespace in the needle is
 * ignored, so "web build" matches "…web…build". Returns `null` when not every
 * needle character is found in order.
 */
export function fuzzyScore(needle: string, haystack: string): FuzzyHit | null {
  const n = needle.toLowerCase().replace(/\s+/g, "");
  if (!n) return { score: 0, positions: [] };

  const h = haystack;
  const hl = haystack.toLowerCase();
  const positions: number[] = [];

  let score = 0;
  let ni = 0;
  let prevMatch = -2;

  for (let i = 0; i < hl.length && ni < n.length; i++) {
    if (hl[i] !== n[ni]) continue;

    let bonus = 1;
    if (i === prevMatch + 1) bonus += 5; // contiguous run
    const prev = i > 0 ? h[i - 1] : "";
    const atWordStart =
      i === 0 || prev === "/" || prev === " " || prev === "-" || prev === "_" || prev === ":" || prev === ".";
    if (atWordStart) bonus += 8;
    const isUpper = h[i] >= "A" && h[i] <= "Z";
    const prevUpper = prev >= "A" && prev <= "Z";
    if (isUpper && !prevUpper) bonus += 6; // camelCase hump

    score += bonus;
    positions.push(i);
    prevMatch = i;
    ni++;
  }

  if (ni < n.length) return null;
  score -= positions[0] * 0.2; // nudge earlier matches ahead
  return { score, positions };
}
