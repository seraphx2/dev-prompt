use serde::Serialize;

use crate::scan::Repo;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    pub label: String,
    /// Short hint shown on the row (usually the resolved command).
    pub hint: String,
    /// Program to spawn. Ignored when `client_side` is true.
    #[serde(skip)]
    pub program: String,
    /// Argument templates; `{{path}}` / `{{file}}` are substituted at launch.
    #[serde(skip)]
    pub args: Vec<String>,
    /// Handled entirely in the frontend (e.g. copy path to clipboard).
    pub client_side: bool,
}

impl Action {
    fn spawn(id: &str, label: &str, hint: &str, program: &str, args: &[&str]) -> Self {
        Action {
            id: id.into(),
            label: label.into(),
            hint: hint.into(),
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            client_side: false,
        }
    }

    fn client(id: &str, label: &str, hint: &str) -> Self {
        Action {
            id: id.into(),
            label: label.into(),
            hint: hint.into(),
            program: String::new(),
            args: Vec::new(),
            client_side: true,
        }
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

/// Milestone 1: universal actions only. The contextual inspection engine
/// (`.sln` / `package.json` / `Cargo.toml` detection) lands in Milestone 2.
pub fn build_actions(_repo: &Repo) -> Vec<Action> {
    let mut actions: Vec<Action> = Vec::new();

    #[cfg(windows)]
    {
        actions.push(Action::spawn(
            "windows-terminal",
            "Open in Windows Terminal",
            "wt -d {{path}}",
            "wt.exe",
            &["-d", "{{path}}"],
        ));
        actions.push(Action::spawn(
            "claude-code",
            "Launch Claude Code",
            "wt -d {{path}} claude",
            "wt.exe",
            &["-d", "{{path}}", "claude"],
        ));
        if on_path("code") {
            actions.push(Action::spawn(
                "vscode",
                "Open in VS Code",
                "code {{path}}",
                "code.cmd",
                &["{{path}}"],
            ));
        }
        actions.push(Action::spawn(
            "explorer",
            "Reveal in Explorer",
            "explorer {{path}}",
            "explorer.exe",
            &["{{path}}"],
        ));
    }

    #[cfg(not(windows))]
    {
        // Minimal POSIX set; fleshed out (terminal-emulator probing) in Milestone 2.
        if on_path("x-terminal-emulator") {
            actions.push(Action::spawn(
                "terminal",
                "Open in Terminal",
                "x-terminal-emulator",
                "x-terminal-emulator",
                &[],
            ));
        }
        if on_path("claude") {
            actions.push(Action::spawn(
                "claude-code",
                "Launch Claude Code",
                "claude",
                "claude",
                &[],
            ));
        }
        if on_path("code") {
            actions.push(Action::spawn(
                "vscode",
                "Open in VS Code",
                "code {{path}}",
                "code",
                &["{{path}}"],
            ));
        }
        if on_path("xdg-open") {
            actions.push(Action::spawn(
                "file-manager",
                "Open folder",
                "xdg-open {{path}}",
                "xdg-open",
                &["{{path}}"],
            ));
        }
    }

    actions.push(Action::client("copy-path", "Copy path", "clipboard"));
    actions
}

/// Look up a single action by id for a repo (used by the launch command).
pub fn find_action(repo: &Repo, action_id: &str) -> Option<Action> {
    build_actions(repo).into_iter().find(|a| a.id == action_id)
}
