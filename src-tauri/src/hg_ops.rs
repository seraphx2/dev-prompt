//! Mercurial's side of `vcs_ops.rs`. Bookmarks and named branches both
//! switch with `hg update --check` (refused with uncommitted changes — the
//! caller refuses first, with a better reason); a new name is a bookmark, the
//! light-weight branch modern Mercurial recommends. A second working copy is
//! a share (`hg share`, bundled, switched on per call). Pulling only brings
//! changesets in; moving onto them is the switcher's job.

use std::path::{Path, PathBuf};

use crate::vcs_ops::{copy_path, plural, run, Branch, BranchList, Details, SyncOp, Target, LOCAL_TIMEOUT, NETWORK_TIMEOUT};
use crate::vcs_state::HG_PLAIN;

/// Never wait on a prompt nobody can see.
const HG_ENV: &[(&str, &str)] = &[("HGPLAIN", "1"), ("HGMERGE", "internal:fail")];

/// A share only sees the bookmarks it shares with its source while the
/// (bundled) share extension is on, so every call here switches it on.
const SHARE_ON: [&str; 2] = ["--config", "extensions.share="];

fn hg(dir: &Path, args: &[&str], network: bool) -> Result<String, String> {
    let mut all = vec!["--noninteractive", SHARE_ON[0], SHARE_ON[1]];
    all.extend_from_slice(args);
    run("hg", dir, &all, if network { NETWORK_TIMEOUT } else { LOCAL_TIMEOUT }, HG_ENV)
}

fn read(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut all = SHARE_ON.to_vec();
    all.extend_from_slice(args);
    run("hg", dir, &all, LOCAL_TIMEOUT, HG_PLAIN)
}

/// `1792770344 14400` (Mercurial's `hgdate`) → Unix seconds.
fn hgdate(s: &str) -> u64 {
    s.split_whitespace().next().and_then(|t| t.parse::<f64>().ok()).map(|t| t as u64).unwrap_or(0)
}

