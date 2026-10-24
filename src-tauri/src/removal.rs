//! "Move to Recycle Bin…" for a listed folder: what would be lost, the move
//! itself, and Undo.
//!
//! Nothing is ever deleted outright. On Windows the folder goes to the Recycle
//! Bin with Windows' own "this will be permanently deleted" warning kept on —
//! so a folder too big for the bin, or on a drive without one, prompts instead
//! of vanishing. On Linux it goes to the freedesktop Trash, which never deletes.
//!
//! Before that, [`assess`] checks what exists nowhere else: uncommitted
//! changes, commits on no remote, stashes / shelves, repos nested inside. Any
//! of those, or a folder without version control, or anything that couldn't be
//! checked, means the dialog asks for the folder's name to be typed. Only a
//! clean repo whose every commit is on a remote gets a one-click confirm.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{self, Config};
use crate::error::{AppError, AppResult};
use crate::scan::Repo;
use crate::vcs_state::{self, RepoStatus};

/// What the confirmation dialog shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    pub name: String,
    pub path: String,
    /// The version-control system found, if any ("Git", "Mercurial", …).
    pub vcs: Option<String>,
    /// What would be lost, worded for the dialog. Empty when nothing is.
    pub risks: Vec<String>,
    /// What couldn't be checked, and why. Treated the same as a risk.
    pub unknown: Vec<String>,
    /// Typing the folder's name is required to confirm.
    pub needs_typed: bool,
}

/// Whether `path` is one of the configured roots — never offered for removal.
pub fn is_root(cfg: &Config, path: &Path) -> bool {
    config::resolved_roots(cfg).iter().any(|r| same_path(r, path))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

/// `path` checked against everything that makes a removal safe to attempt:
/// it's a listed folder, strictly inside a root, not a root, a real folder
/// (not a link or junction, which could resolve somewhere else entirely).
pub fn check_removable(cfg: &Config, repos: &[Repo], path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    if !repos.iter().any(|r| same_path(Path::new(&r.path), &p)) {
        return Err("That folder isn't in the list any more.".into());
    }
    let roots = config::resolved_roots(cfg);
    if roots.iter().any(|r| same_path(r, &p)) {
        return Err("That's one of your root folders; it can't be removed from here.".into());
    }
    if !roots.iter().any(|r| p.starts_with(r)) {
        return Err("That folder isn't inside any of your root folders.".into());
    }
    let meta = p
        .symlink_metadata()
        .map_err(|e| format!("Couldn't read {path}: {e}"))?;
    if meta.file_type().is_symlink() || is_reparse_point(&meta) {
        return Err("That's a link to another folder; remove it in your file manager.".into());
    }
    if !meta.is_dir() {
        return Err(format!("{path} isn't a folder."));
    }
    Ok(p)
}

#[cfg(windows)]
fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_meta: &std::fs::Metadata) -> bool {
    false
}

// --- what would be lost ---------------------------------------------------

/// Check `dir` for anything that exists only there. Runs version-control
/// commands, so call it off the UI thread.
pub fn assess(dir: &Path) -> Assessment {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_string_lossy().into_owned());
    let state = vcs_state::read(dir);
    let mut risks = match state.vcs.as_deref() {
        None => vec!["Nothing in it is under version control, so its contents exist only here.".into()],
        Some(label) => describe(label, &state),
    };

    let nested = nested_repos(dir);
    if !nested.is_empty() {
        let shown: Vec<&str> = nested.iter().take(5).map(String::as_str).collect();
        let more = nested.len().saturating_sub(shown.len());
        risks.push(format!(
            "It holds {} more {} inside it, which would go too: {}{}.",
            nested.len(),
            if nested.len() == 1 { "repository" } else { "repositories" },
            shown.join(", "),
            if more > 0 { format!(" and {more} more") } else { String::new() },
        ));
    }

    // What the system can't tell offline is as unknown here as a failed check.
    let unknown: Vec<String> = state.unknown.into_iter().chain(state.limits).collect();
    let needs_typed = !risks.is_empty() || !unknown.is_empty();
    Assessment {
        name,
        path: dir.to_string_lossy().into_owned(),
        vcs: state.vcs,
        risks,
        unknown,
        needs_typed,
    }
}

