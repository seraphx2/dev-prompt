use std::sync::OnceLock;

use serde::Serialize;

use crate::inspect::{Project, RepoContext};
use crate::scan::Repo;

/// Cap on how many `package.json` scripts we surface per project.
const MAX_SCRIPTS: usize = 20;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    pub label: String,
    /// Short hint shown on the row (usually the resolved command).
    pub hint: String,
    /// Section header to show above this action; "" means "just a divider".
    pub group: String,
    /// Enter on a repo runs the action flagged `default` (falls back to first).
    pub default: bool,
    /// Program to spawn. Ignored when `client_side` is true.
    #[serde(skip)]
    pub program: String,
    /// Argument templates; `{{path}}` / `{{file}}` are substituted at launch.
    #[serde(skip)]
    pub args: Vec<String>,
    /// Working directory template; `None` => the repo root. `{{path}}` expands.
    #[serde(skip)]
    pub cwd: Option<String>,
    /// Handled entirely in the frontend (e.g. copy path to clipboard).
    pub client_side: bool,
}

impl Action {
    fn spawn(id: &str, label: &str, hint: &str, program: &str, args: &[&str]) -> Self {
        Action::spawn_owned(
            id,
            label,
            hint,
            program,
            args.iter().map(|s| s.to_string()).collect(),
        )
    }

    fn spawn_owned(
        id: &str,
        label: &str,
        hint: &str,
        program: &str,
        args: Vec<String>,
    ) -> Self {
        Action {
            id: id.into(),
            label: label.into(),
            hint: hint.into(),
            group: "General".into(),
            default: false,
            program: program.into(),
            args,
            cwd: None,
            client_side: false,
        }
    }

    fn client(id: &str, label: &str, hint: &str) -> Self {
        Action {
            id: id.into(),
            label: label.into(),
            hint: hint.into(),
            group: String::new(),
            default: false,
            program: String::new(),
            args: Vec::new(),
            cwd: None,
            client_side: true,
        }
    }

    fn group(mut self, g: &str) -> Self {
        self.group = g.into();
        self
    }

    // Only the non-Windows `terminal_run` path sets an explicit cwd; Windows
    // bakes the directory into `wt.exe -d`.
    #[cfg_attr(windows, allow(dead_code))]
    fn cwd(mut self, c: &str) -> Self {
        self.cwd = Some(c.into());
        self
    }

    fn as_default(mut self) -> Self {
        self.default = true;
        self
    }
}

/// True when `program` is resolvable on the current PATH.
fn on_path(program: &str) -> bool {
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let cand = dir.join(format!("{program}{ext}"));
            if cand.is_file() {
                return true;
            }
        }
    }
    false
}

// --- editor resolution (memoized for the process lifetime) --------------------

fn visual_studio() -> Option<String> {
    static VS: OnceLock<Option<String>> = OnceLock::new();
    VS.get_or_init(detect_visual_studio).clone()
}

fn rider() -> Option<String> {
    static RIDER: OnceLock<Option<String>> = OnceLock::new();
    RIDER.get_or_init(detect_rider).clone()
}

#[cfg(windows)]
fn detect_visual_studio() -> Option<String> {
    // vswhere ships at a fixed location with any VS 2017+ install.
    let mut candidates = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(base) = std::env::var(var) {
            candidates.push(
                std::path::Path::new(&base)
                    .join("Microsoft Visual Studio")
                    .join("Installer")
                    .join("vswhere.exe"),
            );
        }
    }
    let vswhere = candidates.into_iter().find(|p| p.is_file())?;
    let out = std::process::Command::new(vswhere)
        .args([
            "-latest",
            "-prerelease",
            "-products",
            "*",
            "-property",
            "productPath",
            "-utf8",
        ])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty() && std::path::Path::new(&path).is_file()).then_some(path)
}

#[cfg(not(windows))]
fn detect_visual_studio() -> Option<String> {
    None
}

fn detect_rider() -> Option<String> {
    if on_path("rider") {
        return Some("rider".to_string());
    }
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let programs = std::path::Path::new(&local).join("Programs");
            if let Ok(entries) = std::fs::read_dir(&programs) {
                for ent in entries.flatten() {
                    let name = ent.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with("Rider") || name.starts_with("JetBrains Rider") {
                        let exe = ent.path().join("bin").join("rider64.exe");
                        if exe.is_file() {
                            return Some(exe.to_string_lossy().into_owned());
                        }
                    }
                }
            }
            let shim = std::path::Path::new(&local)
                .join("JetBrains")
                .join("Toolbox")
                .join("scripts")
                .join("rider.cmd");
            if shim.is_file() {
                return Some(shim.to_string_lossy().into_owned());
            }
        }
    }
    None
}

// --- action construction -----------------------------------------------------

