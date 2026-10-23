//! Subversion's side of `vcs_ops.rs`. A branch is a path in the repository
//! (`^/trunk`, `^/branches/x`), so listing branches, switching and making one
//! all talk to the server — the switcher says so while it waits. Switching
//! merges uncommitted changes into the other branch, so the caller requires a
//! clean working copy first. A second working copy is a fresh checkout.

use std::path::{Path, PathBuf};

use crate::vcs_ops::{copy_path, run, Branch, BranchList, Details, SyncOp, Target, LOCAL_TIMEOUT, NETWORK_TIMEOUT};

fn svn(dir: &Path, args: &[&str], network: bool) -> Result<String, String> {
    let mut all = vec!["--non-interactive"];
    all.extend_from_slice(args);
    run("svn", dir, &all, if network { NETWORK_TIMEOUT } else { LOCAL_TIMEOUT }, &[])
}

fn info(dir: &Path, item: &str) -> Result<String, String> {
    svn(dir, &["info", "--show-item", item], false).map(|s| s.trim().to_string())
}

/// The repository's root URL and this working copy's path in it (`^/trunk`).
fn where_am_i(dir: &Path) -> Result<(String, String), String> {
    Ok((info(dir, "repos-root-url")?, info(dir, "relative-url")?))
}

/// A branch's URL: `trunk` is the trunk, anything else under `branches/`.
fn url_of(root: &str, name: &str) -> String {
    if name == "trunk" { format!("{root}/trunk") } else { format!("{root}/branches/{name}") }
}

pub fn branches(dir: &Path) -> Result<BranchList, String> {
    let (root, here) = where_am_i(dir)?;
    let mut out = Vec::new();
    // The standard layout's trunk, then whatever is under branches/.
    if let Ok(xml) = svn(dir, &["list", "--xml", &root], true) {
        for e in list_entries(&xml).into_iter().filter(|e| e.name == "trunk") {
            out.push(branch("trunk", here == "^/trunk", e.date, e.author));
        }
    }
    match svn(dir, &["list", "--xml", &format!("{root}/branches")], true) {
        Ok(xml) => {
            for e in list_entries(&xml) {
                let current = here == format!("^/branches/{}", e.name);
                out.push(branch(&e.name, current, e.date, e.author));
            }
        }
        // No branches/ folder: a layout of its own — only what's checked out.
        Err(_) if out.is_empty() => {
            let label = here.strip_prefix("^/").unwrap_or(&here).to_string();
            out.push(branch(&label, true, 0, String::new()));
        }
        Err(_) => {}
    }
    out.sort_by(|a, b| b.current.cmp(&a.current).then_with(|| b.date.cmp(&a.date)));
    Ok(BranchList { branches: out, worktrees: Vec::new(), remotes: vec![root] })
}

