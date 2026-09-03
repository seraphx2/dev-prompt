//! Milestone 3 — the config-driven rule engine.
//!
//! `evaluate` emits the `universal` actions first, then walks every project in a
//! repo running the merged `rules` over its file list (matching globs, resolving
//! `{{program}}` references, expanding templates, invoking providers). All the
//! per-ecosystem knowledge that used to be hardcoded now lives in
//! `default_config.yaml`; Rust only keeps the providers and the program resolver.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use globset::{Glob, GlobSetBuilder};
use serde::Serialize;

use crate::config::{
    expand_str, slug, Config, MatchSpec, ProgramCandidate, ProgramSpec, Rule, RuleAction, Scope,
};
use crate::inspect::{Project, RepoContext};
use crate::scan::Repo;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    pub label: String,
    /// Short hint on the right of the row (usually the resolved command).
    pub hint: String,
    /// Section header; "" is just a divider.
    pub group: String,
    /// The action `Enter` runs on a repo.
    pub default: bool,
    /// Icon key resolved against `src/lib/icons.ts` in the frontend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip)]
    pub program: String,
    #[serde(skip)]
    pub args: Vec<String>,
    #[serde(skip)]
    pub cwd: Option<String>,
    /// Handled in the frontend (copy path). No process spawned.
    pub client_side: bool,
    /// Opens the "Run command…" input rather than spawning. `hint` is the
    /// template (may contain `{{input}}`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub prompt: bool,
}

// --- program resolution (process-global memo) -----------------------------

fn program_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Forget every memoized program lookup (call after the config changes or a tool
/// is installed while the app runs).
pub fn clear_program_cache() {
    program_cache().lock().unwrap().clear();
}

pub struct Resolver<'a> {
    programs: &'a std::collections::BTreeMap<String, ProgramSpec>,
    /// `config.terminal` — a pinned emulator (key / bare name / absolute path).
    terminal: Option<&'a str>,
    /// `config.terminal_template` — raw `{{dir}}` / `{{cmd}}` invocation.
    terminal_template: Option<&'a str>,
    /// `config.shell` — shell a one-shot terminal command runs inside.
    shell: Option<&'a str>,
}

impl<'a> Resolver<'a> {
    pub fn new(programs: &'a std::collections::BTreeMap<String, ProgramSpec>) -> Self {
        Self {
            programs,
            terminal: None,
            terminal_template: None,
            shell: None,
        }
    }

    /// Attach the terminal settings from `config.yaml`.
    pub fn with_terminal(mut self, terminal: Option<&'a str>, template: Option<&'a str>) -> Self {
        self.terminal = terminal;
        self.terminal_template = template;
        self
    }

    /// Attach `config.shell`.
    pub fn with_shell(mut self, shell: Option<&'a str>) -> Self {
        self.shell = shell;
        self
    }

    /// Resolve a program key to an absolute path, memoized for the process.
    pub fn resolve(&self, key: &str) -> Option<String> {
        if let Some(hit) = program_cache().lock().unwrap().get(key) {
            return hit.clone();
        }
        let resolved = self
            .programs
            .get(key)
            .and_then(|spec| spec.candidates().into_iter().find_map(resolve_candidate));
        program_cache()
            .lock()
            .unwrap()
            .insert(key.to_string(), resolved.clone());
        resolved
    }
}

fn resolve_candidate(c: &ProgramCandidate) -> Option<String> {
    match c {
        ProgramCandidate::Path(raw) => resolve_path_candidate(raw),
        ProgramCandidate::Vswhere { vswhere } => resolve_vswhere(vswhere),
    }
}

fn resolve_path_candidate(raw: &str) -> Option<String> {
    let pat = expand_str(raw).replace('\\', "/");
    if pat.contains(['*', '?', '[']) {
        return glob::glob(&pat)
            .ok()?
            .filter_map(Result::ok)
            .find(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned());
    }
    if !pat.contains('/') {
        return which(&pat);
    }
    Path::new(&pat).is_file().then(|| pat.clone())
}

#[cfg(windows)]
fn resolve_vswhere(args: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    // Without this the console window pops for a frame — visible as a flicker on
    // the first overlay show, when program resolution first runs.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let vswhere = ["ProgramFiles(x86)", "ProgramFiles"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(|base| format!("{base}\\Microsoft Visual Studio\\Installer\\vswhere.exe"))
        .find(|p| Path::new(p).is_file())?;
    let out = Command::new(vswhere)
        .args(args.split_whitespace())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty() && Path::new(&path).is_file()).then_some(path)
}

#[cfg(not(windows))]
fn resolve_vswhere(_args: &str) -> Option<String> {
    None
}

/// `which`-style PATH lookup that returns the resolved path.
pub fn which(program: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
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
        let direct = dir.join(program);
        if direct.is_file() {
            return Some(direct.to_string_lossy().into_owned());
        }
        for ext in exts.iter().filter(|e| !e.is_empty()) {
            let cand = dir.join(format!("{program}{ext}"));
            if cand.is_file() {
                return Some(cand.to_string_lossy().into_owned());
            }
        }
    }
    None
}

// --- template expansion --------------------------------------------------

struct Tmpl<'a> {
    repo: &'a str,
    path: &'a str,
    rel: &'a str,
    name: &'a str,
    file: Option<&'a Path>,
    resolver: &'a Resolver<'a>,
}

fn expand(tmpl: &str, t: &Tmpl) -> String {
    let mut out = String::with_capacity(tmpl.len());
    let mut rest = tmpl;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        rest = &rest[i + 2..];
        match rest.find("}}") {
            Some(j) => {
                out.push_str(&resolve_var(rest[..j].trim(), t));
                rest = &rest[j + 2..];
            }
            None => {
                out.push_str("{{");
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn resolve_var(key: &str, t: &Tmpl) -> String {
    match key {
        "path" => t.path.to_string(),
        "repo" => t.repo.to_string(),
        "rel" => t.rel.to_string(),
        "name" => t.name.to_string(),
        "file" => t
            .file
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
        "file.name" => t
            .file
            .and_then(|f| f.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        "file.stem" => t
            .file
            .and_then(|f| f.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        // Left intact for a `prompt:` action — the frontend fills it in.
        "input" => "{{input}}".to_string(),
        _ => {
            if let Some(var) = key.strip_prefix("env:") {
                std::env::var(var).unwrap_or_default()
            } else {
                t.resolver.resolve(key).unwrap_or_default()
            }
        }
    }
}

/// Split a Windows command-line *argument* string the way `CommandLineToArgvW`
/// does: `\` and `"` interact (`\"` is a literal quote, `\\` a literal
/// backslash), `""` inside a quoted run is a literal `"`, and `'` is an ordinary
/// character. `.lnk` Arguments follow these rules — [`shell_split`]'s POSIX-ish
/// `'`/`"` handling silently eats an unpaired apostrophe.
pub fn win_split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut in_quotes = false;
    let mut backslashes = 0usize;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                for _ in 0..backslashes / 2 {
                    cur.push('\\');
                }
                started = true;
                if backslashes % 2 == 1 {
                    cur.push('"'); // \" -> literal quote
                } else if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"'); // "" inside quotes -> one literal quote
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
                backslashes = 0;
            }
            c if c.is_whitespace() && !in_quotes => {
                for _ in 0..backslashes {
                    cur.push('\\');
                }
                backslashes = 0;
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                for _ in 0..backslashes {
                    cur.push('\\');
                }
                backslashes = 0;
                cur.push(c);
                started = true;
            }
        }
    }
    for _ in 0..backslashes {
        cur.push('\\');
    }
    if started {
        out.push(cur);
    }
    out
}

/// Quote-aware split for `run:` strings.
pub fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in s.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    has = true;
                }
                c if c.is_whitespace() => {
                    if has {
                        out.push(std::mem::take(&mut cur));
                        has = false;
                    }
                }
                _ => {
                    cur.push(c);
                    has = true;
                }
            },
        }
    }
    if has {
        out.push(cur);
    }
    out
}