/// What in `s` exists only here, worded for the dialog in each system's terms.
fn describe(label: &str, s: &RepoStatus) -> Vec<String> {
    let mut out = Vec::new();
    let changed = (s.changed + s.conflicted) as usize;
    if label == "Jujutsu" {
        // jj has no staging step: every change is part of the working-copy commit.
        if changed > 0 {
            out.push(format!("{changed} {} in the working copy.", plural(changed, "changed file", "changed files")));
        }
    } else {
        if changed > 0 {
            out.push(format!("{changed} {} not committed.", plural(changed, "changed file", "changed files")));
        }
        let new = s.untracked as usize;
        if new > 0 {
            out.push(format!("{new} new {} not yet added.", plural(new, "file", "files")));
        }
    }
    if let Some(op) = &s.operation {
        out.push(format!("A {op} is in progress here, not yet finished or aborted."));
    }
    if s.no_remote {
        out.push("It has no remote, so its whole history exists only here.".into());
    }
    if let Some(n) = s.unpushed.filter(|n| *n > 0) {
        let n = n as usize;
        out.push(if label == "Mercurial" {
            format!("{n} {} not pushed yet (draft).", plural(n, "commit", "commits"))
        } else {
            format!("{n} {} on no remote.", plural(n, "commit", "commits"))
        });
    }
    let n = s.stashes as usize;
    if n > 0 {
        out.push(match label {
            "Git" => format!("{n} {}.", plural(n, "stash", "stashes")),
            "Fossil" => format!("{n} stashed {}.", plural(n, "change", "changes")),
            _ => format!("{n} shelved {}.", plural(n, "change", "changes")),
        });
    }
    out
}

