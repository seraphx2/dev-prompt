import { describe, expect, it } from "vitest";
import { classifyHotkey } from "./hotkeys";

const level = (a: string) => classifyHotkey(a).level;

describe("classifyHotkey", () => {
  it("blocks OS-reserved / uninterceptable combos", () => {
    for (const a of [
      "Alt+Tab",
      "Alt+F4",
      "Alt+Space",
      "CmdOrCtrl+Escape",
      "CmdOrCtrl+Alt+Delete",
      "CmdOrCtrl+Shift+Escape",
      "Super+L",
      "Super+D",
    ]) {
      expect(level(a), a).toBe("block");
    }
  });

  it("blocks combos with no modifier, warns on a bare function key", () => {
    expect(level("A")).toBe("block");
    expect(level("Space")).toBe("block");
    expect(level("F5")).toBe("warn");
    expect(level("")).toBe("block");
  });

  it("warns on combos commonly used by other apps", () => {
    expect(level("CmdOrCtrl+Shift+N")).toBe("warn"); // named (Explorer / incognito)
    expect(level("CmdOrCtrl+Shift+T")).toBe("warn");
    expect(level("CmdOrCtrl+N")).toBe("warn"); // bare Ctrl+letter
    expect(level("CmdOrCtrl+5")).toBe("warn"); // bare Ctrl+digit
    expect(level("Alt+F")).toBe("warn"); // menu mnemonic
    expect(level("Super+K")).toBe("warn"); // lone Win+key
  });

  it("accepts the shipped defaults and other roomy combos", () => {
    expect(level("CmdOrCtrl+Shift+Space")).toBe("ok"); // repo browser default
    expect(level("CmdOrCtrl+Shift+Period")).toBe("ok"); // app launcher default
    expect(level("CmdOrCtrl+Alt+J")).toBe("ok");
    expect(level("CmdOrCtrl+Shift+F9")).toBe("ok");
  });

  it("normalises modifier aliases and order", () => {
    expect(level("Ctrl+Shift+N")).toBe("warn");
    expect(level("Control+Shift+N")).toBe("warn");
    expect(level("Shift+CmdOrCtrl+N")).toBe("warn"); // order-independent lookup
  });

  it("carries a reason for non-ok verdicts", () => {
    expect(classifyHotkey("Alt+Tab").reason).toBeTruthy();
    expect(classifyHotkey("CmdOrCtrl+Shift+N").reason).toBeTruthy();
    expect(classifyHotkey("CmdOrCtrl+Shift+Space").reason).toBeUndefined();
  });
});
