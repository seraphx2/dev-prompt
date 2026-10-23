import { describe as group, expect, it } from "vitest";
import { ago, attention, chips, describe, headLabel, vcsActions, vocab } from "./vcsStatus";
import type { RepoStatus, VcsDetails } from "./types";

const NOW = 1_000_000;

function st(over: Partial<RepoStatus> = {}): RepoStatus {
  return {
    vcs: "Git",
    branch: "main",
    beyond: 0,
    unborn: false,
    upstream: "origin/main",
    upstreamGone: false,
    ahead: 0,
    behind: 0,
    changed: 0,
    untracked: 0,
    conflicted: 0,
    conflictedChanges: 0,
    divergent: 0,
    stashes: 0,
    unpushed: 0,
    noRemote: false,
    fetchedAt: NOW - 7200,
    checkedAt: NOW,
    unknown: [],
    ...over,
  };
}

const details: VcsDetails = {
  remotes: ["origin"],
  defaultBranch: "main",
  links: [
    { kind: "repo", label: "Open on GitHub", url: "https://github.com/o/r" },
    { kind: "branch", label: "Open main on GitHub", url: "https://github.com/o/r/tree/main" },
  ],
};
const none: VcsDetails = { remotes: [], links: [] };

const ids = (s: RepoStatus, d: VcsDetails | null = details) => vcsActions(s, d, NOW).map((a) => a.id);
const labels = (s: RepoStatus, d: VcsDetails | null = details) => vcsActions(s, d, NOW).map((a) => a.label);

group("attention", () => {
  it("ranks half-done work above work only here above things worth a look", () => {
    expect(attention(undefined)).toBe(0);
    expect(attention(st())).toBe(0);
    expect(attention(st({ noRemote: true, upstream: null, unpushed: null }))).toBe(0);
    expect(attention(st({ stashes: 1 }))).toBe(1);
    expect(attention(st({ behind: 3 }))).toBe(1);
    expect(attention(st({ branch: null, detached: "abc1234" }))).toBe(1);
    expect(attention(st({ upstreamGone: true }))).toBe(1);
    expect(attention(st({ untracked: 1 }))).toBe(2);
    expect(attention(st({ ahead: 1, unpushed: 1 }))).toBe(2);
    expect(attention(st({ upstream: null, unpushed: 4 }))).toBe(2);
    expect(attention(st({ conflicted: 1 }))).toBe(3);
    expect(attention(st({ operation: "rebase", branch: "topic", detached: "abc1234" }))).toBe(3);
  });

  it("knows each system's own trouble", () => {
    const jj = (o: Partial<RepoStatus>) => st({ vcs: "Jujutsu", upstream: null, fetchedAt: null, ...o });
    expect(attention(jj({ branch: null, detached: "kxyzvqmo" })), "no bookmark is ordinary in jj").toBe(0);
    expect(attention(jj({ conflictedChanges: 2 }))).toBe(3);
    expect(attention(jj({ divergent: 1 }))).toBe(3);
    expect(attention(jj({ operation: "stale" }))).toBe(3);
    expect(attention(st({ vcs: "Subversion", operation: "cleanup" }))).toBe(3);
    expect(attention(st({ vcs: "Mercurial", operation: "graft" }))).toBe(3);
    expect(attention(st({ vcs: "Fossil", unpushed: 2, upstream: null }))).toBe(2);
  });
});

group("row chips", () => {
  it("come most urgent first, each with its sentence", () => {
    const c = chips(st({ operation: "merge", conflicted: 2, changed: 3, untracked: 1, ahead: 1, unpushed: 3, behind: 2, stashes: 1 }));
    expect(c.map((x) => x.text)).toEqual(["merging", "!2", "~3", "+1", "↑3", "↓2", "≡1"]);
    expect(c[4].title).toBe("1 commit ahead of origin/main; 2 more commits on no remote, elsewhere in the history");
    expect(c[5].title).toBe("2 commits behind origin/main");
  });

  it("name a detached HEAD, a deleted upstream and a local-only repo", () => {
    expect(chips(st({ branch: null, detached: "abc1234" })).map((x) => x.text)).toEqual(["detached"]);
    expect(chips(st({ upstreamGone: true })).map((x) => x.text)).toEqual(["gone"]);
    expect(chips(st({ upstream: null, noRemote: true, unpushed: null })).map((x) => x.text)).toEqual(["local"]);
    expect(chips(st()).length).toBe(0);
  });

  it("speak each system's language", () => {
    expect(chips(st({ vcs: "Mercurial", stashes: 2 }))[0].title).toBe("2 shelved changes");
    expect(chips(st({ vcs: "Fossil", stashes: 1 }))[0].title).toBe("1 stashed change");
    expect(chips(st({ vcs: "Mercurial", upstream: null, unpushed: 2 }))[0].title).toBe("2 draft commits on no remote");
    expect(chips(st({ vcs: "Fossil", upstream: null, unpushed: 1 }))[0].title).toBe("1 unsent check-in on no remote");
    const jj = chips(st({ vcs: "Jujutsu", branch: null, detached: "kxyz", conflictedChanges: 2, divergent: 1 }));
    expect(jj.map((x) => x.text)).toEqual(["!2ch", "divergent"]);
    expect(chips(st({ vcs: "Subversion", operation: "cleanup" }))[0]).toMatchObject({ text: "needs cleanup", tone: "red" });
    expect(chips(st({ vcs: "Jujutsu", operation: "stale" }))[0].text).toBe("stale");
    expect(chips(st({ vcs: "Mercurial", operation: "histedit" }))[0].text).toBe("histediting");
  });
});

