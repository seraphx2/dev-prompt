//! Breezy's side of `vcs_ops.rs`. A Breezy branch is usually a folder of its
//! own — each one already listed as a repo — so there's no switcher; what's
//! here is pulling from the parent branch and pushing to the remembered push
//! location. `brz pull` refuses once the branches have diverged.

use std::path::Path;

use crate::vcs_ops::{run, Details, SyncOp, LOCAL_TIMEOUT, NETWORK_TIMEOUT};

fn brz(dir: &Path, args: &[&str], network: bool) -> Result<String, String> {
    run("brz", dir, args, if network { NETWORK_TIMEOUT } else { LOCAL_TIMEOUT }, &[])
}

/// A remembered location (`parent_location`, `push_location`), if set.
fn location(dir: &Path, key: &str) -> Option<String> {
    brz(dir, &["config", key], false).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn sync(dir: &Path, op: SyncOp) -> Result<String, String> {
    match op {
        SyncOp::Pull => {
            let s = crate::vcs_state::read(dir);
            if s.changed + s.conflicted > 0 || s.operation.is_some() {
                return Err("Commit or shelve your changes first — brz pull would merge them.".into());
            }
            let from = location(dir, "parent_location").ok_or("This branch has no parent to pull from.")?;
            brz(dir, &["pull"], true)?;
            Ok(format!("Pulled from {from}."))
        }
        SyncOp::Push => {
            let to = location(dir, "push_location").ok_or("This branch has no push location yet.")?;
            brz(dir, &["push"], true)?;
            Ok(format!("Pushed to {to}."))
        }
        _ => Err("Breezy has no such step here; pull or push instead.".into()),
    }
}

pub fn details(dir: &Path) -> Result<Details, String> {
    let pull_from = location(dir, "parent_location");
    let push_to = location(dir, "push_location");
    let mut d = Details::remotes_only(pull_from.iter().chain(push_to.iter()).cloned().collect());
    d.remotes.dedup();
    d.pull_from = pull_from;
    d.push_to = push_to;
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::which;
    use crate::vcs_state::tests::{tmp, vcs};

    #[test]
    fn pull_from_the_parent_and_push_to_the_push_location() {
        if which("brz").is_none() {
            return;
        }
        // Breezy on Windows resolves relative paths against the drive root:
        // every path here is absolute.
        let base = tmp("brz-ops");
        let (parent, child, mirror) = (base.join("parent"), base.join("child"), base.join("mirror"));
        vcs(&base, "brz", &["init", "-q", &parent.to_string_lossy()]);
        std::fs::write(parent.join("a.txt"), "a\n").unwrap();
        vcs(&parent, "brz", &["add", "-q"]);
        vcs(&parent, "brz", &["commit", "-q", "-m", "a"]);
        vcs(&base, "brz", &["branch", "-q", &parent.to_string_lossy(), &child.to_string_lossy()]);

        let d = details(&child).unwrap();
        assert!(d.pull_from.is_some() && d.push_to.is_none(), "{d:?}");
        assert!(sync(&child, SyncOp::Push).is_err(), "no push location yet");

        std::fs::write(parent.join("b.txt"), "b\n").unwrap();
        vcs(&parent, "brz", &["add", "-q"]);
        vcs(&parent, "brz", &["commit", "-q", "-m", "b"]);
        std::fs::write(child.join("a.txt"), "edited\n").unwrap();
        assert!(sync(&child, SyncOp::Pull).is_err(), "uncommitted changes");
        vcs(&child, "brz", &["revert", "-q"]);
        sync(&child, SyncOp::Pull).unwrap();
        assert!(child.join("b.txt").exists());

        vcs(&child, "brz", &["push", "-q", "--remember", &mirror.to_string_lossy()]);
        std::fs::write(child.join("c.txt"), "c\n").unwrap();
        vcs(&child, "brz", &["add", "-q"]);
        vcs(&child, "brz", &["commit", "-q", "-m", "c"]);
        sync(&child, SyncOp::Push).unwrap();
        assert!(details(&child).unwrap().push_to.is_some());
        let _ = std::fs::remove_dir_all(&base);
    }
}
