import { describe, expect, it } from "vitest";
import { stepZoom, zoomKey, zoomLabel } from "./zoom";

describe("zoom", () => {
  it("steps through the levels and stops at either end", () => {
    expect(stepZoom(1, 1)).toBe(1.1);
    expect(stepZoom(1.1, 1)).toBe(1.25);
    expect(stepZoom(1, -1)).toBe(0.9);
    expect(stepZoom(2, 1)).toBe(2);
    expect(stepZoom(0.8, -1)).toBe(0.8);
    expect(stepZoom(1.75, 0)).toBe(1);
  });

  it("finds its way from a value between levels", () => {
    expect(stepZoom(1.3, 1)).toBe(1.5);
    expect(stepZoom(1.3, -1)).toBe(1.25);
    expect(stepZoom(Number.NaN, 1)).toBe(1.1);
  });

  it("knows its keys — main row and number pad, with Ctrl alone", () => {
    const k = (key: string, more: Partial<{ code: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }> = {}) =>
      zoomKey({ key, ctrlKey: true, altKey: false, metaKey: false, ...more });
    expect(k("=")).toBe(1);
    expect(k("+")).toBe(1);
    expect(k("-")).toBe(-1);
    expect(k("0")).toBe(0);
    expect(k("+", { code: "NumpadAdd" })).toBe(1);
    expect(k("-", { ctrlKey: false })).toBe(null);
    expect(k("=", { altKey: true })).toBe(null);
    expect(k("r")).toBe(null);
    expect(k("_"), "Ctrl+Shift+- is readline's undo").toBe(null);
  });

  it("labels as a percentage", () => {
    expect(zoomLabel(1.25)).toBe("125%");
    expect(zoomLabel(0.9)).toBe("90%");
  });
});