/// An action that runs `argv` in a terminal at `cwd` (`"{{path}}"` for the repo
/// root, or an absolute sub-project directory).
fn terminal_run(id: &str, label: &str, argv: Vec<String>, cwd: &str) -> Action {
    let hint = argv.join(" ");
    #[cfg(windows)]
    {
        let mut args = vec!["-d".to_string(), cwd.to_string()];
        args.extend(argv);
        Action::spawn_owned(id, label, &hint, "wt.exe", args)
    }
    #[cfg(not(windows))]
    {
        // No terminal-emulator picker yet (its own task) — run it directly in
        // `cwd`. Output isn't shown until Linux terminal support lands.
        let mut it = argv.into_iter();
        let prog = it.next().unwrap_or_default();
        Action::spawn_owned(id, label, &hint, &prog, it.collect()).cwd(cwd)
    }
}

fn project_actions(proj: &Project) -> Vec<Action> {
    let mut out: Vec<Action> = Vec::new();

    let header = if proj.rel.is_empty() {
        "Detected".to_string()
    } else {
        format!("Detected · {}", proj.rel)
    };
    let key = if proj.rel.is_empty() {
        "root".to_string()
    } else {
        proj.rel.replace(['/', '\\'], "-")
    };
    let cwd: String = if proj.rel.is_empty() {
        "{{path}}".into()
    } else {
        proj.dir.to_string_lossy().into_owned()
    };

    for sln in &proj.solutions {
        let name = sln
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("solution")
            .to_string();
        let sln_path = sln.to_string_lossy().into_owned();
        if let Some(devenv) = visual_studio() {
            out.push(
                Action::spawn_owned(
                    &format!("vs:{key}:{name}"),
                    &format!("Open {name} in Visual Studio"),
                    "Visual Studio",
                    &devenv,
                    vec![sln_path.clone()],
                )
                .group(&header),
            );
        }
        if let Some(rider_bin) = rider() {
            out.push(
                Action::spawn_owned(
                    &format!("rider:{key}:{name}"),
                    &format!("Open {name} in Rider"),
                    "Rider",
                    &rider_bin,
                    vec![sln_path.clone()],
                )
                .group(&header),
            );
        }
    }

    if let Some(node) = &proj.node {
        for script in node.scripts.iter().take(MAX_SCRIPTS) {
            out.push(
                terminal_run(
                    &format!("node:{key}:{script}"),
                    &format!("{} {}", node.manager.label(), script),
                    node.manager.run_argv(script),
                    &cwd,
                )
                .group(&header),
            );
        }
    }

    if proj.has_cargo {
        for sub in ["run", "build", "test"] {
            out.push(
                terminal_run(
                    &format!("cargo:{key}:{sub}"),
                    &format!("cargo {sub}"),
                    vec!["cargo".into(), sub.into()],
                    &cwd,
                )
                .group(&header),
            );
        }
    }

    if proj.has_go_mod {
        for (sub, tail) in [("run", "."), ("build", "./..."), ("test", "./...")] {
            out.push(
                terminal_run(
                    &format!("go:{key}:{sub}"),
                    &format!("go {sub} {tail}"),
                    vec!["go".into(), sub.into(), tail.into()],
                    &cwd,
                )
                .group(&header),
            );
        }
    }

    if let Some(py) = &proj.python {
        if py.requirements {
            out.push(
                terminal_run(
                    &format!("py:{key}:pip-install"),
                    "pip install -r requirements.txt",
                    vec![
                        "pip".into(),
                        "install".into(),
                        "-r".into(),
                        "requirements.txt".into(),
                    ],
                    &cwd,
                )
                .group(&header),
            );
        }
        match py.runner.as_deref() {
            Some("uv") => out.push(
                terminal_run(
                    &format!("py:{key}:uv-sync"),
                    "uv sync",
                    vec!["uv".into(), "sync".into()],
                    &cwd,
                )
                .group(&header),
            ),
            Some("poetry") => out.push(
                terminal_run(
                    &format!("py:{key}:poetry-install"),
                    "poetry install",
                    vec!["poetry".into(), "install".into()],
                    &cwd,
                )
                .group(&header),
            ),
            _ => {}
        }
    }

    out
}

fn compose_actions() -> Vec<Action> {
    [
        ("up", vec!["docker", "compose", "up"]),
        ("up-detached", vec!["docker", "compose", "up", "-d"]),
        ("down", vec!["docker", "compose", "down"]),
        ("logs", vec!["docker", "compose", "logs", "-f"]),
    ]
    .into_iter()
    .map(|(sub, argv)| {
        terminal_run(
            &format!("compose:{sub}"),
            &argv.join(" "),
            argv.iter().map(|s| s.to_string()).collect(),
            "{{path}}",
        )
        .group("Compose")
    })
    .collect()
}

