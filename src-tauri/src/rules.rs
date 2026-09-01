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
}

impl<'a> Resolver<'a> {
    pub fn new(programs: &'a std::collections::BTreeMap<String, ProgramSpec>) -> Self {
        Self { programs }
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
    let vswhere = ["ProgramFiles(x86)", "ProgramFiles"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(|base| format!("{base}\\Microsoft Visual Studio\\Installer\\vswhere.exe"))
        .find(|p| Path::new(p).is_file())?;
    let out = Command::new(vswhere)
        .args(args.split_whitespace())
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
        _ => {
            if let Some(var) = key.strip_prefix("env:") {
                std::env::var(var).unwrap_or_default()
            } else {
                t.resolver.resolve(key).unwrap_or_default()
            }
        }
    }
}

/// Quote-aware split for `run:` strings.
fn shell_split(s: &str) -> Vec<String> {
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

/// `(program, args, cwd)` to run `argv` in a terminal at `cwd`. Empty `argv`
/// means "just open a terminal there".
fn terminalize(argv: &[String], cwd: &str, resolver: &Resolver) -> (String, Vec<String>, Option<String>) {
    #[cfg(windows)]
    {
        let term = resolver
            .resolve("terminal")
            .unwrap_or_else(|| "wt.exe".to_string());
        let mut args = vec!["-d".to_string(), cwd.to_string()];
        if !argv.is_empty() {
            // Run the command *inside* a shell so it keeps a real console — ANSI
            // colour, a live TTY, and the tab stays open after it exits (WT
            // otherwise closes a bare `wt <cmd>` tab on exit, and the command's
            // stdout isn't a tty so tools like Claude Code render monochrome).
            let shell = if which("pwsh").is_some() {
                "pwsh"
            } else {
                "powershell"
            };
            args.extend([
                shell.to_string(),
                "-NoLogo".to_string(),
                "-NoExit".to_string(),
                "-Command".to_string(),
            ]);
        }
        args.extend(argv.iter().cloned());
        (term, args, None)
    }
    #[cfg(not(windows))]
    {
        // No per-emulator working-dir flag knowledge yet (its own milestone):
        // run the command directly in `cwd`.
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
        });
    }

    if ra.needs.iter().any(|k| resolver.resolve(k).is_none()) {
        return None;
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
        terminalize(&argv, cwd, resolver)
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
        let (p, a, c) = terminalize(&argv, at, resolver);
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
        "gradle-modules" => "gradle",
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
            let mut v = Vec::new();
            if py.requirements {
                v.push(term(
                    format!("py:{ns}:pip"),
                    "pip install -r requirements.txt".into(),
                    vec![
                        "pip".into(),
                        "install".into(),
                        "-r".into(),
                        "requirements.txt".into(),
                    ],
                ));
            }
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
                _ => {}
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
    let resolver = Resolver::new(&config.programs);

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
    let resolver = Resolver::new(&config.programs);

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