fn branch(name: &str, current: bool, date: u64, author: String) -> Branch {
    Branch {
        name: name.to_string(),
        kind: "branch".into(),
        remote: None,
        current,
        upstream: None,
        ahead: 0,
        behind: 0,
        gone: false,
        worktree: None,
        date,
        subject: if author.is_empty() { String::new() } else { format!("last change by {author}") },
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    name: String,
    date: u64,
    author: String,
}

/// The folders in `svn list --xml` output, with their last change.
fn list_entries(xml: &str) -> Vec<Entry> {
    let tag = |s: &str, t: &str| -> Option<String> {
        let a = s.find(&format!("<{t}>"))? + t.len() + 2;
        let b = s[a..].find(&format!("</{t}>"))? + a;
        Some(unescape(&s[a..b]))
    };
    xml.split("<entry")
        .skip(1)
        .filter(|e| e.trim_start().starts_with("kind=\"dir\"") || e.contains("kind=\"dir\""))
        .filter_map(|e| {
            Some(Entry {
                name: tag(e, "name")?,
                date: tag(e, "date").map(|d| iso_secs(&d)).unwrap_or(0),
                author: tag(e, "author").unwrap_or_default(),
            })
        })
        .collect()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// `2026-10-23T15:46:10.123456Z` → Unix seconds (0 when it doesn't parse).
fn iso_secs(s: &str) -> u64 {
    let num = |r: std::ops::Range<usize>| s.get(r).and_then(|t| t.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d), Some(hh), Some(mm), Some(ss)) =
        (num(0..4), num(5..7), num(8..10), num(11..13), num(14..16), num(17..19))
    else {
        return 0;
    };
    // Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (days * 86_400 + hh * 3600 + mm * 60 + ss).max(0) as u64
}

fn check_new_name(name: &str) -> Result<(), String> {
    let bad = name.trim().is_empty()
        || name == "trunk"
        || name.chars().any(|c| matches!(c, '/' | '\\' | ':' | '?' | '#' | '%') || c.is_control());
    if bad {
        return Err(format!("\"{name}\" can't be a branch name here."));
    }
    Ok(())
}

pub fn switch(dir: &Path, target: &Target) -> Result<String, String> {
    let (root, here) = where_am_i(dir)?;
    match target {
        Target::Local { name } | Target::Remote { name, .. } => {
            svn(dir, &["switch", &url_of(&root, name)], true)?;
            Ok(format!("Switched to {name}."))
        }
        Target::New { name } => {
            check_new_name(name)?;
            let from = here.strip_prefix("^/").unwrap_or(&here).to_string();
            let msg = format!("Create branch {name} from {from}");
            svn(dir, &["copy", &format!("{root}/{from}"), &url_of(&root, name), "-m", &msg], true)?;
            svn(dir, &["switch", &url_of(&root, name)], true)?;
            Ok(format!("Created {name} on the server from {from} and switched to it."))
        }
    }
}

/// A fresh checkout of the branch beside this working copy.
pub fn add_checkout(dir: &Path, target: &Target) -> Result<PathBuf, String> {
    let (root, here) = where_am_i(dir)?;
    let name = target.local_name();
    if let Target::New { name } = target {
        check_new_name(name)?;
        let from = here.strip_prefix("^/").unwrap_or(&here).to_string();
        let msg = format!("Create branch {name} from {from}");
        svn(dir, &["copy", &format!("{root}/{from}"), &url_of(&root, name), "-m", &msg], true)?;
    }
    let wc_root = info(dir, "wc-root").map(PathBuf::from).unwrap_or_else(|_| dir.to_path_buf());
    let path = copy_path(&wc_root, name).ok_or("Couldn't work out where the checkout would go.")?;
    svn(dir, &["checkout", &url_of(&root, name), &path.to_string_lossy()], true)?;
    Ok(path)
}

pub fn sync(dir: &Path, op: SyncOp) -> Result<String, String> {
    match op {
        SyncOp::Update => {
            // Conflicts with local changes are left for you to resolve, never
            // settled by picking a side.
            svn(dir, &["update", "--accept", "postpone"], true)?;
            Ok(match info(dir, "revision") {
                Ok(r) => format!("Updated to revision {r}."),
                Err(_) => "Updated.".into(),
            })
        }
        _ => Err("Subversion has no such step; update instead.".into()),
    }
}

pub fn details(dir: &Path) -> Result<Details, String> {
    Ok(Details::remotes_only(vec![info(dir, "repos-root-url")?]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::which;
    use crate::vcs_state::tests::{file_url, tmp, vcs};

    #[test]
    fn iso_dates_and_list_xml() {
        assert_eq!(iso_secs("1970-01-01T00:00:00.000000Z"), 0);
        assert_eq!(iso_secs("2026-10-23T15:46:10.123456Z"), 1_792_770_370);
        assert_eq!(iso_secs("2000-02-29T12:00:00Z"), 951_825_600);
        assert_eq!(iso_secs("garbage"), 0);
        let xml = r#"<?xml version="1.0"?><lists><list path="x">
<entry kind="dir"><name>feature-x</name><commit revision="3"><author>ann</author><date>2026-10-23T15:46:10.1Z</date></commit></entry>
<entry kind="file"><name>README</name><size>3</size><commit revision="1"><author>b</author><date>2026-10-23T15:46:10.1Z</date></commit></entry>
<entry kind="dir"><name>a&amp;b</name><commit revision="4"><author>c</author><date>2026-10-24T00:00:00.0Z</date></commit></entry>
</list></lists>"#;
        let e = list_entries(xml);
        assert_eq!(e.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["feature-x", "a&b"]);
        assert_eq!(e[0].author, "ann");
    }

    #[test]
    fn branches_switching_checkouts_and_update() {
        if which("svn").is_none() || which("svnadmin").is_none() {
            return;
        }
        let base = tmp("svn-ops");
        vcs(&base, "svnadmin", &["create", "repo"]);
        let url = file_url(&base.join("repo"));
        vcs(&base, "svn", &["mkdir", "-q", "-m", "layout", &format!("{url}/trunk"), &format!("{url}/branches")]);
        vcs(&base, "svn", &["checkout", "-q", &format!("{url}/trunk"), "wc"]);
        let wc = base.join("wc");
        std::fs::write(wc.join("a.txt"), "a").unwrap();
        vcs(&wc, "svn", &["add", "-q", "a.txt"]);
        vcs(&wc, "svn", &["commit", "-q", "-m", "a"]);
        vcs(&base, "svn", &["copy", "-q", "-m", "b", &format!("{url}/trunk"), &format!("{url}/branches/old-work")]);

        let list = branches(&wc).unwrap();
        let names: Vec<(&str, bool)> = list.branches.iter().map(|b| (b.name.as_str(), b.current)).collect();
        assert_eq!(names[0], ("trunk", true), "{list:?}");
        assert!(names.contains(&("old-work", false)), "{list:?}");

        switch(&wc, &Target::Local { name: "old-work".into() }).unwrap();
        assert_eq!(crate::vcs_state::read(&wc).branch.as_deref(), Some("old-work"));
        switch(&wc, &Target::New { name: "fresh".into() }).unwrap();
        assert_eq!(crate::vcs_state::read(&wc).branch.as_deref(), Some("fresh"));
        assert!(switch(&wc, &Target::New { name: "a/b".into() }).is_err());

        // Switching would merge local changes into the other branch: refused.
        std::fs::write(wc.join("a.txt"), "edited").unwrap();
        let err = crate::vcs_ops::switch(&wc, &Target::Local { name: "trunk".into() }).unwrap_err();
        assert!(err.contains("uncommitted"), "{err}");
        vcs(&wc, "svn", &["revert", "-q", "a.txt"]);

        let co = add_checkout(&wc, &Target::Local { name: "trunk".into() }).unwrap();
        assert_eq!(co.file_name().unwrap(), "wc-trunk");
        assert_eq!(crate::vcs_state::read(&co).branch.as_deref(), Some("trunk"));

        // A commit from the other checkout arrives with an update.
        switch(&wc, &Target::Local { name: "trunk".into() }).unwrap();
        std::fs::write(co.join("b.txt"), "b").unwrap();
        vcs(&co, "svn", &["add", "-q", "b.txt"]);
        vcs(&co, "svn", &["commit", "-q", "-m", "b"]);
        assert!(!wc.join("b.txt").exists());
        let msg = sync(&wc, SyncOp::Update).unwrap();
        assert!(msg.starts_with("Updated to revision"), "{msg}");
        assert!(wc.join("b.txt").exists());
        assert_eq!(details(&wc).unwrap().remotes, vec![url.clone()]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
