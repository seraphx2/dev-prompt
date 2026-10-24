//! The state of a repository's working copy: the branch checked out, what
//! hasn't been committed or pushed, an operation left half-done. Shared by
//! the repo list's status flags (`status.rs`) and "Move to Recycle Bin…"
//! (`removal.rs`), so the two always agree on what exists only here.
//!
//! Git gets the full picture — branch, upstream, ahead / behind, conflicts, a
//! rebase or merge in progress — from one `git status --porcelain=v2` plus two
//! cheap queries and a few file reads. The other systems report what they can
//! answer offline; anything they can't goes in `unknown`, never counts as clean.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::provision::run_captured_env;
use crate::rules::which;

/// Only a command that's stuck (a network drive gone, a lock held elsewhere)
/// is stopped, and then counts as "couldn't check". A slow answer is waited
/// for: the first look at a big repository on a cold disk can take minutes,
/// and these reads run in the background anyway.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Reads must never write. Without this `git status` refreshes the index as a
/// side effect, which the watcher would see as a change and read again.
pub(crate) const READ_ONLY: &[(&str, &str)] = &[("GIT_OPTIONAL_LOCKS", "0")];

/// Mercurial's output with no user aliases, colours or localisation.
pub(crate) const HG_PLAIN: &[(&str, &str)] = &[("HGPLAIN", "1")];

/// The environment a read with `program` runs in.
fn read_env(program: &str) -> &'static [(&'static str, &'static str)] {
    match program {
        "git" => READ_ONLY,
        "hg" => HG_PLAIN,
        _ => &[],
    }
}

/// The version-control systems recognised, in the order they're looked for:
/// (label, markers). Jujutsu first: a colocated `.jj` repo also has a `.git`,
/// and jj's view is the authoritative one.
pub const SYSTEMS: &[(&str, &[&str])] = &[
    ("Jujutsu", &[".jj"]),
    ("Git", &[".git"]),
    ("Mercurial", &[".hg"]),
    ("Subversion", &[".svn"]),
    ("Breezy", &[".bzr"]),
    ("Darcs", &["_darcs"]),
    ("Fossil", &[".fslckout", "_FOSSIL_"]),
];

/// What a repository's working copy holds. Every count is 0 and every option
/// `None` when it doesn't apply or couldn't be read — `unknown` says which.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RepoStatus {
    /// "Git", "Mercurial", … — `None` for a folder without version control.
    pub vcs: Option<String>,
    /// The branch checked out (Mercurial: the active bookmark, else the
    /// branch). During a rebase, the branch being rebased.
    pub branch: Option<String>,
    /// Short commit id when HEAD isn't on a branch (Jujutsu: the working-copy
    /// change's id, when no bookmark is behind it).
    pub detached: Option<String>,
    /// Jujutsu: changes with content between `branch` (the nearest bookmark)
    /// and the working copy — "main +2".
    pub beyond: u32,
    /// No commits yet.
    pub unborn: bool,
    /// The branch's upstream (`origin/main`), when it has one.
    pub upstream: Option<String>,
    /// The upstream is configured but its remote branch is gone.
    pub upstream_gone: bool,
    /// Commits on the branch that its upstream doesn't have, and the reverse —
    /// as of the last fetch.
    pub ahead: u32,
    pub behind: u32,
    /// Tracked files changed, staged or not. Conflicted files aren't included.
    pub changed: u32,
    /// New files not yet added.
    pub untracked: u32,
    /// Files with unresolved conflicts.
    pub conflicted: u32,
    /// Jujutsu: changes other than the working copy that hold a conflict
    /// (jj records conflicts in history rather than stopping).
    pub conflicted_changes: u32,
    /// Jujutsu: changes that have become divergent — one change, several
    /// commits, usually from editing it in two places at once.
    pub divergent: u32,
    /// Stashes (Git), shelved changes (Mercurial, Breezy), stashed changes (Fossil).
    pub stashes: u32,
    /// Commits on no remote, across every local branch. `None` when there's
    /// nothing to compare with (no remote) or it can't be told offline.
    pub unpushed: Option<u32>,
    /// It has commits but no remote at all: its whole history exists only here.
    pub no_remote: bool,
    /// An operation left in progress: "rebase", "merge", "cherry-pick",
    /// "revert", "bisect", "am" (Git); "graft", "histedit", "unshelve",
    /// "transplant", "update" (Mercurial); "backout" (Fossil); "cleanup" (a
    /// Subversion working copy locked by an interrupted command); "stale" (a
    /// Jujutsu workspace another one has moved on from).
    pub operation: Option<String>,
    /// Unix seconds of the last fetch, when the system records one.
    pub fetched_at: Option<u64>,
    /// Unix seconds when this was read.
    pub checked_at: u64,
    /// What couldn't be checked, and why.
    pub unknown: Vec<String>,
    /// What this system can never tell without asking its remote (Breezy and
    /// Darcs: whether every commit was pushed). Not a failed check, so the
    /// repo list doesn't flag it; the Recycle Bin dialog counts it as unknown.
    pub limits: Vec<String>,
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn mtime_secs(p: &Path) -> Option<u64> {
    let t = p.metadata().ok()?.modified().ok()?;
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// The version-control system whose marker sits directly in `dir`.
pub fn detect(dir: &Path) -> Option<&'static str> {
    SYSTEMS
        .iter()
        .find(|(_, markers)| markers.iter().any(|m| dir.join(m).exists()))
        .map(|(label, _)| *label)
}

/// The command-line tool for a system label.
pub fn tool_for(label: &str) -> &'static str {
    match label {
        "Jujutsu" => "jj",
        "Git" => "git",
        "Mercurial" => "hg",
        "Subversion" => "svn",
        "Breezy" => "brz",
        "Darcs" => "darcs",
        _ => "fossil",
    }
}

/// Read `dir`'s state. Runs version-control commands, so call it off the UI
/// thread.
pub fn read(dir: &Path) -> RepoStatus {
    let mut s = RepoStatus { checked_at: now_secs(), ..Default::default() };
    let Some(label) = detect(dir) else { return s };
    s.vcs = Some(label.to_string());
    let tool = tool_for(label);
    if which(tool).is_none() {
        s.unknown.push(format!(
            "{label} isn't installed here (`{tool}` isn't on PATH), so its state couldn't be checked."
        ));
        return s;
    }
    let mut c = Checker { dir, s: &mut s };
    match label {
        "Jujutsu" => c.jj(),
        "Git" => c.git(),
        "Mercurial" => c.hg(),
        "Subversion" => c.svn(),
        "Breezy" => c.brz(),
        "Darcs" => c.darcs(),
        _ => c.fossil(),
    }
    s
}

// --- git's own files ------------------------------------------------------

/// Where a Git working copy keeps its state. In a linked worktree (or a
/// submodule) `.git` is a file pointing at `git_dir`; the refs, config and
/// stash that all worktrees share live in `common_dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDirs {
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
}

