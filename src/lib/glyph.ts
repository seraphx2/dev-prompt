// Glyph lookup shared by the action menu and the settings diagnostics list.
// `icons.ts` is generated; this is the hand-written policy layer on top.

import { icons, type Glyph } from "./icons";

/** Resolve an `icon:` key to a glyph, falling back to `terminal` — most
 *  icon-less actions launch a CLI, and it beats a bare play triangle. */
export const glyphFor = (key?: string | null): Glyph =>
  (key && icons[key]) || icons.terminal;

type Rgb = [number, number, number];

/** The overlay's panel (`--color-panel`) and the right-click menu's own background. */
export const PANEL: Rgb = [20, 20, 24];
export const MENU: Rgb = [28, 31, 38];

/** What an icon needs to stand out from what's behind it (WCAG's non-text
 *  contrast) — and, once a colour has to be lightened anyway, what it's
 *  lightened to: a bare 3:1 reads dim beside its neighbours, and small marks
 *  like Rails' or MySQL's need more than the minimum to make out. */
const ICON_CONTRAST = 3;
const LIFTED_CONTRAST = 4.5;

const rgb = (hex: string): Rgb => {
  const n = parseInt(hex.slice(1), 16);
  return [n >> 16, (n >> 8) & 255, n & 255];
};

const luminance = (c: Rgb) => {
  const [r, g, b] = c.map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

export const contrast = (a: Rgb, b: Rgb) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

const mix = (from: Rgb, to: Rgb, t: number): Rgb =>
  from.map((v, i) => Math.round(v + (to[i] - v) * t)) as Rgb;

const hex = (c: Rgb) => "#" + c.map((v) => v.toString(16).padStart(2, "0")).join("");

/** The colour to draw a brand glyph in, or null to tint it with a neutral.
 *  A brand colour that holds 3:1 against `bg` once drawn at `alpha` is used
 *  as is. One that doesn't is lightened toward white until it holds 4.5:1 —
 *  C++ stays blue, only brighter — unless it has no real hue to keep (the
 *  JetBrains family, GitHub, Rust, Cursor, …): that would only lighten to
 *  grey, so it gets null and the caller's neutral instead. */
export function glyphColor(g: Glyph, alpha = 1, bg: Rgb = PANEL): string | null {
  if (!g.hex) return null;
  const c = rgb(g.hex);
  const drawn = (t: number) => mix(bg, mix(c, [255, 255, 255], t), alpha);
  if (contrast(drawn(0), bg) >= ICON_CONTRAST) return g.hex;
  if (Math.max(...c) - Math.min(...c) < 0.15 * 255) return null;
  let [lo, hi] = [0, 1];
  for (let i = 0; i < 12; i++) {
    const t = (lo + hi) / 2;
    if (contrast(drawn(t), bg) >= LIFTED_CONTRAST) hi = t;
    else lo = t;
  }
  return hex(mix(c, [255, 255, 255], hi));
}
