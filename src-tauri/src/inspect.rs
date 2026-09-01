//! Milestone 2 — contextual inspection engine.
//!
//! Reads a repo's directory tree (root + one level of side-by-side sub-projects,
//! plus a level inside `packages/` / `apps/` / … container dirs) and reports the
//! project markers found (`*.sln`, `package.json` + scripts, `Cargo.toml`,
//! `go.mod`, Python), plus repo-level `docker-compose`. Pure file reads — no
//! OS-specific calls — so the result is identical on every platform; turning the
//! context into runnable actions is `actions.rs`'s job.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use globset::GlobSet;
use serde::{Deserialize, Serialize};

/// Cap on discovered sub-projects, to keep the action list bounded.
const MAX_SUBPROJECTS: usize = 12;

/// Directory names never treated as (or descended into for) sub-projects.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "bin",
    "obj",
    "vendor",
    "venv",
    ".venv",
    "__pycache__",
    "coverage",
    ".git",
];

/// Directory names whose *children* are scanned one extra level for projects.
const CONTAINER_DIRS: &[&str] =
    &["packages", "apps", "services", "crates", "libs", "modules", "projects"];

const COMPOSE_FILES: &[&str] = &[
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
];

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoContext {
    /// `projects[0]` is always the repo root (`rel == ""`); the rest are
    /// sub-projects, sorted by their relative path.
    pub projects: Vec<Project>,
    /// A `docker-compose` / `compose` file sits at the repo root.
    pub compose: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// "" for the repo root, else the relative dir (`"web"`, `"packages/ui"`).
    pub rel: String,
    /// Absolute path to this project's directory.
    pub dir: PathBuf,
    /// Top-level entry names in this directory (for rule glob matching).
    /// Persisted in the repo cache so rules re-evaluate without a fresh walk.
    #[serde(default)]
    pub files: Vec<String>,
    pub solutions: Vec<PathBuf>,
    pub node: Option<NodeInfo>,
    pub has_cargo: bool,
    pub has_go_mod: bool,
    pub python: Option<PythonInfo>,
}