fn basename(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string())
}

// --- terminal wrapping -------------------------------------------------

/// Terminal emulators whose command-line dev-prompt knows how to build.
/// Anything else needs a `terminal_template`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TermKind {
    WindowsTerminal,
    Alacritty,
    WezTerm,
    Unknown,
}

fn term_kind(binary: &str) -> TermKind {
    match basename(binary)
        .to_lowercase()
        .trim_end_matches(".exe")
    {
        "wt" | "windowsterminal" => TermKind::WindowsTerminal,
        "alacritty" => TermKind::Alacritty,
        "wezterm" | "wezterm-gui" => TermKind::WezTerm,
        _ => TermKind::Unknown,
    }
}

/// `(id, label)` for every configured terminal candidate that resolves on this
/// machine *and* has a known invocation. Feeds the Settings dropdown.
pub fn terminal_options(config: &Config) -> Vec<(String, String)> {
    let Some(spec) = config.programs.get("terminal") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for c in spec.candidates() {
        let ProgramCandidate::Path(raw) = c else {
            continue;
        };
        let Some(resolved) = resolve_path_candidate(raw) else {
            continue;
        };
        if term_kind(&resolved) == TermKind::Unknown {
            continue;
        }
        // A plain name / absolute path round-trips as-is; a glob stores the hit.
        let id = if raw.contains(['*', '?', '[']) {
            resolved.clone()
        } else {
            raw.clone()
        };
        let label = basename(&resolved);
        if !out.iter().any(|(_, l): &(String, String)| *l == label) {
            out.push((id, label));
        }
    }
    out
}

