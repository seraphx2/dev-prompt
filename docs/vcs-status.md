# Branch and working-copy status

A branch name on its own changes nothing you do. What does is knowing which
repos have work that exists only on this machine, or something left half-done,
and being one keystroke from dealing with it. So dev-prompt shows the branch,
but the point is the **flags** next to it and what they let you do, all without
growing into a version-control client. It works for every system dev-prompt
reads — Git, Jujutsu, Mercurial, Subversion, Fossil and Breezy fully, Darcs as
far as Darcs goes — each in its own terms.

## What you see

**On every repo row** — what's checked out, and flags for anything worth
knowing, most urgent first. Hover one for the sentence it stands for.

What's checked out is the branch (Git, Subversion, Fossil), the active
bookmark or else the branch (Mercurial), the branch nickname (Breezy), or the
nearest bookmark below the working copy, with how many changes sit on top of
it — `main +2` (Jujutsu; with no bookmark below, the change's own id).
`@abc1234` is a Git HEAD off every branch.

| Flag        | Means                                                           |
| ----------- | --------------------------------------------------------------- |
| `rebasing` `merging` `cherry-picking` `reverting` `bisecting` `grafting` `histediting` `unshelving` `backing out` … | an operation stopped half-way |
| `needs cleanup` | Subversion: an interrupted command left the working copy locked (`svn cleanup`) |
| `stale`     | Jujutsu: the working copy was moved while jj wasn't looking at it (`jj workspace update-stale`) |
| `!2`        | files with unresolved conflicts                                 |
| `!2ch`      | Jujutsu: changes holding a conflict (jj records them in history rather than stopping) |
| `divergent` | Jujutsu: a change with more than one commit                     |
| `~3`        | changed files not committed                                     |
| `+1`        | new files not yet added                                         |
| `↑2`        | commits on no remote: ahead of the upstream, unpushed anywhere, Mercurial drafts, Fossil's unsent check-ins |
| `↓4`        | commits on the upstream you don't have, as of the last fetch (Git, Jujutsu) |
| `≡2`        | stashes, shelved changes                                        |
| `detached`  | Git: HEAD isn't on a branch                                     |
| `gone`      | the upstream branch (bookmark) was deleted on the remote        |
| `local`     | no remote at all: the whole history exists only here            |
| `?`         | something couldn't be checked (the tooltip says what)           |

What a system can't answer without asking its server isn't guessed:
Mercurial, Subversion, Fossil and Breezy have no `↓` (only Git and Jujutsu keep
the remote's state locally), and Breezy and Darcs can't tell what's unpushed —
which the Recycle Bin dialog counts as unknown, though the row doesn't flag it.

**Needs attention** — a flag in the footer counts the repos that need a look;
click it, or type `!` in the search box, for just those, most urgent first:

1. something half-done or broken — an operation in progress, conflicts
   (in files or in changes), divergent changes, a locked or stale working copy;
2. work that exists only here — uncommitted changes, unpushed commits;
3. worth a look — stashes, behind the upstream, a Git detached HEAD, an
   upstream that's gone.

Type after the `!` to filter that list. A repo with no remote isn't counted on
its own: keeping a repo local is a choice, and its row says so. Nor is a
Jujutsu working copy with no bookmark below it — that's how jj is mostly used.

**Search finds branches** — the repo search matches what's checked out, after
the repo's name and before its path. Type a ticket number and every repo on a
branch named after it comes up.

**The action menu** — a line under the header spells the state out
("origin/main: 2 ahead (fetched 3 h ago) · 1 changed file not committed"), in
amber when there's work only here, red when something's half-done. After the
everyday launchers comes a **Version control** section, offering only what
makes sense right now:

| System | Entries |
| ------ | ------- |
| Git | **Pull N commits** (`--ff-only`; behind, not ahead, nothing uncommitted) · **Push N commits** to the upstream (ahead, not behind) · **Publish** a branch with no upstream · **Fetch** (with when it last happened) |
| Jujutsu | **Push** the bookmark to the remote it tracks (ahead, not behind) · **Publish** a bookmark with no remote counterpart · **Fetch** — tracked bookmarks follow along |
| Mercurial | **Pull** (brings changesets in; moving onto them is the switcher's job) · **Push N draft commits** — the ones under the working copy, with its bookmark |
| Subversion | **Update from the server** — saying so when it'll merge into your local changes |
| Fossil | **Sync** with the remote · **Update to the latest on the branch** (only with nothing uncommitted) · **Open Fossil's web UI** (`fossil ui`, in a terminal) |
| Breezy | **Pull from the parent branch** (only with nothing uncommitted; refused once the branches diverge) · **Push to the push location** |

…and for Git, Jujutsu and Fossil remotes on the web: **Open on GitHub**,
**Open x on GitHub**, **Create pull request on GitHub** (GitLab, Bitbucket,
Azure DevOps, Codeberg and Gitea too) — the branch page once it's pushed, the
pull request unless it's the default branch.

Diverged (ahead *and* behind) gets neither a pull nor a push — that's a merge
or a rebase, a decision for a real client, and the menu's GUIs are right there.

**The branch switcher** (`Ctrl+B`, the name in the menu header, or **Switch
…** on a repo's right-click menu) is the same fuzzy list as the rest of the
app, with each system's own words:

| | Lists | Switching | `Ctrl+Enter` makes a |
| --- | --- | --- | --- |
| Git | branches (last checked out first), then remote branches nothing tracks | `git switch` — uncommitted changes come along; git refuses if one would be overwritten | **worktree** |
| Jujutsu | bookmarks, then remote bookmarks nothing tracks | `jj new <bookmark>`: a new change on top; the one you were in stays as it was | **workspace** |
| Mercurial | bookmarks, then named branches | `hg update --check` | **share** (bookmarks shared with it) |
| Subversion | `trunk` and `branches/*`, asked of the server | `svn switch` | **checkout** |
| Fossil | branches | `fossil update <branch>` | **checkout** of the same repository |

- Mercurial, Subversion and Fossil merge uncommitted changes into the branch
  you switch to, so there the switcher asks you to commit (or shelve) first —
  or to make a second working copy instead, which leaves them where they are.
- Nothing switches while something's half-done or conflicted.
- A branch already checked out in another working copy says where, and
  `Enter` goes to that one's action menu instead.
- `Ctrl+Enter` puts the second working copy beside the repo
  (`<repo>-<branch>`) and opens its action menu — a separate copy to run an AI
  agent or a long build in, without touching the one you're working in.
- Type a name that isn't there and the last row creates it, from where you
  are: a branch, a bookmark (Jujutsu, Mercurial), or a Subversion branch made
  on the server with `svn copy`.

Breezy has no switcher: a Breezy branch is normally a folder of its own, and
each one is already a repo in the list. Darcs has no branches at all.

## Where it stops

The rule: **dev-prompt does what fits in one keystroke and is fully described by
its label. Anything that needs you to look at something first is a real
client's job.** Staging, commit messages, diffs, history, conflict resolution,
rebasing, deleting branches — none of it is here, and won't be. The action
menu hands off instead: Fork, GitKraken, Sublime Merge, Git Extensions, GitHub
Desktop, TortoiseGit, gitg, lazygit, gitui and tig, each offered when
installed.

That's also why every operation is the safe form: a Git pull only
fast-forwards, a push only goes to the branch's own upstream, a switch never
forces or merges uncommitted work in, Subversion's update postpones conflicts
rather than picking a side, nothing stashes on your behalf.

## How it stays current

- **`vcs_state.rs`** reads one repo, each system its own way and offline:
  Git's `status --porcelain=v2` plus a couple of queries and file reads (the
  stash reflog, `FETCH_HEAD`'s time, a rebase or merge in progress); jj's
  revsets (`conflicts()`, `divergent`, the nearest bookmark and how far its
  remote is); Mercurial's status, phases and operation state files; `svn
  status` and `svn info`; `fossil changes` and its unsent table; Breezy's
  status, conflicts and nick. Reads never write — Git's set
  `GIT_OPTIONAL_LOCKS=0`, the others don't need to. **Move to Recycle Bin…**
  uses the same reading, so the dialog and the flags always agree.
- **`status.rs`** runs the reads on three background threads, never during a
  scan or on the UI thread, and saves them to `status.json` so the next launch
  shows the last-known flags at once. It reads a repo when:
  - **the folder watcher** sees something change in it: a file in the working
    tree, or one of the few files in its version control's own folder that
    say what's checked out and committed — Git's `HEAD`, `index` and refs;
    Mercurial's bookmarks, branch, changelog, phases and operation state;
    jj's operation heads; Subversion's `wc.db`; Breezy's dirstate and last
    revision; Fossil's checkout database. A linked Git worktree keeps its
    `HEAD` and index in its repo's `.git/worktrees/<name>/`, so a change
    there is traced to the worktree, and a change to the refs they share (a
    commit on any branch, a fetch) re-reads the repo and every worktree.
    Object stores, logs, locks and
    anything a status read itself writes (Mercurial's dirstate, jj's
    working-copy snapshot) are ignored. Changes are debounced per repo, so a
    build writing a thousand files is one read;
  - **the overlay opens**, for repos not read in the last five minutes — the
    backstop for a network share, or (on Linux, where only the folders the
    scan visits and each repo's version-control folders are watched) an edit
    deep in a repo;
  - **its action menu opens**, or an operation from dev-prompt finishes.
- **`vcs_ops.rs`** dispatches to `git_ops.rs`, `jj_ops.rs`, `hg_ops.rs`,
  `svn_ops.rs`, `fossil_ops.rs` and `brz_ops.rs`: the branch list, switching
  (refused while something's half-done, and where it would merge uncommitted
  changes), second working copies, the remote steps and the web links. Remote
  URLs (`https`, `ssh://`, `git@host:path`, Azure's `v3/` form) are turned
  into the forge's web address; an unrecognised host just gets no links.

Nothing errors when a tool is missing: without `hg` on `PATH`, Mercurial repos
show no flags and no Version control section, the same as any other missing
tool.

## Trying it out

`scripts/vcs-scenarios.sh <root> [git jj hg svn fossil brz | all] [--reset]`
builds a throwaway repo for each state, named `_<system>-*` so they sort to
the top (type `_jj` to see just Jujutsu's). Systems whose tool isn't installed
are skipped. Remotes live where the scan won't list them: Git's in
`_git-remotes/` (bare), everyone else's in `_vcs-remotes/` (a Git repo that
ignores its contents, so the repos inside fold into it). Everything works
offline except fetching `_git-github` and `_jj-github`, which really goes to
GitHub. Run it again with `--reset` to start over, and with `--remove` when
you're done — it deletes only the folders it makes (and their remotes), for
every system or just the ones you name:

    scripts/vcs-scenarios.sh /d/git --remove        # all of them
    scripts/vcs-scenarios.sh /d/git hg --remove     # just Mercurial's

| Folder | State |
| ------ | ----- |
| `_git-clean` | up to date — no flags |
| `_git-dirty` | `~2 +1`, on `PROJ-1234-invoice-api` |
| `_git-ahead` | `↑2` — offers a push; on `PROJ-1234-invoice-export` (search `PROJ-1234`) |
| `_git-behind` | `↓3` — offers a pull |
| `_git-diverged` | `↑1 ↓2` — neither |
| `_git-unpublished` | `feature/login` with no upstream — offers to publish |
| `_git-stash` | `≡2` |
| `_git-detached` | `detached` |
| `_git-rebase-conflict` / `_git-merge-conflict` | `rebasing !1` / `merging !1` |
| `_git-no-remote` | `local` — flagged, but not in the attention list |
| `_git-gone-upstream` | `gone` |
| `_git-worktrees` | branches to switch between, remote-only ones, and one in the worktree `_git-worktrees-feature-search` |
| `_git-github` | a GitHub remote — the web links, and a pull request for `feature/branch-status` |
| `_jj-clean` | up to date |
| `_jj-working` | `~2` in the working-copy change |
| `_jj-ahead` | `main` 2 ahead of `main@origin` — offers a push |
| `_jj-beyond` | `main +2`: two changes on top of the bookmark |
| `_jj-unpublished` | `feature/login` with no remote counterpart — offers to publish |
| `_jj-conflict` / `_jj-conflicted-change` | `!1` in the working copy / `!1ch` left in a change |
| `_jj-divergent` | `divergent` |
| `_jj-stale` | `stale` |
| `_jj-no-bookmark` | the change id instead of a branch, `local` |
| `_jj-workspaces` | bookmarks to switch to, a remote-only one, and one under the workspace `_jj-workspaces-feature-search` |
| `_jj-github` | a GitHub remote (colocated) — the web links |
| `_hg-clean`, `_hg-dirty` | up to date / `~1 +1` |
| `_hg-drafts` | `↑2` drafts — offers a push |
| `_hg-bookmarks` | bookmarks and a named branch to switch between |
| `_hg-rebase-conflict` / `_hg-merge-conflict` | `rebasing !1` / `merging !1` |
| `_hg-shelved` | `≡2` |
| `_hg-no-remote` | `local` |
| `_svn-trunk` | trunk; the switcher lists three branches from the server |
| `_svn-branch` | on `feature-x` with `~1 +1` — the switcher asks you to commit first |
| `_svn-conflict` | `!1` from an update |
| `_svn-locked` | `needs cleanup` |
| `_fossil-clean`, `_fossil-dirty` | up to date / `~1 +1` |
| `_fossil-unsent` | `↑2` unsent check-ins — offers a sync |
| `_fossil-merge-conflict` | `merging !1` |
| `_fossil-branches` | branches to switch between, one in the checkout `_fossil-branches-feature-search` |
| `_fossil-stash` | `≡1` |
| `_fossil-no-remote` | `local` |
| `_brz-trunk`, `_brz-feature` | a parent and its branch — `_brz-feature` offers a pull (trunk moved on) and a push (to its remembered mirror) |
| `_brz-conflict` | `merging !1` |
| `_brz-dirty` | `~2 +1 ≡1` |

To check what each one reads as, without the app:
`DP_SCENARIO_ROOT=<root> cargo test scenario_report -- --ignored --nocapture`
(from `src-tauri/`).