pub fn git_dirs(work: &Path) -> Option<GitDirs> {
    let dot = work.join(".git");
    let git_dir = if dot.is_dir() {
        dot
    } else {
        let text = std::fs::read_to_string(&dot).ok()?;
        let rel = text.lines().next()?.strip_prefix("gitdir:")?.trim();
        let p = Path::new(rel);
        if p.is_absolute() { p.to_path_buf() } else { work.join(p) }
    };
    let common_dir = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(t) => {
            let p = Path::new(t.trim());
            if p.is_absolute() { p.to_path_buf() } else { git_dir.join(p) }
        }
        Err(_) => git_dir.clone(),
    };
    Some(GitDirs { git_dir, common_dir })
}

/// A repository's linked worktrees, from what Git keeps for each in
/// `<common_dir>/worktrees/<name>/gitdir` (the path of the worktree's `.git`
/// file): their names and folders.
pub fn linked_worktrees(common_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(common_dir.join("worktrees")) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let gitdir = std::fs::read_to_string(e.path().join("gitdir")).ok()?;
            let folder = Path::new(gitdir.trim()).parent()?.to_path_buf();
            Some((e.file_name().to_string_lossy().into_owned(), folder))
        })
        .collect()
}

/// The operation left in progress in this working copy, from the files git
/// keeps while one runs.
fn git_operation(git_dir: &Path) -> Option<String> {
    let has = |name: &str| git_dir.join(name).exists();
    let op = if has("rebase-merge") {
        "rebase"
    } else if has("rebase-apply") {
        if has("rebase-apply/applying") { "am" } else { "rebase" }
    } else if has("MERGE_HEAD") {
        "merge"
    } else if has("CHERRY_PICK_HEAD") {
        "cherry-pick"
    } else if has("REVERT_HEAD") {
        "revert"
    } else if has("BISECT_LOG") {
        "bisect"
    } else {
        return None;
    };
    Some(op.to_string())
}

/// The branch a rebase in progress is rebasing (HEAD itself is detached).
fn rebasing_branch(git_dir: &Path) -> Option<String> {
    ["rebase-merge/head-name", "rebase-apply/head-name"]
        .iter()
        .find_map(|f| std::fs::read_to_string(git_dir.join(f)).ok())
        .and_then(|t| t.trim().strip_prefix("refs/heads/").map(str::to_string))
}

/// How many stashes there are: one reflog line each. `None` when the refs
/// aren't plain files (the reftable backend) and git has to be asked.
fn stash_count(common_dir: &Path) -> Option<u32> {
    if common_dir.join("reftable").is_dir() {
        return None;
    }
    match std::fs::read_to_string(common_dir.join("logs/refs/stash")) {
        Ok(t) => Some(t.lines().filter(|l| !l.trim().is_empty()).count() as u32),
        Err(_) => Some(0),
    }
}