/// Wrap `argv` in a shell that runs the command and then stays open with a real
/// console (ANSI colour, a live TTY — tools like Claude Code render monochrome
/// otherwise). `shell` = `config.shell`, else `pwsh` → Windows PowerShell.
///
/// Each of these shells takes the command as one string, so every `argv` element
/// is quoted for that shell first — `argv.join(" ")` alone lets the shell
/// re-split on a space inside a path (`C:\Users\First Last\...`).
#[cfg(windows)]
fn shell_wrap(argv: &[String], shell: Option<&str>) -> Vec<String> {
    let shell = shell.unwrap_or(if which("pwsh").is_some() {
        "pwsh"
    } else {
        "powershell"
    });
    let kind = basename(shell).to_lowercase();
    let kind = kind.trim_end_matches(".exe");

    fn join_quoted(argv: &[String], q: impl Fn(&str) -> String) -> String {
        argv.iter().map(|a| q(a)).collect::<Vec<_>>().join(" ")
    }
    // PowerShell / nu: single quotes are literal (`''` escapes one quote).
    let ps = |s: &str| format!("'{}'", s.replace('\'', "''"));
    // POSIX: `'\''` closes, escapes a literal quote, reopens.
    let sh = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    // cmd: double-quote anything with whitespace or a quote.
    let cm = |s: &str| {
        if s.is_empty() || s.contains([' ', '\t', '"']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };

    match kind {
        // `& <quoted>` so a quoted program *path* is invoked, not echoed.
        "pwsh" | "powershell" => vec![
            shell.to_string(),
            "-NoLogo".to_string(),
            "-NoExit".to_string(),
            "-Command".to_string(),
            format!("& {}", join_quoted(argv, ps)),
        ],
        // Outer quotes: `cmd /k` strips the outermost pair before parsing.
        "cmd" => vec![
            shell.to_string(),
            "/k".to_string(),
            format!("\"{}\"", join_quoted(argv, cm)),
        ],
        "bash" | "zsh" | "sh" | "fish" => vec![
            shell.to_string(),
            "-c".to_string(),
            format!("{}; exec {kind}", join_quoted(argv, sh)),
        ],
        "nu" => vec![
            shell.to_string(),
            "-e".to_string(),
            join_quoted(argv, ps),
        ],
        _ => vec![
            shell.to_string(),
            "-c".to_string(),
            join_quoted(argv, sh),
        ],
    }
}

/// `(program, args, cwd)` to run `argv` in a terminal at `cwd`. Empty `argv`
/// means "just open a terminal there". `wrap` = run `argv` inside a shell that
/// stays open (one-shot commands); `false` = hand `argv` to the emulator raw
/// (e.g. `argv == ["bash"]` → an interactive bash).
fn terminalize(
    argv: &[String],
    cwd: &str,
    resolver: &Resolver,
    wrap: bool,
) -> (String, Vec<String>, Option<String>) {
    #[cfg(windows)]
    {
        // 1. Which binary — a pinned `terminal:`, else the first resolving
        //    `programs.terminal` candidate, else Windows Terminal.
        let term = resolver
            .terminal
            .and_then(|t| {
                if t.contains(['/', '\\']) {
                    Some(t.to_string()) // absolute / relative path, use as-is
                } else {
                    which(t) // bare name → PATH
                }
            })
            .or_else(|| resolver.resolve("terminal"))
            .unwrap_or_else(|| "wt.exe".to_string());

        // 2. Raw template override. Split the template first, then splice `argv`
        //    in where `{{cmd}}` stands as its own token — joining `argv` and
        //    re-splitting the whole line shreds any element with a space in it.
        if let Some(tmpl) = resolver.terminal_template {
            let mut parts: Vec<String> = Vec::new();
            for tok in shell_split(tmpl) {
                if tok == "{{cmd}}" {
                    parts.extend(argv.iter().cloned());
                } else {
                    parts.push(tok.replace("{{dir}}", cwd));
                }
            }
            return (
                parts.first().cloned().unwrap_or_else(|| term.clone()),
                parts.into_iter().skip(1).collect(),
                None,
            );
        }

        // The command portion, shell-wrapped or raw.
        let run: Vec<String> = if argv.is_empty() {
            Vec::new()
        } else if wrap {
            shell_wrap(argv, resolver.shell)
        } else {
            argv.to_vec()
        };

        // 3. Known-emulator table.
        match term_kind(&term) {
            TermKind::Alacritty => {
                let mut args = vec!["--working-directory".to_string(), cwd.to_string()];
                if !run.is_empty() {
                    args.push("-e".to_string());
                    args.extend(run);
                }
                (term, args, None)
            }
            TermKind::WezTerm => {
                let mut args =
                    vec!["start".to_string(), "--cwd".to_string(), cwd.to_string()];
                if !run.is_empty() {
                    args.push("--".to_string());
                    args.extend(run);
                }
                (term, args, None)
            }
            TermKind::WindowsTerminal => {
                let mut args = vec!["-d".to_string(), cwd.to_string()];
                args.extend(run);
                (term, args, None)
            }
            TermKind::Unknown => {
                // No table entry and no template: best effort — hand the
                // command straight to the binary with a working directory.
                (term, argv.to_vec(), Some(cwd.to_string()))
            }
        }
    }
    #[cfg(not(windows))]
    {
        // Per-emulator handling on non-Windows is its own milestone
        // (docs/config-design.md #10); run the command directly in `cwd`.
        let _ = wrap;
        if argv.is_empty() {
            let term = resolver
                .resolve("terminal")
                .unwrap_or_else(|| "xterm".to_string());
            (term, Vec::new(), Some(cwd.to_string()))
        } else {
            (argv[0].clone(), argv[1..].to_vec(), Some(cwd.to_string()))
        }
    }
}

/// Build a spawnable `(program, args, cwd)` for a free-form command line — the
/// seam the "Run command…" backend uses. `wrap: false` opens `command` (a shell
/// name) interactively.
pub fn terminal_command(
    command: &str,
    cwd: &str,
    resolver: &Resolver,
    wrap: bool,
) -> (String, Vec<String>, Option<String>) {
    terminalize(&shell_split(command), cwd, resolver, wrap)
}

// --- action construction ---------------------------------------------

fn build_action(
    ra: &RuleAction,
    id: String,
    group: &str,
    cwd: &str,
    t: &Tmpl,
    resolver: &Resolver,
) -> Option<Action> {
    if ra.client {
        return Some(Action {
            id,
            label: expand(&ra.name, t),
            hint: String::new(),
            group: group.to_string(),
            default: ra.default,
            icon: ra.icon.clone(),
            program: String::new(),
            args: Vec::new(),
            cwd: None,
            client_side: true,
            prompt: false,
        });
    }

    if ra.needs.iter().any(|k| resolver.resolve(k).is_none()) {
        return None;
    }

    if ra.prompt {
        // No process here — the frontend opens the "Run command…" input. `run:`
        // (if any) is the template shown, `{{input}}` left intact for it.
        return Some(Action {
            id,
            label: expand(&ra.name, t),
            hint: ra.run.as_deref().map(|r| expand(r, t)).unwrap_or_default(),
            group: group.to_string(),
            default: ra.default,
            icon: ra.icon.clone(),
            program: String::new(),
            args: Vec::new(),
            cwd: Some(cwd.to_string()),
            client_side: false,
            prompt: true,
        });
    }

    let (program, args, hint): (String, Vec<String>, String) = if let Some(prog) = &ra.program {
        let p = expand(prog, t);
        if p.is_empty() {
            return None;
        }
        let a: Vec<String> = ra.args.iter().map(|arg| expand(arg, t)).collect();
        let hint = format!("{} {}", basename(&p), a.join(" ")).trim().to_string();
        (p, a, hint)
    } else if let Some(run) = &ra.run {
        let expanded = expand(run, t);
        let parts = shell_split(&expanded);
        (
            parts.first().cloned().unwrap_or_default(),
            parts.into_iter().skip(1).collect(),
            expanded,
        )
    } else {
        (String::new(), Vec::new(), "terminal".to_string())
    };

    let (final_prog, final_args, final_cwd) = if ra.terminal {
        let argv: Vec<String> = if program.is_empty() {
            Vec::new()
        } else {
            std::iter::once(program.clone()).chain(args).collect()
        };
        terminalize(&argv, cwd, resolver, true)
    } else {
        if program.is_empty() {
            return None;
        }
        (program, args, Some(cwd.to_string()))
    };

    Some(Action {
        id,
        label: expand(&ra.name, t),
        hint,
        group: group.to_string(),
        default: ra.default,
        icon: ra.icon.clone(),
        program: final_prog,
        args: final_args,
        cwd: final_cwd,
        client_side: false,
        prompt: false,
    })
}

// --- providers -----------------------------------------------------

fn manager_run(mgr: &str, script: &str) -> Vec<String> {
    match mgr {
        "yarn" => vec!["yarn".into(), script.into()],
        m => vec![m.into(), "run".into(), script.into()],
    }
}

fn provider_actions(
    name: &str,
    rule: &Rule,
    proj: &Project,
    group: &str,
    ns: &str,
    cwd: &str,
    resolver: &Resolver,
) -> Vec<Action> {
    let term_in = |id: String, label: String, argv: Vec<String>, at: &str| -> Action {
        let hint = argv.join(" ");
        let (p, a, c) = terminalize(&argv, at, resolver, true);
        Action {
            id,
            label,
            hint,
            group: group.to_string(),
            default: false,
            icon: None,
            program: p,
            args: a,
            cwd: c,
            client_side: false,
            prompt: false,
        }
    };
    let term = |id: String, label: String, argv: Vec<String>| term_in(id, label, argv, cwd);

    let prov_icon = match name {
        "npm-scripts" => "npm",
        "cargo" => "rust",
        "go" | "go-work" => "go",
        "python" => "python",
        "compose" => "docker",
        "dotnet" => "dotnet",
        "maven-modules" => "java",
        "gradle-modules" | "flutter-android" => "gradle",
        _ => "run",
    };

    let mut acts = match name {
        "npm-scripts" => {
            let Some(node) = &proj.node else {
                return Vec::new();
            };
            let mgr = match rule.manager.as_deref() {
                Some(m) if m != "auto" => m.to_string(),
                _ => node.manager.label().to_string(),
            };
            node.scripts
                .iter()
                .take(30)
                .map(|s| {
                    term(
                        format!("npm:{ns}:{s}"),
                        format!("{mgr} {s}"),
                        manager_run(&mgr, s),
                    )
                })
                .collect()
        }
        "cargo" => ["run", "build", "test", "check", "clippy"]
            .iter()
            .map(|s| {
                term(
                    format!("cargo:{ns}:{s}"),
                    format!("cargo {s}"),
                    vec!["cargo".into(), (*s).into()],
                )
            })
            .collect(),
        "go" => [
            ("run", "."),
            ("build", "./..."),
            ("test", "./..."),
        ]
        .iter()
        .map(|(s, tail)| {
            term(
                format!("go:{ns}:{s}"),
                format!("go {s} {tail}"),
                vec!["go".into(), (*s).into(), (*tail).into()],
            )
        })
        .collect(),
        "python" => {
            let Some(py) = &proj.python else {
                return Vec::new();
            };

            // Prefer the project's own venv interpreter (absolute, so it works
            // whatever the terminal's cwd/shell), else a bare `python`.
            let python: String = py
                .venv
                .as_deref()
                .map(|v| {
                    let rel = if cfg!(windows) {
                        "Scripts/python.exe"
                    } else {
                        "bin/python"
                    };
                    proj.dir.join(v).join(rel).to_string_lossy().into_owned()
                })
                .unwrap_or_else(|| "python".to_string());

            let mut v = Vec::new();

            // Install / sync dependencies.
            match py.runner.as_deref() {
                Some("uv") => v.push(term(
                    format!("py:{ns}:uv"),
                    "uv sync".into(),
                    vec!["uv".into(), "sync".into()],
                )),
                Some("poetry") => v.push(term(
                    format!("py:{ns}:poetry"),
                    "poetry install".into(),
                    vec!["poetry".into(), "install".into()],
                )),
                Some("pipenv") => v.push(term(
                    format!("py:{ns}:pipenv"),
                    "pipenv install".into(),
                    vec!["pipenv".into(), "install".into()],
                )),
                Some("pdm") => v.push(term(
                    format!("py:{ns}:pdm"),
                    "pdm install".into(),
                    vec!["pdm".into(), "install".into()],
                )),
                _ if py.requirements => v.push(term(
                    format!("py:{ns}:pip"),
                    "pip install -r requirements.txt".into(),
                    vec![
                        python.clone(),
                        "-m".into(),
                        "pip".into(),
                        "install".into(),
                        "-r".into(),
                        "requirements.txt".into(),
                    ],
                )),
                _ => {}
            }

            // Django.
            if py.manage_py {
                for sub in ["runserver", "migrate", "test"] {
                    v.push(term(
                        format!("py:{ns}:manage:{sub}"),
                        format!("python manage.py {sub}"),
                        vec![python.clone(), "manage.py".into(), sub.into()],
                    ));
                }
            }

            // pytest.
            if py.pytest {
                v.push(term(
                    format!("py:{ns}:pytest"),
                    "pytest".into(),
                    vec![python.clone(), "-m".into(), "pytest".into()],
                ));
            }

            // Run the obvious entry point.
            if let Some(entry) = &py.entry {
                v.push(term(
                    format!("py:{ns}:run"),
                    format!("python {entry}"),
                    vec![python.clone(), entry.clone()],
                ));
            }

            v
        }
        "compose" => ["up", "up -d", "down", "logs -f"]
            .iter()
            .map(|s| {
                let mut argv = vec!["docker".to_string(), "compose".to_string()];
                argv.extend(s.split(' ').map(String::from));
                term(
                    format!("compose:{}", slug(s)),
                    format!("docker compose {s}"),
                    argv,
                )
            })
            .collect(),
        "dotnet" => {
            let mut used = std::collections::HashSet::new();
            let mut v = Vec::new();
            for u in crate::dotnet::units(&proj.dir, &proj.files) {
                let p = u.path.to_string_lossy().into_owned();
                let mut key = slug(&u.name);
                while !used.insert(key.clone()) {
                    key.push('_');
                }
                v.push(term(
                    format!("dotnet:{ns}:build:{key}"),
                    format!("dotnet build · {}", u.name),
                    vec!["dotnet".into(), "build".into(), p.clone()],
                ));
                v.push(term(
                    format!("dotnet:{ns}:run:{key}"),
                    format!("dotnet run · {}", u.name),
                    vec![
                        "dotnet".into(),
                        "run".into(),
                        "--project".into(),
                        p.clone(),
                    ],
                ));
                if u.is_test {
                    v.push(term(
                        format!("dotnet:{ns}:test:{key}"),
                        format!("dotnet test · {}", u.name),
                        vec!["dotnet".into(), "test".into(), p],
                    ));
                }
            }
            v
        }
        // Workspace manifests that name modules living elsewhere: one build/test
        // per module, run from *that* module's directory.
        "go-work" => crate::gowork::modules(&proj.dir)
            .into_iter()
            .flat_map(|(m, dir)| {
                let at = dir.to_string_lossy().into_owned();
                let k = slug(&m);
                [
                    term_in(
                        format!("gowork:{ns}:{k}:build"),
                        format!("go build · {m}"),
                        vec!["go".into(), "build".into(), "./...".into()],
                        &at,
                    ),
                    term_in(
                        format!("gowork:{ns}:{k}:test"),
                        format!("go test · {m}"),
                        vec!["go".into(), "test".into(), "./...".into()],
                        &at,
                    ),
                ]
            })
            .collect(),
        "maven-modules" => crate::maven::modules(&proj.dir)
            .into_iter()
            .flat_map(|(m, dir)| {
                let at = dir.to_string_lossy().into_owned();
                let k = slug(&m);
                [
                    term_in(
                        format!("mvnmod:{ns}:{k}:compile"),
                        format!("mvn compile · {m}"),
                        vec!["mvn".into(), "-B".into(), "compile".into()],
                        &at,
                    ),
                    term_in(
                        format!("mvnmod:{ns}:{k}:test"),
                        format!("mvn test · {m}"),
                        vec!["mvn".into(), "-B".into(), "test".into()],
                        &at,
                    ),
                ]
            })
            .collect(),
        // Gradle is root-centric: `gradle :path:proj:task` from the settings dir.
        "gradle-modules" => crate::gradle::projects(&proj.dir)
            .into_iter()
            .flat_map(|(m, gpath)| {
                let k = slug(&m);
                [
                    term(
                        format!("gradlemod:{ns}:{k}:build"),
                        format!("gradle {gpath}:build"),
                        vec!["gradle".into(), format!("{gpath}:build")],
                    ),
                    term(
                        format!("gradlemod:{ns}:{k}:test"),
                        format!("gradle {gpath}:test"),
                        vec!["gradle".into(), format!("{gpath}:test")],
                    ),
                ]
            })
            .collect(),
        // A Flutter app's Gradle project lives one level down, in `android/` —
        // invisible to `gradle-modules` (which only checks the project's own
        // top-level files). Point the same parser at that subdir instead. Uses
        // the bundled `gradlew` wrapper when present (the common case for
        // Flutter, which rarely has a global `gradle` on PATH), else falls
        // back to PATH. Silently empty when there's no `android/` — nothing to
        // gate on beyond that.
        "flutter-android" => {
            let android_dir = proj.dir.join("android");
            let wrapper_name = if cfg!(windows) { "gradlew.bat" } else { "gradlew" };
            let gradle_cmd = if android_dir.join(wrapper_name).is_file() {
                android_dir.join(wrapper_name).to_string_lossy().into_owned()
            } else {
                "gradle".to_string()
            };
            let at = android_dir.to_string_lossy().into_owned();
            crate::gradle::projects(&android_dir)
                .into_iter()
                .flat_map(|(m, gpath)| {
                    let k = slug(&m);
                    [
                        term_in(
                            format!("flutterandroid:{ns}:{k}:build"),
                            format!("gradle {gpath}:build (android)"),
                            vec![gradle_cmd.clone(), format!("{gpath}:build")],
                            &at,
                        ),
                        term_in(
                            format!("flutterandroid:{ns}:{k}:test"),
                            format!("gradle {gpath}:test (android)"),
                            vec![gradle_cmd.clone(), format!("{gpath}:test")],
                            &at,
                        ),
                    ]
                })
                .collect()
        }
        _ => Vec::new(),
    };

    for a in &mut acts {
        a.icon.get_or_insert_with(|| prov_icon.to_string());
    }
    acts
}

// --- evaluation --------------------------------------------------

fn os_matches(when: Option<&str>) -> bool {
    match when.map(str::to_lowercase).as_deref() {
        None => true,
        Some("windows" | "win") => cfg!(windows),
        Some("linux") => cfg!(target_os = "linux"),
        Some("macos" | "mac" | "darwin") => cfg!(target_os = "macos"),
        Some("unix") => cfg!(unix),
        Some(_) => true,
    }
}

fn matched_files(m: &MatchSpec, files: &[String]) -> Vec<String> {
    let mut builder = GlobSetBuilder::new();
    for g in m.globs() {
        if let Ok(glob) = Glob::new(&g.to_lowercase()) {
            builder.add(glob);
        }
    }
    let Ok(set) = builder.build() else {
        return Vec::new();
    };
    files
        .iter()
        .filter(|f| set.is_match(f.to_lowercase()))
        .cloned()
        .collect()
}

/// `None` = the rule passes its machine-level gates; `Some(reason)` = it can't
/// fire anywhere on this machine/config (disabled, wrong OS, a missing
/// `requires:` binary, or an unresolved `needs:` program).
fn rule_gate(rule: &Rule, resolver: &Resolver) -> Option<String> {
    if rule.disable.is_some() {
        return Some("disabled".into());
    }
    if !os_matches(rule.when.as_deref()) {
        return Some(format!("os ≠ {}", rule.when.clone().unwrap_or_default()));
    }
    if let Some(b) = rule.requires.iter().find(|b| which(b).is_none()) {
        return Some(format!("requires `{b}` — not on PATH"));
    }
    if let Some(k) = rule.needs.iter().find(|k| resolver.resolve(k).is_none()) {
        return Some(format!("needs `{k}` — unresolved"));
    }
    None
}

/// Actions produced by one rule against one project, assuming [`rule_gate`] has
/// already passed. Empty when the rule's globs hit nothing in the project.
fn rule_project_actions(
    rule: &Rule,
    proj: &Project,
    repo: &Repo,
    resolver: &Resolver,
) -> Vec<Action> {
    let matched = matched_files(&rule.match_, &proj.files);
    if matched.is_empty() {
        return Vec::new();
    }

    let group = if proj.rel.is_empty() {
        "Detected".to_string()
    } else {
        format!("Detected · {}", proj.rel)
    };
    let ns = if proj.rel.is_empty() {
        "root".to_string()
    } else {
        proj.rel.replace(['/', '\\'], "-")
    };
    let proj_dir = proj.dir.to_string_lossy().into_owned();
    let proj_name = if proj.rel.is_empty() {
        repo.name.clone()
    } else {
        proj.rel
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&proj.rel)
            .to_string()
    };
    let cwd = match rule.scope {
        Scope::Repo => repo.path.clone(),
        Scope::Project => proj_dir.clone(),
    };

    if let Some(provider) = &rule.provider {
        return provider_actions(provider, rule, proj, &group, &ns, &cwd, resolver);
    }

    let mut out = Vec::new();
    let targets: Vec<Option<PathBuf>> = if rule.per_file {
        matched.iter().map(|f| Some(proj.dir.join(f))).collect()
    } else {
        vec![None]
    };
    for file in targets {
        let fref = file.as_deref();
        let t = Tmpl {
            repo: &repo.path,
            path: &cwd,
            rel: &proj.rel,
            name: &proj_name,
            file: fref,
            resolver,
        };
        for (ai, ra) in rule.actions.iter().enumerate() {
            let base = ra.id.clone().unwrap_or_else(|| {
                format!("{}-{ai}", rule.id.clone().unwrap_or_else(|| slug(&ra.name)))
            });
            let id = match fref.and_then(|f| f.file_stem()).and_then(|s| s.to_str()) {
                Some(stem) => format!("{ns}:{base}:{stem}"),
                None => format!("{ns}:{base}"),
            };
            if let Some(a) = build_action(ra, id, &group, &cwd, &t, resolver) {
                out.push(a);
            }
        }
    }
    out
}

