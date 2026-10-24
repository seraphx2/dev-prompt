import { describe, expect, it } from "vitest";
import { MENU, PANEL, contrast, glyphColor } from "./glyph";
import { icons } from "./icons";

const rgb = (hex: string): [number, number, number] => {
  const n = parseInt(hex.slice(1), 16);
  return [n >> 16, (n >> 8) & 255, n & 255];
};
const over = (hex: string, alpha: number, bg: [number, number, number]) =>
  rgb(hex).map((v, i) => Math.round(bg[i] + (v - bg[i]) * alpha)) as [number, number, number];

describe("glyphColor", () => {
  it("keeps a brand colour that already stands out", () => {
    expect(glyphColor(icons.yarn)).toBe(icons.yarn.hex);
    expect(glyphColor(icons.c)).toBe(icons.c.hex); // pale and greyish, but it passes
  });

  it("lightens a dark brand colour to 4.5:1, keeping its hue", () => {
    const c = glyphColor(icons.cpp)!;
    expect(c).not.toBe(icons.cpp.hex);
    const [r, , b] = rgb(c);
    expect(b).toBeGreaterThan(r); // still blue
    expect(contrast(rgb(c), PANEL)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(rgb(c), PANEL)).toBeLessThan(4.7);
  });

  it("leaves a mark with no real hue to the caller's neutral", () => {
    expect(glyphColor(icons.github)).toBeNull();
    expect(glyphColor(icons.rust)).toBeNull();
    expect(glyphColor(icons.terminal)).toBeNull();
  });

  it("holds 3:1 for every brand icon, wherever and however faintly it's drawn", () => {
    for (const [bg, alpha] of [[PANEL, 1], [PANEL, 0.9], [PANEL, 0.8], [MENU, 1]] as const) {
      for (const [key, g] of Object.entries(icons)) {
        const c = glyphColor(g, alpha, bg);
        if (c) expect(contrast(over(c, alpha, bg), bg), `${key} at ${alpha}`).toBeGreaterThanOrEqual(3);
      }
    }
  });
});
