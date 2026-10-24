import { describe, expect, it } from "vitest";
import { cloneLabel, cloneSource } from "./clone";

describe("cloneSource", () => {
  it("knows a URL in any of the shapes people paste", () => {
    for (const u of [
      "https://github.com/seraphx2/dev-prompt",
      "https://github.com/seraphx2/dev-prompt.git",
      "git@github.com:seraphx2/dev-prompt.git",
      "ssh://git@example.com:2222/team/repo.git",
      "svn://svn.example.org/project/trunk",
      "svn+ssh://host/repo",
      "https://hg.mozilla.org/mozilla-central",
      "file:///srv/repos/thing.git",
      "lp:bzr",
      "  https://example.com/x  ",
    ]) {
      expect(cloneSource(u), u).toBe("url");
    }
  });

  it("takes owner/repo as GitHub's shorthand", () => {
    expect(cloneSource("seraphx2/dev-prompt")).toBe("shorthand");
    expect(cloneSource("rust-lang/rust.vim")).toBe("shorthand");
  });

  it("leaves ordinary searches alone", () => {
    for (const q of ["dev-prompt", "dev prompt", "a/b/c", "../x", "", "feature/", "/abs", "C:\\code\\x", "TICKET-12"]) {
      expect(cloneSource(q), q).toBeNull();
    }
  });
});

describe("cloneLabel", () => {
  it("drops the scheme and .git", () => {
    expect(cloneLabel("https://github.com/a/b.git")).toBe("github.com/a/b");
    expect(cloneLabel("git@github.com:a/b.git")).toBe("git@github.com:a/b");
    expect(cloneLabel("a/b")).toBe("a/b");
  });
});