group("sentences", () => {
  it("spell out the state", () => {
    expect(describe(st(), NOW)).toEqual(["Up to date with origin/main (fetched 2 h ago)"]);
    expect(describe(st({ ahead: 2, behind: 1, unpushed: 2, changed: 1, untracked: 2 }), NOW)).toEqual([
      "origin/main: 2 ahead, 1 behind (fetched 2 h ago)",
      "1 changed file not committed, 2 new files not yet added",
    ]);
    expect(describe(st({ operation: "rebase", branch: "topic", conflicted: 1 }), NOW)[0]).toBe(
      "Rebasing topic — 1 conflicted file",
    );
    expect(describe(st({ branch: "feat", upstream: null, unpushed: 3 }), NOW)).toEqual([
      "feat isn't published to a remote",
      "3 commits on no remote",
    ]);
    expect(describe(st({ fetchedAt: null }), NOW)[0]).toContain("never fetched");
  });

  it("explain each system's own states", () => {
    expect(describe(st({ vcs: "Subversion", branch: "trunk", upstream: null, operation: "cleanup" }), NOW)[0]).toContain(
      "svn cleanup",
    );
    expect(describe(st({ vcs: "Jujutsu", operation: "stale", upstream: null }), NOW)[0]).toContain("update-stale");
    const jj = describe(st({ vcs: "Jujutsu", beyond: 2, upstream: "main@origin", ahead: 1, fetchedAt: null }), NOW);
    expect(jj).toEqual(["2 changes on top of main", "main@origin: 1 ahead"]);
    expect(describe(st({ vcs: "Jujutsu", branch: null, detached: "kxyz", upstream: null, fetchedAt: null }), NOW)).toEqual([
      "No bookmark below the working copy (kxyz)",
    ]);
    // Systems without remote-tracking state don't claim to be "unpublished".
    expect(describe(st({ vcs: "Mercurial", branch: "default", upstream: null, fetchedAt: null }), NOW)).toEqual([]);
  });

  it("show what's checked out", () => {
    expect(headLabel(st())).toBe("main");
    expect(headLabel(st({ branch: null, detached: "abc1234" }))).toBe("@abc1234");
    expect(headLabel(st({ vcs: "Jujutsu", beyond: 2 }))).toBe("main +2");
    expect(headLabel(null)).toBe(null);
  });

  it("age in words", () => {
    expect(ago(NOW - 30, NOW)).toBe("just now");
    expect(ago(NOW - 300, NOW)).toBe("5 min ago");
    expect(ago(NOW - 3 * 3600, NOW)).toBe("3 h ago");
    expect(ago(NOW - 86400, NOW)).toBe("yesterday");
    expect(ago(NOW - 5 * 86400, NOW)).toBe("5 days ago");
  });
});

group("git menu", () => {
  it("offers a pull only for a clean branch that's just behind", () => {
    expect(ids(st({ behind: 2 }))).toContain("vcs:pull");
    expect(vcsActions(st({ behind: 2 }), details, NOW)[1].label).toBe("Pull 2 commits from origin/main");
    expect(ids(st({ behind: 2, changed: 1 }))).not.toContain("vcs:pull");
    expect(ids(st({ behind: 2, ahead: 1 }))).not.toContain("vcs:pull");
    expect(ids(st({ behind: 2, operation: "merge" }))).not.toContain("vcs:pull");
    // Untracked files don't stop a fast-forward.
    expect(ids(st({ behind: 2, untracked: 3 }))).toContain("vcs:pull");
  });

  it("offers a push only when the upstream has nothing newer", () => {
    expect(ids(st({ ahead: 1 }))).toContain("vcs:push");
    expect(ids(st({ ahead: 1, behind: 1 }))).not.toContain("vcs:push");
    expect(ids(st({ ahead: 1, upstreamGone: true }))).not.toContain("vcs:push");
  });

  it("offers to publish a branch without an upstream, to origin first", () => {
    const s = st({ branch: "feat", upstream: null, unpushed: 2 });
    expect(ids(s)).toContain("vcs:publish:origin");
    expect(ids(s, { ...details, remotes: ["upstream", "origin"] })).toContain("vcs:publish:origin");
    expect(ids(s, { ...details, remotes: ["fork"] })).toContain("vcs:publish:fork");
    expect(ids(s, null)).not.toContain("vcs:publish:origin");
    expect(ids(st({ branch: "x", upstream: null, unborn: true }))).not.toContain("vcs:publish:origin");
  });

  it("always offers switching, fetch with a remote, and the web links", () => {
    expect(ids(st())).toEqual(["vcs:switch", "vcs:fetch", "vcs:link:repo", "vcs:link:branch"]);
    expect(ids(st({ noRemote: true, upstream: null }), none)).toEqual(["vcs:switch"]);
    const fetch = vcsActions(st(), details, NOW).find((a) => a.id === "vcs:fetch")!;
    expect(fetch.label).toBe("Fetch from origin");
    expect(fetch.hint).toBe("fetched 2 h ago");
    expect(vcsActions(st(), details, NOW).at(-1)!.icon).toBe("github");
  });
});