fn universal_actions() -> Vec<Action> {
    let mut actions: Vec<Action> = Vec::new();

    #[cfg(windows)]
    {
        actions.push(
            Action::spawn(
                "windows-terminal",
                "Open in Windows Terminal",
                "wt -d {{path}}",
                "wt.exe",
                &["-d", "{{path}}"],
            )
            .group("General")
            .as_default(),
        );
        actions.push(
            Action::spawn(
                "claude-code",
                "Launch Claude Code",
                "wt -d {{path}} claude",
                "wt.exe",
                &["-d", "{{path}}", "claude"],
            )
            .group("General"),
        );
        if on_path("code") {
            actions.push(
                Action::spawn(
                    "vscode",
                    "Open in VS Code",
                    "code {{path}}",
                    "code.cmd",
                    &["{{path}}"],
                )
                .group("General"),
            );
        }
        actions.push(
            Action::spawn(
                "explorer",
                "Reveal in Explorer",
                "explorer {{path}}",
                "explorer.exe",
                &["{{path}}"],
            )
            .group("General"),
        );
    }

    #[cfg(not(windows))]
    {
        if on_path("x-terminal-emulator") {
            actions.push(
                Action::spawn(
                    "terminal",
                    "Open in Terminal",
                    "x-terminal-emulator",
                    "x-terminal-emulator",
                    &[],
                )
                .group("General")
                .as_default(),
            );
        }
        if on_path("claude") {
            actions.push(
                Action::spawn("claude-code", "Launch Claude Code", "claude", "claude", &[])
                    .group("General"),
            );
        }
        if on_path("code") {
            actions.push(
                Action::spawn(
                    "vscode",
                    "Open in VS Code",
                    "code {{path}}",
                    "code",
                    &["{{path}}"],
                )
                .group("General"),
            );
        }
        if on_path("xdg-open") {
            actions.push(
                Action::spawn(
                    "file-manager",
                    "Open folder",
                    "xdg-open {{path}}",
                    "xdg-open",
                    &["{{path}}"],
                )
                .group("General"),
            );
        }
    }

    actions
}

/// Build the full action list for a repo: per-project detected actions first
/// (root, then each sub-project), then compose, then universal, then clipboard.
pub fn build_actions(_repo: &Repo, ctx: &RepoContext) -> Vec<Action> {
    let mut actions: Vec<Action> = Vec::new();
    for proj in &ctx.projects {
        actions.extend(project_actions(proj));
    }
    if ctx.compose {
        actions.extend(compose_actions());
    }
    actions.extend(universal_actions());
    actions.push(Action::client("copy-path", "Copy path", "clipboard"));
    actions
}

/// Look up a single action by id (used by the launch command).
pub fn find_action(repo: &Repo, ctx: &RepoContext, action_id: &str) -> Option<Action> {
    build_actions(repo, ctx)
        .into_iter()
        .find(|a| a.id == action_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{NodeInfo, PkgManager, Project};

    fn repo() -> Repo {
        Repo {
            name: "demo".into(),
            path: if cfg!(windows) { "C:\\demo".into() } else { "/demo".into() },
            sentinels: vec![],
            last_seen: 0,
        }
    }

    fn ctx_with(projects: Vec<Project>, compose: bool) -> RepoContext {
        RepoContext { projects, compose }
    }

    #[test]
    fn empty_repo_has_one_default_and_copy_path() {
        let acts = build_actions(&repo(), &ctx_with(vec![Project::default()], false));
        assert_eq!(acts.iter().filter(|a| a.default).count(), 1);
        assert!(acts.iter().any(|a| a.id == "copy-path"));
    }

    #[test]
    fn subproject_scripts_are_namespaced_and_grouped() {
        let root = Project {
            rel: String::new(),
            node: Some(NodeInfo {
                manager: PkgManager::Npm,
                scripts: vec!["dev".into()],
            }),
            ..Default::default()
        };
        let web_dir = if cfg!(windows) { "C:\\demo\\web" } else { "/demo/web" };
        let web = Project {
            rel: "web".into(),
            dir: std::path::PathBuf::from(web_dir),
            node: Some(NodeInfo {
                manager: PkgManager::Pnpm,
                scripts: vec!["build".into(), "lint".into()],
            }),
            ..Default::default()
        };
        let acts = build_actions(&repo(), &ctx_with(vec![root, web], false));

        assert!(acts.iter().any(|a| a.id == "node:root:dev" && a.group == "Detected"));
        assert!(acts
            .iter()
            .any(|a| a.id == "node:web:build" && a.group == "Detected · web"));
        // sub-project actions target the sub-project dir, not the repo root
        let web_build = acts.iter().find(|a| a.id == "node:web:build").unwrap();
        if cfg!(windows) {
            assert!(web_build.args.iter().any(|a| a == web_dir));
        } else {
            assert_eq!(web_build.cwd.as_deref(), Some(web_dir));
        }
        // detected actions precede the General ones
        let first_general = acts.iter().position(|a| a.group == "General").unwrap();
        let last_detected = acts.iter().rposition(|a| a.group.starts_with("Detected")).unwrap();
        assert!(last_detected < first_general);
    }

    #[test]
    fn cargo_and_compose_actions() {
        let root = Project {
            has_cargo: true,
            ..Default::default()
        };
        let acts = build_actions(&repo(), &ctx_with(vec![root], true));
        for want in [
            "cargo:root:run",
            "cargo:root:build",
            "cargo:root:test",
            "compose:up",
            "compose:down",
        ] {
            assert!(acts.iter().any(|a| a.id == want), "missing {want}");
        }
    }
}