impl Project {
    /// Is this directory worth surfacing as its own sub-project? True for the
    /// ecosystems inspect knows by name (below — these feed typed provider
    /// data, e.g. `node.scripts`), OR when a top-level file matches `discovery`
    /// — the same marker-and-rule glob set `scan.rs` uses to find repos, so any
    /// rule you add is automatically enough to surface a matching sub-project
    /// too, with nothing to keep in sync by hand.
    pub fn has_any_marker(&self, discovery: &GlobSet) -> bool {
        !self.solutions.is_empty()
            || self.node.is_some()
            || self.has_cargo
            || self.has_go_mod
            || self.python.is_some()
            || self
                .files
                .iter()
                .any(|f| discovery.is_match(f.to_lowercase()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub manager: PkgManager,
    /// `scripts` keys from `package.json`, in file order.
    pub scripts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PkgManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PkgManager {
    /// `"npm"` / `"pnpm"` / `"yarn"` / `"bun"` — the `npm-scripts` provider uses
    /// this both as the run command and the action label.
    pub fn label(self) -> &'static str {
        match self {
            PkgManager::Npm => "npm",
            PkgManager::Pnpm => "pnpm",
            PkgManager::Yarn => "yarn",
            PkgManager::Bun => "bun",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PythonInfo {
    /// `"uv"` / `"poetry"` / `"pipenv"` / `"pdm"` from a lockfile / `Pipfile`,
    /// else `None` (plain `pip`).
    pub runner: Option<String>,
    /// A `requirements.txt` sits in this project.
    pub requirements: bool,
    /// `.venv` / `venv` directory name, when one exists here — its interpreter
    /// is preferred over a bare `python` for this project's actions.
    pub venv: Option<String>,
    /// A Django `manage.py` sits in this project.
    pub manage_py: bool,
    /// `main.py` / `app.py` / `__main__.py` at the project root, first found.
    pub entry: Option<String>,
    /// `conftest.py` / `pytest.ini` / `tox.ini` / a `tests/` dir — offer pytest.
    pub pytest: bool,
}

fn has_ext_ignore_case(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(ext))
        .unwrap_or(false)
}

fn is_dir(entry: &std::fs::DirEntry) -> bool {
    entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
}

/// Inspect a single directory (no recursion) for project markers.
fn inspect_dir(dir: &Path, rel: &str) -> Project {
    let mut p = Project {
        rel: rel.to_string(),
        dir: dir.to_path_buf(),
        ..Default::default()
    };

    let Ok(entries) = std::fs::read_dir(dir) else {
        return p;
    };
    let mut names: Vec<String> = Vec::new();
    for ent in entries.flatten() {
        if let Some(name) = ent.file_name().to_str() {
            if has_ext_ignore_case(Path::new(name), "sln")
                || has_ext_ignore_case(Path::new(name), "slnx")
            {
                p.solutions.push(dir.join(name));
            }
            names.push(name.to_string());
        }
    }
    p.solutions.sort();
    names.sort();
    p.files = names.clone();
    let has = |n: &str| names.iter().any(|x| x == n);

    if has("package.json") {
        let manager = if has("bun.lockb") || has("bun.lock") {
            PkgManager::Bun
        } else if has("pnpm-lock.yaml") {
            PkgManager::Pnpm
        } else if has("yarn.lock") {
            PkgManager::Yarn
        } else {
            PkgManager::Npm
        };
        p.node = Some(NodeInfo {
            manager,
            scripts: read_package_scripts(&dir.join("package.json")),
        });
    }

    p.has_cargo = has("Cargo.toml");
    p.has_go_mod = has("go.mod");

    let requirements = has("requirements.txt");
    let is_python = has("pyproject.toml")
        || requirements
        || has("setup.py")
        || has("setup.cfg")
        || has("Pipfile")
        || has("manage.py")
        || has(".venv")
        || has("venv");
    if is_python {
        let runner = if has("uv.lock") {
            Some("uv".to_string())
        } else if has("poetry.lock") {
            Some("poetry".to_string())
        } else if has("Pipfile.lock") || has("Pipfile") {
            Some("pipenv".to_string())
        } else if has("pdm.lock") {
            Some("pdm".to_string())
        } else {
            None
        };
        let venv = if has(".venv") {
            Some(".venv".to_string())
        } else if has("venv") {
            Some("venv".to_string())
        } else {
            None
        };
        let entry = ["main.py", "app.py", "__main__.py"]
            .into_iter()
            .find(|f| has(f))
            .map(str::to_string);
        p.python = Some(PythonInfo {
            runner,
            requirements,
            venv,
            manage_py: has("manage.py"),
            entry,
            pytest: has("conftest.py")
                || has("pytest.ini")
                || has("tox.ini")
                || has("tests"),
        });
    }

    p
}

/// Inspect a repo: root project + side-by-side sub-projects. A missing /
/// unreadable root still yields one (empty) root project. `discovery` is the
/// compiled marker-and-rule glob set (`config::discovery_globs`) — see
/// [`Project::has_any_marker`].
pub fn inspect(root: &Path, discovery: &GlobSet) -> RepoContext {
    let mut ctx = RepoContext::default();
    ctx.projects.push(inspect_dir(root, ""));
    ctx.compose = COMPOSE_FILES.iter().any(|f| root.join(f).is_file());

    let Ok(entries) = std::fs::read_dir(root) else {
        return ctx;
    };

    let mut subs: Vec<Project> = Vec::new();
    for ent in entries.flatten() {
        if subs.len() >= MAX_SUBPROJECTS {
            break;
        }
        if !is_dir(&ent) {
            continue;
        }
        let name = ent.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') || SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            continue;
        }
        let dir = ent.path();

        let proj = inspect_dir(&dir, name);
        if proj.has_any_marker(discovery) {
            subs.push(proj);
            continue;
        }

        // `packages/*`, `apps/*`, … — look one level deeper.
        if CONTAINER_DIRS.contains(&name) {
            let Ok(children) = std::fs::read_dir(&dir) else {
                continue;
            };
            for child in children.flatten() {
                if subs.len() >= MAX_SUBPROJECTS {
                    break;
                }
                if !is_dir(&child) {
                    continue;
                }
                let Some(cname) = child.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if cname.starts_with('.') {
                    continue;
                }
                let cp = inspect_dir(&child.path(), &format!("{name}/{cname}"));
                if cp.has_any_marker(discovery) {
                    subs.push(cp);
                }
            }
        }
    }

    subs.sort_by(|a, b| a.rel.cmp(&b.rel));
    ctx.projects.extend(subs);
    ctx
}

/// Inspect every repo path, producing the `path -> context` map that is cached
/// next to the repo list. Runs on the caller's thread — the scan already calls
/// this from a blocking task, so the extra walks stay off the UI thread.
pub fn inspect_all<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    discovery: &GlobSet,
) -> HashMap<String, RepoContext> {
    paths
        .into_iter()
        .map(|p| (p.to_string(), inspect(Path::new(p), discovery)))
        .collect()
}

fn read_package_scripts(path: &Path) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct PkgJson {
        #[serde(default)]
        scripts: serde_json::Map<String, serde_json::Value>,
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<PkgJson>(&text) {
        Ok(pkg) => pkg.scripts.keys().cloned().collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globset::{Glob, GlobSetBuilder};
    use std::fs;

    /// Compile a discovery globset for tests, same shape as
    /// `config::discovery_globs` + `config::compile_globset`.
    fn gs(patterns: &[&str]) -> GlobSet {
        let mut b = GlobSetBuilder::new();
        for p in patterns {
            b.add(Glob::new(&p.to_lowercase()).unwrap());
        }
        b.build().unwrap()
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "dp-inspect-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(path: PathBuf, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn root_project_detects_markers() {
        let d = scratch("root");
        write(d.join("Foo.SLN"), "");
        write(d.join("Cargo.toml"), "");
        write(d.join("requirements.txt"), "");
        write(d.join("uv.lock"), "");
        write(d.join("pyproject.toml"), "");
        let ctx = inspect(&d, &gs(&[]));
        let root = &ctx.projects[0];
        assert_eq!(root.rel, "");
        assert_eq!(root.solutions.len(), 1);
        assert!(root.has_cargo);
        let py = root.python.as_ref().unwrap();
        assert_eq!(py.runner.as_deref(), Some("uv"));
        assert!(py.requirements);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn python_detects_venv_django_entry_and_pytest() {
        let d = scratch("py");
        write(d.join("requirements.txt"), "");
        write(d.join("manage.py"), "");
        write(d.join("main.py"), "");
        write(d.join("conftest.py"), "");
        fs::create_dir_all(d.join(".venv")).unwrap();

        let py = inspect(&d, &gs(&[]))
            .projects
            .remove(0)
            .python
            .expect("requirements.txt makes it a python project");
        assert!(py.requirements);
        assert_eq!(py.venv.as_deref(), Some(".venv"));
        assert!(py.manage_py);
        assert_eq!(py.entry.as_deref(), Some("main.py"));
        assert!(py.pytest);
        assert_eq!(py.runner, None); // no lockfile / Pipfile

        // A Pipfile alone is enough, and picks the pipenv runner.
        let d2 = scratch("py-pipenv");
        write(d2.join("Pipfile"), "");
        let py2 = inspect(&d2, &gs(&[]))
            .projects
            .remove(0)
            .python
            .expect("Pipfile makes it a python project");
        assert_eq!(py2.runner.as_deref(), Some("pipenv"));

        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&d2);
    }

    #[test]
    fn node_scripts_parsed_in_order_with_pnpm() {
        let d = scratch("node");
        write(
            d.join("package.json"),
            r#"{ "scripts": { "dev": "vite", "build": "vite build", "check": "tsc" } }"#,
        );
        write(d.join("pnpm-lock.yaml"), "");
        let node = inspect(&d, &gs(&[])).projects.remove(0).node.unwrap();
        assert_eq!(node.manager, PkgManager::Pnpm);
        assert_eq!(node.scripts, vec!["dev", "build", "check"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn discovers_side_by_side_and_container_subprojects() {
        let d = scratch("multi");
        write(d.join("package.json"), r#"{ "scripts": { "dev": "x" } }"#);
        write(d.join("web/package.json"), r#"{ "scripts": { "build": "x" } }"#);
        write(d.join("packages/ui/package.json"), r#"{ "scripts": { "t": "x" } }"#);
        write(d.join("node_modules/junk/package.json"), r#"{ "scripts": {} }"#);
        write(d.join("docs/readme.md"), "no project here");
        write(d.join("docker-compose.yml"), "services: {}");
        let ctx = inspect(&d, &gs(&[]));
        let rels: Vec<&str> = ctx.projects.iter().map(|p| p.rel.as_str()).collect();
        assert_eq!(rels, vec!["", "packages/ui", "web"]);
        assert!(ctx.compose);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn discovers_subproject_via_generic_discovery_glob() {
        // `pom.xml` isn't one of the typed markers (no `has_maven` field) — it
        // only surfaces `backend/` as a sub-project through the discovery
        // globset, same one `scan.rs` uses to find repos in the first place.
        let d = scratch("generic");
        write(d.join("backend/pom.xml"), "<project/>");

        assert_eq!(inspect(&d, &gs(&[])).projects.len(), 1); // root only

        let ctx = inspect(&d, &gs(&["pom.xml"]));
        let rels: Vec<&str> = ctx.projects.iter().map(|p| p.rel.as_str()).collect();
        assert_eq!(rels, vec!["", "backend"]);

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn inspect_all_keys_by_path_and_round_trips_through_json() {
        let d = scratch("all");
        write(d.join("Cargo.toml"), "");
        let key = d.to_string_lossy().into_owned();
        let map = inspect_all([key.as_str()], &gs(&[]));
        assert!(map[&key].projects[0].has_cargo);
        assert!(map[&key].projects[0].files.iter().any(|f| f == "Cargo.toml"));

        // The cache persists this map as JSON — it must survive the round trip
        // (in particular `files`, which used to be `#[serde(skip)]`).
        let json = serde_json::to_string(&map).unwrap();
        let back: HashMap<String, RepoContext> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, map);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_dir_yields_lone_empty_root() {
        let ctx = inspect(Path::new("/no/such/path/hopefully"), &gs(&[]));
        assert_eq!(ctx.projects.len(), 1);
        assert!(!ctx.projects[0].has_any_marker(&gs(&[])));
        assert!(!ctx.compose);
    }
}