group("the other systems' menus", () => {
  it("Jujutsu: push a bookmark ahead of its remote, publish one with none, fetch", () => {
    const jj = (o: Partial<RepoStatus>) => st({ vcs: "Jujutsu", fetchedAt: null, ...o });
    expect(labels(jj({ upstream: "main@origin", ahead: 2 }), { ...details, links: [] })).toEqual([
      "Switch bookmark…",
      "Push main to origin (2 commits)",
      "Fetch from origin",
    ]);
    expect(ids(jj({ upstream: "main@origin", ahead: 2, behind: 1 }))).not.toContain("vcs:push");
    expect(ids(jj({ branch: "topic", upstream: null }))).toContain("vcs:publish:origin");
    expect(ids(jj({ branch: null, detached: "kx", upstream: null }))).not.toContain("vcs:publish:origin");
  });

  it("Mercurial: pull, and push the drafts under the working copy", () => {
    const hg = (o: Partial<RepoStatus>) => st({ vcs: "Mercurial", upstream: null, fetchedAt: null, ...o });
    const d: VcsDetails = { remotes: ["default"], links: [] };
    expect(labels(hg({ unpushed: 2 }), d)).toEqual([
      "Switch bookmark or branch…",
      "Pull from default",
      "Push 2 draft commits to default",
    ]);
    expect(ids(hg({ unpushed: 0 }), d)).not.toContain("vcs:push");
  });

  it("Subversion: update, saying when it merges into local changes", () => {
    const svn = (o: Partial<RepoStatus>) => st({ vcs: "Subversion", branch: "trunk", upstream: null, fetchedAt: null, ...o });
    const d: VcsDetails = { remotes: ["file:///repo"], links: [] };
    expect(labels(svn({}), d)).toEqual(["Switch branch…", "Update from the server"]);
    expect(labels(svn({ changed: 3 }), d)[1]).toBe("Update from the server (merges into your 3 changes)");
    expect(ids(svn({ operation: "cleanup" }), d)).not.toContain("vcs:update");
  });

  it("Fossil: sync with the remote, update only when clean", () => {
    const f = (o: Partial<RepoStatus>) => st({ vcs: "Fossil", branch: "trunk", upstream: null, fetchedAt: null, ...o });
    const d: VcsDetails = { remotes: ["https://user@chiselapp.com/user/x/repository/proj"], links: [] };
    expect(labels(f({}), d)).toEqual([
      "Switch branch…",
      "Sync with chiselapp.com/user/x/repository/proj",
      "Update to the latest on trunk",
    ]);
    expect(ids(f({ changed: 1 }), d)).not.toContain("vcs:update");
  });

  it("Breezy: pull from the parent, push to the push location — no switcher", () => {
    const b = st({ vcs: "Breezy", branch: "trunk", upstream: null, fetchedAt: null });
    const d: VcsDetails = { remotes: [], links: [], pullFrom: "file:///srv/bzr/trunk/", pushTo: "bzr+ssh://host/x" };
    expect(labels(b, d)).toEqual(["Pull from /srv/bzr/trunk", "Push to host/x"]);
    expect(ids({ ...b, changed: 1 }, d)).toEqual(["vcs:push"]);
    expect(vocab("Breezy")).toBe(null);
  });

  it("nothing for a folder without version control", () => {
    expect(vcsActions(st({ vcs: null }), details, NOW)).toEqual([]);
    expect(vcsActions(null, details, NOW)).toEqual([]);
  });
});

group("vocabulary", () => {
  it("names what each system switches and copies", () => {
    expect(vocab("Git")).toMatchObject({ noun: "branch", copy: "worktree", changes: "carry" });
    expect(vocab("Jujutsu")).toMatchObject({ noun: "bookmark", copy: "workspace", changes: "stay" });
    expect(vocab("Mercurial")).toMatchObject({ copy: "share", changes: "block" });
    expect(vocab("Subversion")).toMatchObject({ copy: "checkout", server: true });
    expect(vocab("Fossil")).toMatchObject({ copy: "checkout", changes: "block" });
    expect(vocab("Darcs")).toBe(null);
  });
});
