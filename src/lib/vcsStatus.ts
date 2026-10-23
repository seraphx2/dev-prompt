// What a repo's working-copy status (`RepoStatus`, kept current by the
// backend's status.rs) means on screen, for every version-control system:
// how much it needs attention, the compact flags on its row, the sentences
// behind them, each system's own words (branch or bookmark; worktree,
// workspace, share or checkout), and the menu entries it makes sensible.
// Pure functions — the store is `statuses.svelte.ts`.

import type { Action, RepoStatus, VcsDetails } from "./types";

/** How a system talks about switching, and what switching does to
 *  uncommitted changes. `null` from [`vocab`]: no switcher (Breezy's branches
 *  are folders of their own, each already a repo in the list; Darcs has none). */
export interface Vocab {
  /** "branch", "bookmark", "bookmark or branch". */
  noun: string;
  /** The switcher's title and menu entry. */
  title: string;
  /** What a second working copy is called. */
  copy: string;
  /** Uncommitted changes: "carry" (Git: they come along, git refuses if
   *  one would be overwritten), "stay" (Jujutsu: they stay in their own
   *  change), "block" (switching would merge them — commit or shelve first). */
  changes: "carry" | "stay" | "block";
  /** Listing branches asks the server (Subversion). */
  server: boolean;
  /** What a new name becomes. */
  create: string;
}

export function vocab(vcs: string | null | undefined): Vocab | null {
  switch (vcs) {
    case "Git":
      return { noun: "branch", title: "Branches", copy: "worktree", changes: "carry", server: false, create: "branch" };
    case "Jujutsu":
      return { noun: "bookmark", title: "Bookmarks", copy: "workspace", changes: "stay", server: false, create: "bookmark" };
    case "Mercurial":
      return { noun: "bookmark or branch", title: "Bookmarks and branches", copy: "share", changes: "block", server: false, create: "bookmark" };
    case "Subversion":
      return { noun: "branch", title: "Branches", copy: "checkout", changes: "block", server: true, create: "branch" };
    case "Fossil":
      return { noun: "branch", title: "Branches", copy: "checkout", changes: "block", server: false, create: "branch" };
    default:
      return null;
  }
}

/**
 * How much a repo needs looking at:
 *  3 — something left half-done or broken: a rebase / merge / … in progress,
 *      conflicts (in files or, in Jujutsu, in changes), divergent changes, a
 *      locked Subversion working copy, a stale Jujutsu workspace.
 *  2 — work that exists only here: uncommitted changes, unpushed commits.
 *  1 — worth a look: stashes, behind its upstream, a detached HEAD (Git —
 *      in Jujutsu that's ordinary), an upstream deleted on the remote.
 *  0 — nothing. Having no remote at all isn't on its own: a local-only repo
 *      is a choice, and its row says so.
 */
export function attention(s: RepoStatus | null | undefined): number {
  if (!s) return 0;
  if (s.operation || s.conflicted > 0 || s.conflictedChanges > 0 || s.divergent > 0) return 3;
  if (s.changed + s.untracked > 0 || (s.unpushed ?? 0) > 0 || s.ahead > 0) return 2;
  if (s.stashes > 0 || s.behind > 0 || gitDetached(s) || s.upstreamGone) return 1;
  return 0;
}

/** A Git HEAD off every branch — worth a look. (Jujutsu's equivalent, a
 *  working copy with no bookmark below it, is how jj is mostly used.) */
const gitDetached = (s: RepoStatus) => s.vcs === "Git" && !!s.detached && !s.operation;

