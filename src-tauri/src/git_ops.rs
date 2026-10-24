//! Git's side of `vcs_ops.rs`: switch to or create a branch, add a worktree,
//! fetch, a fast-forward-only pull, push, publish a branch — each left to git
//! to refuse rather than forced when it would lose something — and the remote's
//! web links.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::vcs_ops::{
    native, run, same_dir, web_links, Branch, BranchList, Details, SyncOp, Target, Worktree,
    LOCAL_TIMEOUT, NETWORK_TIMEOUT,
};
use crate::vcs_state::{RepoStatus, READ_ONLY};
/// Never wait on a terminal prompt nobody can see (stdin is closed anyway).
/// A credential manager's own window still works.
const NO_PROMPT: &[(&str, &str)] = &[("GIT_TERMINAL_PROMPT", "0")];

fn git(dir: &Path, args: &[&str], timeout: Duration, env: &[(&str, &str)]) -> Result<String, String> {
    run("git", dir, args, timeout, env)
}

fn read(dir: &Path, args: &[&str]) -> Result<String, String> {
    git(dir, args, LOCAL_TIMEOUT, READ_ONLY)
}

// --- branches and worktrees --------------------------------------------------

pub fn branches(dir: &Path) -> Result<BranchList, String> {
    let remotes: Vec<String> = read(dir, &["remote"])?.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    let refs = read(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(committerdate:unix)%00%(worktreepath)%00%(subject)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    let head = read(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok().map(|s| s.trim().to_string());
    // Branches by when they were last checked out, most recent first.
    let recent: Vec<String> = read(dir, &["log", "-g", "--format=%gs", "-n", "300", "HEAD", "--"])
        .map(|out| recent_checkouts(&out))
        .unwrap_or_default();

    let mut local = Vec::new();
    let mut remote = Vec::new();
    for line in refs.lines() {
        let f: Vec<&str> = line.split('\0').collect();
        if f.len() < 6 {
            continue;
        }
        let (refname, upstream, track, date, wt, subject) = (f[0], f[1], f[2], f[3], f[4], f[5]);
        let (ahead, behind, gone) = parse_track(track);
        let date = date.parse().unwrap_or(0);
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            local.push(Branch {
                name: name.to_string(),
                kind: "branch".into(),
                remote: None,
                current: head.as_deref() == Some(name),
                upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
                ahead,
                behind,
                gone,
                // Checked out elsewhere — unless that folder is gone
                // (`git worktree prune` territory): nothing to go to.
                worktree: (!wt.is_empty() && !same_dir(wt, dir) && Path::new(wt).exists()).then(|| native(wt)),
                date,
                subject: subject.to_string(),
            });
        } else if let Some(short) = refname.strip_prefix("refs/remotes/") {
            let Some(r) = remotes.iter().filter(|r| short.starts_with(&format!("{r}/"))).max_by_key(|r| r.len()) else {
                continue;
            };
            if short == format!("{r}/HEAD") {
                continue;
            }
            remote.push(Branch {
                name: short.to_string(),
                kind: "remote".into(),
                remote: Some(r.clone()),
                current: false,
                upstream: None,
                ahead: 0,
                behind: 0,
                gone: false,
                worktree: None,
                date,
                subject: subject.to_string(),
            });
        }
    }
    // A remote branch a local one already tracks (or shares a name with)
    // would only be a second way to the same place.
    remote.retain(|r| {
        let bare = &r.name[r.remote.as_ref().map_or(0, |x| x.len() + 1)..];
        !local.iter().any(|l| l.upstream.as_deref() == Some(r.name.as_str()) || l.name == bare)
    });

    let rank = |b: &Branch| recent.iter().position(|n| *n == b.name).unwrap_or(usize::MAX);
    local.sort_by(|a, b| {
        b.current
            .cmp(&a.current)
            .then_with(|| rank(a).cmp(&rank(b)))
            .then_with(|| b.date.cmp(&a.date))
    });
    remote.sort_by_key(|b| std::cmp::Reverse(b.date));
    local.extend(remote);

    let worktrees = read(dir, &["worktree", "list", "--porcelain"]).map(|o| parse_worktrees(&o, dir)).unwrap_or_default();
    Ok(BranchList { branches: local, worktrees, remotes })
}

/// "ahead 1, behind 2" / "gone" / "".
fn parse_track(track: &str) -> (u32, u32, bool) {
    if track.trim() == "gone" {
        return (0, 0, true);
    }
    let (mut ahead, mut behind) = (0, 0);
    for part in track.split(',') {
        let mut it = part.split_whitespace();
        match (it.next(), it.next().and_then(|n| n.parse().ok())) {
            (Some("ahead"), Some(n)) => ahead = n,
            (Some("behind"), Some(n)) => behind = n,
            _ => {}
        }
    }
    (ahead, behind, false)
}

/// Branch names in the order the reflog last moved to (or from) them.
fn recent_checkouts(reflog: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in reflog.lines() {
        let Some(rest) = line.strip_prefix("checkout: moving from ") else { continue };
        let Some((from, to)) = rest.split_once(" to ") else { continue };
        for name in [to, from] {
            if !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn parse_worktrees(out: &str, here: &Path) -> Vec<Worktree> {
    let mut list = Vec::new();
    for block in out.split("\n\n") {
        let mut wt: Option<Worktree> = None;
        let mut bare = false;
        for line in block.lines() {
            let (key, val) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "worktree" => {
                    wt = Some(Worktree {
                        path: native(val),
                        branch: None,
                        head: String::new(),
                        current: same_dir(val, here),
                        prunable: false,
                    })
                }
                "HEAD" => {
                    if let Some(w) = wt.as_mut() {
                        w.head = val.chars().take(7).collect();
                    }
                }
                "branch" => {
                    if let Some(w) = wt.as_mut() {
                        w.branch = Some(val.strip_prefix("refs/heads/").unwrap_or(val).to_string());
                    }
                }
                "prunable" => {
                    if let Some(w) = wt.as_mut() {
                        w.prunable = true;
                    }
                }
                "bare" => bare = true,
                _ => {}
            }
        }
        if let Some(w) = wt.filter(|_| !bare) {
            list.push(w);
        }
    }
    list
}

fn check_new_name(dir: &Path, name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("A branch needs a name.".into());
    }
    read(dir, &["check-ref-format", "--branch", name])
        .map(|_| ())
        .map_err(|_| format!("\"{name}\" isn't a valid branch name."))
}

/// Check out `target` here. Uncommitted changes come along; git refuses
/// (and nothing changes) when they'd be overwritten. Returns what happened.
pub fn switch(dir: &Path, target: &Target) -> Result<String, String> {
    match target {
        Target::Local { name } => {
            git(dir, &["switch", "--no-guess", name], LOCAL_TIMEOUT, NO_PROMPT)?;
            Ok(format!("Switched to {name}."))
        }
        Target::Remote { name, .. } => {
            let local = target.local_name();
            git(dir, &["switch", "-c", local, "--track", name], LOCAL_TIMEOUT, NO_PROMPT)?;
            Ok(format!("Switched to {local}, tracking {name}."))
        }
        Target::New { name } => {
            check_new_name(dir, name)?;
            git(dir, &["switch", "-c", name], LOCAL_TIMEOUT, NO_PROMPT)?;
            Ok(format!("Created {name} and switched to it."))
        }
    }
}

/// Where a new worktree for `branch` goes: beside the main working copy,
/// named after both, with a number added if that's taken.
fn worktree_path(dir: &Path, branch: &str) -> Option<PathBuf> {
    // A linked worktree's siblings sit beside the main one, not beside it.
    let main = read(dir, &["worktree", "list", "--porcelain"])
        .ok()
        .and_then(|o| o.lines().next().and_then(|l| l.strip_prefix("worktree ")).map(|p| PathBuf::from(native(p))))
        .unwrap_or_else(|| dir.to_path_buf());
    crate::vcs_ops::copy_path(&main, branch)
}

/// Check `target` out in a new worktree beside this one. Returns its folder.
pub fn add_worktree(dir: &Path, target: &Target) -> Result<PathBuf, String> {
    if let Target::New { name } = target {
        check_new_name(dir, name)?;
    }
    let path = worktree_path(dir, target.local_name()).ok_or("Couldn't work out where the worktree would go.")?;
    let p = path.to_string_lossy().into_owned();
    match target {
        Target::Local { name } => git(dir, &["worktree", "add", &p, name], LOCAL_TIMEOUT, NO_PROMPT)?,
        Target::Remote { name, .. } => git(
            dir,
            &["worktree", "add", "--track", "-b", target.local_name(), &p, name],
            LOCAL_TIMEOUT,
            NO_PROMPT,
        )?,
        Target::New { name } => git(dir, &["worktree", "add", "-b", name, &p], LOCAL_TIMEOUT, NO_PROMPT)?,
    };
    Ok(path)
}

// --- talking to the remote ---------------------------------------------------

pub fn sync(dir: &Path, op: SyncOp, remote: Option<&str>) -> Result<String, String> {
    match op {
        SyncOp::Fetch => {
            git(dir, &["fetch", "--all", "--prune", "--quiet"], NETWORK_TIMEOUT, NO_PROMPT)?;
            Ok("Fetched.".into())
        }
        SyncOp::Pull => {
            git(dir, &["pull", "--ff-only", "--quiet"], NETWORK_TIMEOUT, NO_PROMPT)?;
            Ok("Pulled.".into())
        }
        SyncOp::Push => {
            // Exactly where the label said: the upstream, even when its name
            // differs from the branch's (which `push.default=simple` refuses).
            let branch = read(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
                .map_err(|_| "HEAD isn't on a branch.".to_string())?;
            let branch = branch.trim();
            let cfg = |key: &str| read(dir, &["config", "--get", &format!("branch.{branch}.{key}")]).map(|s| s.trim().to_string());
            let (remote, merge) = match (cfg("remote"), cfg("merge")) {
                (Ok(r), Ok(m)) => (r, m),
                _ => return Err(format!("{branch} has no upstream to push to.")),
            };
            git(dir, &["push", "--quiet", &remote, &format!("HEAD:{merge}")], NETWORK_TIMEOUT, NO_PROMPT)?;
            Ok(format!("Pushed {branch} to {remote}."))
        }
        SyncOp::Update | SyncOp::Sync => Err("Git has no such step; fetch or pull instead.".into()),
        SyncOp::Publish => {
            let remote = remote.ok_or("No remote to publish to.")?;
            let branch = read(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
                .map_err(|_| "HEAD isn't on a branch.".to_string())?;
            let branch = branch.trim();
            git(dir, &["push", "--quiet", "--set-upstream", remote, branch], NETWORK_TIMEOUT, NO_PROMPT)?;
            Ok(format!("Published {branch} to {remote}."))
        }
    }
}

/// What the action menu needs beyond the status: remotes, the default branch
/// and the web links. Reads only local state — no network.
pub fn details(dir: &Path, s: &RepoStatus) -> Result<Details, String> {
    let remotes: Vec<String> = read(dir, &["remote"])?.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    // The upstream's remote, else `origin`, else the first.
    let upstream_remote = s.upstream.as_deref().and_then(|u| {
        remotes.iter().filter(|r| u.starts_with(&format!("{r}/"))).max_by_key(|r| r.len()).cloned()
    });
    let Some(remote) = upstream_remote
        .clone()
        .or_else(|| remotes.iter().find(|r| *r == "origin").cloned())
        .or_else(|| remotes.first().cloned())
    else {
        return Ok(Details::remotes_only(remotes));
    };

    let default_branch = read(dir, &["symbolic-ref", "--quiet", "--short", &format!("refs/remotes/{remote}/HEAD")])
        .ok()
        .and_then(|s| s.trim().strip_prefix(&format!("{remote}/")).map(str::to_string))
        .or_else(|| {
            let out = read(
                dir,
                &["for-each-ref", "--format=%(refname:lstrip=3)", &format!("refs/remotes/{remote}/main"), &format!("refs/remotes/{remote}/master")],
            )
            .ok()?;
            out.lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
        });

    let url = read(dir, &["remote", "get-url", &remote]).unwrap_or_default();
    // The branch page exists once the branch is on that remote.
    let pushed = match (&upstream_remote, &s.upstream) {
        (Some(r), Some(u)) if *r == remote && !s.upstream_gone => u.strip_prefix(&format!("{r}/")),
        _ => None,
    };
    let links = web_links(&url, pushed, default_branch.as_deref());
    Ok(Details { remotes, default_branch, links, pull_from: None, push_to: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::which;
    use crate::vcs_state::tests::{cloned, commit, git as run};

    #[test]
    fn small_parsers() {
        assert_eq!(parse_track("ahead 1, behind 2"), (1, 2, false));
        assert_eq!(parse_track("behind 5"), (0, 5, false));
        assert_eq!(parse_track("gone"), (0, 0, true));
        assert_eq!(parse_track(""), (0, 0, false));
        assert_eq!(
            recent_checkouts("checkout: moving from main to b\ncommit: x\ncheckout: moving from a to main\n"),
            vec!["b", "main", "a"]
        );
        assert_eq!(crate::vcs_ops::worktree_folder_name("dev-prompt", "feature/log in"), "dev-prompt-feature-log-in");
        assert_eq!(crate::vcs_ops::tidy("exit code 128.\nfatal: not a git repository"), "not a git repository");
        assert_eq!(
            crate::vcs_ops::tidy("exit code 1.\nerror: Your local changes would be overwritten\nhint: stash them"),
            "Your local changes would be overwritten"
        );
    }

    #[test]
    fn branches_switching_and_worktrees() {
        if which("git").is_none() {
            return;
        }
        let (base, _remote, work) = cloned("branches");
        // A branch only on the remote, one local, and recent checkouts.
        run(&work, &["push", "-q", "origin", "main:remote-only"]);
        run(&work, &["fetch", "-q"]);
        run(&work, &["branch", "older"]);
        run(&work, &["switch", "-q", "-c", "topic"]);
        commit(&work, "t.txt", "t");
        run(&work, &["switch", "-q", "main"]);

        let list = branches(&work).unwrap();
        let names: Vec<&str> = list.branches.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names[0], "main");
        assert!(list.branches[0].current);
        assert_eq!(names[1], "topic", "most recently checked out next: {names:?}");
        assert!(names.contains(&"older") && names.contains(&"origin/remote-only"), "{names:?}");
        assert!(!names.contains(&"origin/main"), "tracked by main: {names:?}");
        assert_eq!(list.remotes, vec!["origin"]);

        // A change that would be overwritten: git refuses, nothing moves.
        std::fs::write(work.join("t.txt"), "mine").unwrap();
        let err = switch(&work, &Target::Local { name: "topic".into() }).unwrap_err();
        assert!(err.contains("overwritten") || err.contains("untracked"), "{err}");
        std::fs::remove_file(work.join("t.txt")).unwrap();

        assert!(switch(&work, &Target::New { name: "bad..name".into() }).is_err());
        switch(&work, &Target::Remote { remote: "origin".into(), name: "origin/remote-only".into() }).unwrap();
        assert_eq!(crate::vcs_state::read(&work).upstream.as_deref(), Some("origin/remote-only"));
        switch(&work, &Target::New { name: "fresh".into() }).unwrap();
        assert_eq!(crate::vcs_state::read(&work).branch.as_deref(), Some("fresh"));

        // A worktree beside the repo, named after both; the branch then
        // reports where it's checked out.
        let wt = add_worktree(&work, &Target::Local { name: "topic".into() }).unwrap();
        assert_eq!(wt.file_name().unwrap(), "work-topic");
        assert_eq!(crate::vcs_state::read(&wt).branch.as_deref(), Some("topic"));
        let list = branches(&work).unwrap();
        let topic = list.branches.iter().find(|b| b.name == "topic").unwrap();
        assert!(topic.worktree.as_deref().is_some_and(|p| p.ends_with("work-topic")), "{topic:?}");
        assert_eq!(list.worktrees.len(), 2);
        assert!(list.worktrees.iter().any(|w| w.current && w.branch.as_deref() == Some("fresh")));
        // From inside the linked one, a new worktree still goes beside the main one.
        assert_eq!(worktree_path(&wt, "next").unwrap().file_name().unwrap(), "work-next");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pull_push_and_publish() {
        if which("git").is_none() {
            return;
        }
        let (base, remote, work) = cloned("sync");
        run(&base, &["clone", "-q", &remote.to_string_lossy(), "other"]);
        let other = base.join("other");
        commit(&other, "o.txt", "o");
        run(&other, &["push", "-q"]);

        sync(&work, SyncOp::Fetch, None).unwrap();
        assert_eq!(crate::vcs_state::read(&work).behind, 1);
        sync(&work, SyncOp::Pull, None).unwrap();
        assert_eq!(crate::vcs_state::read(&work).behind, 0);

        commit(&work, "w.txt", "w");
        sync(&work, SyncOp::Push, None).unwrap();
        assert_eq!(crate::vcs_state::read(&work).ahead, 0);

        run(&work, &["switch", "-q", "-c", "new-one"]);
        assert!(sync(&work, SyncOp::Push, None).is_err(), "no upstream yet");
        sync(&work, SyncOp::Publish, Some("origin")).unwrap();
        let s = crate::vcs_state::read(&work);
        assert_eq!(s.upstream.as_deref(), Some("origin/new-one"));

        // Diverged: a fast-forward pull is refused, not merged.
        run(&other, &["pull", "-q", "--ff-only"]);
        commit(&other, "o2.txt", "o2");
        run(&other, &["push", "-q", "origin", "main"]);
        run(&work, &["switch", "-q", "main"]);
        commit(&work, "w2.txt", "w2");
        sync(&work, SyncOp::Fetch, None).unwrap();
        assert!(sync(&work, SyncOp::Pull, None).is_err());

        let d = details(&work, &crate::vcs_state::read(&work)).unwrap();
        assert_eq!(d.remotes, vec!["origin"]);
        assert!(d.links.is_empty(), "a file remote has no web page");
        let _ = std::fs::remove_dir_all(&base);
    }
}
