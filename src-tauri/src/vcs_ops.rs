//! The few version-control operations dev-prompt does itself, for every
//! system that has them. Each is one keystroke, fully described by its label,
//! and never forced: switch to or create a branch (bookmark), make a second
//! working copy (a Git worktree, a Jujutsu workspace, a Mercurial share, a
//! Subversion or Fossil checkout), and the safe ways of talking to a remote.
//! Plus the web links (repo, branch, new pull request) worked out from a
//! remote's URL.
//!
//! Each system's own commands live in `git_ops.rs`, `jj_ops.rs`, `hg_ops.rs`,
//! `svn_ops.rs`, `fossil_ops.rs` and `brz_ops.rs`; this file holds what they
//! share and picks the one a folder needs. Anything that needs reviewing
//! first — staging, commit messages, diffs, conflicts, history — is a GUI's
//! job; the action menu hands off to one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::provision::run_captured_env;
use crate::rules::which;
use crate::vcs_state::{self, RepoStatus};

/// Local queries and branch switches. Like the status reads, only a stuck
/// command is stopped; a big repository on a cold disk is just slow.
pub(crate) const LOCAL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Anything that talks to a remote: a big fetch over a slow link takes as
/// long as a clone does.
pub(crate) const NETWORK_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Run `program args` in `dir`; its own error words on failure (see [`tidy`]).
pub(crate) fn run(program: &str, dir: &Path, args: &[&str], timeout: Duration, env: &[(&str, &str)]) -> Result<String, String> {
    let resolved = which(program).ok_or_else(|| format!("`{program}` isn't installed here (it isn't on PATH)."))?;
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    run_captured_env(&resolved, &args, dir, timeout, env).map_err(|e| tidy(&e))
}

/// The tool's own words without the "exit code 1." line `run_captured` leads
/// with, its "hint:" lines, or an `error:` / `fatal:` / `abort:` prefix.
pub(crate) fn tidy(e: &str) -> String {
    let mut lines: Vec<&str> = e.lines().collect();
    if lines.first().is_some_and(|l| l.starts_with("exit code") && lines.len() > 1) {
        lines.remove(0);
    }
    let kept: Vec<&str> = lines.into_iter().filter(|l| !l.starts_with("hint:")).collect();
    let text = kept.join("\n");
    let t = text.trim();
    ["error: ", "fatal: ", "abort: ", "Error: ", "svn: ", "brz: ERROR: "]
        .iter()
        .find_map(|p| t.strip_prefix(p))
        .unwrap_or(t)
        .to_string()
}

// --- branches and working copies ---------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    /// `feature/x`; for a remote branch nothing local tracks yet,
    /// `origin/feature/x` (Git) or `feature/x@origin` (Jujutsu).
    pub name: String,
    /// "branch", "bookmark" (Jujutsu, Mercurial) or "remote".
    pub kind: String,
    /// The remote it's on, for a remote branch; `None` for a local one.
    pub remote: Option<String>,
    /// Checked out in this working copy.
    pub current: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Its upstream's remote branch was deleted.
    pub gone: bool,
    /// Where it's checked out, when that's another working copy.
    pub worktree: Option<String>,
    /// Unix seconds of its last commit.
    pub date: u64,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// Another working copy of the same repository: a Git worktree, a
    /// Jujutsu workspace, a Fossil checkout.
    pub path: String,
    pub branch: Option<String>,
    /// Short commit id checked out.
    pub head: String,
    /// The working copy this list was asked for.
    pub current: bool,
    /// Its folder is gone (`git worktree prune` would drop it).
    pub prunable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchList {
    /// Local branches (current first, then most recently checked out), then
    /// the remote branches no local one tracks.
    pub branches: Vec<Branch>,
    pub worktrees: Vec<Worktree>,
    pub remotes: Vec<String>,
}

/// Paths as the tools print them (`D:/git/x`) made comparable with ours.
pub(crate) fn same_dir(a: &str, b: &Path) -> bool {
    let norm = |s: &str| {
        let s = s.replace('\\', "/").trim_end_matches('/').to_string();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(&b.to_string_lossy())
}

pub(crate) fn native(path: &str) -> String {
    if cfg!(windows) { path.replace('/', "\\") } else { path.to_string() }
}

/// What to check out: a local branch (bookmark), a remote one (a local one
/// tracking it is made), or a new one from where the working copy is.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    Local { name: String },
    Remote { remote: String, name: String },
    New { name: String },
}