fn universal_actions(config: &Config, repo: &Repo, resolver: &Resolver) -> Vec<Action> {
    config
        .universal
        .actions
        .iter()
        .filter_map(|ra| {
            let id = ra.action_id();
            let mut ra = ra.clone();
            ra.default = ra.default || config.universal.default.as_deref() == Some(&id);
            let t = Tmpl {
                repo: &repo.path,
                path: &repo.path,
                rel: "",
                name: &repo.name,
                file: None,
                resolver,
            };
            build_action(&ra, id, "General", &repo.path, &t, resolver)
        })
        .collect()
}

pub fn evaluate(config: &Config, ctx: &RepoContext, repo: &Repo) -> Vec<Action> {
    let resolver = Resolver::new(&config.programs)
        .with_terminal(config.terminal.as_deref(), config.terminal_template.as_deref())
        .with_shell(config.shell.as_deref());

    // Universal actions first — "open in terminal / editor / file manager" is the
    // common case; the detected per-ecosystem stuff sits below it.
    let mut out = universal_actions(config, repo, &resolver);

    for proj in &ctx.projects {
        for rule in &config.rules {
            if rule_gate(rule, &resolver).is_some() {
                continue;
            }
            out.extend(rule_project_actions(rule, proj, repo, &resolver));
        }
    }

    out
}

