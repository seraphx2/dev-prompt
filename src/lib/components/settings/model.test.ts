import { describe, expect, it } from "vitest";
import type { AppConfig } from "../../types";
import {
  CUSTOM,
  addFolders,
  appsPatch,
  cleanList,
  collapsePatch,
  depthPatch,
  filemanagerPatch,
  formFromConfig,
  depthProblem,
  rescanNeeds,
  rootsPatch,
  sortFolders,
  templateProblem,
  terminalPatch,
  ttlPatch,
  ttlProblem,
} from "./model";

const base = (over: Partial<AppConfig> = {}): AppConfig => ({
  hotkey: "Ctrl+Backslash",
  apps_hotkey: "Ctrl+Shift+Backslash",
  roots: ["D:\\git"],
  scan: { max_depth: 4, collapse_nested: true, top_level_folders: true },
  cache_ttl_secs: 900,
  dismiss: "always",
  apps: { enabled: true, extra_dirs: [], exclude: [] },
  ...over,
});

describe("formFromConfig", () => {
  it("maps a config onto the form", () => {
    const f = formFromConfig(base());
    expect(f.roots).toEqual(["D:\\git"]);
    expect(f.ttlMin).toBe(15);
    expect(f.scanDepth).toBe(4);
    expect(f.collapseNested).toBe("true");
    expect(f.terminalSel).toBe("");
    expect(f.appsHotkey).toBe("Ctrl+Shift+Backslash");
  });

  it("always leaves one root row to type into", () => {
    expect(formFromConfig(base({ roots: [] })).roots).toEqual([""]);
  });

  it("selects Custom when a template is set", () => {
    const f = formFromConfig(
      base({ terminal: "wt", terminal_template: "x {{dir}}", filemanager_template: "y {{path}}" }),
    );
    expect(f.terminalSel).toBe(CUSTOM);
    expect(f.filemanagerSel).toBe(CUSTOM);
  });

  it("copies lists instead of sharing them with the config", () => {
    const c = base();
    formFromConfig(c).roots.push("E:\\src");
    expect(c.roots).toEqual(["D:\\git"]);
  });

  it("keeps auto collapse", () => {
    const f = formFromConfig(base({ scan: { max_depth: 3, collapse_nested: "auto" } }));
    expect(f.collapseNested).toBe("auto");
    expect(collapsePatch(f)).toEqual({ collapse_nested: "auto" });
  });
});

describe("patches", () => {
  it("cleans the root list", () => {
    expect(rootsPatch([" D:\\git ", "", "E:\\src", "D:\\git"])).toEqual({
      roots: ["D:\\git", "E:\\src"],
    });
  });

  it("skips a number field mid-edit", () => {
    const f = formFromConfig(base());
    f.ttlMin = NaN;
    f.scanDepth = 0;
    expect(ttlPatch(f)).toEqual({});
    expect(depthPatch(f)).toEqual({});
    f.ttlMin = 2;
    f.scanDepth = 6;
    expect(ttlPatch(f)).toEqual({ cache_ttl_secs: 120 });
    expect(depthPatch(f)).toEqual({ scan_max_depth: 6 });
  });

  it("sends either a pin or a template, never both", () => {
    const f = formFromConfig(base());
    f.terminalSel = "wezterm";
    expect(terminalPatch(f)).toEqual({ terminal: "wezterm", terminal_template: "" });
    f.terminalSel = CUSTOM;
    f.terminalTemplate = " foo {{dir}} ";
    expect(terminalPatch(f)).toEqual({ terminal: "", terminal_template: "foo {{dir}}" });
    f.filemanagerSel = "";
    expect(filemanagerPatch(f)).toEqual({ filemanager: "", filemanager_template: "" });
  });

  it("sends the app excludes back untouched", () => {
    const f = formFromConfig(base({ apps: { enabled: true, extra_dirs: ["D:\\tools", ""], exclude: ["Foo"] } }));
    expect(appsPatch(f, f.appExtraDirs)).toEqual({
      apps: { enabled: true, extra_dirs: ["D:\\tools"], exclude: ["Foo"] },
    });
  });
});

describe("folder lists", () => {
  it("saves only the folders that exist", () => {
    const exists = new Map([
      ["D:\\git", true],
      ["D:\\gi", false],
    ]);
    const got = sortFolders([" D:\\git", "D:\\gi", "", "E:\\typed-since"], exists);
    expect(got.good).toEqual(["D:\\git"]);
    expect(got.bad).toEqual(["D:\\gi"]);
    expect(got.problems).toEqual([null, "Folder not found.", null, null]);
  });

  it("adds picked folders once, dropping blank rows", () => {
    expect(addFolders(["D:\\git", ""], ["E:\\src", "D:\\git"])).toEqual(["D:\\git", "E:\\src"]);
    expect(cleanList(["", " "])).toEqual([]);
  });
});

describe("rescanNeeds", () => {
  it("rescans repos only for discovery settings", () => {
    expect(rescanNeeds(base(), base({ terminal: "wt", cache_ttl_secs: 60 }))).toEqual({
      repos: false,
      apps: false,
    });
    expect(rescanNeeds(base(), base({ roots: ["E:\\src"] })).repos).toBe(true);
    expect(
      rescanNeeds(base(), base({ scan: { max_depth: 4, collapse_nested: true, top_level_folders: false } }))
        .repos,
    ).toBe(true);
  });

  it("rescans apps when the app settings change while the index is on", () => {
    const on = base({ apps: { enabled: true, extra_dirs: ["D:\\tools"], exclude: [] } });
    expect(rescanNeeds(base(), on).apps).toBe(true);
    const off = base({ apps: { enabled: false, extra_dirs: [], exclude: [] } });
    expect(rescanNeeds(base(), off).apps).toBe(false);
    expect(rescanNeeds(off, base()).apps).toBe(true);
  });
});

describe("validation", () => {
  it("bounds the numbers", () => {
    expect(depthProblem(4)).toBeNull();
    expect(depthProblem(0)).not.toBeNull();
    expect(depthProblem(13)).not.toBeNull();
    expect(depthProblem(2.5)).not.toBeNull();
    expect(depthProblem(NaN)).not.toBeNull();
    expect(ttlProblem(1)).toBeNull();
    expect(ttlProblem(0)).not.toBeNull();
  });

  it("accepts a template with its placeholders", () => {
    expect(templateProblem("terminal", "wezterm start --cwd {{dir}} -- {{cmd}}")).toBeNull();
    expect(templateProblem("terminal", "x --cd {{dir}}")).toBeNull();
    expect(templateProblem("filemanager", "dopus /cmd Go {{path}}")).toBeNull();
  });

  it("rejects an empty, unfinished or wrong template", () => {
    expect(templateProblem("terminal", "  ")).toMatch(/Type a template/);
    expect(templateProblem("terminal", "wezterm start --cwd {{di")).toMatch(/isn't closed/);
    expect(templateProblem("terminal", "wezterm }} x")).toMatch(/isn't closed/);
    expect(templateProblem("terminal", "x {{path}}")).toMatch(/isn't a placeholder here/);
    expect(templateProblem("terminal", "x {{ dir }}")).toMatch(/isn't a placeholder here/);
    expect(templateProblem("filemanager", "dopus")).toMatch(/needs \{\{path\}\}/);
  });
});
