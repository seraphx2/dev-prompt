//! "Clone repository": a URL (or GitHub's `owner/repo`) typed or pasted into
//! the repo search opens the New repository form in clone mode, which clones
//! it into one of the roots with the tool that can read it.
//!
//! What's at the URL is told from its shape where it can be (a `.git` suffix,
//! `svn://`, a known host); otherwise each installed tool is asked in turn.
//! Same contract as provision.rs: nothing interactive (stdin closed, no
//! credential prompts in a terminal nobody sees), output captured for the
//! form, and a failed clone takes its folder with it.

use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::time::Duration;

use serde::Serialize;

use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::provision::{self, ToolView};
use crate::scan::{self, Repo};

/// A big repository over a slow link takes minutes; past this it's stuck.
const CLONE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Asking a tool whether it can read the URL, while the form waits.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// Never stop to ask for a password on a terminal there isn't: Git fails
/// instead (its credential manager's own window still works), Mercurial
/// answers like a script.
const ENV: &[(&str, &str)] = &[("GIT_TERMINAL_PROMPT", "0"), ("HGPLAIN", "1")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Git,
    Hg,
    Svn,
    Fossil,
    Brz,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Git => "Git",
            Kind::Hg => "Mercurial",
            Kind::Svn => "Subversion",
            Kind::Fossil => "Fossil",
            Kind::Brz => "Breezy",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClonePlan {
    /// What will be cloned (`owner/repo` spelled out).
    pub url: String,
    /// The folder name the URL suggests.
    pub name: String,
    /// The installed tools that can clone it, the usual one first. `run` is
    /// the commands with `{{dir}}` for the new folder, for the preview.
    pub tools: Vec<ToolView>,
}

/// GitHub's `owner/repo` shorthand, as a URL; anything else as typed.
pub fn normalize(src: &str) -> String {
    let src = src.trim();
    let part = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    match src.split_once('/') {
        Some((owner, repo)) if part(owner) && part(repo) && !owner.starts_with('.') => {
            format!("https://github.com/{owner}/{repo}")
        }
        _ => src.to_string(),
    }
}

/// The folder name a URL suggests: its last path part, without `.git` /
/// `.fossil`; for Subversion's `…/project/trunk`, the project.
pub fn name_for(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url).trim_end_matches('/');
    let mut parts = path.rsplit(['/', ':', '\\']).filter(|p| !p.is_empty());
    let mut name = parts.next().unwrap_or("");
    if name.eq_ignore_ascii_case("trunk") {
        name = parts.next().unwrap_or(name);
    }
    let name = name
        .strip_suffix(".git")
        .or_else(|| name.strip_suffix(".fossil"))
        .unwrap_or(name)
        .trim_start_matches('~');
    if name.is_empty() { "repository".into() } else { name.into() }
}