impl Target {
    /// The local name it ends up as: `origin/x` (Git) and `x@origin`
    /// (Jujutsu) are both `x`.
    pub(crate) fn local_name(&self) -> &str {
        match self {
            Target::Local { name } | Target::New { name } => name,
            Target::Remote { remote, name } => name
                .strip_prefix(&format!("{remote}/"))
                .or_else(|| name.strip_suffix(&format!("@{remote}")))
                .unwrap_or(name),
        }
    }
}


/// `dev-prompt` + `feature/login` → `dev-prompt-feature-login`.
pub(crate) fn worktree_folder_name(repo: &str, branch: &str) -> String {
    let slug: String = branch
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' })
        .collect();
    let slug = slug.trim_matches(|c| c == '-' || c == '.');
    format!("{repo}-{slug}")
}

// --- talking to the remote ---------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncOp {
    /// Bring the remote's commits in without touching the working copy:
    /// `git fetch`, `jj git fetch`, `hg pull`.
    Fetch,
    /// Git: `pull --ff-only`, never a merge commit or a rebase. Breezy:
    /// `pull`, which refuses once the branches have diverged.
    Pull,
    /// Push the current branch (bookmark, drafts) to where it goes.
    Push,
    /// Push a branch (bookmark) with no upstream to `remote` and track it.
    Publish,
    /// Bring the working copy up to date: `svn update`, `fossil update`.
    Update,
    /// Fossil: `fossil sync`, both ways, working copy untouched.
    Sync,
}

// --- the remote on the web -----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Forge {
    GitHub,
    GitLab,
    Bitbucket,
    Azure,
    Gitea,
}

impl Forge {
    fn name(self) -> &'static str {
        match self {
            Forge::GitHub => "GitHub",
            Forge::GitLab => "GitLab",
            Forge::Bitbucket => "Bitbucket",
            Forge::Azure => "Azure DevOps",
            Forge::Gitea => "Gitea",
        }
    }
}

/// A remote recognised as a repository on a forge, with its web address.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WebRepo {
    forge: Forge,
    /// `https://github.com/owner/repo`.
    base: String,
    /// What the forge calls itself here (Codeberg is Gitea underneath).
    label: String,
}

/// Work out the web address of the repository a remote URL points at:
/// `https://host/path`, `ssh://user@host:port/path`, or scp-style
/// `user@host:path`. `None` for a local path or a host that isn't recognised.
fn web_repo(url: &str) -> Option<WebRepo> {
    let url = url.trim();
    let (host, path) = if let Some((scheme, rest)) = url.split_once("://") {
        if !matches!(scheme, "https" | "http" | "ssh" | "git") {
            return None;
        }
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?;
        // An SSH port means nothing on the web; an HTTPS one is kept.
        let host = if scheme.starts_with("http") { host } else { host.split(':').next()? };
        (host.to_string(), path.to_string())
    } else {
        let (authority, path) = url.split_once(':')?;
        if authority.contains('/') || authority.len() == 1 {
            return None; // a local path (`./x`, `C:\x`)
        }
        (authority.rsplit('@').next()?.to_string(), path.to_string())
    };
    let host = host.to_lowercase();
    let path = path.trim_matches('/').trim_end_matches(".git").trim_matches('/').to_string();
    if path.is_empty() {
        return None;
    }

    // Azure DevOps addresses its repos differently over SSH and HTTPS.
    if host == "ssh.dev.azure.com" || host == "vs-ssh.visualstudio.com" {
        let parts: Vec<&str> = path.strip_prefix("v3/")?.split('/').collect();
        let [org, project, repo] = parts[..] else { return None };
        let base = if host == "ssh.dev.azure.com" {
            format!("https://dev.azure.com/{org}/{project}/_git/{repo}")
        } else {
            format!("https://{org}.visualstudio.com/{project}/_git/{repo}")
        };
        return Some(WebRepo { forge: Forge::Azure, base, label: Forge::Azure.name().into() });
    }
    let base = format!("https://{host}/{path}");
    let (forge, label) = if host == "dev.azure.com" || host.ends_with(".visualstudio.com") {
        (Forge::Azure, "Azure DevOps")
    } else if host == "github.com" || host.starts_with("github.") {
        (Forge::GitHub, "GitHub")
    } else if host == "gitlab.com" || host.starts_with("gitlab.") {
        (Forge::GitLab, "GitLab")
    } else if host == "bitbucket.org" {
        (Forge::Bitbucket, "Bitbucket")
    } else if host == "codeberg.org" {
        (Forge::Gitea, "Codeberg")
    } else if host.starts_with("gitea.") || host.starts_with("forgejo.") {
        (Forge::Gitea, "Gitea")
    } else {
        return None;
    };
    Some(WebRepo { forge, base, label: label.into() })
}