export const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** "just now", "4 min ago", "3 h ago", "2 days ago". */
export function ago(secs: number, now = Date.now() / 1000): string {
  const d = Math.max(0, now - secs);
  if (d < 60) return "just now";
  if (d < 3600) return `${Math.floor(d / 60)} min ago`;
  if (d < 86400) return `${Math.floor(d / 3600)} h ago`;
  const days = Math.floor(d / 86400);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

/** The chip for an operation in progress. */
const OP_VERB: Record<string, string> = {
  rebase: "rebasing",
  merge: "merging",
  "cherry-pick": "cherry-picking",
  revert: "reverting",
  bisect: "bisecting",
  am: "applying patches",
  graft: "grafting",
  histedit: "histediting",
  unshelve: "unshelving",
  transplant: "transplanting",
  update: "update interrupted",
  backout: "backing out",
  cleanup: "needs cleanup",
  stale: "stale",
};

/** The sentence for an operation in progress (`branch` is the one it's on). */
function opSentence(s: RepoStatus): string {
  switch (s.operation) {
    case "cleanup":
      return "An interrupted command left the working copy locked — `svn cleanup` clears it";
    case "stale":
      return "This workspace is stale — another one rewrote its change; `jj workspace update-stale` catches up";
    case "update":
      return "An `hg update` was interrupted — run it again to finish";
    default: {
      const verb = OP_VERB[s.operation ?? ""] ?? s.operation ?? "";
      return `${capitalise(verb)}${s.branch ? ` ${s.branch}` : ""}`;
    }
  }
}

/** What's checked out, for the row and the menu header: the branch (with
 *  Jujutsu's changes on top of the bookmark: `main +2`), or `@abc1234` when
 *  nothing names it. */
export function headLabel(s: RepoStatus | null | undefined): string | null {
  if (!s) return null;
  if (s.branch) return s.beyond > 0 ? `${s.branch} +${s.beyond}` : s.branch;
  if (s.detached) return `@${s.detached}`;
  return null;
}

export type Tone = "red" | "amber" | "sky" | "violet" | "dim";
export interface Chip {
  text: string;
  tone: Tone;
  /** The sentence it stands for. */
  title: string;
}

/** The compact flags on a repo's row, most urgent first. */
export function chips(s: RepoStatus | null | undefined): Chip[] {
  if (!s) return [];
  const out: Chip[] = [];
  if (s.operation) {
    out.push({ text: OP_VERB[s.operation] ?? s.operation, tone: "red", title: opSentence(s) });
  }
  if (s.conflicted > 0) {
    out.push({ text: `!${s.conflicted}`, tone: "red", title: `${plural(s.conflicted, "file")} with unresolved conflicts` });
  }
  if (s.conflictedChanges > 0) {
    out.push({
      text: `!${s.conflictedChanges}ch`,
      tone: "red",
      title: `${plural(s.conflictedChanges, "change")} holding a conflict (jj log -r 'conflicts()')`,
    });
  }
  if (s.divergent > 0) {
    out.push({ text: "divergent", tone: "red", title: `${plural(s.divergent, "change")} with more than one commit` });
  }
  if (s.changed > 0) {
    out.push({ text: `~${s.changed}`, tone: "amber", title: `${plural(s.changed, "changed file")} not committed` });
  }
  if (s.untracked > 0) {
    out.push({ text: `+${s.untracked}`, tone: "amber", title: `${plural(s.untracked, "new file")} not yet added` });
  }
  const unpushed = Math.max(s.ahead, s.unpushed ?? 0);
  if (unpushed > 0) {
    out.push({ text: `↑${unpushed}`, tone: "sky", title: unpushedSentence(s) });
  }
  if (s.behind > 0) {
    out.push({ text: `↓${s.behind}`, tone: "sky", title: `${plural(s.behind, "commit")} behind ${s.upstream}` });
  }
  if (s.stashes > 0) {
    out.push({ text: `≡${s.stashes}`, tone: "violet", title: capitalise(stashSentence(s)) });
  }
  if (gitDetached(s)) {
    out.push({ text: "detached", tone: "amber", title: `HEAD isn't on a branch (at ${s.detached})` });
  }
  if (s.upstreamGone) {
    out.push({ text: "gone", tone: "dim", title: `${s.upstream} was deleted on the remote` });
  }
  if (s.noRemote) {
    out.push({ text: "local", tone: "dim", title: "No remote: its whole history exists only here" });
  }
  if (s.unknown.length) {
    out.push({ text: "?", tone: "dim", title: s.unknown.join("\n") });
  }
  return out;
}

/** What "draft" / "unsent" / "unpushed" commits are called. */
function unpushedNoun(s: RepoStatus): string {
  if (s.vcs === "Mercurial") return "draft commit";
  if (s.vcs === "Fossil") return "unsent check-in";
  return "commit";
}

function unpushedSentence(s: RepoStatus): string {
  const parts: string[] = [];
  if (s.ahead > 0) parts.push(`${plural(s.ahead, "commit")} ahead of ${s.upstream}`);
  const elsewhere = (s.unpushed ?? 0) - s.ahead;
  if (s.ahead === 0 && (s.unpushed ?? 0) > 0) parts.push(`${plural(s.unpushed ?? 0, unpushedNoun(s))} on no remote`);
  else if (elsewhere > 0) parts.push(`${plural(elsewhere, "more commit")} on no remote, elsewhere in the history`);
  return parts.join("; ");
}

function stashSentence(s: RepoStatus): string {
  if (s.vcs === "Git") return plural(s.stashes, "stash", "stashes");
  if (s.vcs === "Fossil") return plural(s.stashes, "stashed change");
  return plural(s.stashes, "shelved change");
}

/** Every flag spelled out, one sentence each — the row's tooltip and the
 *  action menu's status line. */
export function describe(s: RepoStatus | null | undefined, now = Date.now() / 1000): string[] {
  if (!s) return [];
  const out: string[] = [];
  if (s.operation) {
    const conflicts = s.conflicted ? ` — ${plural(s.conflicted, "conflicted file")}` : "";
    out.push(opSentence(s) + conflicts);
  } else if (s.conflicted) {
    out.push(`${plural(s.conflicted, "file")} with unresolved conflicts`);
  }
  if (s.conflictedChanges) out.push(`${plural(s.conflictedChanges, "change")} holding a conflict`);
  if (s.divergent) out.push(`${plural(s.divergent, "divergent change")} — one change, several commits`);
  if (s.unborn) out.push(`No commits yet${s.branch ? ` on ${s.branch}` : ""}`);
  else if (gitDetached(s)) out.push(`Detached at ${s.detached} — not on a branch`);
  else if (s.vcs === "Jujutsu" && !s.branch && s.detached) out.push(`No bookmark below the working copy (${s.detached})`);
  if (s.vcs === "Jujutsu" && s.branch && s.beyond) {
    out.push(`${plural(s.beyond, "change")} on top of ${s.branch}`);
  }

  if (s.upstream && s.upstreamGone) {
    out.push(`${s.upstream} was deleted on the remote`);
  } else if (s.upstream) {
    const rel: string[] = [];
    if (s.ahead) rel.push(`${s.ahead} ahead`);
    if (s.behind) rel.push(`${s.behind} behind`);
    const fetched = s.fetchedAt ? ` (fetched ${ago(s.fetchedAt, now)})` : s.vcs === "Git" ? " (never fetched)" : "";
    out.push(rel.length ? `${s.upstream}: ${rel.join(", ")}${fetched}` : `Up to date with ${s.upstream}${fetched}`);
  } else if (s.branch && !s.unborn && !s.noRemote && (s.vcs === "Git" || s.vcs === "Jujutsu")) {
    out.push(`${s.branch} isn't published to a remote`);
  }
  const elsewhere = (s.unpushed ?? 0) - s.ahead;
  if (elsewhere > 0) {
    out.push(
      s.upstream
        ? `${plural(elsewhere, "more commit")} on no remote, elsewhere in the history`
        : `${plural(s.unpushed ?? 0, unpushedNoun(s))} on no remote`,
    );
  }
  const work: string[] = [];
  if (s.changed) work.push(`${plural(s.changed, "changed file")} not committed`);
  if (s.untracked) work.push(`${plural(s.untracked, "new file")} not yet added`);
  if (work.length) out.push(capitalise(work.join(", ")));
  if (s.stashes) out.push(capitalise(stashSentence(s)));
  if (s.noRemote) out.push("No remote — its whole history exists only here");
  out.push(...s.unknown);
  return out;
}

const capitalise = (t: string) => t.charAt(0).toUpperCase() + t.slice(1);

/** Prefix of the action ids the menu's version-control section uses. */
export const VCS_ACTION = "vcs:";

function vcsAction(id: string, label: string, hint: string, icon: string): Action {
  return {
    id: `${VCS_ACTION}${id}`,
    label,
    hint,
    group: "Version control",
    cluster: "",
    usageScore: 0,
    icon,
    clientSide: true,
  };
}

/** A location shown in a label: a URL's host and path, without the scheme. */
const place = (url: string) => url.replace(/^[a-z+]+:\/\/(?:[^@/]*@)?/i, "").replace(/\/+$/, "");

/**
 * The version-control section of a repo's action menu. Each entry is one
 * keystroke and says exactly what it does; one that wouldn't make sense now
 * isn't offered (a Git pull only fast-forwards a clean branch, a push only
 * goes to the branch's upstream when nothing there is newer, an update that
 * would merge into uncommitted changes waits for them to be committed).
 * `details` (remotes, links) may still be loading — `null` leaves out what
 * needs it.
 */
export function vcsActions(
  s: RepoStatus | null | undefined,
  details: VcsDetails | null,
  now = Date.now() / 1000,
): Action[] {
  if (!s?.vcs) return [];
  const out: Action[] = [];
  const v = vocab(s.vcs);
  if (v) {
    out.push(vcsAction("switch", `Switch ${v.noun}…`, headLabel(s) ?? "", "branch"));
  }
  const busy = !!s.operation || s.conflicted > 0;
  const clean = s.changed + s.conflicted === 0;
  const tracking = !!s.upstream && !s.upstreamGone;
  const remotes = details?.remotes ?? [];
  const remote = remotes.includes("origin") ? "origin" : remotes[0];

  switch (s.vcs) {
    case "Git":
      if (!busy && tracking && s.behind > 0 && s.ahead === 0 && clean) {
        out.push(vcsAction("pull", `Pull ${plural(s.behind, "commit")} from ${s.upstream}`, "git pull --ff-only", "git"));
      }
      if (!busy && tracking && s.ahead > 0 && s.behind === 0) {
        out.push(vcsAction("push", `Push ${plural(s.ahead, "commit")} to ${s.upstream}`, "git push", "git"));
      }
      if (!busy && s.branch && !tracking && !s.unborn && remote) {
        out.push(vcsAction(`publish:${remote}`, `Publish ${s.branch} to ${remote}`, `git push -u ${remote} ${s.branch}`, "git"));
      }
      if (remote) {
        const target = remotes.length === 1 ? remote : "all remotes";
        const when = s.fetchedAt ? `fetched ${ago(s.fetchedAt, now)}` : "never fetched";
        out.push(vcsAction("fetch", `Fetch from ${target}`, when, "git"));
      }
      break;
    case "Jujutsu":
      if (s.branch && tracking && s.ahead > 0 && s.behind === 0) {
        const remoteName = s.upstream?.split("@").pop() ?? "";
        out.push(vcsAction("push", `Push ${s.branch} to ${remoteName} (${plural(s.ahead, "commit")})`, `jj git push -b ${s.branch}`, "git"));
      }
      if (s.branch && !tracking && remote) {
        out.push(vcsAction(`publish:${remote}`, `Publish ${s.branch} to ${remote}`, `jj git push -b ${s.branch} --remote ${remote}`, "git"));
      }
      if (remote) out.push(vcsAction("fetch", `Fetch from ${remotes.length === 1 ? remote : "all remotes"}`, "jj git fetch", "git"));
      break;
    case "Mercurial":
      if (remote) out.push(vcsAction("fetch", `Pull from ${remote}`, "hg pull — then switch to what arrived", "git"));
      if (!busy && remote && (s.unpushed ?? 0) > 0) {
        out.push(vcsAction("push", `Push ${plural(s.unpushed ?? 0, "draft commit")} to ${remote}`, "hg push -r .", "git"));
      }
      break;
    case "Subversion":
      if (!busy) {
        const dirty = s.changed;
        out.push(
          vcsAction(
            "update",
            dirty ? `Update from the server (merges into your ${plural(dirty, "change")})` : "Update from the server",
            "svn update",
            "git",
          ),
        );
      }
      break;
    case "Fossil":
      if (remote) out.push(vcsAction("sync", `Sync with ${place(remote)}`, "fossil sync", "git"));
      if (!busy && clean) {
        out.push(vcsAction("update", `Update to the latest on ${s.branch ?? "this branch"}`, "fossil update", "git"));
      }
      break;
    case "Breezy":
      if (details?.pullFrom && !busy && clean) {
        out.push(vcsAction("pull", `Pull from ${place(details.pullFrom)}`, "brz pull", "git"));
      }
      if (details?.pushTo) out.push(vcsAction("push", `Push to ${place(details.pushTo)}`, "brz push", "git"));
      break;
  }
  for (const l of details?.links ?? []) {
    const icon = l.label.endsWith("GitHub") ? "github" : l.label.endsWith("GitLab") ? "gitlab" : "git";
    out.push(vcsAction(`link:${l.kind}`, l.label, l.url, icon));
  }
  return out;
}
