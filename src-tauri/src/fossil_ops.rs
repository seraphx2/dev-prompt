//! Fossil's side of `vcs_ops.rs`. Branches switch with `fossil update`, which
//! merges uncommitted changes in — so the caller requires a clean checkout
//! first, and so does Update here. A new branch is `fossil branch new` from
//! the current check-in. A second working copy is another checkout of the
//! same repository file, which the repository itself keeps a list of.

use std::path::{Path, PathBuf};

use crate::vcs_ops::{
    copy_path, native, run, same_dir, Branch, BranchList, Details, SyncOp, Target, WebLink, Worktree, LOCAL_TIMEOUT,
    NETWORK_TIMEOUT,
};

fn fossil(dir: &Path, args: &[&str], network: bool) -> Result<String, String> {
    run("fossil", dir, args, if network { NETWORK_TIMEOUT } else { LOCAL_TIMEOUT }, &[])
}

/// A Julian day (Fossil's timestamps) → Unix seconds.
fn julian_secs(j: f64) -> u64 {
    ((j - 2_440_587.5) * 86_400.0).max(0.0) as u64
}

/// The repository file this checkout belongs to.
fn repository(dir: &Path) -> Result<PathBuf, String> {
    let info = fossil(dir, &["info"], false)?;
    info.lines()
        .find_map(|l| l.strip_prefix("repository:"))
        .map(|p| PathBuf::from(native(p.trim())))
        .ok_or_else(|| "Couldn't find this checkout's repository file.".to_string())
}

/// `fossil branch list` marks the current branch with `*`.
fn parse_branch_list(out: &str) -> Vec<(String, bool)> {
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let current = l.trim_start().starts_with('*');
            (l.trim().trim_start_matches('*').trim().to_string(), current)
        })
        .collect()
}