/// Fill the branch and file counts from `git status --porcelain=v2 --branch`.
fn parse_porcelain_v2(out: &str, s: &mut RepoStatus) {
    let mut ab_seen = false;
    let mut oid = String::new();
    for line in out.lines() {
        if let Some(h) = line.strip_prefix("# ") {
            let (key, val) = h.split_once(' ').unwrap_or((h, ""));
            match key {
                "branch.oid" => {
                    s.unborn = val == "(initial)";
                    oid = val.to_string();
                }
                "branch.head" if val == "(detached)" => s.branch = None,
                "branch.head" => s.branch = Some(val.to_string()),
                "branch.upstream" => s.upstream = Some(val.to_string()),
                "branch.ab" => {
                    ab_seen = true;
                    for part in val.split_whitespace() {
                        if let Some(n) = part.strip_prefix('+') {
                            s.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            s.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        match line.as_bytes().first() {
            Some(b'1' | b'2') => s.changed += 1,
            Some(b'u') => s.conflicted += 1,
            Some(b'?') => s.untracked += 1,
            _ => {}
        }
    }
    if s.branch.is_none() && !s.unborn && oid.len() >= 7 {
        s.detached = Some(oid[..7].to_string());
    }
    // An upstream with no ahead/behind line: its remote branch is gone.
    s.upstream_gone = s.upstream.is_some() && !ab_seen;
}

/// Why `git status` failed, for the row's tooltip. Git refusing a repo another
/// account owns is said plainly, with the fix, instead of git's eight lines —
/// and never bypassed: that check stops a planted repo running commands
/// through its config.
fn git_status_failure(e: &str) -> String {
    if e.contains("dubious ownership") {
        "Git won't read this repo: its folder belongs to another account (often one from an earlier Windows \
         install). Take ownership of the folder, or add it to git's safe.directory."
            .into()
    } else {
        format!("Couldn't check for uncommitted changes: {e}")
    }
}

/// "3 changed, 2 new" from a status listing where lines starting with
/// `untracked` are new files.
fn count_changes(status: &str, untracked: &str) -> (u32, u32) {
    let lines: Vec<&str> = status.lines().filter(|l| !l.trim().is_empty()).collect();
    let new = if untracked.is_empty() {
        0
    } else {
        lines.iter().filter(|l| l.trim_start().starts_with(untracked)).count()
    };
    ((lines.len() - new) as u32, new as u32)
}

// --- per-system checks ----------------------------------------------------

/// Runs one system's checks, filling in `s` as it goes.
struct Checker<'a> {
    dir: &'a Path,
    s: &'a mut RepoStatus,
}

impl Checker<'_> {
    /// Run `program args` in the repo; `None` (and an "unknown" note naming
    /// `what`) when it fails or times out.
    fn run(&mut self, what: &str, program: &str, args: &[&str]) -> Option<String> {
        match self.run_raw(program, args) {
            Ok(out) => Some(out),
            Err(e) => {
                self.s.unknown.push(format!("Couldn't check {what}: {e}"));
                None
            }
        }
    }

    /// Like [`Checker::run`], but a failure is just `None` — for queries whose
    /// failure is itself an answer (an older jj revset name).
    fn run_quiet(&mut self, program: &str, args: &[&str]) -> Option<String> {
        self.run_raw(program, args).ok()
    }

    /// Run `program args` in the repo with its read-only environment.
    fn run_raw(&mut self, program: &str, args: &[&str]) -> Result<String, String> {
        let resolved = which(program).ok_or_else(|| format!("`{program}` isn't on PATH"))?;
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        run_captured_env(&resolved, &args, self.dir, CHECK_TIMEOUT, read_env(program))
    }

    fn git(&mut self) {
        let dirs = git_dirs(self.dir);
        if let Some(d) = &dirs {
            self.s.operation = git_operation(&d.git_dir);
            self.s.fetched_at = mtime_secs(&d.common_dir.join("FETCH_HEAD"));
        }
        let out = match self.run_raw("git", &["status", "--porcelain=v2", "--branch"]) {
            Ok(out) => out,
            Err(e) => {
                self.s.unknown.push(git_status_failure(&e));
                return;
            }
        };
        parse_porcelain_v2(&out, self.s);
        if self.s.operation.as_deref() == Some("rebase") {
            if let Some(b) = dirs.as_ref().and_then(|d| rebasing_branch(&d.git_dir)) {
                self.s.branch = Some(b);
            }
        }

        match dirs.as_ref().and_then(|d| stash_count(&d.common_dir)) {
            Some(n) => self.s.stashes = n,
            None => {
                if let Some(out) = self.run("for stashes", "git", &["stash", "list"]) {
                    self.s.stashes = out.lines().filter(|l| !l.trim().is_empty()).count() as u32;
                }
            }
        }

        if self.s.unborn {
            return;
        }
        let remotes = self.run("for remotes", "git", &["remote"]);
        match remotes.as_deref().map(str::trim) {
            Some("") => self.s.no_remote = true,
            Some(_) => {
                // Commits on any local branch, or a detached HEAD, that no
                // remote-tracking ref contains.
                if let Some(out) = self.run(
                    "for unpushed commits",
                    "git",
                    &["rev-list", "--count", "--branches", "HEAD", "--not", "--remotes"],
                ) {
                    match out.trim().parse::<u32>() {
                        Ok(n) => self.s.unpushed = Some(n),
                        Err(_) => self.s.unknown.push("Couldn't read git's count of unpushed commits.".into()),
                    }
                }
            }
            None => {}
        }
    }

    fn hg(&mut self) {
        // The active bookmark is what someone working with bookmarks thinks of
        // as their branch; without one, the named branch.
        let hg = self.dir.join(".hg");
        let read = |f: &str| {
            std::fs::read_to_string(hg.join(f)).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
        };
        self.s.branch = read("bookmarks.current").or_else(|| read("branch")).or_else(|| Some("default".into()));
        self.s.operation = hg_operation(&hg);

        if let Some(out) = self.run("for uncommitted changes", "hg", &["status"]) {
            (self.s.changed, self.s.untracked) = count_changes(&out, "?");
        }
        // Unresolved files only exist while a merge (or a rebase, graft, …
        // stopped on one) is in progress.
        if hg.join("merge").is_dir() {
            if let Some(out) = self.run("for unresolved conflicts", "hg", &["resolve", "--list"]) {
                self.s.conflicted = out.lines().filter(|l| l.starts_with("U ")).count() as u32;
                // `hg status` lists a conflicted file as modified too.
                self.s.changed = self.s.changed.saturating_sub(self.s.conflicted);
            }
        }
        // Failing to read the history counts as unknown, not as "no history".
        let tip = self.run("for commits", "hg", &["log", "-r", "tip", "-T", "{rev}"]);
        let has_commits = match tip.as_deref().map(str::trim) {
            Some("-1") => {
                self.s.unborn = true;
                false
            }
            Some(_) => true,
            None => false,
        };
        let paths = self.run("for remotes", "hg", &["paths"]);
        match paths.as_deref().map(str::trim) {
            Some("") => self.s.no_remote = has_commits,
            Some(_) if !has_commits => {}
            Some(_) => {
                // Mercurial marks every commit not yet pushed to a publishing
                // remote as draft (or secret) — answerable offline. What the
                // remote has that this clone doesn't isn't: `hg incoming` asks it.
                if let Some(out) = self.run(
                    "for unpushed commits",
                    "hg",
                    &["log", "-r", "draft() or secret()", "-T", "x\\n"],
                ) {
                    self.s.unpushed = Some(out.lines().filter(|l| !l.is_empty()).count() as u32);
                }
            }
            None => {}
        }
        // `shelve` is a bundled extension that may not be switched on.
        if let Some(out) = self.run(
            "for shelved changes",
            "hg",
            &["--config", "extensions.shelve=", "shelve", "--list"],
        ) {
            self.s.stashes = out.lines().filter(|l| !l.trim().is_empty()).count() as u32;
        }
    }

    fn jj(&mut self) {
        // The working copy is itself a commit (@). Reading its diff takes the
        // snapshot every later query here then skips (`--ignore-working-copy`).
        match self.run_raw("jj", &["--no-pager", "--color", "never", "diff", "-r", "@", "--summary"]) {
            Ok(out) => self.s.changed = out.lines().filter(|l| !l.trim().is_empty()).count() as u32,
            // Another workspace rewrote this one's working-copy commit:
            // `jj workspace update-stale` is the way out, not an error.
            Err(e) if e.contains("stale") => self.s.operation = Some("stale".into()),
            Err(e) => self.s.unknown.push(format!("Couldn't check for uncommitted changes: {e}")),
        }
        let log = |c: &mut Self, what: &str, revset: &str, template: &str| {
            c.run(what, "jj", &["--no-pager", "--ignore-working-copy", "log", "--no-graph", "-r", revset, "-T", template])
        };

        // What's checked out: the bookmark nearest below @ ("main", and how
        // many changes with content sit on top of it), else @'s own change id.
        let near = log(
            self,
            "for the nearest bookmark",
            "heads(::@ & bookmarks())",
            r#"commit_id.short(12) ++ "\t" ++ local_bookmarks.map(|b| b.name()).join(" ") ++ "\n""#,
        );
        let head = near.as_deref().and_then(|o| o.lines().next()).and_then(|l| l.split_once('\t'));
        match head {
            Some((commit, names)) if !names.trim().is_empty() => {
                let name = names.split_whitespace().next().unwrap_or_default().to_string();
                // Committed changes only: edits still in @ are counted as edits.
                if let Some(out) = log(self, "for changes on top of the bookmark", &format!("{commit}..@ & ~empty() & ~@"), "\"x\\n\"") {
                    self.s.beyond = out.lines().filter(|l| !l.is_empty()).count() as u32;
                }
                self.jj_tracking(&name, commit);
                self.s.branch = Some(name);
            }
            _ => {
                if let Some(id) = log(self, "for the working-copy change", "@", "change_id.shortest(8)") {
                    self.s.detached = Some(id.trim().to_string()).filter(|s| !s.is_empty());
                }
            }
        }

        if let Some(out) = log(self, "for conflicts", "@", r#"if(conflict, "conflict")"#) {
            if out.contains("conflict") {
                if let Some(files) = self.run_quiet("jj", &["--no-pager", "--ignore-working-copy", "resolve", "--list", "-r", "@"]) {
                    self.s.conflicted = files.lines().filter(|l| !l.trim().is_empty()).count() as u32;
                }
                self.s.conflicted = self.s.conflicted.max(1);
                // A conflicted file shows in the diff too.
                self.s.changed = self.s.changed.saturating_sub(self.s.conflicted);
            }
        }
        if let Some(out) = log(self, "for conflicted changes", "(conflicts() & mutable()) ~ @", "\"x\\n\"") {
            self.s.conflicted_changes = out.lines().filter(|l| !l.is_empty()).count() as u32;
        }
        if let Some(out) = log(self, "for divergent changes", "mutable()", r#"if(divergent, change_id.short() ++ "\n")"#) {
            let mut ids: Vec<&str> = out.lines().filter(|l| !l.is_empty()).collect();
            ids.sort_unstable();
            ids.dedup();
            self.s.divergent = ids.len() as u32;
        }

        let remotes = self.run(
            "for remotes",
            "jj",
            &["--no-pager", "--ignore-working-copy", "git", "remote", "list"],
        );
        match remotes.as_deref().map(str::trim) {
            Some("") => self.s.no_remote = true,
            Some(_) => {
                // Non-empty commits besides @ that no remote bookmark contains.
                // `remote_bookmarks()` was `remote_branches()` before jj 0.22.
                let count = |c: &mut Self, f: &str| {
                    let revset = format!("~::{f}() & ~empty() & ~@ & ~root()");
                    c.run_quiet(
                        "jj",
                        &["--no-pager", "--ignore-working-copy", "log", "--no-graph", "-r", &revset, "-T", "\"x\\n\""],
                    )
                    .map(|o| o.lines().filter(|l| !l.is_empty()).count() as u32)
                };
                match count(self, "remote_bookmarks").or_else(|| count(self, "remote_branches")) {
                    Some(n) => self.s.unpushed = Some(n),
                    None => self.s.unknown.push("Couldn't ask jj which commits are on no remote.".into()),
                }
            }
            None => {}
        }
    }

    /// The remote bookmark `name` tracks (origin's first), and how far the
    /// local one (at `commit`) is ahead of and behind it — all local refs.
    fn jj_tracking(&mut self, name: &str, commit: &str) {
        let Some(out) = self.run_quiet(
            "jj",
            &[
                "--no-pager",
                "--ignore-working-copy",
                "bookmark",
                "list",
                "--tracked",
                "-T",
                r#"if(remote, remote ++ "\n")"#,
                &format!("exact:{name}"),
            ],
        ) else {
            return;
        };
        let remotes: Vec<&str> = out.lines().map(str::trim).filter(|r| !r.is_empty() && *r != "git").collect();
        let Some(remote) = remotes.iter().find(|r| **r == "origin").or(remotes.first()).copied() else { return };
        let theirs = format!("remote_bookmarks(exact:{}, exact:{})", revset_str(name), revset_str(remote));
        let count = |c: &mut Self, revset: String| {
            c.run_quiet("jj", &["--no-pager", "--ignore-working-copy", "log", "--no-graph", "-r", &revset, "-T", "\"x\\n\""])
                .map(|o| o.lines().filter(|l| !l.is_empty()).count() as u32)
        };
        // A tracked bookmark deleted on the remote leaves nothing to compare with.
        match count(self, theirs.clone()) {
            Some(0) | None => {
                self.s.upstream = Some(format!("{name}@{remote}"));
                self.s.upstream_gone = true;
            }
            Some(_) => {
                self.s.upstream = Some(format!("{name}@{remote}"));
                self.s.ahead = count(self, format!("{theirs}..{commit}")).unwrap_or(0);
                self.s.behind = count(self, format!("{commit}..{theirs}")).unwrap_or(0);
            }
        }
    }

    fn svn(&mut self) {
        // Every Subversion commit goes straight to the server, so local
        // changes are the only thing that can be lost. What's checked out is
        // a path in the repository: `^/trunk`, `^/branches/x`.
        if let Some(out) = self.run("for the branch", "svn", &["info", "--show-item", "relative-url"]) {
            self.s.branch = svn_branch_label(out.trim());
        }
        if let Some(out) = self.run("for uncommitted changes", "svn", &["status"]) {
            let c = parse_svn_status(&out);
            (self.s.changed, self.s.untracked, self.s.conflicted) = (c.changed, c.untracked, c.conflicted);
            if c.locked {
                self.s.operation = Some("cleanup".into());
            }
        }
    }

    fn brz(&mut self) {
        if let Some(out) = self.run("for the branch", "brz", &["nick"]) {
            self.s.branch = Some(out.trim().to_string()).filter(|s| !s.is_empty());
        }
        if let Some(out) = self.run("for uncommitted changes", "brz", &["status", "--short"]) {
            let c = parse_brz_status(&out);
            (self.s.changed, self.s.untracked) = (c.changed, c.untracked);
            if c.pending_merge {
                self.s.operation = Some("merge".into());
            }
        }
        if let Some(out) = self.run("for conflicts", "brz", &["conflicts"]) {
            self.s.conflicted = out.lines().filter(|l| !l.trim().is_empty()).count() as u32;
            self.s.changed = self.s.changed.saturating_sub(self.s.conflicted);
        }
        // `shelve --list` exits 1 when there *are* shelves, and lists them.
        let shelves = match self.run_raw("brz", &["shelve", "--list"]) {
            Ok(out) => Some(out),
            Err(e) if e.starts_with("exit code 1.") => Some(e),
            Err(e) => {
                self.s.unknown.push(format!("Couldn't check for shelved changes: {e}"));
                None
            }
        };
        if let Some(out) = shelves {
            self.s.stashes =
                out.lines().filter(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())).count() as u32;
        }
        self.s.limits.push(
            "Breezy can only tell whether every commit was pushed by asking the remote, which isn't done here."
                .into(),
        );
    }

    fn darcs(&mut self) {
        // `whatsnew` exits 1 with "No changes!" when there's nothing to report;
        // any other failure (a timeout included) leaves the changes unchecked.
        if let Some(darcs) = which("darcs") {
            let args = ["whatsnew", "--summary", "--look-for-adds"].map(String::from);
            match run_captured_env(&darcs, &args, self.dir, CHECK_TIMEOUT, &[]) {
                Ok(out) => (self.s.changed, self.s.untracked) = count_changes(&out, "a"),
                Err(e) if e.starts_with("exit code 1.") && e.contains("No changes!") => {}
                Err(e) => self.s.unknown.push(format!("Couldn't check for uncommitted changes: {e}")),
            }
        }
        self.s.limits.push(
            "Darcs can only tell whether every patch was pushed by asking the remote, which isn't done here."
                .into(),
        );
    }

    fn fossil(&mut self) {
        if let Some(out) = self.run("for the branch", "fossil", &["branch", "current"]) {
            self.s.branch = Some(out.trim().to_string()).filter(|s| !s.is_empty());
        }
        if let Some(out) = self.run("for uncommitted changes", "fossil", &["changes"]) {
            let c = parse_fossil_changes(&out);
            (self.s.changed, self.s.conflicted) = (c.changed, c.conflicted);
            self.s.operation = c.operation;
        }
        if let Some(out) = self.run("for files not added", "fossil", &["extras"]) {
            self.s.untracked = out.lines().filter(|l| !l.trim().is_empty()).count() as u32;
        }
        if let Some(out) = self.run("for stashed changes", "fossil", &["stash", "list"]) {
            self.s.stashes =
                out.lines().filter(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())).count() as u32;
        }
        match self.run("for remotes", "fossil", &["remote"]).as_deref().map(str::trim) {
            Some("off" | "") => self.s.no_remote = true,
            Some(_) => {
                // Fossil keeps a list of what it hasn't sent to its remote yet;
                // its check-ins are the commits on no remote.
                let sql = "SELECT count(*) FROM unsent JOIN event ON event.objid = unsent.rid WHERE event.type = 'ci'";
                match self.run_quiet("fossil", &["sql", sql]).and_then(|o| o.trim().trim_matches('\'').parse().ok()) {
                    Some(n) => self.s.unpushed = Some(n),
                    None => self.s.unknown.push("Couldn't ask Fossil which check-ins haven't been sent yet.".into()),
                }
            }
            None => {}
        }
    }
}

/// Mercurial's operation left in progress, from the state file it keeps
/// while one runs. A plain merge leaves only `merge/state` behind, which the
/// others also write — so it's checked last.
fn hg_operation(hg: &Path) -> Option<String> {
    let op = [
        ("rebasestate", "rebase"),
        ("histedit-state", "histedit"),
        ("graftstate", "graft"),
        ("shelvedstate", "unshelve"),
        ("transplant/journal", "transplant"),
        ("updatestate", "update"),
        ("bisect.state", "bisect"),
        ("merge/state", "merge"),
        ("merge/state2", "merge"),
    ]
    .iter()
    .find(|(f, _)| hg.join(f).exists())?
    .1;
    Some(op.to_string())
}

/// `^/branches/feature-x` → `feature-x`; `^/trunk` → `trunk`; anything else
/// as it is, without the `^/`.
fn svn_branch_label(relative_url: &str) -> Option<String> {
    let p = relative_url.strip_prefix("^/").unwrap_or(relative_url).trim_matches('/');
    if p.is_empty() {
        return None;
    }
    Some(p.strip_prefix("branches/").unwrap_or(p).to_string())
}

#[derive(Debug, Default, PartialEq, Eq)]
struct SvnCounts {
    changed: u32,
    untracked: u32,
    conflicted: u32,
    /// A working-copy lock left by an interrupted command: `svn cleanup`.
    locked: bool,
}

/// `svn status`: seven status columns, a space, the path. Tree-conflict
/// details (`>   …`) and the closing summary aren't files.
fn parse_svn_status(out: &str) -> SvnCounts {
    let mut c = SvnCounts::default();
    for line in out.lines() {
        let cols: Vec<char> = line.chars().take(8).collect();
        if cols.len() < 8 || cols[7] != ' ' || cols[0] == '>' || line.starts_with("Summary of conflicts") {
            continue;
        }
        if !cols[..7].iter().all(|ch| " ACDIMRX?!~LKOTBS+C*".contains(*ch)) {
            continue;
        }
        if cols[2] == 'L' {
            c.locked = true;
        }
        match cols[0] {
            '?' => c.untracked += 1,
            'I' | 'X' => {}
            _ if cols[0] == 'C' || cols[1] == 'C' || cols[6] == 'C' => c.conflicted += 1,
            'A' | 'D' | 'M' | 'R' | '!' | '~' => c.changed += 1,
            _ if cols[1] == 'M' => c.changed += 1,
            _ => {}
        }
    }
    c
}

#[derive(Debug, Default, PartialEq, Eq)]
struct BrzCounts {
    changed: u32,
    untracked: u32,
    pending_merge: bool,
}

/// `brz status --short`: `?` unknown files, `P` a pending merge's revisions,
/// everything else a change.
fn parse_brz_status(out: &str) -> BrzCounts {
    let mut c = BrzCounts::default();
    for line in out.lines().filter(|l| !l.trim().is_empty()) {
        match line.trim_start().chars().next() {
            Some('?') => c.untracked += 1,
            Some('P') => c.pending_merge = true,
            _ => c.changed += 1,
        }
    }
    c
}

#[derive(Debug, Default, PartialEq, Eq)]
struct FossilCounts {
    changed: u32,
    conflicted: u32,
    operation: Option<String>,
}

/// `fossil changes`: one `KEYWORD path` line per file, plus a line naming an
/// uncommitted merge, cherry-pick or backout.
fn parse_fossil_changes(out: &str) -> FossilCounts {
    let mut c = FossilCounts::default();
    for line in out.lines().filter(|l| !l.trim().is_empty()) {
        let op = match line.split_whitespace().next().unwrap_or_default() {
            "CONFLICT" => {
                c.conflicted += 1;
                continue;
            }
            "MERGED_WITH" | "INTEGRATE" => "merge",
            "CHERRYPICK" => "cherry-pick",
            "BACKOUT" => "backout",
            _ => {
                c.changed += 1;
                continue;
            }
        };
        c.operation.get_or_insert_with(|| op.to_string());
    }
    c
}

/// A jj revset string literal.
fn revset_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "dp-vcs-{tag}-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Run git in `dir` with an identity set, so commits work on a machine
    /// that has none configured. Returns stdout.
    pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A clone of a bare "remote" with one pushed commit on `main`.
    pub(crate) fn cloned(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = tmp(tag);
        let (remote, work) = (base.join("remote.git"), base.join("work"));
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare", "-q", "--initial-branch=main"]);
        git(&base, &["clone", "-q", &remote.to_string_lossy(), "work"]);
        git(&work, &["checkout", "-q", "-B", "main"]);
        commit(&work, "a.txt", "a");
        git(&work, &["push", "-q", "-u", "origin", "main"]);
        (base, remote, work)
    }

    pub(crate) fn commit(dir: &Path, file: &str, text: &str) {
        std::fs::write(dir.join(file), text).unwrap();
        git(dir, &["add", file]);
        git(dir, &["commit", "-q", "-m", file]);
    }

    #[test]
    fn a_repo_git_refuses_says_why_plainly() {
        let refused = "exit code 128.\nfatal: detected dubious ownership in repository at 'D:/git/x'\n'D:/git/x' is owned by:";
        let said = git_status_failure(refused);
        assert!(said.contains("belongs to another account") && said.contains("safe.directory"), "{said}");
        assert!(!said.contains("fatal"), "{said}");
        assert_eq!(git_status_failure("exit code 1.\nboom"), "Couldn't check for uncommitted changes: exit code 1.\nboom");
    }

    #[test]
    fn count_changes_splits_changed_and_new() {
        assert_eq!(count_changes("", "??"), (0, 0));
        assert_eq!(count_changes(" M a.rs\n?? b.rs\n?? c/\nA  d.rs\n", "??"), (2, 2));
        assert_eq!(count_changes("M x\n", "?"), (1, 0));
        assert_eq!(count_changes("EDITED x\nADDED y\n", ""), (2, 0));
    }

    #[test]
    fn porcelain_v2_reads_branch_counts_and_conflicts() {
        let mut s = RepoStatus::default();
        parse_porcelain_v2(
            "# branch.oid 1234567890abcdef\n# branch.head feature/x\n# branch.upstream origin/feature/x\n\
             # branch.ab +2 -3\n1 .M N... 100644 100644 100644 a a a.txt\n\
             2 R. N... 100644 100644 100644 a a R100 b.txt\tc.txt\n\
             u UU N... 100644 100644 100644 100644 a a a d.txt\n? new.txt\n! ignored\n",
            &mut s,
        );
        assert_eq!(s.branch.as_deref(), Some("feature/x"));
        assert_eq!(s.upstream.as_deref(), Some("origin/feature/x"));
        assert_eq!((s.ahead, s.behind), (2, 3));
        assert_eq!((s.changed, s.conflicted, s.untracked), (2, 1, 1));
        assert!(!s.upstream_gone && !s.unborn && s.detached.is_none());

        let mut s = RepoStatus::default();
        parse_porcelain_v2("# branch.oid abcdef0123\n# branch.head (detached)\n", &mut s);
        assert_eq!((s.branch, s.detached.as_deref()), (None, Some("abcdef0")));

        let mut s = RepoStatus::default();
        parse_porcelain_v2("# branch.oid (initial)\n# branch.head main\n", &mut s);
        assert!(s.unborn && s.detached.is_none());

        let mut s = RepoStatus::default();
        parse_porcelain_v2("# branch.oid abcdef0123\n# branch.head x\n# branch.upstream origin/x\n", &mut s);
        assert!(s.upstream_gone);
    }

    #[test]
    fn a_plain_folder_has_no_status() {
        let dir = tmp("plain");
        let s = read(&dir);
        assert_eq!(s.vcs, None);
        assert_eq!(s, RepoStatus { checked_at: s.checked_at, ..Default::default() });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_branch_upstream_and_divergence() {
        if which("git").is_none() {
            return;
        }
        let (base, _remote, work) = cloned("diverge");
        let s = read(&work);
        assert_eq!(s.vcs.as_deref(), Some("Git"));
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind, s.unpushed), (0, 0, Some(0)));
        assert!(s.unknown.is_empty(), "{s:?}");

        // Someone else pushes; we fetch and commit our own.
        git(&base, &["clone", "-q", &base.join("remote.git").to_string_lossy(), "other"]);
        commit(&base.join("other"), "theirs.txt", "t");
        git(&base.join("other"), &["push", "-q"]);
        git(&work, &["fetch", "-q"]);
        commit(&work, "mine.txt", "m");
        let s = read(&work);
        assert_eq!((s.ahead, s.behind, s.unpushed), (1, 1, Some(1)));
        assert!(s.fetched_at.is_some());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn git_changes_stashes_detached_and_no_remote() {
        if which("git").is_none() {
            return;
        }
        let dir = tmp("local");
        git(&dir, &["init", "-q", "--initial-branch=trunk"]);
        let s = read(&dir);
        assert!(s.unborn && !s.no_remote && s.branch.as_deref() == Some("trunk"), "{s:?}");

        commit(&dir, "a.txt", "a");
        commit(&dir, "b.txt", "b");
        std::fs::write(dir.join("a.txt"), "stashed").unwrap();
        git(&dir, &["stash", "-q"]);
        std::fs::write(dir.join("a.txt"), "edited").unwrap();
        std::fs::write(dir.join("new.txt"), "n").unwrap();
        let s = read(&dir);
        assert!(s.no_remote && s.unpushed.is_none(), "{s:?}");
        assert_eq!((s.changed, s.untracked, s.stashes), (1, 1, 1));

        git(&dir, &["checkout", "-q", "--detach", "HEAD~1"]);
        let s = read(&dir);
        assert_eq!(s.branch, None);
        assert_eq!(s.detached.as_deref().map(str::len), Some(7));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_rebase_left_in_conflict_names_its_branch() {
        if which("git").is_none() {
            return;
        }
        let dir = tmp("rebase");
        git(&dir, &["init", "-q", "--initial-branch=main"]);
        commit(&dir, "f.txt", "base");
        git(&dir, &["checkout", "-q", "-b", "topic"]);
        commit(&dir, "f.txt", "topic");
        git(&dir, &["checkout", "-q", "main"]);
        commit(&dir, "f.txt", "main");
        git(&dir, &["checkout", "-q", "topic"]);
        let failed = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "rebase", "main"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(!failed.status.success());

        let s = read(&dir);
        assert_eq!(s.operation.as_deref(), Some("rebase"));
        assert_eq!(s.branch.as_deref(), Some("topic"));
        assert_eq!(s.conflicted, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_linked_worktree_reads_through_its_git_file() {
        if which("git").is_none() {
            return;
        }
        let dir = tmp("wt");
        let main = dir.join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "--initial-branch=main"]);
        commit(&main, "a.txt", "a");
        git(&main, &["worktree", "add", "-q", "-b", "side", &dir.join("side").to_string_lossy()]);

        let d = git_dirs(&dir.join("side")).unwrap();
        assert!(d.git_dir.ends_with("worktrees/side") || d.git_dir.ends_with("worktrees\\side"), "{d:?}");
        assert_eq!(
            std::fs::canonicalize(&d.common_dir).unwrap(),
            std::fs::canonicalize(main.join(".git")).unwrap()
        );
        // The repo knows its worktrees, by name and folder.
        let linked = linked_worktrees(&main.join(".git"));
        assert_eq!(linked.len(), 1, "{linked:?}");
        assert_eq!(linked[0].0, "side");
        assert_eq!(
            std::fs::canonicalize(&linked[0].1).unwrap(),
            std::fs::canonicalize(dir.join("side")).unwrap()
        );
        let s = read(&dir.join("side"));
        assert_eq!(s.branch.as_deref(), Some("side"));
        assert!(s.unknown.is_empty(), "{s:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Run a version-control command in `dir` with an identity set for every
    /// system, so commits work on a machine that has none configured.
    pub(crate) fn vcs(dir: &Path, program: &str, args: &[&str]) -> String {
        let out = std::process::Command::new(which(program).unwrap_or_else(|| program.into()))
            .args(args)
            .current_dir(dir)
            .env("HGUSER", "t <t@t>")
            .env("HGPLAIN", "1")
            .env("JJ_USER", "t")
            .env("JJ_EMAIL", "t@t")
            .env("BRZ_EMAIL", "t <t@t>")
            .env("FOSSIL_USER", "t")
            .output()
            .unwrap_or_else(|e| panic!("{program} {args:?}: {e}"));
        assert!(
            out.status.success(),
            "{program} {args:?}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Like [`vcs`], for a command expected to fail (a merge that conflicts).
    pub(crate) fn vcs_may_fail(dir: &Path, program: &str, args: &[&str]) {
        let _ = std::process::Command::new(which(program).unwrap_or_else(|| program.into()))
            .args(args)
            .current_dir(dir)
            .env("HGUSER", "t <t@t>")
            .env("JJ_USER", "t")
            .env("JJ_EMAIL", "t@t")
            .env("BRZ_EMAIL", "t <t@t>")
            .env("FOSSIL_USER", "t")
            .output();
    }

    pub(crate) fn file_url(p: &Path) -> String {
        let s = p.to_string_lossy().replace('\\', "/");
        if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
    }

    /// Not a check: prints what each repo `scripts/vcs-scenarios.sh` made
    /// reads as, to compare with what the script says it made.
    /// `DP_SCENARIO_ROOT=/d/git cargo test scenario_report -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scenario_report() {
        let root = PathBuf::from(std::env::var("DP_SCENARIO_ROOT").expect("DP_SCENARIO_ROOT"));
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('_')) && detect(p).is_some())
            .collect();
        dirs.sort();
        for d in dirs {
            let s = read(&d);
            let mut flags = Vec::new();
            for (n, v) in [
                ("beyond", s.beyond),
                ("ahead", s.ahead),
                ("behind", s.behind),
                ("changed", s.changed),
                ("new", s.untracked),
                ("conflicted", s.conflicted),
                ("conflicted-changes", s.conflicted_changes),
                ("divergent", s.divergent),
                ("stashes", s.stashes),
            ] {
                if v > 0 {
                    flags.push(format!("{n}={v}"));
                }
            }
            if let Some(n) = s.unpushed.filter(|n| *n > 0) {
                flags.push(format!("unpushed={n}"));
            }
            for (n, v) in [("unborn", s.unborn), ("no-remote", s.no_remote), ("upstream-gone", s.upstream_gone)] {
                if v {
                    flags.push(n.to_string());
                }
            }
            println!(
                "{:<34} {:<10} {:<28} {:<26} {} {}{}",
                d.file_name().unwrap().to_string_lossy(),
                s.vcs.as_deref().unwrap_or("-"),
                s.branch.as_deref().or(s.detached.as_deref()).unwrap_or("-"),
                s.upstream.as_deref().unwrap_or("-"),
                s.operation.as_deref().map(|o| format!("[{o}] ")).unwrap_or_default(),
                flags.join(" "),
                if s.unknown.is_empty() { String::new() } else { format!("  UNKNOWN: {:?}", s.unknown) },
            );
        }
    }

    #[test]
    fn small_parsers_for_the_other_systems() {
        assert_eq!(svn_branch_label("^/trunk").as_deref(), Some("trunk"));
        assert_eq!(svn_branch_label("^/branches/feature-x").as_deref(), Some("feature-x"));
        assert_eq!(svn_branch_label("^/tags/v1.0").as_deref(), Some("tags/v1.0"));
        assert_eq!(svn_branch_label("^/").as_deref(), None);

        let svn = parse_svn_status(
            "M       a.txt\n A  +   b.txt\n?       n.txt\nC       c.txt\n M      props.txt\n      C tree.txt\n\
             >   local edit, incoming delete upon update\n  L     .\nI       ignored\n\
             Summary of conflicts:\n  Text conflicts: 1\n",
        );
        assert_eq!(svn, SvnCounts { changed: 2, untracked: 1, conflicted: 2, locked: true }, "{svn:?}");

        let brz = parse_brz_status(" M  a.txt\n+N  b.txt\n?   n.txt\nP   t@t-2026-x\n");
        assert_eq!(brz, BrzCounts { changed: 2, untracked: 1, pending_merge: true });

        let f = parse_fossil_changes("EDITED     a.txt\nCONFLICT   b.txt\nADDED      c.txt\nMERGED_WITH 827f68\n");
        assert_eq!(f, FossilCounts { changed: 2, conflicted: 1, operation: Some("merge".into()) });

        assert_eq!(revset_str(r#"a"b\c"#), r#""a\"b\\c""#);

        let hg = tmp("hgop");
        assert_eq!(hg_operation(&hg), None);
        std::fs::create_dir_all(hg.join("merge")).unwrap();
        std::fs::write(hg.join("merge/state"), "x").unwrap();
        assert_eq!(hg_operation(&hg).as_deref(), Some("merge"));
        std::fs::write(hg.join("rebasestate"), "x").unwrap();
        assert_eq!(hg_operation(&hg).as_deref(), Some("rebase"), "a rebase stopped on a conflict is a rebase");
        let _ = std::fs::remove_dir_all(&hg);
    }

    #[test]
    fn mercurial_bookmark_rebase_conflict_and_drafts() {
        if which("hg").is_none() {
            return;
        }
        let base = tmp("hg-state");
        let (remote, work) = (base.join("remote"), base.join("work"));
        std::fs::create_dir_all(&remote).unwrap();
        vcs(&remote, "hg", &["init"]);
        vcs(&base, "hg", &["clone", "-q", &remote.to_string_lossy(), "work"]);
        let s = read(&work);
        assert!(s.unborn && !s.no_remote, "{s:?}");
        std::fs::write(work.join("a.txt"), "base").unwrap();
        vcs(&work, "hg", &["add", "a.txt"]);
        vcs(&work, "hg", &["commit", "-m", "base"]);
        vcs(&work, "hg", &["push", "-q"]);

        vcs(&work, "hg", &["bookmark", "feat"]);
        std::fs::write(work.join("a.txt"), "feat").unwrap();
        vcs(&work, "hg", &["commit", "-m", "feat"]);
        let s = read(&work);
        assert_eq!(s.branch.as_deref(), Some("feat"));
        assert_eq!(s.unpushed, Some(1));

        vcs(&work, "hg", &["update", "-q", "-r", "0"]);
        std::fs::write(work.join("a.txt"), "other").unwrap();
        vcs(&work, "hg", &["commit", "-q", "-m", "other"]);
        vcs_may_fail(&work, "hg", &["rebase", "--config", "extensions.rebase=", "-s", ".", "-d", "feat"]);
        let s = read(&work);
        assert_eq!(s.operation.as_deref(), Some("rebase"), "{s:?}");
        assert_eq!(s.conflicted, 1, "{s:?}");
        assert_eq!(s.changed, 0, "the conflicted file isn't counted twice: {s:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn jujutsu_bookmark_distance_tracking_and_conflicts() {
        if which("jj").is_none() || which("git").is_none() {
            return;
        }
        let base = tmp("jj-state");
        let (remote, work) = (base.join("remote.git"), base.join("work"));
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare", "-q"]);
        std::fs::create_dir_all(&work).unwrap();
        vcs(&work, "jj", &["git", "init"]);
        vcs(&work, "jj", &["git", "remote", "add", "origin", &remote.to_string_lossy()]);
        std::fs::write(work.join("a.txt"), "base").unwrap();
        vcs(&work, "jj", &["commit", "-m", "base"]);
        vcs(&work, "jj", &["bookmark", "create", "main", "-r", "@-"]);
        vcs(&work, "jj", &["git", "push", "--bookmark", "main"]);

        let s = read(&work);
        assert_eq!(s.vcs.as_deref(), Some("Jujutsu"));
        assert_eq!(s.branch.as_deref(), Some("main"), "{s:?}");
        assert_eq!((s.beyond, s.ahead, s.behind), (0, 0, 0), "{s:?}");
        assert_eq!(s.upstream.as_deref(), Some("main@origin"));

        // Two changes with content on top, the bookmark moved along to the
        // first: ahead of origin by one, one more change beyond it.
        std::fs::write(work.join("b.txt"), "b").unwrap();
        vcs(&work, "jj", &["commit", "-m", "b"]);
        vcs(&work, "jj", &["bookmark", "move", "main", "--to", "@-"]);
        std::fs::write(work.join("c.txt"), "c").unwrap();
        vcs(&work, "jj", &["commit", "-m", "c"]);
        let s = read(&work);
        assert_eq!((s.branch.as_deref(), s.beyond, s.ahead), (Some("main"), 1, 1), "{s:?}");
        assert_eq!(s.unpushed, Some(2));

        // Siblings editing one file, then one rebased onto the other: a
        // conflict in the working copy, recorded in history rather than stopping.
        vcs(&work, "jj", &["new", "main", "-m", "x"]);
        std::fs::write(work.join("a.txt"), "x").unwrap();
        vcs(&work, "jj", &["new", "main", "-m", "y"]);
        std::fs::write(work.join("a.txt"), "y").unwrap();
        vcs(&work, "jj", &["rebase", "-r", "@", "-d", "description(substring:x)"]);
        let s = read(&work);
        assert_eq!(s.conflicted, 1, "{s:?}");
        // Moving off it leaves the conflict in a change of its own.
        vcs(&work, "jj", &["new", "main"]);
        let s = read(&work);
        assert_eq!((s.conflicted, s.conflicted_changes), (0, 1), "{s:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn jujutsu_without_a_bookmark_names_the_change_and_finds_divergence() {
        if which("jj").is_none() {
            return;
        }
        let dir = tmp("jj-div");
        vcs(&dir, "jj", &["git", "init"]);
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        vcs(&dir, "jj", &["describe", "-m", "one"]);
        let s = read(&dir);
        assert_eq!(s.branch, None);
        assert!(s.detached.as_deref().is_some_and(|d| !d.is_empty()), "{s:?}");
        assert!(s.no_remote);

        // Describe the same change from an earlier operation as well: two
        // commits for one change.
        let ops = vcs(&dir, "jj", &["op", "log", "--no-graph", "-T", "id.short() ++ \"\\n\"", "-n", "2"]);
        let earlier = ops.lines().nth(1).unwrap().trim().to_string();
        vcs(&dir, "jj", &["--at-op", &earlier, "describe", "-m", "two"]);
        let s = read(&dir);
        assert_eq!(s.divergent, 1, "{s:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn subversion_branch_changes_and_conflicts() {
        if which("svn").is_none() || which("svnadmin").is_none() {
            return;
        }
        let base = tmp("svn-state");
        vcs(&base, "svnadmin", &["create", "repo"]);
        let url = file_url(&base.join("repo"));
        vcs(&base, "svn", &["mkdir", "-q", "-m", "layout", &format!("{url}/trunk"), &format!("{url}/branches")]);
        vcs(&base, "svn", &["copy", "-q", "-m", "branch", &format!("{url}/trunk"), &format!("{url}/branches/feature-x")]);
        vcs(&base, "svn", &["checkout", "-q", &format!("{url}/branches/feature-x"), "wc"]);
        vcs(&base, "svn", &["checkout", "-q", &format!("{url}/branches/feature-x"), "other"]);
        let (wc, other) = (base.join("wc"), base.join("other"));
        std::fs::write(wc.join("a.txt"), "a").unwrap();
        vcs(&wc, "svn", &["add", "-q", "a.txt"]);
        vcs(&wc, "svn", &["commit", "-q", "-m", "a"]);

        let s = read(&wc);
        assert_eq!(s.branch.as_deref(), Some("feature-x"));
        assert_eq!((s.changed, s.untracked, s.conflicted), (0, 0, 0), "{s:?}");

        vcs(&other, "svn", &["update", "-q"]);
        std::fs::write(other.join("a.txt"), "theirs").unwrap();
        vcs(&other, "svn", &["commit", "-q", "-m", "theirs"]);
        std::fs::write(wc.join("a.txt"), "mine").unwrap();
        std::fs::write(wc.join("new.txt"), "n").unwrap();
        vcs_may_fail(&wc, "svn", &["update", "-q", "--accept", "postpone"]);
        let s = read(&wc);
        assert_eq!(s.conflicted, 1, "{s:?}");
        assert!(s.untracked >= 1, "{s:?}");
        assert!(s.unknown.is_empty(), "{s:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn fossil_branch_merge_conflict_and_unsent_check_ins() {
        if which("fossil").is_none() {
            return;
        }
        let base = tmp("fossil-state");
        vcs(&base, "fossil", &["init", "-q", "r.fossil"]);
        std::fs::create_dir_all(base.join("wc")).unwrap();
        let wc = base.join("wc");
        vcs(&wc, "fossil", &["open", "-q", "../r.fossil"]);
        std::fs::write(wc.join("a.txt"), "base").unwrap();
        vcs(&wc, "fossil", &["add", "a.txt"]);
        vcs(&wc, "fossil", &["commit", "-q", "-m", "base", "--no-warnings"]);
        let s = read(&wc);
        assert_eq!(s.branch.as_deref(), Some("trunk"));
        assert!(s.no_remote, "{s:?}");

        // A different size: Fossil spots edits by size and time, and an
        // edit of the same size in the same second goes unseen.
        std::fs::write(wc.join("a.txt"), "on feat").unwrap();
        vcs(&wc, "fossil", &["commit", "-q", "-m", "feat", "--branch", "feat", "--no-warnings"]);
        assert_eq!(read(&wc).branch.as_deref(), Some("feat"));
        vcs(&wc, "fossil", &["update", "-q", "trunk"]);
        std::fs::write(wc.join("a.txt"), "trunk").unwrap();
        vcs(&wc, "fossil", &["commit", "-q", "-m", "trunk", "--no-warnings"]);
        vcs_may_fail(&wc, "fossil", &["merge", "feat"]);
        let s = read(&wc);
        assert_eq!(s.operation.as_deref(), Some("merge"), "{s:?}");
        assert_eq!((s.conflicted, s.changed), (1, 0), "MERGED_WITH isn't a file: {s:?}");

        // A clone remembers where it came from; a new check-in waits to be sent.
        vcs(&base, "fossil", &["clone", "-q", &base.join("r.fossil").to_string_lossy(), "c.fossil"]);
        std::fs::create_dir_all(base.join("cw")).unwrap();
        let cw = base.join("cw");
        vcs(&cw, "fossil", &["open", "-q", "../c.fossil"]);
        vcs(&cw, "fossil", &["settings", "autosync", "off"]);
        std::fs::write(cw.join("b.txt"), "b").unwrap();
        vcs(&cw, "fossil", &["add", "b.txt"]);
        vcs(&cw, "fossil", &["commit", "-q", "-m", "b", "--no-warnings"]);
        let s = read(&cw);
        assert!(!s.no_remote, "{s:?}");
        assert_eq!(s.unpushed, Some(1), "{s:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn breezy_nick_conflicts_and_pending_merge() {
        if which("brz").is_none() {
            return;
        }
        let base = tmp("brz-state");
        // Breezy on Windows resolves a relative file argument against the
        // drive root, so paths here are absolute or left out.
        let (trunk, feature) = (base.join("trunk"), base.join("feature"));
        vcs(&base, "brz", &["init", "-q", &trunk.to_string_lossy()]);
        std::fs::write(trunk.join("a.txt"), "base\n").unwrap();
        vcs(&trunk, "brz", &["add", "-q"]);
        vcs(&trunk, "brz", &["commit", "-q", "-m", "base"]);
        vcs(&base, "brz", &["branch", "-q", &trunk.to_string_lossy(), &feature.to_string_lossy()]);
        assert_eq!(read(&feature).branch.as_deref(), Some("feature"));

        std::fs::write(feature.join("a.txt"), "feature\n").unwrap();
        vcs(&feature, "brz", &["commit", "-q", "-m", "feature"]);
        std::fs::write(trunk.join("a.txt"), "trunk\n").unwrap();
        vcs(&trunk, "brz", &["commit", "-q", "-m", "trunk"]);
        vcs_may_fail(&trunk, "brz", &["merge", "-q", &feature.to_string_lossy()]);
        let s = read(&trunk);
        assert_eq!(s.conflicted, 1, "{s:?}");
        assert_eq!(s.operation.as_deref(), Some("merge"), "{s:?}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
