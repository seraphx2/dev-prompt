//! Jujutsu's side of `vcs_ops.rs`. Switching never carries anything along:
//! the working copy is a commit of its own, so "switch to a bookmark" is
//! `jj new <bookmark>` — a fresh change on top of it, the one you were in
//! left exactly as it was. A second working copy is a workspace.

use std::path::{Path, PathBuf};

use crate::vcs_ops::{
    copy_path, native, plural, run, same_dir, web_links, Branch, BranchList, Details, SyncOp, Target,
    Worktree, LOCAL_TIMEOUT, NETWORK_TIMEOUT,
};
use crate::vcs_state::RepoStatus;

/// Every query here reads what the last snapshot recorded; only switching
/// (which snapshots first) touches the working copy.
fn read(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut all = vec!["--no-pager", "--color", "never", "--ignore-working-copy"];
    all.extend_from_slice(args);
    run("jj", dir, &all, LOCAL_TIMEOUT, &[])
}

fn jj(dir: &Path, args: &[&str], network: bool) -> Result<String, String> {
    let mut all = vec!["--no-pager", "--color", "never"];
    all.extend_from_slice(args);
    run("jj", dir, &all, if network { NETWORK_TIMEOUT } else { LOCAL_TIMEOUT }, &[])
}

/// A revset string literal: bookmark names may hold `/`, `-`, `.`.
fn lit(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

const BOOKMARK_ROW: &str = concat!(
    r#"name ++ "\t" ++ if(remote, remote, "") ++ "\t" ++ if(tracked, "t", "") ++ "\t""#,
    r#" ++ if(normal_target, normal_target.committer().timestamp().utc().format("%s") ++ "\t""#,
    r#" ++ normal_target.description().first_line(), "\t") ++ "\n""#,
);

/// One row of `jj bookmark list --all-remotes` with [`BOOKMARK_ROW`].
struct Row<'a> {
    name: &'a str,
    remote: &'a str,
    tracked: bool,
    date: u64,
    subject: &'a str,
}

fn rows(out: &str) -> Vec<Row<'_>> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(5, '\t').collect();
            (f.len() == 5).then(|| Row {
                name: f[0],
                remote: f[1],
                tracked: f[2] == "t",
                date: f[3].parse().unwrap_or(0),
                subject: f[4],
            })
        })
        .collect()
}