pub fn branches(dir: &Path) -> Result<BranchList, String> {
    let list = parse_branch_list(&fossil(dir, &["branch", "list"], false)?);
    // When each branch last had a check-in.
    let sql = "SELECT substr(tagname, 5), max(event.mtime) FROM tag JOIN tagxref USING(tagid) \
               JOIN event ON event.objid = tagxref.rid WHERE tagname GLOB 'sym-*' AND tagxref.tagtype > 0 \
               GROUP BY tagname";
    let dates: Vec<(String, u64)> = fossil(dir, &["sql", sql], false)
        .map(|o| {
            o.lines()
                .filter_map(|l| l.rsplit_once('|'))
                .map(|(n, d)| (n.trim_matches('\'').to_string(), d.trim().parse().map(julian_secs).unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default();

    // Every checkout the repository knows of, and the branch each is on.
    let ckouts = fossil(dir, &["sql", "SELECT substr(name, 7) FROM config WHERE name GLOB 'ckout:*'"], false).unwrap_or_default();
    let worktrees: Vec<Worktree> = ckouts
        .lines()
        .map(|l| l.trim().trim_matches('\'').trim_end_matches(['/', '\\']).to_string())
        .filter(|p| !p.is_empty())
        .map(|p| {
            let path = PathBuf::from(native(&p));
            let current = same_dir(&p, dir);
            let branch = if current || !path.is_dir() {
                None
            } else {
                fossil(&path, &["branch", "current"], false).ok().map(|b| b.trim().to_string()).filter(|b| !b.is_empty())
            };
            Worktree { path: native(&p), branch, head: String::new(), current, prunable: !path.is_dir() }
        })
        .collect();

    let mut out: Vec<Branch> = list
        .into_iter()
        .map(|(name, current)| Branch {
            date: dates.iter().find(|(n, _)| *n == name).map(|(_, d)| *d).unwrap_or(0),
            worktree: worktrees
                .iter()
                .find(|w| !w.current && !w.prunable && w.branch.as_deref() == Some(&name))
                .map(|w| w.path.clone()),
            name,
            kind: "branch".into(),
            remote: None,
            current,
            upstream: None,
            ahead: 0,
            behind: 0,
            gone: false,
            subject: String::new(),
        })
        .collect();
    out.sort_by(|a, b| b.current.cmp(&a.current).then_with(|| b.date.cmp(&a.date)));
    let remotes = remote_url(dir).into_iter().collect();
    Ok(BranchList { branches: out, worktrees, remotes })
}

fn check_new_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.trim() != name || name.chars().any(|c| c.is_control()) {
        return Err(format!("\"{name}\" isn't a valid branch name."));
    }
    Ok(())
}

pub fn switch(dir: &Path, target: &Target) -> Result<String, String> {
    match target {
        Target::Local { name } | Target::Remote { name, .. } => {
            fossil(dir, &["update", name], true)?;
            Ok(format!("Updated to {name}."))
        }
        Target::New { name } => {
            check_new_name(name)?;
            fossil(dir, &["branch", "new", name, "current"], true)?;
            fossil(dir, &["update", name], true)?;
            Ok(format!("Created {name} from here and switched to it."))
        }
    }
}

/// Another checkout of the same repository file, beside this one.
pub fn add_checkout(dir: &Path, target: &Target) -> Result<PathBuf, String> {
    let name = target.local_name();
    if let Target::New { name } = target {
        check_new_name(name)?;
        fossil(dir, &["branch", "new", name, "current"], true)?;
    }
    let repo = repository(dir)?;
    let path = copy_path(dir, name).ok_or("Couldn't work out where the checkout would go.")?;
    std::fs::create_dir_all(&path).map_err(|e| format!("Couldn't make {}: {e}", path.display()))?;
    if let Err(e) = fossil(&path, &["open", &repo.to_string_lossy(), name], true) {
        let _ = std::fs::remove_dir(&path);
        return Err(e);
    }
    Ok(path)
}

/// The remote's URL, when there is one (`fossil remote` says `off`).
fn remote_url(dir: &Path) -> Option<String> {
    fossil(dir, &["remote"], false).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "off")
}

pub fn sync(dir: &Path, op: SyncOp) -> Result<String, String> {
    match op {
        SyncOp::Sync => {
            fossil(dir, &["sync"], true)?;
            Ok("Synced.".into())
        }
        SyncOp::Update => {
            // It merges uncommitted changes into what arrives: only when there are none.
            let s = crate::vcs_state::read(dir);
            if s.changed + s.conflicted > 0 || s.operation.is_some() {
                return Err("Commit or stash your changes first — fossil update would merge them in.".into());
            }
            fossil(dir, &["update"], true)?;
            Ok("Updated to the latest on this branch.".into())
        }
        _ => Err("Fossil has no such step here; sync or update instead.".into()),
    }
}

/// The remote, and its web page when it's served over HTTP(S) — every Fossil
/// repository is a website too. A password in the URL is left out.
pub fn details(dir: &Path) -> Result<Details, String> {
    let Some(url) = remote_url(dir) else { return Ok(Details::remotes_only(Vec::new())) };
    let mut d = Details::remotes_only(vec![url.clone()]);
    if let Some((scheme, rest)) = url.split_once("://").filter(|(s, _)| *s == "https" || *s == "http") {
        let rest = rest.split_once('@').map(|(_, h)| h).unwrap_or(rest);
        let host = rest.split('/').next().unwrap_or(rest);
        d.links.push(WebLink { kind: "repo".into(), label: format!("Open {host}"), url: format!("{scheme}://{rest}") });
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::which;
    use crate::vcs_state::tests::{tmp, vcs};

    #[test]
    fn small_parsers() {
        assert_eq!(julian_secs(2_440_587.5), 0);
        assert_eq!(julian_secs(2_440_588.5), 86_400);
        assert_eq!(
            parse_branch_list("   feat\n * trunk\n"),
            vec![("feat".to_string(), false), ("trunk".to_string(), true)]
        );
    }

    #[test]
    fn branches_switching_checkouts_and_sync() {
        if which("fossil").is_none() {
            return;
        }
        let base = tmp("fossil-ops");
        vcs(&base, "fossil", &["init", "-q", "r.fossil"]);
        let wc = base.join("wc");
        std::fs::create_dir_all(&wc).unwrap();
        vcs(&wc, "fossil", &["open", "-q", "../r.fossil"]);
        // What `fossil init` sets from the login name when no identity is forced.
        vcs(&wc, "fossil", &["user", "default", "t"]);
        std::fs::write(wc.join("a.txt"), "a").unwrap();
        vcs(&wc, "fossil", &["add", "a.txt"]);
        vcs(&wc, "fossil", &["commit", "-q", "-m", "a", "--no-warnings"]);
        vcs(&wc, "fossil", &["branch", "new", "feat", "trunk"]);

        let list = branches(&wc).unwrap();
        assert_eq!(list.branches[0].name, "trunk");
        assert!(list.branches[0].current && list.branches[0].date > 0, "{list:?}");
        assert!(list.branches.iter().any(|b| b.name == "feat" && !b.current));

        switch(&wc, &Target::Local { name: "feat".into() }).unwrap();
        assert_eq!(crate::vcs_state::read(&wc).branch.as_deref(), Some("feat"));
        switch(&wc, &Target::New { name: "fresh".into() }).unwrap();
        assert_eq!(crate::vcs_state::read(&wc).branch.as_deref(), Some("fresh"));

        std::fs::write(wc.join("a.txt"), "edited").unwrap();
        let err = crate::vcs_ops::switch(&wc, &Target::Local { name: "trunk".into() }).unwrap_err();
        assert!(err.contains("uncommitted"), "{err}");
        assert!(sync(&wc, SyncOp::Update).is_err(), "update would merge the edit in");
        vcs(&wc, "fossil", &["revert"]);

        let co = add_checkout(&wc, &Target::Local { name: "trunk".into() }).unwrap();
        assert_eq!(co.file_name().unwrap(), "wc-trunk");
        assert_eq!(crate::vcs_state::read(&co).branch.as_deref(), Some("trunk"));
        let list = branches(&wc).unwrap();
        let trunk = list.branches.iter().find(|b| b.name == "trunk").unwrap();
        assert!(trunk.worktree.as_deref().is_some_and(|p| p.ends_with("wc-trunk")), "{list:?}");
        assert_eq!(list.worktrees.iter().filter(|w| !w.prunable).count(), 2, "{list:?}");

        sync(&wc, SyncOp::Update).unwrap();
        assert!(details(&wc).unwrap().remotes.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }
}