pub fn build_actions(repo: &Repo, ctx: &RepoContext, config: &Config) -> Vec<Action> {
    evaluate(config, ctx, repo)
}

// --- per-repo rule trace (settings "trace a repo" view) ------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoTrace {
    pub repo_name: String,
    pub repo_path: String,
    /// Universal action ids that resolve for this repo.
    pub universal: Vec<String>,
    /// One entry per configured rule, in config order.
    pub rules: Vec<RuleTrace>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleTrace {
    pub id: String,
    /// The glob(s) the rule matches on.
    pub globs: Vec<String>,
    /// "" when the rule resolved; otherwise why it produced nothing — a machine
    /// gate ("disabled", "needs `x` — unresolved", …) or "no matching files".
    pub gate: String,
    /// Per-project results — populated only when `gate` is "".
    pub hits: Vec<ProjectHit>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHit {
    /// "" = repo root, else the sub-project's relative path.
    pub project: String,
    /// Files in that project the rule's globs matched.
    pub matched: Vec<String>,
    /// Action ids the rule produced there.
    pub produced: Vec<String>,
}

/// Explain, rule by rule, what one repo produces and why — the data behind the
/// settings "trace a repo" view. Shares [`rule_gate`] and [`rule_project_actions`]
/// with [`evaluate`], so its verdicts match what the action menu actually shows.
pub fn trace(config: &Config, ctx: &RepoContext, repo: &Repo) -> RepoTrace {
    let resolver = Resolver::new(&config.programs)
        .with_terminal(config.terminal.as_deref(), config.terminal_template.as_deref())
        .with_shell(config.shell.as_deref());

    let universal = universal_actions(config, repo, &resolver)
        .into_iter()
        .map(|a| a.id)
        .collect();

    let rules = config
        .rules
        .iter()
        .map(|rule| {
            let id = rule.id.clone().unwrap_or_else(|| "(unnamed)".into());
            let globs = rule.match_.globs().iter().map(|s| s.to_string()).collect();

            if let Some(reason) = rule_gate(rule, &resolver) {
                return RuleTrace {
                    id,
                    globs,
                    gate: reason,
                    hits: Vec::new(),
                };
            }

            let hits: Vec<ProjectHit> = ctx
                .projects
                .iter()
                .filter_map(|proj| {
                    let matched = matched_files(&rule.match_, &proj.files);
                    if matched.is_empty() {
                        return None;
                    }
                    let produced = rule_project_actions(rule, proj, repo, &resolver)
                        .into_iter()
                        .map(|a| a.id)
                        .collect();
                    Some(ProjectHit {
                        project: proj.rel.clone(),
                        matched,
                        produced,
                    })
                })
                .collect();

            let gate = if hits.is_empty() {
                "no matching files in this repo".to_string()
            } else {
                String::new()
            };
            RuleTrace {
                id,
                globs,
                gate,
                hits,
            }
        })
        .collect();

    RepoTrace {
        repo_name: repo.name.clone(),
        repo_path: repo.path.clone(),
        universal,
        rules,
    }
}

// --- read-only summary (settings "what's active" view) --------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSummary {
    pub rules_path: String,
    pub marker_count: usize,
    pub programs: Vec<ProgramStatus>,
    pub rules: Vec<RuleStatus>,
    pub universal: Vec<UniversalStatus>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramStatus {
    pub key: String,
    pub resolved: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleStatus {
    pub id: String,
    pub matches: Vec<String>,
    pub kind: String,
    pub scope: String,
    pub available: bool,
    pub missing: Vec<String>,
    /// The user turned this built-in off (`rules_disable` / bare `disable:`).
    pub disabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UniversalStatus {
    pub id: String,
    pub label: String,
    pub default: bool,
    pub available: bool,
    /// The user turned this built-in off (`universal.disable`).
    pub disabled: bool,
}

pub fn summarize(config: &Config, rules_path: String) -> ConfigSummary {
    let resolver = Resolver::new(&config.programs);

    let programs = config
        .programs
        .keys()
        .map(|k| ProgramStatus {
            key: k.clone(),
            resolved: resolver.resolve(k),
        })
        .collect();

    let rule_status = |r: &Rule, disabled: bool| {
        let mut missing = Vec::new();
        if !os_matches(r.when.as_deref()) {
            missing.push(format!("os≠{}", r.when.clone().unwrap_or_default()));
        }
        for k in &r.needs {
            if resolver.resolve(k).is_none() {
                missing.push(format!("{k}?"));
            }
        }
        for b in &r.requires {
            if which(b).is_none() {
                missing.push(format!("{b} (PATH)"));
            }
        }
        RuleStatus {
            id: r.id.clone().unwrap_or_else(|| "(unnamed)".into()),
            matches: r.match_.globs().iter().map(|s| s.to_string()).collect(),
            kind: match &r.provider {
                Some(p) => format!("provider: {p}"),
                None => format!("{} action(s)", r.actions.len()),
            },
            scope: format!("{:?}", r.scope).to_lowercase(),
            available: !disabled && missing.is_empty(),
            missing,
            disabled,
        }
    };
    let rules = config
        .rules
        .iter()
        .filter(|r| r.disable.is_none())
        .map(|r| rule_status(r, false))
        .chain(config.disabled_rules.iter().map(|r| rule_status(r, true)))
        .collect();

    let universal_status = |a: &RuleAction, disabled: bool| {
        let id = a.action_id();
        UniversalStatus {
            default: a.default || config.universal.default.as_deref() == Some(&id),
            available: !disabled
                && (a.client || a.needs.iter().all(|k| resolver.resolve(k).is_some())),
            label: a.name.clone(),
            id,
            disabled,
        }
    };
    let universal = config
        .universal
        .actions
        .iter()
        .map(|a| universal_status(a, false))
        .chain(
            config
                .universal
                .disabled
                .iter()
                .map(|a| universal_status(a, true)),
        )
        .collect();

    ConfigSummary {
        rules_path,
        marker_count: config.markers.len(),
        programs,
        rules,
        universal,
    }
}

pub fn find_action(
    repo: &Repo,
    ctx: &RepoContext,
    config: &Config,
    action_id: &str,
) -> Option<Action> {
    build_actions(repo, ctx, config)
        .into_iter()
        .find(|a| a.id == action_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::bundled_defaults;
    use crate::inspect::{NodeInfo, PkgManager};

    fn repo() -> Repo {
        Repo {
            name: "demo".into(),
            path: if cfg!(windows) { "C:\\demo".into() } else { "/demo".into() },
            vcs: None,
            sentinels: vec![],
            last_seen: 0,
        }
    }

    fn ctx_one(proj: Project) -> RepoContext {
        RepoContext {
            projects: vec![proj],
            compose: false,
        }
    }

    #[cfg(windows)]
    #[test]
    fn shell_wrap_quotes_each_arg_per_shell() {
        let argv = vec!["cargo".to_string(), "build".to_string()];
        assert_eq!(
            shell_wrap(&argv, Some("cmd")),
            vec!["cmd", "/k", "\"cargo build\""]
        );
        assert_eq!(
            shell_wrap(&argv, Some("bash")),
            vec!["bash", "-c", "'cargo' 'build'; exec bash"]
        );
        let ps = shell_wrap(&argv, Some("pwsh"));
        assert_eq!(&ps[..4], &["pwsh", "-NoLogo", "-NoExit", "-Command"]);
        assert_eq!(ps.last().unwrap(), "& 'cargo' 'build'");
        // None -> pwsh (or powershell) with the -Command wrap.
        assert!(shell_wrap(&argv, None).contains(&"-Command".to_string()));

        // A space in a path must survive the round-trip through each shell.
        let spaced = vec!["C:\\a b\\tool.exe".to_string(), "run".to_string()];
        assert_eq!(
            shell_wrap(&spaced, Some("pwsh")).last().unwrap(),
            "& 'C:\\a b\\tool.exe' 'run'"
        );
        assert_eq!(
            shell_wrap(&spaced, Some("cmd")).last().unwrap(),
            "\"\"C:\\a b\\tool.exe\" run\""
        );
    }

    #[test]
    fn prompt_action_yields_a_no_program_action_keeping_the_template() {
        let programs = std::collections::BTreeMap::new();
        let r = Resolver::new(&programs);
        let t = Tmpl {
            repo: "/r",
            path: "/r",
            rel: "",
            name: "r",
            file: None,
            resolver: &r,
        };
        let ra = crate::config::RuleAction {
            name: "npm run…".into(),
            run: Some("npm run {{input}}".into()),
            prompt: true,
            terminal: true,
            ..Default::default()
        };
        let a = build_action(&ra, "npm-run".into(), "General", "/r", &t, &r).unwrap();
        assert!(a.prompt);
        assert!(a.program.is_empty());
        assert_eq!(a.hint, "npm run {{input}}"); // {{input}} survives expand()
    }

    #[test]
    fn shell_split_handles_quotes() {
        assert_eq!(shell_split(r#"docker compose up -d"#), ["docker", "compose", "up", "-d"]);
        assert_eq!(
            shell_split(r#"code "my dir""#),
            ["code", "my dir"]
        );
        assert!(shell_split("").is_empty());
    }

    #[test]
    fn win_split_args_follows_commandline_to_argv_rules() {
        // An apostrophe is an ordinary character, not a quote.
        assert_eq!(win_split_args("--user=O'Brien"), ["--user=O'Brien"]);
        // Double quotes group; spaces inside survive.
        assert_eq!(
            win_split_args(r#"--fullscreen "--title=My Movie""#),
            ["--fullscreen", "--title=My Movie"]
        );
        // Backslash/quote interaction: \" is a literal quote, \\ a literal slash.
        assert_eq!(win_split_args(r#"a\"b"#), [r#"a"b"#]);
        assert_eq!(win_split_args(r#"a\\b"#), [r"a\\b"]);
        // "" inside a quoted run is one literal quote.
        assert_eq!(win_split_args(r#""a""b""#), [r#"a"b"#]);
        // A quoted path keeps its spaces and internal backslashes.
        assert_eq!(win_split_args(r#""C:\Some Path\x""#), [r"C:\Some Path\x"]);
        assert!(win_split_args("").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn terminalize_drives_known_emulators_and_templates() {
        let programs = std::collections::BTreeMap::new();
        let argv = vec!["cargo".to_string(), "test".to_string()];
        let cwd = "C:\\repo";
        // Absolute-path pins bypass the PATH lookup so the branch is deterministic.
        let pin = |p: &'static str, t: Option<&'static str>| {
            Resolver::new(&programs).with_terminal(Some(p), t)
        };

        let (p, a, c) = terminalize(&argv, cwd, &pin("C:/x/wt.exe", None), true);
        assert_eq!(p, "C:/x/wt.exe");
        assert_eq!(&a[..2], &["-d", cwd]);
        assert!(a.contains(&"-Command".to_string()));
        assert_eq!(a.last().unwrap(), "& 'cargo' 'test'");
        assert_eq!(c, None);

        let (_, a, _) = terminalize(&argv, cwd, &pin("C:/x/alacritty.exe", None), true);
        assert_eq!(&a[..2], &["--working-directory", cwd]);
        assert!(a.contains(&"-e".to_string()));

        let (_, a, _) = terminalize(&argv, cwd, &pin("C:/x/wezterm.exe", None), true);
        assert_eq!(&a[..4], &["start", "--cwd", cwd, "--"]);

        // Empty argv = "just open a terminal here" — no shell wrap.
        let (_, a, _) = terminalize(&[], cwd, &pin("C:/x/alacritty.exe", None), true);
        assert_eq!(a, vec!["--working-directory", cwd]);

        // wrap = false — argv handed to the emulator raw (e.g. an interactive shell).
        let (_, a, _) =
            terminalize(&["bash".to_string()], cwd, &pin("C:/x/wt.exe", None), false);
        assert_eq!(a, vec!["-d", cwd, "bash"]);

        // config.shell picks the wrapper.
        let r = Resolver::new(&programs)
            .with_terminal(Some("C:/x/wt.exe"), None)
            .with_shell(Some("cmd"));
        let (_, a, _) = terminalize(&argv, cwd, &r, true);
        assert_eq!(&a[2..], &["cmd", "/k", "\"cargo test\""]);

        // Unknown emulator, no template -> command handed to the binary + cwd.
        let (p, a, c) = terminalize(&argv, cwd, &pin("C:/x/kitty.exe", None), true);
        assert_eq!(p, "C:/x/kitty.exe");
        assert_eq!(a, argv);
        assert_eq!(c, Some(cwd.to_string()));

        // Template override wins over the table.
        let (p, a, c) = terminalize(
            &argv,
            cwd,
            &pin("C:/x/kitty.exe", Some("kitty --directory {{dir}} -- {{cmd}}")),
            true,
        );
        assert_eq!(p, "kitty");
        assert_eq!(a, vec!["--directory", cwd, "--", "cargo", "test"]);
        assert_eq!(c, None);

        // A space in cwd or in an argv element survives the template splice.
        let spaced = vec!["my tool".to_string(), "go".to_string()];
        let (_, a, _) = terminalize(
            &spaced,
            "C:\\my repo",
            &pin("C:/x/kitty.exe", Some("kitty --directory {{dir}} -- {{cmd}}")),
            true,
        );
        assert_eq!(a, vec!["--directory", "C:\\my repo", "--", "my tool", "go"]);
    }

    #[test]
    fn template_expands_known_vars() {
        let programs = std::collections::BTreeMap::new();
        let r = Resolver::new(&programs);
        let t = Tmpl {
            repo: "/r",
            path: "/r/web",
            rel: "web",
            name: "web",
            file: Some(Path::new("/r/web/App.sln")),
            resolver: &r,
        };
        assert_eq!(expand("{{path}} {{name}} {{file.stem}}", &t), "/r/web web App");
        assert_eq!(expand("no vars", &t), "no vars");
    }

    #[test]
    fn defaults_produce_universal_actions_with_one_default() {
        let cfg = bundled_defaults();
        let acts = build_actions(&repo(), &ctx_one(Project::default()), &cfg);
        assert!(acts.iter().any(|a| a.id == "copy-path" && a.client_side));
        assert_eq!(acts.iter().filter(|a| a.default).count(), 1);
        assert_eq!(acts.iter().find(|a| a.default).unwrap().id, "terminal");
    }

    #[test]
    fn node_project_yields_script_actions_via_provider() {
        let cfg = bundled_defaults();
        let proj = Project {
            rel: String::new(),
            files: vec!["package.json".into()],
            node: Some(NodeInfo {
                manager: PkgManager::Pnpm,
                scripts: vec!["dev".into(), "build".into()],
            }),
            ..Default::default()
        };
        let acts = build_actions(&repo(), &ctx_one(proj), &cfg);
        assert!(acts.iter().any(|a| a.id == "npm:root:dev" && a.group == "Detected"));
        assert!(acts.iter().any(|a| a.id == "npm:root:build"));
    }

    #[test]
    fn cargo_project_yields_cargo_actions() {
        let cfg = bundled_defaults();
        let proj = Project {
            files: vec!["Cargo.toml".into()],
            has_cargo: true,
            ..Default::default()
        };
        let acts = build_actions(&repo(), &ctx_one(proj), &cfg);
        for want in ["cargo:root:run", "cargo:root:build", "cargo:root:test"] {
            assert!(acts.iter().any(|a| a.id == want), "missing {want}");
        }
    }

    #[test]
    fn dotnet_provider_emits_build_run_and_test_per_project() {
        let d = std::env::temp_dir().join(format!(
            "dp-rules-dotnet-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("App.sln"),
            "Project(\"{X}\") = \"App\", \"App.csproj\", \"{Y}\"\n\
             Project(\"{X}\") = \"App.Tests\", \"App.Tests.csproj\", \"{Z}\"\n",
        )
        .unwrap();

        let proj = Project {
            dir: d.clone(),
            files: vec!["App.sln".into()],
            ..Default::default()
        };
        let programs = std::collections::BTreeMap::new();
        let r = Resolver::new(&programs);
        let cwd = d.to_string_lossy().into_owned();
        let acts = provider_actions(
            "dotnet",
            &crate::config::Rule::default(),
            &proj,
            "Detected",
            "root",
            &cwd,
            &r,
        );

        assert_eq!(
            acts.iter()
                .filter(|a| a.id.starts_with("dotnet:root:build:"))
                .count(),
            2
        );
        assert_eq!(
            acts.iter()
                .filter(|a| a.id.starts_with("dotnet:root:run:"))
                .count(),
            2
        );
        // only the `.Tests` project gets a `dotnet test` action
        assert_eq!(
            acts.iter()
                .filter(|a| a.id.starts_with("dotnet:root:test:"))
                .count(),
            1
        );
        assert!(acts.iter().any(|a| a.label == "dotnet build · App"));
        assert!(acts.iter().any(|a| a.label == "dotnet test · App.Tests"));
        assert!(acts.iter().all(|a| a.icon.as_deref() == Some("dotnet")));

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn workspace_providers_emit_one_action_set_per_module() {
        let d = std::env::temp_dir().join(format!(
            "dp-rules-ws-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("go.work"), "go 1.22\nuse ./api\nuse ./worker\n").unwrap();
        std::fs::write(
            d.join("pom.xml"),
            "<project><modules><module>svc-a</module><module>svc-b</module></modules></project>",
        )
        .unwrap();
        std::fs::write(
            d.join("settings.gradle"),
            "include ':app', 'core:data'\n",
        )
        .unwrap();

        let proj = Project {
            dir: d.clone(),
            ..Default::default()
        };
        let programs = std::collections::BTreeMap::new();
        let r = Resolver::new(&programs);
        let cwd = d.to_string_lossy().into_owned();
        let call = |name: &str| {
            provider_actions(
                name,
                &crate::config::Rule::default(),
                &proj,
                "Detected",
                "root",
                &cwd,
                &r,
            )
        };

        let go = call("go-work");
        assert_eq!(go.len(), 4); // build + test for api and worker
        assert!(go.iter().any(|a| a.label == "go build · api"));
        assert!(go.iter().all(|a| a.icon.as_deref() == Some("go")));

        let mvn = call("maven-modules");
        assert_eq!(mvn.len(), 4);
        assert!(mvn.iter().any(|a| a.label == "mvn test · svc-b"));
        assert!(mvn.iter().all(|a| a.icon.as_deref() == Some("java")));

        let gr = call("gradle-modules");
        assert_eq!(gr.len(), 4); // build + test for :app and :core:data
        assert!(gr.iter().any(|a| a.label == "gradle :core:data:build"));
        assert!(gr.iter().all(|a| a.icon.as_deref() == Some("gradle")));

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn python_provider_uses_venv_interpreter_and_django_pytest_run() {
        use crate::inspect::PythonInfo;
        let cfg = bundled_defaults();
        let proj = Project {
            rel: String::new(),
            dir: if cfg!(windows) {
                std::path::PathBuf::from("C:\\svc")
            } else {
                std::path::PathBuf::from("/svc")
            },
            files: vec!["requirements.txt".into(), "manage.py".into()],
            python: Some(PythonInfo {
                requirements: true,
                venv: Some(".venv".into()),
                manage_py: true,
                entry: Some("main.py".into()),
                pytest: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let acts = build_actions(&repo(), &ctx_one(proj), &cfg);

        let py_hint = acts
            .iter()
            .find(|a| a.id == "py:root:pip")
            .expect("pip install action")
            .hint
            .clone();
        // The venv interpreter, not a bare `python`.
        assert!(py_hint.contains(".venv"), "{py_hint}");
        assert!(py_hint.ends_with("-m pip install -r requirements.txt"), "{py_hint}");

        for want in ["py:root:manage:runserver", "py:root:manage:migrate", "py:root:pytest", "py:root:run"] {
            assert!(acts.iter().any(|a| a.id == want), "missing {want}");
        }
        assert!(acts
            .iter()
            .filter(|a| a.id.starts_with("py:root:"))
            .all(|a| a.icon.as_deref() == Some("python")));
    }

    #[test]
    fn flutter_android_reaches_into_the_android_subdir() {
        let d = std::env::temp_dir().join(format!(
            "dp-rules-flutter-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(d.join("android")).unwrap();
        std::fs::write(d.join("pubspec.yaml"), "name: child_flutter\n").unwrap();
        std::fs::write(
            d.join("android").join("settings.gradle.kts"),
            "include(\":app\")\n",
        )
        .unwrap();

        let proj = Project {
            dir: d.clone(),
            ..Default::default()
        };
        let programs = std::collections::BTreeMap::new();
        let r = Resolver::new(&programs);
        let cwd = d.to_string_lossy().into_owned();
        let acts = provider_actions(
            "flutter-android",
            &crate::config::Rule::default(),
            &proj,
            "Detected",
            "root",
            &cwd,
            &r,
        );

        assert_eq!(acts.len(), 2); // build + test for :app
        assert!(acts.iter().all(|a| a.icon.as_deref() == Some("gradle")));
        // no bundled wrapper in this fixture -> falls back to plain "gradle".
        // `hint` is the pre-terminalize argv, so it's the same on every OS.
        assert!(acts.iter().any(|a| a.hint == "gradle :app:build"));
        assert!(acts.iter().any(|a| a.hint == "gradle :app:test"));
        // The android/ dir made it through as the working directory — on
        // Windows that's baked into the terminal wrapper's `-d` arg rather
        // than `Action.cwd`.
        let android_str = d.join("android").to_string_lossy().into_owned();
        assert!(acts
            .iter()
            .all(|a| a.cwd.as_deref() == Some(android_str.as_str())
                || a.args.contains(&android_str)));

        // A Flutter project with no android/ dir at all yields nothing (no
        // panic, no crash) — the rule is self-gating.
        let no_android = std::env::temp_dir().join(format!(
            "dp-rules-flutter-bare-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&no_android).unwrap();
        let bare = Project {
            dir: no_android.clone(),
            ..Default::default()
        };
        assert!(provider_actions(
            "flutter-android",
            &crate::config::Rule::default(),
            &bare,
            "Detected",
            "root",
            &cwd,
            &r
        )
        .is_empty());

        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&no_android);
    }

    #[test]
    fn trace_explains_fired_and_idle_rules() {
        let cfg = bundled_defaults();
        let proj = Project {
            files: vec!["Cargo.toml".into()],
            has_cargo: true,
            ..Default::default()
        };
        let t = trace(&cfg, &ctx_one(proj), &repo());

        assert_eq!(t.repo_name, "demo");
        assert!(t.universal.iter().any(|id| id == "terminal"));

        // The Cargo rule fires against the root and lists its action ids.
        let cargo = t
            .rules
            .iter()
            .find(|r| r.globs.iter().any(|g| g.eq_ignore_ascii_case("Cargo.toml")))
            .expect("defaults carry a Cargo.toml rule");
        assert_eq!(cargo.gate, "");
        assert!(cargo
            .hits
            .iter()
            .any(|h| h.produced.iter().any(|id| id.starts_with("cargo:root:"))));

        // …and a bare Rust repo leaves plenty of rules idle, each with a reason.
        let idle = t.rules.iter().find(|r| !r.gate.is_empty()).unwrap();
        assert!(idle.hits.is_empty());
        assert!(!idle.gate.is_empty());
    }
}