/// What the URL's shape says it is, when it says.
fn kind_from_url(url: &str) -> Option<Kind> {
    let u = url.to_ascii_lowercase();
    let host = u
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&u)
        .split(['/', ':'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("");
    let path = u.split(['?', '#']).next().unwrap_or(&u).trim_end_matches('/');
    if u.starts_with("svn://") || u.starts_with("svn+ssh://") {
        Some(Kind::Svn)
    } else if u.starts_with("bzr://") || u.starts_with("bzr+ssh://") || u.starts_with("lp:") {
        Some(Kind::Brz)
    } else if u.starts_with("git://") || u.starts_with("git@") || path.ends_with(".git") {
        Some(Kind::Git)
    } else if path.ends_with(".fossil") || matches!(host, "chiselapp.com" | "fossil-scm.org" | "www.fossil-scm.org") {
        Some(Kind::Fossil)
    } else if matches!(host, "hg.sr.ht" | "hg.mozilla.org") {
        Some(Kind::Hg)
    } else if matches!(
        host,
        "github.com" | "gitlab.com" | "bitbucket.org" | "codeberg.org" | "gitea.com" | "git.sr.ht"
            | "dev.azure.com" | "ssh.dev.azure.com"
    ) || host.ends_with(".visualstudio.com")
    {
        Some(Kind::Git)
    } else {
        None
    }
}

/// Ask each installed tool whether it can read `url`, cheapest question first.
fn probe(url: &str) -> Option<Kind> {
    let home = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    let asks: [(Kind, &str, Vec<&str>); 4] = [
        (Kind::Git, "git", vec!["ls-remote", "--", url, "HEAD"]),
        (Kind::Hg, "hg", vec!["identify", "--noninteractive", url]),
        (Kind::Svn, "svn", vec!["info", "--non-interactive", url]),
        (Kind::Brz, "brz", vec!["info", url]),
    ];
    asks.into_iter().find_map(|(kind, tool, args)| {
        let program = provision::resolve_program(tool)?;
        let args: Vec<String> = args.into_iter().map(String::from).collect();
        provision::run_captured_env(&program, &args, &home, PROBE_TIMEOUT, ENV)
            .ok()
            .map(|_| kind)
    })
}

/// `(id, label, commands)` for each tool that clones `kind`, the usual first.
/// `{{dir}}` is the new folder (absolute — Breezy on Windows needs it so).
fn recipes(kind: Kind, url: &str) -> Vec<(&'static str, &'static str, Vec<Vec<String>>)> {
    let v = |args: &[&str]| args.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    let dir = "{{dir}}";
    match kind {
        Kind::Git => vec![
            ("git", "Git", vec![v(&["git", "clone", "--", url, dir])]),
            // Colocated, so Git tools keep working alongside.
            ("jj", "Jujutsu", vec![v(&["jj", "git", "clone", "--colocate", url, dir])]),
        ],
        Kind::Hg => vec![("hg", "Mercurial", vec![v(&["hg", "clone", "--noninteractive", url, dir])])],
        Kind::Svn => vec![("svn", "Subversion", vec![v(&["svn", "checkout", "--non-interactive", url, dir])])],
        Kind::Brz => vec![("brz", "Breezy", vec![v(&["brz", "branch", url, dir])])],
        // The repository file lives inside the checkout, where Fossil leaves
        // it out of the working files.
        Kind::Fossil => {
            let file = format!("{dir}{MAIN_SEPARATOR}.fossil");
            vec![(
                "fossil",
                "Fossil",
                vec![v(&["fossil", "clone", url, &file]), v(&["fossil", "open", &file, "--workdir", dir])],
            )]
        }
    }
}

/// What `src` is and which installed tools can clone it. May ask the network
/// (each tool in turn) when the URL doesn't say — call it off the UI thread.
pub fn plan(src: &str) -> AppResult<ClonePlan> {
    let url = normalize(src);
    let kind = kind_from_url(&url).or_else(|| probe(&url)).ok_or_else(|| {
        AppError::msg(format!(
            "Couldn't read {url} as a repository. Check the address, and that you can reach \
             it — Git, Mercurial, Subversion and Breezy were asked, whichever are installed."
        ))
    })?;
    let tools: Vec<ToolView> = recipes(kind, &url)
        .into_iter()
        .filter(|(id, _, _)| provision::resolve_program(id).is_some())
        .map(|(id, label, run)| ToolView { id: id.into(), label: label.into(), fields: Vec::new(), run })
        .collect();
    if tools.is_empty() {
        return Err(AppError::msg(format!(
            "That's a {} repository, and {} isn't installed.",
            kind.label(),
            recipes(kind, &url)[0].1
        )));
    }
    Ok(ClonePlan { name: name_for(&url), url, tools })
}

/// Clone `url` into `<root>/<name>` with `tool` and return the repo as the
/// scan sees it. If it fails, the folder goes again.
pub fn clone(cfg: &Config, root: &str, name: &str, url: &str, tool: &str) -> AppResult<Repo> {
    let dir = provision::check_target(cfg, root, name).map_err(AppError::msg)?;
    let kind = kind_from_url(url)
        .or_else(|| [Kind::Git, Kind::Hg, Kind::Svn, Kind::Fossil, Kind::Brz]
            .into_iter()
            .find(|k| recipes(*k, url).iter().any(|(id, _, _)| *id == tool)))
        .ok_or_else(|| AppError::msg(format!("Can't clone with \"{tool}\".")))?;
    let (_, label, commands) = recipes(kind, url)
        .into_iter()
        .find(|(id, _, _)| *id == tool)
        .ok_or_else(|| AppError::msg(format!("{} can't clone a {} repository.", tool, kind.label())))?;
    let at = dir.to_string_lossy().into_owned();
    if kind == Kind::Fossil {
        std::fs::create_dir(&dir).map_err(|e| AppError::msg(format!("Couldn't create {at}: {e}")))?;
    }
    let root_dir = PathBuf::from(root);
    for argv in &commands {
        let argv: Vec<String> = argv.iter().map(|a| a.replace("{{dir}}", &at)).collect();
        let program = provision::resolve_program(&argv[0])
            .ok_or_else(|| AppError::msg(format!("`{}` isn't on PATH.", argv[0])))?;
        if let Err(e) = provision::run_captured_env(&program, &argv[1..], &root_dir, CLONE_TIMEOUT, ENV) {
            let cleanup = remove(&dir);
            return Err(AppError::msg(format!(
                "`{}` failed: {e}{cleanup}",
                provision::display_command(&argv)
            )));
        }
    }
    match scan::probe(&dir, cfg) {
        Some(repo) if repo.vcs.is_some() => Ok(repo),
        _ => Err(AppError::msg(format!(
            "{label} finished, but the folder isn't a checkout dev-prompt recognizes. It was kept: {at}"
        ))),
    }
}

/// Remove what a failed clone left; any extra text is for the error.
fn remove(dir: &Path) -> String {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => String::new(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => format!("\n\nThe folder couldn't be removed ({e}): {}", dir.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorthand_means_github_and_urls_stay_as_typed() {
        assert_eq!(normalize(" seraphx2/dev-prompt "), "https://github.com/seraphx2/dev-prompt");
        assert_eq!(normalize("https://example.com/x/y"), "https://example.com/x/y");
        assert_eq!(normalize("git@github.com:a/b.git"), "git@github.com:a/b.git");
        assert_eq!(normalize("../sibling"), "../sibling");
    }

    #[test]
    fn the_url_names_the_folder() {
        assert_eq!(name_for("https://github.com/seraphx2/dev-prompt.git"), "dev-prompt");
        assert_eq!(name_for("git@github.com:seraphx2/dev-prompt.git"), "dev-prompt");
        assert_eq!(name_for("https://gitlab.com/group/sub/proj/"), "proj");
        assert_eq!(name_for("svn://svn.example.org/project/trunk"), "project");
        assert_eq!(name_for("https://chiselapp.com/user/x/repository/thing.fossil"), "thing");
        assert_eq!(name_for("lp:bzr"), "bzr");
        assert_eq!(name_for("https://hg.example.org/repo?branch=x"), "repo");
    }

    #[test]
    fn the_url_says_what_it_is_where_it_can() {
        assert_eq!(kind_from_url("https://github.com/a/b"), Some(Kind::Git));
        assert_eq!(kind_from_url("git@gitlab.com:a/b.git"), Some(Kind::Git));
        assert_eq!(kind_from_url("https://example.com/a/b.git"), Some(Kind::Git));
        assert_eq!(kind_from_url("https://user@dev.azure.com/org/p/_git/r"), Some(Kind::Git));
        assert_eq!(kind_from_url("svn+ssh://host/repo/trunk"), Some(Kind::Svn));
        assert_eq!(kind_from_url("https://hg.mozilla.org/mozilla-central"), Some(Kind::Hg));
        assert_eq!(kind_from_url("https://chiselapp.com/user/x/repository/y"), Some(Kind::Fossil));
        assert_eq!(kind_from_url("lp:bzr"), Some(Kind::Brz));
        assert_eq!(kind_from_url("https://example.com/some/repo"), None);
    }

    /// End to end against a local Git repository, when Git is installed: the
    /// plan asks Git (a plain path says nothing), the clone lands in the root,
    /// and a second clone to the same name is refused before running.
    #[test]
    fn clones_a_local_git_repository() {
        if provision::resolve_program("git").is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!(
            "dp-clone-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let (src, root) = (base.join("src"), base.join("root"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            let ok = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
            assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
        };
        git(&src, &["init", "-q", "-b", "main"]);
        std::fs::write(src.join("a.txt"), "hi").unwrap();
        git(&src, &["add", "."]);
        git(&src, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "first"]);

        let url = src.to_string_lossy().into_owned();
        let p = plan(&url).unwrap();
        assert_eq!(p.name, "src");
        assert_eq!(p.tools[0].id, "git");

        let mut cfg = crate::config::bundled_defaults();
        cfg.roots = vec![root.to_string_lossy().into_owned()];
        let r = root.to_string_lossy().into_owned();
        let repo = clone(&cfg, &r, "copy", &url, "git").unwrap();
        assert!(repo.vcs.is_some());
        assert!(root.join("copy").join("a.txt").is_file());
        assert!(clone(&cfg, &r, "copy", &url, "git").is_err(), "the folder exists now");

        // A failed clone leaves nothing behind (Fossil's makes its folder first).
        if provision::resolve_program("fossil").is_some() {
            let fsl = base.join("origin.fossil");
            let fossil = |dir: &Path, args: &[&str]| {
                let ok = std::process::Command::new("fossil").args(args).current_dir(dir).env("USER", "t").output().unwrap();
                assert!(ok.status.success(), "fossil {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
            };
            fossil(&base, &["init", "-q", "--admin-user", "t", &fsl.to_string_lossy()]);
            let url = fsl.to_string_lossy().into_owned();
            let repo = clone(&cfg, &r, "fsl", &url, "fossil").unwrap();
            assert!(repo.vcs.is_some());
            assert!(root.join("fsl").join(".fossil").is_file());
            let gone = base.join("missing.fossil").to_string_lossy().into_owned();
            assert!(clone(&cfg, &r, "fsl-broken", &gone, "fossil").is_err());
            assert!(!root.join("fsl-broken").exists());
        }
        let gone = base.join("nothing-here").to_string_lossy().into_owned();
        assert!(clone(&cfg, &r, "broken", &gone, "git").is_err());
        assert!(!root.join("broken").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
