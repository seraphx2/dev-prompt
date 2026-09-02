import { describe, expect, it } from "vitest";
import { fuzzyScore } from "./fuzzy";

const score = (n: string, h: string) => fuzzyScore(n, h)?.score ?? null;

describe("fuzzyScore", () => {
  it("returns null when the needle is not a subsequence", () => {
    expect(fuzzyScore("xyz", "DBeaver")).toBeNull();
    expect(fuzzyScore("dbx", "DBeaver")).toBeNull();
    expect(fuzzyScore("rev", "DBeaver")).toBeNull(); // order matters
  });

  it("treats an empty / whitespace needle as a trivial match", () => {
    expect(fuzzyScore("", "anything")).toEqual({ score: 0, positions: [] });
    expect(fuzzyScore("   ", "anything")).toEqual({ score: 0, positions: [] });
  });

  it("ignores whitespace inside the needle", () => {
    const hit = fuzzyScore("web build", "my-web-build-tool");
    expect(hit).not.toBeNull();
    expect(hit!.positions).toEqual([...hit!.positions].sort((a, b) => a - b));
  });

  it("reports ascending matched positions", () => {
    const hit = fuzzyScore("dbe", "DBeaver")!;
    expect(hit.positions).toEqual([0, 1, 2]);
  });

  it("ranks a tight prefix above a scattered word-initial match", () => {
    // The regression: 'D…B…e' across two words used to beat contiguous 'DBe'.
    expect(score("dbe", "DBeaver")!).toBeGreaterThan(
      score("dbe", "Detroit: Become Human")!,
    );
  });

  it("still lets a scattered match win when it is the only match", () => {
    expect(score("dbe", "Detroit: Become Human")).not.toBeNull();
  });

  it("rewards a match that lands on a path boundary", () => {
    expect(score("src", "app/src")!).toBeGreaterThan(score("src", "appsrc")!);
  });
});