/// Bookmarks below each workspace's working copy, nearest first.
fn nearest_bookmarks(dir: &Path, revset: &str) -> Vec<String> {
    read(dir, &["log", "--no-graph", "-r", &format!("heads(::{revset} & bookmarks())"), "-T", r#"local_bookmarks.map(|b| b.name()).join("\n") ++ "\n""#])
        .map(|o| o.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}

pub fn branches(dir: &Path) -> Result<BranchList, String> {
    let out = read(dir, &["bookmark", "list", "--all-remotes", "-T", BOOKMARK_ROW])?;
    let rows = rows(&out);
    let here = nearest_bookmarks(dir, "@");
    let remotes: Vec<String> = read(dir, &["git", "remote", "list"])
        .map(|o| o.lines().filter_map(|l| l.split_whitespace().next()).map(String::from).collect())
        .unwrap_or_default();

    // Each workspace's folder, and the bookmarks its working copy sits on.
    let worktrees: Vec<Worktree> = read(dir, &["workspace", "list", "-T", r#"name ++ "\t" ++ self.root() ++ "\t" ++ target.change_id().shortest(8) ++ "\n""#])
        .map(|o| {
            o.lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.splitn(3, '\t').collect();
                    (f.len() == 3).then(|| Worktree {
                        path: native(f[1]),
                        branch: nearest_bookmarks(dir, &format!("{}@", lit(f[0]))).into_iter().next(),
                        head: f[2].to_string(),
                        current: same_dir(f[1], dir),
                        prunable: !Path::new(f[1]).exists(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let mut local: Vec<Branch> = Vec::new();
    let mut remote: Vec<Branch> = Vec::new();
    for r in rows.iter().filter(|r| r.remote.is_empty()) {
        let tracking = rows.iter().find(|t| t.name == r.name && t.tracked && !t.remote.is_empty() && t.remote != "git");
        let (mut ahead, mut behind, mut gone) = (0, 0, false);
        if let Some(t) = tracking {
            let theirs = format!("remote_bookmarks(exact:{}, exact:{})", lit(t.name), lit(t.remote));
            let mine = format!("bookmarks(exact:{})", lit(r.name));
            let count = |revset: String| {
                read(dir, &["log", "--no-graph", "-r", &revset, "-T", "\"x\\n\""]).map(|o| o.lines().count() as u32).unwrap_or(0)
            };
            if count(theirs.clone()) == 0 {
                gone = true;
            } else {
                ahead = count(format!("{theirs}..{mine}"));
                behind = count(format!("{mine}..{theirs}"));
            }
        }
        local.push(Branch {
            name: r.name.to_string(),
            kind: "bookmark".into(),
            remote: None,
            current: here.iter().any(|h| h == r.name),
            upstream: tracking.map(|t| format!("{}@{}", t.name, t.remote)),
            ahead,
            behind,
            gone,
            worktree: worktrees
                .iter()
                .find(|w| !w.current && !w.prunable && w.branch.as_deref() == Some(r.name))
                .map(|w| w.path.clone()),
            date: r.date,
            subject: r.subject.to_string(),
        });
    }
    // Remote bookmarks with no local bookmark of that name.
    for r in rows.iter().filter(|r| !r.remote.is_empty() && r.remote != "git") {
        if local.iter().any(|l| l.name == r.name) {
            continue;
        }
        remote.push(Branch {
            name: format!("{}@{}", r.name, r.remote),
            kind: "remote".into(),
            remote: Some(r.remote.to_string()),
            current: false,
            upstream: None,
            ahead: 0,
            behind: 0,
            gone: false,
            worktree: None,
            date: r.date,
            subject: r.subject.to_string(),
        });
    }
    local.sort_by(|a, b| b.current.cmp(&a.current).then_with(|| b.date.cmp(&a.date)));
    remote.sort_by_key(|b| std::cmp::Reverse(b.date));
    local.extend(remote);
    Ok(BranchList { branches: local, worktrees, remotes })
}

fn check_new_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("\"{name}\" isn't a valid bookmark name."));
    }
    Ok(())
}

/// Start a new change on top of the bookmark; the change you were in stays as
/// it was. A remote bookmark is tracked first, so a local one follows it.
pub fn switch(dir: &Path, target: &Target) -> Result<String, String> {
    match target {
        Target::Local { name } => {
            jj(dir, &["new", &format!("bookmarks(exact:{})", lit(name))], false)?;
            Ok(format!("Started a new change on top of {name}."))
        }
        Target::Remote { remote, .. } => {
            let name = target.local_name();
            track(dir, name, remote)?;
            jj(dir, &["new", &format!("bookmarks(exact:{})", lit(name))], false)?;
            Ok(format!("Tracking {name}@{remote}; started a new change on top of it."))
        }
        Target::New { name } => {
            check_new_name(name)?;
            jj(dir, &["bookmark", "create", name, "-r", "@"], false)?;
            Ok(format!("Created the bookmark {name} at the working-copy change."))
        }
    }
}

/// `jj bookmark track` took `name@remote` until it grew `--remote`.
fn track(dir: &Path, name: &str, remote: &str) -> Result<(), String> {
    jj(dir, &["bookmark", "track", &format!("{name}@{remote}")], false).map(|_| ())
}

/// The default workspace's folder: new workspaces go beside it.
fn main_root(dir: &Path) -> PathBuf {
    read(dir, &["workspace", "list", "-T", r#"if(name == "default", self.root() ++ "\n")"#])
        .ok()
        .and_then(|o| o.lines().next().map(|l| PathBuf::from(native(l.trim()))))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| dir.to_path_buf())
}

/// A new workspace beside the default one, its working copy a new change on
/// top of the bookmark (made at the current change, for a new name).
pub fn add_workspace(dir: &Path, target: &Target) -> Result<PathBuf, String> {
    let name = target.local_name().to_string();
    match target {
        Target::New { name } => {
            check_new_name(name)?;
            jj(dir, &["bookmark", "create", name, "-r", "@"], false)?;
        }
        Target::Remote { remote, .. } => track(dir, &name, remote)?,
        Target::Local { .. } => {}
    }
    let path = copy_path(&main_root(dir), &name).ok_or("Couldn't work out where the workspace would go.")?;
    let ws = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| name.clone());
    jj(
        dir,
        &["workspace", "add", &path.to_string_lossy(), "--name", &ws, "-r", &format!("bookmarks(exact:{})", lit(&name))],
        false,
    )?;
    Ok(path)
}

/// The remote a bookmark tracks (`main@origin` → `origin`).
fn tracked_remote(s: &RepoStatus) -> Option<String> {
    s.upstream.as_deref().and_then(|u| u.rsplit_once('@')).map(|(_, r)| r.to_string())
}

pub fn sync(dir: &Path, op: SyncOp, remote: Option<&str>) -> Result<String, String> {
    match op {
        SyncOp::Fetch => {
            jj(dir, &["git", "fetch", "--all-remotes"], true)?;
            Ok("Fetched.".into())
        }
        SyncOp::Push | SyncOp::Publish => {
            let s = crate::vcs_state::read(dir);
            let bookmark = s.branch.clone().ok_or("The working copy isn't on top of a bookmark.")?;
            let remote = match op {
                SyncOp::Push => tracked_remote(&s).ok_or_else(|| format!("{bookmark} doesn't track a remote bookmark."))?,
                _ => remote.ok_or("No remote to publish to.")?.to_string(),
            };
            let args = ["git", "push", "--bookmark", &bookmark, "--remote", &remote];
            match jj(dir, &args, true) {
                // Before jj made a pushed bookmark tracked by itself, a new
                // one needed `--allow-new`.
                Err(e) if e.contains("--allow-new") => {
                    let mut with = args.to_vec();
                    with.push("--allow-new");
                    jj(dir, &with, true)?;
                }
                r => {
                    r?;
                }
            }
            Ok(match op {
                SyncOp::Push => format!("Pushed {} of {bookmark} to {remote}.", plural(s.ahead, "commit")),
                _ => format!("Published {bookmark} to {remote}."),
            })
        }
        SyncOp::Pull | SyncOp::Update | SyncOp::Sync => {
            Err("Jujutsu has no such step: fetch, and tracked bookmarks follow along.".into())
        }
    }
}

/// Remotes, the default branch (`trunk()`), and web links for the remote the
/// bookmark tracks (else `origin`, else the first).
pub fn details(dir: &Path, s: &RepoStatus) -> Result<Details, String> {
    let list = read(dir, &["git", "remote", "list"])?;
    let pairs: Vec<(String, String)> = list
        .lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .map(|(n, u)| (n.to_string(), u.trim().to_string()))
        .collect();
    let remotes: Vec<String> = pairs.iter().map(|(n, _)| n.clone()).collect();
    let tracked = tracked_remote(s);
    let Some(remote) = tracked
        .clone()
        .or_else(|| remotes.iter().find(|r| *r == "origin").cloned())
        .or_else(|| remotes.first().cloned())
    else {
        return Ok(Details::remotes_only(remotes));
    };
    let default_branch = read(dir, &["log", "--no-graph", "-r", "trunk()", "-T", r#"remote_bookmarks.map(|b| b.name() ++ "@" ++ b.remote()).join("\n")"#])
        .ok()
        .and_then(|o| {
            o.lines()
                .filter_map(|l| l.trim().rsplit_once('@'))
                .find(|(_, r)| *r == remote)
                .map(|(n, _)| n.to_string())
        });
    let url = pairs.iter().find(|(n, _)| *n == remote).map(|(_, u)| u.as_str()).unwrap_or_default();
    let pushed = match (&tracked, &s.branch) {
        (Some(r), Some(b)) if *r == remote && !s.upstream_gone => Some(b.as_str()),
        _ => None,
    };
    let links = web_links(url, pushed, default_branch.as_deref());
    Ok(Details { remotes, default_branch, links, pull_from: None, push_to: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::which;
    use crate::vcs_state::tests::{git, tmp, vcs};

    /// A colocated-free jj repo whose `main` is pushed to a bare Git remote,
    /// plus a second bookmark only on the remote.
    fn repo(tag: &str) -> (PathBuf, PathBuf) {
        let base = tmp(tag);
        let (remote, work) = (base.join("remote.git"), base.join("work"));
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare", "-q"]);
        std::fs::create_dir_all(&work).unwrap();
        vcs(&work, "jj", &["git", "init"]);
        vcs(&work, "jj", &["git", "remote", "add", "origin", &remote.to_string_lossy()]);
        std::fs::write(work.join("a.txt"), "a").unwrap();
        vcs(&work, "jj", &["commit", "-m", "first"]);
        vcs(&work, "jj", &["bookmark", "create", "main", "-r", "@-"]);
        std::fs::write(work.join("there.txt"), "t").unwrap();
        vcs(&work, "jj", &["commit", "-m", "only there"]);
        vcs(&work, "jj", &["bookmark", "create", "only-there", "-r", "@-"]);
        vcs(&work, "jj", &["git", "push", "--bookmark", "main", "--bookmark", "only-there"]);
        vcs(&work, "jj", &["bookmark", "forget", "only-there"]);
        vcs(&work, "jj", &["git", "fetch"]);
        vcs(&work, "jj", &["new", "main"]);
        (base, work)
    }

    #[test]
    fn bookmarks_switching_and_workspaces() {
        if which("jj").is_none() || which("git").is_none() {
            return;
        }
        let (base, work) = repo("jj-ops");
        let list = branches(&work).unwrap();
        let names: Vec<(&str, &str)> = list.branches.iter().map(|b| (b.name.as_str(), b.kind.as_str())).collect();
        assert_eq!(names, vec![("main", "bookmark"), ("only-there@origin", "remote")], "{list:?}");
        assert!(list.branches[0].current);
        assert_eq!(list.branches[0].upstream.as_deref(), Some("main@origin"));
        assert_eq!(list.remotes, vec!["origin"]);

        // The change you were in stays; a new one starts on the bookmark.
        std::fs::write(work.join("wip.txt"), "wip").unwrap();
        switch(&work, &Target::Remote { remote: "origin".into(), name: "only-there@origin".into() }).unwrap();
        let s = crate::vcs_state::read(&work);
        assert_eq!(s.branch.as_deref(), Some("only-there"), "{s:?}");
        assert_eq!(s.changed, 0, "the new change starts empty: {s:?}");
        assert!(!work.join("wip.txt").exists(), "wip stays in its own change");

        switch(&work, &Target::New { name: "topic".into() }).unwrap();
        assert!(branches(&work).unwrap().branches.iter().any(|b| b.name == "topic" && b.current));
        assert!(switch(&work, &Target::New { name: "two words".into() }).is_err());

        let ws = add_workspace(&work, &Target::Local { name: "main".into() }).unwrap();
        assert_eq!(ws.file_name().unwrap(), "work-main");
        assert!(ws.join("a.txt").exists());
        let list = branches(&work).unwrap();
        assert_eq!(list.worktrees.len(), 2, "{list:?}");
        let main = list.branches.iter().find(|b| b.name == "main").unwrap();
        assert!(main.worktree.as_deref().is_some_and(|p| p.ends_with("work-main")), "{main:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn push_publish_and_details() {
        if which("jj").is_none() || which("git").is_none() {
            return;
        }
        let (base, work) = repo("jj-sync");
        std::fs::write(work.join("b.txt"), "b").unwrap();
        vcs(&work, "jj", &["commit", "-m", "second"]);
        vcs(&work, "jj", &["bookmark", "move", "main", "--to", "@-"]);
        assert_eq!(crate::vcs_state::read(&work).ahead, 1);
        sync(&work, SyncOp::Push, None).unwrap();
        assert_eq!(crate::vcs_state::read(&work).ahead, 0);

        vcs(&work, "jj", &["bookmark", "create", "fresh", "-r", "@-"]);
        // Nearest bookmarks are now main and fresh, on the same commit; the
        // status names the first. Publish the one that has no remote.
        vcs(&work, "jj", &["bookmark", "delete", "main"]);
        sync(&work, SyncOp::Publish, Some("origin")).unwrap();
        let s = crate::vcs_state::read(&work);
        assert_eq!(s.upstream.as_deref(), Some("fresh@origin"), "{s:?}");

        sync(&work, SyncOp::Fetch, None).unwrap();
        let d = details(&work, &s).unwrap();
        assert_eq!(d.remotes, vec!["origin"]);
        assert!(d.links.is_empty(), "a local remote has no web page");
        let _ = std::fs::remove_dir_all(&base);
    }
}
