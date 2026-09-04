// Glyph lookup shared by the action menu and the settings diagnostics list.
// `icons.ts` is generated; this is the hand-written policy layer on top.

import { icons, type Glyph } from "./icons";

/** Resolve an `icon:` key to a glyph, falling back to `terminal` — most
 *  icon-less actions launch a CLI, and it beats a bare play triangle. */
export const glyphFor = (key?: string | null): Glyph =>
  (key && icons[key]) || icons.terminal;

/** True when a brand glyph's hex is too dark to read on the dark overlay
 *  (the JetBrains family, GitHub, Rust, Copilot, opencode, Cursor, …). The
 *  caller then tints it with a neutral instead of the brand colour. */
export function glyphDim(g: Glyph): boolean {
  if (!g.hex) return false;
  const n = parseInt(g.hex.slice(1), 16);
  const r = (n >> 16) / 255;
  const gr = ((n >> 8) & 255) / 255;
  const b = (n & 255) / 255;
  return 0.2126 * r + 0.7152 * gr + 0.0722 * b < 0.28;
}