/// Percent-encode for a URL path (`/` kept) or a query value (`/` encoded too).
fn encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebLink {
    /// "repo", "branch" or "pr".
    pub kind: String,
    pub label: String,
    pub url: String,
}

/// The repo page, the branch's page once it's pushed, and a new pull request
/// for it unless it's the default branch. `branch` is the remote branch name.
fn links_for(web: &WebRepo, branch: Option<&str>, default: Option<&str>) -> Vec<WebLink> {
    let site = &web.label;
    let mut out = vec![WebLink { kind: "repo".into(), label: format!("Open on {site}"), url: web.base.clone() }];
    let Some(b) = branch else { return out };
    let (bp, bq) = (encode(b, true), encode(b, false));
    let branch_url = match web.forge {
        Forge::GitHub => format!("{}/tree/{bp}", web.base),
        Forge::GitLab => format!("{}/-/tree/{bp}", web.base),
        Forge::Bitbucket => format!("{}/src/{bp}", web.base),
        Forge::Azure => format!("{}?version=GB{bq}", web.base),
        Forge::Gitea => format!("{}/src/branch/{bp}", web.base),
    };
    out.push(WebLink { kind: "branch".into(), label: format!("Open {b} on {site}"), url: branch_url });
    if default == Some(b) {
        return out;
    }
    let dq = default.map(|d| encode(d, false));
    let dp = default.map(|d| encode(d, true));
    let (noun, url) = match web.forge {
        Forge::GitHub | Forge::Gitea => (
            "pull request",
            match &dp {
                Some(d) => format!("{}/compare/{d}...{bp}{}", web.base, if web.forge == Forge::GitHub { "?expand=1" } else { "" }),
                None => format!("{}/compare/{bp}", web.base),
            },
        ),
        Forge::GitLab => (
            "merge request",
            format!(
                "{}/-/merge_requests/new?merge_request%5Bsource_branch%5D={bq}{}",
                web.base,
                dq.as_ref().map(|d| format!("&merge_request%5Btarget_branch%5D={d}")).unwrap_or_default()
            ),
        ),
        Forge::Bitbucket => (
            "pull request",
            format!("{}/pull-requests/new?source={bq}{}", web.base, dq.as_ref().map(|d| format!("&dest={d}")).unwrap_or_default()),
        ),
        Forge::Azure => (
            "pull request",
            format!(
                "{}/pullrequestcreate?sourceRef={bq}{}",
                web.base,
                dq.as_ref().map(|d| format!("&targetRef={d}")).unwrap_or_default()
            ),
        ),
    };
    out.push(WebLink { kind: "pr".into(), label: format!("Create {noun} on {site}"), url });
    out
}


/// The web links for a remote URL (none for a host that isn't recognised):
/// the repo, `branch`'s page once it's on that remote, and a new pull request
/// for it unless it's `default`.
pub(crate) fn web_links(url: &str, branch: Option<&str>, default: Option<&str>) -> Vec<WebLink> {
    web_repo(url).map(|w| links_for(&w, branch, default)).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    /// Remote names (Git, Jujutsu, Mercurial paths) or locations (the
    /// Subversion repository, Fossil's remote, Breezy's parent).
    pub remotes: Vec<String>,
    /// The remote's default branch (`main`), when it's known locally.
    pub default_branch: Option<String>,
    pub links: Vec<WebLink>,
    /// Where a pull comes from, when the system remembers it (Breezy's parent).
    pub pull_from: Option<String>,
    /// Where a push goes, when the system remembers it (Breezy's push location).
    pub push_to: Option<String>,
}

impl Details {
    pub(crate) fn remotes_only(remotes: Vec<String>) -> Self {
        Details { remotes, default_branch: None, links: Vec::new(), pull_from: None, push_to: None }
    }
}

// --- choosing the system ------------------------------------------------------

