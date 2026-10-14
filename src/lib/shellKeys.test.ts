import { describe, expect, it } from "vitest";
import { classifyShellKey, type KeyLike } from "./shellKeys";

const k = (key: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  key,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  ...mods,
});

describe("classifyShellKey", () => {
  it("leaves plain Esc to the shell, Shift+Esc steps back", () => {
    expect(classifyShellKey(k("Escape"), false)).toBe("shell");
    expect(classifyShellKey(k("Escape", { shiftKey: true }), false)).toBe("back");
    // Ctrl+Shift+Esc never reaches the app on Windows (Task Manager); either
    // way it isn't ours.
    expect(classifyShellKey(k("Escape", { shiftKey: true, ctrlKey: true }), false)).toBe(
      "shell",
    );
  });

  it("copies with a selection, interrupts without one", () => {
    expect(classifyShellKey(k("c", { ctrlKey: true }), false)).toBe("shell");
    expect(classifyShellKey(k("c", { ctrlKey: true }), true)).toBe("copy");
    expect(classifyShellKey(k("C", { ctrlKey: true, shiftKey: true }), false)).toBe("copy");
  });

  it("pastes on Ctrl+V and Ctrl+Shift+V", () => {
    expect(classifyShellKey(k("v", { ctrlKey: true }), false)).toBe("paste");
    expect(classifyShellKey(k("V", { ctrlKey: true, shiftKey: true }), false)).toBe("paste");
  });

  it("hands everything else to the shell", () => {
    for (const key of ["a", "r", "l", "d", "z", "ArrowUp", "Tab", "Enter"]) {
      expect(classifyShellKey(k(key, { ctrlKey: true }), true)).toBe("shell");
      expect(classifyShellKey(k(key), true)).toBe("shell");
    }
    expect(classifyShellKey(k("c", { ctrlKey: true, altKey: true }), true)).toBe("shell");
  });
});