/// Folders below `dir` (relative paths) that carry a version-control marker of
/// their own — submodules, nested clones, vendored checkouts.
fn nested_repos(dir: &Path) -> Vec<String> {
    let markers: Vec<&str> = vcs_state::SYSTEMS.iter().flat_map(|(_, m)| m.iter().copied()).collect();
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(dir)
        .max_depth(Some(6))
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !crate::scan::HARD_SKIP.iter().any(|s| n.eq_ignore_ascii_case(s))
        })
        .build();
    for ent in walker.flatten() {
        let p = ent.path();
        if p == dir || !ent.file_type().is_some_and(|t| t.is_dir()) {
            continue;
        }
        if markers.iter().any(|m| p.join(m).exists()) {
            if let Ok(rel) = p.strip_prefix(dir) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

// --- moving it, and back --------------------------------------------------

/// Move `dir` to the Recycle Bin / Trash. Returns the trash entry it became,
/// for Undo — `None` when it went but can't be found again in the bin.
/// `settle`: how long to keep trying while something lets go of the folder —
/// a shell of dev-prompt's own that was just closed takes a moment to exit.
/// Zero tries once: a retry could repeat Windows' "permanently delete?" prompt.
pub fn move_to_trash(dir: &Path, settle: std::time::Duration) -> AppResult<Option<trash::TrashItem>> {
    let before: HashSet<OsString> = trashed_at(dir).into_iter().map(|i| i.id).collect();
    let started = std::time::Instant::now();
    let moved = loop {
        match trash::delete(dir) {
            Err(_) if started.elapsed() < settle && dir.exists() => {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            other => break other,
        }
    };
    moved.map_err(|e| {
        AppError::msg(format!(
            "Couldn't move {} to the {}: {e}. If a program has files in it open, or a terminal is working in it, close it and try again.",
            dir.display(),
            bin_name()
        ))
    })?;
    if dir.exists() {
        // Windows' "permanently delete?" prompt was declined, or only part of
        // it could be moved.
        return Err(AppError::msg(format!(
            "{} was only partly moved, or not at all. What's left is still at {}.",
            dir.display(),
            dir.display()
        )));
    }
    Ok(trashed_at(dir).into_iter().find(|i| !before.contains(&i.id)))
}

/// Put a trashed folder back where it was.
pub fn restore(item: trash::TrashItem) -> AppResult<PathBuf> {
    let path = item.original_path();
    trash::os_limited::restore_all([item]).map_err(|e| match e {
        trash::Error::RestoreCollision { path, .. } => AppError::msg(format!(
            "Something new is already at {}; move it away first.",
            path.display()
        )),
        e => AppError::msg(format!("Couldn't restore {}: {e}", path.display())),
    })?;
    Ok(path)
}

/// Every item in the bin that came from `dir`.
fn trashed_at(dir: &Path) -> Vec<trash::TrashItem> {
    trash::os_limited::list()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| same_path(&i.original_path(), dir))
        .collect()
}

/// What the bin is called on this OS, for messages and the menu label.
pub fn bin_name() -> &'static str {
    if cfg!(windows) { "Recycle Bin" } else { "Trash" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::which;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "dp-removal-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn each_system_is_described_in_its_own_terms() {
        let s = RepoStatus {
            changed: 1,
            conflicted: 1,
            untracked: 2,
            unpushed: Some(1),
            stashes: 3,
            ..Default::default()
        };
        assert_eq!(
            describe("Git", &s),
            vec!["2 changed files not committed.", "2 new files not yet added.", "1 commit on no remote.", "3 stashes."]
        );
        let hg = describe("Mercurial", &s).join(" | ");
        assert!(hg.contains("1 commit not pushed yet (draft)") && hg.contains("3 shelved changes"), "{hg}");
        let jj = describe("Jujutsu", &RepoStatus { changed: 1, ..Default::default() });
        assert_eq!(jj, vec!["1 changed file in the working copy."]);
        let op = describe("Git", &RepoStatus { operation: Some("rebase".into()), ..Default::default() });
        assert_eq!(op, vec!["A rebase is in progress here, not yet finished or aborted."]);
        assert!(describe("Git", &RepoStatus::default()).is_empty());
    }

    #[test]
    fn a_folder_without_version_control_needs_typing() {
        let dir = tmp("plain");
        std::fs::write(dir.join("notes.md"), "x").unwrap();
        let a = assess(&dir);
        assert_eq!(a.vcs, None);
        assert!(a.needs_typed);
        assert!(a.risks[0].contains("only here"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clean_pushed_git_repo_is_one_click_and_unpushed_work_is_not() {
        if which("git").is_none() {
            return;
        }
        let base = tmp("git");
        let (remote, work) = (base.join("remote.git"), base.join("work"));
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare", "-q"]);
        git(&base, &["clone", "-q", &remote.to_string_lossy(), "work"]);
        std::fs::write(work.join("a.txt"), "a").unwrap();
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "a"]);
        git(&work, &["push", "-q", "origin", "HEAD"]);

        let a = assess(&work);
        assert_eq!(a.vcs.as_deref(), Some("Git"));
        assert!(!a.needs_typed, "{a:?}");

        // A commit that never left.
        std::fs::write(work.join("b.txt"), "b").unwrap();
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "b"]);
        // An edit, a new file and a stash.
        std::fs::write(work.join("a.txt"), "changed").unwrap();
        git(&work, &["stash", "-q"]);
        std::fs::write(work.join("a.txt"), "changed again").unwrap();
        std::fs::write(work.join("new.txt"), "n").unwrap();

        let a = assess(&work);
        assert!(a.needs_typed);
        let all = a.risks.join(" | ");
        assert!(all.contains("1 commit on no remote"), "{all}");
        assert!(all.contains("1 changed file not committed"), "{all}");
        assert!(all.contains("1 new file not yet added"), "{all}");
        assert!(all.contains("1 stash"), "{all}");
        assert!(a.unknown.is_empty(), "{:?}", a.unknown);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_git_repo_with_no_remote_says_so() {
        if which("git").is_none() {
            return;
        }
        let dir = tmp("noremote");
        git(&dir, &["init", "-q"]);
        // No commits yet: nothing about history, but the file is at risk.
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        let a = assess(&dir);
        assert!(!a.risks.iter().any(|r| r.contains("no remote")), "{a:?}");
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "a"]);
        let a = assess(&dir);
        assert!(a.risks.iter().any(|r| r.contains("no remote")), "{a:?}");
        assert!(a.needs_typed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Run a version-control command in `dir`, with an identity set so
    /// commits work on a machine that has none configured.
    fn vcs(dir: &Path, program: &str, args: &[&str]) {
        let out = std::process::Command::new(program)
            .args(args)
            .current_dir(dir)
            .env("HGUSER", "t <t@t>")
            .env("JJ_USER", "t")
            .env("JJ_EMAIL", "t@t")
            .output()
            .unwrap_or_else(|e| panic!("{program} {args:?}: {e}"));
        assert!(
            out.status.success(),
            "{program} {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn mercurial_drafts_changes_and_shelves_are_found() {
        if which("hg").is_none() {
            return;
        }
        let base = tmp("hg");
        let (remote, work) = (base.join("remote"), base.join("work"));
        std::fs::create_dir_all(&remote).unwrap();
        vcs(&remote, "hg", &["init"]);
        vcs(&base, "hg", &["clone", "-q", &remote.to_string_lossy(), "work"]);
        std::fs::write(work.join("a.txt"), "a").unwrap();
        vcs(&work, "hg", &["add", "a.txt"]);
        vcs(&work, "hg", &["commit", "-m", "a"]);
        vcs(&work, "hg", &["push", "-q"]);

        let a = assess(&work);
        assert_eq!(a.vcs.as_deref(), Some("Mercurial"));
        assert!(!a.needs_typed, "a pushed, clean repo: {a:?}");

        std::fs::write(work.join("b.txt"), "b").unwrap();
        vcs(&work, "hg", &["add", "b.txt"]);
        vcs(&work, "hg", &["commit", "-m", "b"]);
        std::fs::write(work.join("a.txt"), "shelved").unwrap();
        vcs(&work, "hg", &["--config", "extensions.shelve=", "shelve", "-q"]);
        std::fs::write(work.join("a.txt"), "edited").unwrap();
        std::fs::write(work.join("new.txt"), "n").unwrap();

        let a = assess(&work);
        let all = a.risks.join(" | ");
        assert!(all.contains("1 commit not pushed yet"), "{all}");
        assert!(all.contains("1 changed file not committed"), "{all}");
        assert!(all.contains("1 new file not yet added"), "{all}");
        assert!(all.contains("1 shelved change"), "{all}");
        assert!(a.unknown.is_empty(), "{:?}", a.unknown);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn jujutsu_working_copy_and_commits_off_every_remote_are_found() {
        if which("jj").is_none() || which("git").is_none() {
            return;
        }
        let base = tmp("jj");
        let (remote, work) = (base.join("remote.git"), base.join("work"));
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare", "-q"]);
        std::fs::create_dir_all(&work).unwrap();
        vcs(&work, "jj", &["git", "init"]);
        vcs(&work, "jj", &["git", "remote", "add", "origin", &remote.to_string_lossy()]);
        std::fs::write(work.join("a.txt"), "a").unwrap();
        vcs(&work, "jj", &["commit", "-m", "a"]);
        vcs(&work, "jj", &["bookmark", "create", "main", "-r", "@-"]);
        vcs(&work, "jj", &["git", "push", "--bookmark", "main"]);

        let a = assess(&work);
        assert_eq!(a.vcs.as_deref(), Some("Jujutsu"));
        assert!(!a.needs_typed, "a pushed, clean repo: {a:?}");

        std::fs::write(work.join("b.txt"), "b").unwrap();
        vcs(&work, "jj", &["commit", "-m", "b"]);
        std::fs::write(work.join("c.txt"), "c").unwrap();

        let a = assess(&work);
        let all = a.risks.join(" | ");
        assert!(all.contains("1 commit on no remote"), "{all}");
        assert!(all.contains("1 changed file in the working copy"), "{all}");
        assert!(a.unknown.is_empty(), "{:?}", a.unknown);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn nested_repos_are_named() {
        let dir = tmp("nested");
        std::fs::create_dir_all(dir.join("vendor/lib/.git")).unwrap();
        std::fs::create_dir_all(dir.join("plugins/mine/.hg")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x/.git")).unwrap(); // skipped like the scan
        assert_eq!(nested_repos(&dir), vec!["plugins/mine", "vendor/lib"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_listed_folders_inside_a_root_can_be_removed() {
        let root = tmp("root");
        let inside = root.join("proj");
        std::fs::create_dir_all(&inside).unwrap();
        let outside = tmp("outside");
        let mut cfg = config::bundled_defaults();
        cfg.roots = vec![root.to_string_lossy().into_owned()];
        let listed = |p: &Path| Repo {
            name: String::new(),
            path: p.to_string_lossy().into_owned(),
            vcs: None,
            sentinels: Vec::new(),
            last_seen: 0,
        };
        let repos = vec![listed(&inside), listed(&root), listed(&outside)];

        assert!(check_removable(&cfg, &repos, &inside.to_string_lossy()).is_ok());
        assert!(check_removable(&cfg, &repos, &root.to_string_lossy()).unwrap_err().contains("root"));
        assert!(check_removable(&cfg, &repos, &outside.to_string_lossy()).is_err());
        let unlisted = root.join("other");
        std::fs::create_dir_all(&unlisted).unwrap();
        assert!(check_removable(&cfg, &repos, &unlisted.to_string_lossy()).unwrap_err().contains("list"));
        assert!(is_root(&cfg, &root) && !is_root(&cfg, &inside));

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// The real Recycle Bin / Trash: a folder goes, and Undo brings it back
    /// with its contents.
    #[test]
    fn a_folder_goes_to_the_bin_and_comes_back() {
        // On Linux, under home: the home Trash is on the same filesystem,
        // where /tmp (often tmpfs) may not have one.
        #[cfg(target_os = "linux")]
        let dir = {
            let d = dirs::home_dir().unwrap().join(format!(".dp-removal-bin-{}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            d
        };
        #[cfg(not(target_os = "linux"))]
        let dir = tmp("bin");
        std::fs::write(dir.join("keep.txt"), "precious").unwrap();
        let item = move_to_trash(&dir, Default::default()).unwrap().expect("found again in the bin");
        assert!(!dir.exists());
        let back = restore(item).unwrap();
        assert!(same_path(&back, &dir));
        assert_eq!(std::fs::read_to_string(dir.join("keep.txt")).unwrap(), "precious");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
