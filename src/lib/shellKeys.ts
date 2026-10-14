/**
 * Who handles a keystroke in the built-in shell. Almost everything belongs to
 * the shell — Esc included, since vim, fzf and PSReadLine all use it. The few
 * exceptions:
 *
 * - **Shift+Esc** steps back out of the shell. Shells and TUIs read it as a
 *   plain Esc, so claiming it costs them nothing.
 * - **Copy**: Ctrl+Shift+C, or Ctrl+C while text is selected (as in Windows
 *   Terminal). With nothing selected, Ctrl+C is the shell's interrupt.
 * - **Paste**: Ctrl+V or Ctrl+Shift+V. Ctrl+V on its own means "insert the
 *   next key literally" to a few Unix shells; terminals on both platforms
 *   trade that away for paste, and so does this one.
 */
export type ShellKey = "back" | "copy" | "paste" | "shell";

export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

export function classifyShellKey(e: KeyLike, hasSelection: boolean): ShellKey {
  if (e.altKey || e.metaKey) return "shell";
  if (e.key === "Escape" && e.shiftKey && !e.ctrlKey) return "back";
  if (!e.ctrlKey) return "shell";
  const k = e.key.toLowerCase();
  if (k === "c" && (e.shiftKey || hasSelection)) return "copy";
  if (k === "v") return "paste";
  return "shell";
}