pub fn branches(dir: &Path) -> Result<BranchList, String> {
    let current_branch = read(dir, &["log", "-r", ".", "-T", "{branch}"]).map(|s| s.trim().to_string()).unwrap_or_default();
    let bookmarks = read(dir, &["bookmarks", "-T", "{bookmark}\\t{active}\\t{date|hgdate}\\t{desc|firstline}\\n"])?;
    let named = read(dir, &["branches", "-T", "{branch}\\t{date|hgdate}\\t{desc|firstline}\\n"])?;
    let mut out: Vec<Branch> = Vec::new();
    let mut any_active = false;
    for l in bookmarks.lines() {
        let f: Vec<&str> = l.splitn(4, '\t').collect();
        if f.len() < 4 {
            continue;
        }
        let active = f[1].eq_ignore_ascii_case("true");
        any_active |= active;
        out.push(branch(f[0], "bookmark", active, hgdate(f[2]), f[3]));
    }
    for l in named.lines() {
        let f: Vec<&str> = l.splitn(3, '\t').collect();
        if f.len() < 3 {
            continue;
        }
        // With a bookmark active, that's what's checked out — not its branch.
        out.push(branch(f[0], "branch", !any_active && f[0] == current_branch, hgdate(f[1]), f[2]));
    }
    out.sort_by(|a, b| b.current.cmp(&a.current).then_with(|| b.date.cmp(&a.date)));
    let remotes = read(dir, &["paths", "-T", "{name}\\n"])
        .map(|o| o.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    // A share records its source, but the source doesn't record its shares.
    Ok(BranchList { branches: out, worktrees: Vec::new(), remotes })
}

fn branch(name: &str, kind: &str, current: bool, date: u64, subject: &str) -> Branch {
    Branch {
        name: name.to_string(),
        kind: kind.into(),
        remote: None,
        current,
        upstream: None,
        ahead: 0,
        behind: 0,
        gone: false,
        worktree: None,
        date,
        subject: subject.to_string(),
    }
}

fn check_new_name(name: &str) -> Result<(), String> {
    let bad = name.trim().is_empty()
        || name.trim() != name
        || name.chars().any(|c| matches!(c, ':' | '\n' | '\r'))
        || name.parse::<i64>().is_ok();
    if bad {
        return Err(format!("\"{name}\" isn't a valid bookmark name."));
    }
    Ok(())
}

pub fn switch(dir: &Path, target: &Target) -> Result<String, String> {
    match target {
        Target::Local { name } | Target::Remote { name, .. } => {
            hg(dir, &["update", "--check", "-r", name], false)?;
            Ok(format!("Updated to {name}."))
        }
        Target::New { name } => {
            check_new_name(name)?;
            hg(dir, &["bookmark", name], false)?;
            Ok(format!("Created the bookmark {name} here and made it active."))
        }
    }
}

/// A share beside this one (sharing the history, not the working copy),
/// updated to `target`.
pub fn add_share(dir: &Path, target: &Target) -> Result<PathBuf, String> {
    if let Target::New { name } = target {
        check_new_name(name)?;
    }
    // Shares go beside the repo they share from; a share names it in
    // `.hg/sharedpath` (its `.hg` folder).
    let source = std::fs::read_to_string(dir.join(".hg/sharedpath"))
        .ok()
        .and_then(|p| Path::new(p.trim()).parent().map(Path::to_path_buf))
        .unwrap_or_else(|| dir.to_path_buf());
    let name = target.local_name();
    let path = copy_path(&source, name).ok_or("Couldn't work out where the share would go.")?;
    let p = path.to_string_lossy().into_owned();
    // Bookmarks shared too, as Git worktrees share branches: a bookmark
    // moved in one is moved in both. The share's own config switches the
    // extension on, so plain `hg` there sees them as well.
    hg(dir, &["share", "--bookmarks", "--noupdate", ".", &p], false)?;
    let hgrc = path.join(".hg").join("hgrc");
    let mut cfg = std::fs::read_to_string(&hgrc).unwrap_or_default();
    if !cfg.contains("[extensions]") {
        cfg.push_str("\n[extensions]\nshare =\n");
        std::fs::write(&hgrc, cfg).map_err(|e| format!("Couldn't write {}: {e}", hgrc.display()))?;
    }
    match target {
        Target::New { name } => {
            // Where this working copy is, with the new bookmark on it.
            let here = read(dir, &["log", "-r", ".", "-T", "{node}"])?;
            hg(&path, &["update", "-r", here.trim()], false)?;
            hg(&path, &["bookmark", name], false)?;
        }
        _ => {
            hg(&path, &["update", "-r", name], false)?;
        }
    }
    Ok(path)
}

pub fn sync(dir: &Path, op: SyncOp) -> Result<String, String> {
    match op {
        SyncOp::Fetch => {
            hg(dir, &["pull"], true)?;
            Ok("Pulled — update to the new changesets from the switcher.".into())
        }
        SyncOp::Push => {
            // The commits under the working copy only — and its bookmark, so
            // the remote learns where it points. A named branch the remote
            // hasn't seen is created there; a second head on one it has is
            // still refused, never forced.
            let drafts = read(dir, &["log", "-r", "draft() and ::.", "-T", "x\\n"]).map(|o| o.lines().count() as u32).unwrap_or(0);
            let bookmark = read(dir, &["log", "-r", ".", "-T", "{activebookmark}"]).map(|s| s.trim().to_string()).unwrap_or_default();
            let mut args = vec!["push", "--new-branch", "-r", "."];
            if !bookmark.is_empty() {
                args.extend_from_slice(&["-B", &bookmark]);
            }
            match hg(dir, &args, true) {
                // `hg push` exits 1 when there was nothing to push.
                Err(e) if e.contains("no changes found") => {}
                r => {
                    r?;
                }
            }
            Ok(format!("Pushed {}.", plural(drafts, "draft commit")))
        }
        _ => Err("Mercurial has no such step here; pull, push or switch instead.".into()),
    }
}

pub fn details(dir: &Path) -> Result<Details, String> {
    let remotes = read(dir, &["paths", "-T", "{name}\\n"])?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    Ok(Details::remotes_only(remotes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::which;
    use crate::vcs_state::tests::{tmp, vcs};

    #[test]
    fn bookmarks_branches_switching_shares_and_sync() {
        if which("hg").is_none() {
            return;
        }
        let base = tmp("hg-ops");
        let (remote, work) = (base.join("remote"), base.join("work"));
        std::fs::create_dir_all(&remote).unwrap();
        vcs(&remote, "hg", &["init"]);
        vcs(&base, "hg", &["clone", "-q", &remote.to_string_lossy(), "work"]);
        std::fs::write(work.join("a.txt"), "a").unwrap();
        vcs(&work, "hg", &["add", "a.txt"]);
        vcs(&work, "hg", &["commit", "-m", "a"]);
        vcs(&work, "hg", &["branch", "stable"]);
        std::fs::write(work.join("a.txt"), "stable").unwrap();
        vcs(&work, "hg", &["commit", "-m", "on stable"]);
        vcs(&work, "hg", &["update", "-q", "default"]);
        vcs(&work, "hg", &["bookmark", "feat"]);

        let list = branches(&work).unwrap();
        let first = &list.branches[0];
        assert_eq!((first.name.as_str(), first.kind.as_str(), first.current), ("feat", "bookmark", true), "{list:?}");
        assert!(list.branches.iter().any(|b| b.name == "stable" && b.kind == "branch" && !b.current));
        assert_eq!(list.remotes, vec!["default"]);

        switch(&work, &Target::Local { name: "stable".into() }).unwrap();
        assert_eq!(crate::vcs_state::read(&work).branch.as_deref(), Some("stable"));
        switch(&work, &Target::New { name: "topic".into() }).unwrap();
        assert_eq!(crate::vcs_state::read(&work).branch.as_deref(), Some("topic"));
        assert!(switch(&work, &Target::New { name: "123".into() }).is_err());

        // The dispatcher refuses with uncommitted changes, before hg would.
        std::fs::write(work.join("a.txt"), "edited").unwrap();
        let err = crate::vcs_ops::switch(&work, &Target::Local { name: "default".into() }).unwrap_err();
        assert!(err.contains("uncommitted"), "{err}");
        vcs(&work, "hg", &["revert", "-q", "--all", "--no-backup"]);

        let share = add_share(&work, &Target::Local { name: "feat".into() }).unwrap();
        assert_eq!(share.file_name().unwrap(), "work-feat");
        assert_eq!(crate::vcs_state::read(&share).branch.as_deref(), Some("feat"));

        // Two drafts under the working copy go up; the bookmark with them.
        std::fs::write(work.join("b.txt"), "b").unwrap();
        vcs(&work, "hg", &["add", "b.txt"]);
        vcs(&work, "hg", &["commit", "-m", "b"]);
        assert!(crate::vcs_state::read(&work).unpushed.unwrap() > 0);
        sync(&work, SyncOp::Push).unwrap();
        sync(&work, SyncOp::Fetch).unwrap();
        let after = crate::vcs_state::read(&work);
        assert_eq!(after.unpushed, Some(0), "{after:?}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
