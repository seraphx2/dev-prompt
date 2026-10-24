// The overlay's zoom: how much bigger (or smaller) everything is drawn, on top
// of the OS's own display scaling. Ctrl + / Ctrl − step through these levels,
// Ctrl 0 goes back to 100%; Settings ▸ General picks one directly. Kept in
// config.yaml — the backend applies it (`apply_zoom` in commands.rs), zooming
// the web view and sizing the window to match. Mirrors `ZOOM_STEPS` in
// config.rs.

export const ZOOM_STEPS = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

/** The level one step bigger (`+1`) or smaller (`-1`) than `current`, which
 *  may sit between levels (a hand-edited config); `0` is back to 100%. Stays
 *  put at either end. */
export function stepZoom(current: number, direction: -1 | 0 | 1): number {
  if (direction === 0) return 1;
  const z = Number.isFinite(current) ? current : 1;
  if (direction > 0) return ZOOM_STEPS.find((s) => s > z + 1e-9) ?? ZOOM_STEPS[ZOOM_STEPS.length - 1];
  return [...ZOOM_STEPS].reverse().find((s) => s < z - 1e-9) ?? ZOOM_STEPS[0];
}

/** The zoom key in a keydown, if it is one: Ctrl with `+` / `=` (the same key
 *  on most layouts), `-`, or `0` — the main row or the number pad, as Windows
 *  Terminal has them. Not Ctrl+Shift+- (`_`): that's readline's undo. */
export function zoomKey(e: { key: string; code?: string; ctrlKey: boolean; altKey: boolean; metaKey: boolean }): -1 | 0 | 1 | null {
  if (!e.ctrlKey || e.altKey || e.metaKey) return null;
  if (e.key === "+" || e.key === "=" || e.code === "NumpadAdd") return 1;
  if (e.key === "-" || e.code === "NumpadSubtract") return -1;
  if (e.key === "0" || e.code === "Numpad0") return 0;
  return null;
}

/** `1.25` → `"125%"`. */
export const zoomLabel = (z: number) => `${Math.round(z * 100)}%`;