fn system(dir: &Path) -> Result<&'static str, String> {
    vcs_state::detect(dir).ok_or_else(|| "This folder isn't under version control.".to_string())
}

/// Systems where switching merges uncommitted changes into the other branch
/// (and can leave conflicts) rather than refusing as Git does when one would
/// be overwritten. Jujutsu never carries anything: the working copy is a commit.
fn switch_merges_changes(label: &str) -> bool {
    matches!(label, "Mercurial" | "Subversion" | "Fossil")
}

/// Why a switch can't happen here right now, if it can't: an operation left
/// half-done, unresolved conflicts, or (where switching would merge them)
/// uncommitted changes.
fn ready_to_switch(dir: &Path, label: &str) -> Result<(), String> {
    let s = vcs_state::read(dir);
    if let Some(op) = &s.operation {
        return Err(match op.as_str() {
            "stale" => "This workspace is stale — run `jj workspace update-stale` in it first.".into(),
            "cleanup" => "An interrupted command left this working copy locked — run `svn cleanup` first.".into(),
            _ => format!("A {op} is in progress here — finish or abort it first."),
        });
    }
    if s.conflicted > 0 && label != "Jujutsu" {
        return Err(format!("{} with unresolved conflicts — resolve them first.", plural(s.conflicted, "file")));
    }
    let dirty = s.changed + s.conflicted;
    if switch_merges_changes(label) && dirty > 0 {
        return Err(format!(
            "Commit or shelve the {} first — {label} would merge {} into the other branch.",
            plural(dirty, "uncommitted change"),
            if dirty == 1 { "it" } else { "them" }
        ));
    }
    Ok(())
}

pub(crate) fn plural(n: u32, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// Branches (bookmarks), remote ones nothing local tracks yet, and other
/// working copies — for the branch switcher.
pub fn branches(dir: &Path) -> Result<BranchList, String> {
    match system(dir)? {
        "Git" => crate::git_ops::branches(dir),
        "Jujutsu" => crate::jj_ops::branches(dir),
        "Mercurial" => crate::hg_ops::branches(dir),
        "Subversion" => crate::svn_ops::branches(dir),
        "Fossil" => crate::fossil_ops::branches(dir),
        other => Err(format!("{other} branches are folders of their own here; each is listed as a repo.")),
    }
}

/// Check `target` out here. Returns what happened.
pub fn switch(dir: &Path, target: &Target) -> Result<String, String> {
    let label = system(dir)?;
    ready_to_switch(dir, label)?;
    match label {
        "Git" => crate::git_ops::switch(dir, target),
        "Jujutsu" => crate::jj_ops::switch(dir, target),
        "Mercurial" => crate::hg_ops::switch(dir, target),
        "Subversion" => crate::svn_ops::switch(dir, target),
        "Fossil" => crate::fossil_ops::switch(dir, target),
        other => Err(format!("{other} has no branch switcher here.")),
    }
}

/// Check `target` out in a second working copy beside this one. Returns its
/// folder.
pub fn add_copy(dir: &Path, target: &Target) -> Result<PathBuf, String> {
    match system(dir)? {
        "Git" => crate::git_ops::add_worktree(dir, target),
        "Jujutsu" => crate::jj_ops::add_workspace(dir, target),
        "Mercurial" => crate::hg_ops::add_share(dir, target),
        "Subversion" => crate::svn_ops::add_checkout(dir, target),
        "Fossil" => crate::fossil_ops::add_checkout(dir, target),
        other => Err(format!("{other} can't make a second working copy from here.")),
    }
}

pub fn sync(dir: &Path, op: SyncOp, remote: Option<&str>) -> Result<String, String> {
    match system(dir)? {
        "Git" => crate::git_ops::sync(dir, op, remote),
        "Jujutsu" => crate::jj_ops::sync(dir, op, remote),
        "Mercurial" => crate::hg_ops::sync(dir, op),
        "Subversion" => crate::svn_ops::sync(dir, op),
        "Fossil" => crate::fossil_ops::sync(dir, op),
        "Breezy" => crate::brz_ops::sync(dir, op),
        other => Err(format!("{other} can't do that from here.")),
    }
}

/// What the action menu needs beyond the status. Local reads only.
pub fn details(dir: &Path, s: &RepoStatus) -> Result<Details, String> {
    match system(dir)? {
        "Git" => crate::git_ops::details(dir, s),
        "Jujutsu" => crate::jj_ops::details(dir, s),
        "Mercurial" => crate::hg_ops::details(dir),
        "Subversion" => crate::svn_ops::details(dir),
        "Fossil" => crate::fossil_ops::details(dir),
        "Breezy" => crate::brz_ops::details(dir),
        _ => Ok(Details::remotes_only(Vec::new())),
    }
}

/// Where a second working copy for `name` goes: beside `main` (the repo's
/// main working copy), named after both, with a number added if that's taken.
pub(crate) fn copy_path(main: &Path, name: &str) -> Option<PathBuf> {
    let parent = main.parent()?;
    let base = worktree_folder_name(&main.file_name()?.to_string_lossy(), name);
    (1..100)
        .map(|n| parent.join(if n == 1 { base.clone() } else { format!("{base}-{n}") }))
        .find(|p| p.symlink_metadata().is_err())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_urls_become_web_addresses() {
        let base = |u: &str| web_repo(u).map(|w| (w.label, w.base));
        let gh = Some(("GitHub".to_string(), "https://github.com/seraphx2/dev-prompt".to_string()));
        assert_eq!(base("https://github.com/seraphx2/dev-prompt.git"), gh);
        assert_eq!(base("https://user@github.com/seraphx2/dev-prompt/"), gh);
        assert_eq!(base("git@github.com:seraphx2/dev-prompt.git"), gh);
        assert_eq!(base("ssh://git@github.com:22/seraphx2/dev-prompt.git"), gh);
        assert_eq!(
            base("git@gitlab.com:group/sub/proj.git"),
            Some(("GitLab".into(), "https://gitlab.com/group/sub/proj".into()))
        );
        assert_eq!(base("https://git.example.com/a/b.git"), None, "an unrecognised host gets no links");
        assert_eq!(
            base("https://gitlab.corp.io:8443/a/b.git"),
            Some(("GitLab".into(), "https://gitlab.corp.io:8443/a/b".into()))
        );
        assert_eq!(
            base("git@ssh.dev.azure.com:v3/org/proj/repo"),
            Some(("Azure DevOps".into(), "https://dev.azure.com/org/proj/_git/repo".into()))
        );
        assert_eq!(
            base("https://org@dev.azure.com/org/proj/_git/repo"),
            Some(("Azure DevOps".into(), "https://dev.azure.com/org/proj/_git/repo".into()))
        );
        assert_eq!(base("git@codeberg.org:me/thing.git").map(|b| b.0), Some("Codeberg".into()));
        assert_eq!(base("https://bitbucket.org/team/repo.git").map(|b| b.0), Some("Bitbucket".into()));
        assert_eq!(base(r"C:\remotes\x.git"), None);
        assert_eq!(base("/srv/git/x.git"), None);
        assert_eq!(base("../x.git"), None);
        assert_eq!(base("file:///srv/git/x.git"), None);
    }

    #[test]
    fn links_cover_repo_branch_and_a_new_pull_request() {
        let w = web_repo("git@github.com:o/r.git").unwrap();
        let l = links_for(&w, Some("feature/a b"), Some("main"));
        let urls: Vec<&str> = l.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://github.com/o/r",
                "https://github.com/o/r/tree/feature/a%20b",
                "https://github.com/o/r/compare/main...feature/a%20b?expand=1",
            ]
        );
        assert_eq!(l[2].label, "Create pull request on GitHub");
        // The default branch itself, or a branch not on the remote: no PR.
        assert_eq!(links_for(&w, Some("main"), Some("main")).len(), 2);
        assert_eq!(links_for(&w, None, Some("main")).len(), 1);

        let gl = links_for(&web_repo("git@gitlab.com:g/p.git").unwrap(), Some("x/y"), Some("main"));
        assert_eq!(
            gl[2].url,
            "https://gitlab.com/g/p/-/merge_requests/new?merge_request%5Bsource_branch%5D=x%2Fy&merge_request%5Btarget_branch%5D=main"
        );
        assert_eq!(gl[2].label, "Create merge request on GitLab");
        let az = links_for(&web_repo("git@ssh.dev.azure.com:v3/o/p/r").unwrap(), Some("x/y"), None);
        assert_eq!(az[1].url, "https://dev.azure.com/o/p/_git/r?version=GBx%2Fy");
        assert_eq!(az[2].url, "https://dev.azure.com/o/p/_git/r/pullrequestcreate?sourceRef=x%2Fy");
    }
}
