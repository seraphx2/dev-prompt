//! "New repository" (Ctrl+Shift+N on the repo list): create a folder under one
//! of the configured roots and run a `provision:` entry's commands inside it,
//! so the folder carries a VCS marker the scan recognizes.
//!
//! Nothing here opens a terminal — every command is non-interactive (stdin is
//! closed) and its output is captured, so a failure can be shown on the form.
//! When a command fails, the folder this call created is removed again.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::config::{self, Config, Provisioner};
use crate::error::{AppError, AppResult};
use crate::rules::which;
use crate::scan::{self, Repo};

/// Placeholders every `run` argument can use, besides the entry's own fields.
const BUILTIN_VARS: &[&str] = &["dir", "name", "root", "root_url"];

/// A repository setup shouldn't take long; anything past this is stuck
/// (waiting on a lock, a network share that went away, …).
const RUN_TIMEOUT: Duration = Duration::from_secs(60);
/// `default_from` runs while the form opens, so it gets far less.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Lines of a failing command's output shown on the form.
const ERROR_TAIL_LINES: usize = 12;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewRepoOptions {
    /// Configured roots that exist right now, expanded.
    pub roots: Vec<String>,
    /// `provision:` entries whose programs are all installed.
    pub tools: Vec<ToolView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    pub id: String,
    pub label: String,
    pub fields: Vec<FieldView>,
    /// The raw `run` templates, for the form's command preview.
    pub run: Vec<Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldView {
    pub id: String,
    pub label: String,
    /// `default_from`'s output, else `default`.
    pub default: String,
}

/// Everything the "New repository" form needs. Runs each `default_from`
/// command, so call it off the UI thread.
pub fn options(cfg: &Config) -> NewRepoOptions {
    let mut roots: Vec<String> = Vec::new();
    for r in config::resolved_roots(cfg) {
        let r = r.to_string_lossy().into_owned();
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    let tools = cfg
        .provision
        .iter()
        .filter(|p| is_available(p))
        .map(|p| ToolView {
            id: p.id.clone(),
            label: if p.label.is_empty() { p.id.clone() } else { p.label.clone() },
            fields: p
                .fields
                .iter()
                .map(|f| FieldView {
                    id: f.id.clone(),
                    label: if f.label.is_empty() { f.id.clone() } else { f.label.clone() },
                    default: field_default(&f.default_from).unwrap_or_else(|| f.default.clone()),
                })
                .collect(),
            run: p.run.clone(),
        })
        .collect();
    NewRepoOptions { roots, tools }
}

/// Validate a `provision:` entry's shape: field ids are usable placeholders,
/// and every `{{…}}` in `run` names a field or a built-in variable.
pub fn check_template(p: &Provisioner) -> Result<(), String> {
    if p.id.trim().is_empty() {
        return Err("an entry has no `id`".into());
    }
    if p.run.is_empty() || p.run.iter().any(|argv| argv.is_empty()) {
        return Err(format!("`{}`: `run` needs at least one non-empty command", p.id));
    }
    let mut vars: HashMap<&str, String> = BUILTIN_VARS.iter().map(|v| (*v, String::new())).collect();
    for f in &p.fields {
        let valid = !f.id.is_empty()
            && f.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid {
            return Err(format!("`{}`: field id `{}` must be letters, digits, `-` or `_`", p.id, f.id));
        }
        if vars.insert(f.id.as_str(), String::new()).is_some() {
            return Err(format!("`{}`: field id `{}` is taken (built-in or repeated)", p.id, f.id));
        }
    }
    for arg in p.run.iter().flatten() {
        expand(arg, &vars).map_err(|e| format!("`{}`: {e}", p.id))?;
    }
    Ok(())
}

/// Whether any `provision:` entry is installed — gates the "Set up version
/// control…" action. `which` is memoized, so this is cheap after the first call.
pub fn any_available(cfg: &Config) -> bool {
    cfg.provision.iter().any(is_available)
}

/// Offered only when the entry is well-formed and everything it runs is
/// installed — a missing tool just doesn't appear.
fn is_available(p: &Provisioner) -> bool {
    check_template(p).is_ok()
        && p.requires.iter().all(|b| which(b).is_some())
        && p.run
            .iter()
            .filter(|argv| !argv[0].contains("{{"))
            .all(|argv| resolve_program(&argv[0]).is_some())
}

/// An absolute program path is used as-is when it exists; a bare name is
/// looked up on `PATH`.
fn resolve_program(program: &str) -> Option<String> {
    if !program.contains(['/', '\\']) {
        return which(program);
    }
    // Under Flatpak an absolute path is a host path, which the sandbox can't
    // stat — `flatpak-spawn` reports it if it's missing.
    if crate::launch::in_flatpak() || Path::new(program).is_file() {
        return Some(program.to_string());
    }
    None
}

fn field_default(argv: &[String]) -> Option<String> {
    let (program, args) = argv.split_first()?;
    let program = resolve_program(program)?;
    let cwd = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    let out = run_captured(&program, args, &cwd, DEFAULT_TIMEOUT).ok()?;
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(String::from)
}

/// Replace every `{{key}}` in `s`; an unknown key is an error, not left in.
fn expand(s: &str, vars: &HashMap<&str, String>) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| format!("unclosed `{{{{` in `{s}`"))?;
        let key = after[..end].trim();
        let val = vars
            .get(key)
            .ok_or_else(|| format!("unknown placeholder `{{{{{key}}}}}` in `{s}`"))?;
        out.push_str(val);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `file://` URL for an absolute path — `D:\git` → `file:///D:/git`,
/// `/home/me/git` → `file:///home/me/git` — for tools that address a local
/// repository by URL (`svn checkout`). Anything outside the URL-safe set is
/// percent-encoded byte by byte.
fn file_url(path: &str) -> String {
    let path = path.replace('\\', "/");
    let path = path.trim_end_matches('/');
    let mut out = String::from("file://");
    if !path.starts_with('/') {
        out.push('/');
    }
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// --- the folder name ------------------------------------------------------

#[cfg(windows)]
const FORBIDDEN_CHARS: &[char] = &['/', '\\', '<', '>', ':', '"', '|', '?', '*'];
#[cfg(not(windows))]
const FORBIDDEN_CHARS: &[char] = &['/', '\\'];

/// Names Windows reserves for devices, with or without an extension.
#[cfg(windows)]
fn is_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| {
            stem.strip_prefix(p)
                .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
        })
}

/// Is `name` usable as a single new folder name? `Err` carries the reason,
/// worded for the form.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Enter a folder name.".into());
    }
    if name != name.trim() {
        return Err("The name can't start or end with a space.".into());
    }
    if name == "." || name == ".." {
        return Err(format!("\"{name}\" isn't a folder name."));
    }
    if let Some(c) = name.chars().find(|c| FORBIDDEN_CHARS.contains(c)) {
        return Err(format!("The name can't contain \"{c}\"."));
    }
    if name.chars().any(char::is_control) {
        return Err("The name can't contain control characters.".into());
    }
    if name.len() > 255 {
        return Err("That name is too long.".into());
    }
    #[cfg(windows)]
    {
        if name.ends_with('.') {
            return Err("On Windows a folder name can't end with \".\".".into());
        }
        if is_reserved_name(name) {
            return Err(format!("\"{name}\" is a name Windows reserves."));
        }
    }
    Ok(())
}

/// `root` must be one of the configured roots as [`options`] reported it.
fn known_root(cfg: &Config, root: &str) -> Option<PathBuf> {
    config::resolved_roots(cfg)
        .into_iter()
        .find(|r| r.as_path() == Path::new(root))
}

/// The folder `create` would make, or why it can't. Feeds the form's
/// as-you-type check.
pub fn check_target(cfg: &Config, root: &str, name: &str) -> Result<PathBuf, String> {
    let root = known_root(cfg, root).ok_or("Pick a location.")?;
    check_name(name)?;
    let dir = root.join(name);
    // `symlink_metadata` so a dangling link still counts as taken.
    if dir.symlink_metadata().is_ok() {
        return Err(format!("\"{name}\" already exists there."));
    }
    Ok(dir)
}

// --- creating it ----------------------------------------------------------

/// Create `<root>/<name>` and run provisioner `tool` in it. On success the new
/// repo is returned as the scan would see it. If a command fails, the folder
/// is removed again — this call created it, so everything in it came from the
/// failed setup.
pub fn create(
    cfg: &Config,
    root: &str,
    name: &str,
    tool: &str,
    values: &HashMap<String, String>,
) -> AppResult<Repo> {
    let dir = check_target(cfg, root, name).map_err(AppError::msg)?;
    let p = installed(cfg, tool)?;
    let commands = bind(p, root, &dir, name, values).map_err(AppError::msg)?;

    std::fs::create_dir(&dir).map_err(|e| {
        AppError::msg(format!("Couldn't create {}: {e}", dir.display()))
    })?;

    run_all(&commands, &dir, || match std::fs::remove_dir_all(&dir) {
        Ok(()) => String::new(),
        Err(re) => format!("\n\nThe folder couldn't be removed ({re}): {}", dir.display()),
    })?;
    versioned(cfg, p, &dir)
}

/// Run provisioner `tool` in an existing folder ("Set up version control…" on
/// a folder that has none). If a command fails, only what the setup added to
/// the top of the folder is removed — everything that was there before stays,
/// though a command that *changed* an existing file isn't undone.
pub fn init_existing(
    cfg: &Config,
    path: &str,
    tool: &str,
    values: &HashMap<String, String>,
) -> AppResult<Repo> {
    let dir = PathBuf::from(path);
    if !dir.is_dir() {
        return Err(AppError::msg(format!("{path} isn't a folder any more.")));
    }
    // The deepest root it sits in — `{{root}}` for recipes that need one.
    let root = config::resolved_roots(cfg)
        .into_iter()
        .filter(|r| dir.starts_with(r) && dir != *r)
        .max_by_key(|r| r.as_os_str().len())
        .ok_or_else(|| AppError::msg(format!("{path} isn't inside any of your root folders.")))?;
    if scan::probe(&dir, cfg).is_some_and(|r| r.vcs.is_some()) {
        return Err(AppError::msg(format!("{path} already has version control.")));
    }
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let p = installed(cfg, tool)?;
    let commands =
        bind(p, &root.to_string_lossy(), &dir, &name, values).map_err(AppError::msg)?;

    let before = entries(&dir).map_err(|e| AppError::msg(format!("Couldn't read {path}: {e}")))?;
    run_all(&commands, &dir, || remove_added(&dir, &before))?;
    versioned(cfg, p, &dir)
}

/// The `provision:` entry `tool`, if it's installed.
fn installed<'a>(cfg: &'a Config, tool: &str) -> AppResult<&'a Provisioner> {
    let p = cfg
        .provision
        .iter()
        .find(|p| p.id == tool)
        .ok_or_else(|| AppError::msg(format!("No version-control setup called \"{tool}\".")))?;
    if !is_available(p) {
        return Err(AppError::msg(format!("{} isn't available on this machine.", p.label)));
    }
    Ok(p)
}

/// Run every command in `dir`, stopping at the first failure. `undo` runs
/// then and returns any extra text for the error (why cleanup fell short).
fn run_all(commands: &[Vec<String>], dir: &Path, undo: impl FnOnce() -> String) -> AppResult<()> {
    for argv in commands {
        if let Err(e) = run_step(argv, dir) {
            let cleanup = undo();
            return Err(AppError::msg(format!("{e}{cleanup}")));
        }
    }
    Ok(())
}

/// The repo the finished setup left, or — when it created no version-control
/// marker dev-prompt knows — an error saying so. The folder is kept either way:
/// the commands themselves succeeded.
fn versioned(cfg: &Config, p: &Provisioner, dir: &Path) -> AppResult<Repo> {
    match scan::probe(dir, cfg) {
        Some(repo) if repo.vcs.is_some() => Ok(repo),
        _ => Err(AppError::msg(format!(
            "{} finished, but left no version-control marker dev-prompt recognizes. \
             The folder was kept: {}. Add the marker it creates to `markers:` in \
             rules.yaml, with `kind: vcs`.",
            p.label,
            dir.display()
        ))),
    }
}

/// Names directly inside `dir`.
fn entries(dir: &Path) -> std::io::Result<HashSet<OsString>> {
    Ok(std::fs::read_dir(dir)?.flatten().map(|e| e.file_name()).collect())
}

/// Remove whatever appeared at the top of `dir` since `before` was taken.
/// Returns extra error text naming anything that couldn't be removed.
fn remove_added(dir: &Path, before: &HashSet<OsString>) -> String {
    let added: Vec<PathBuf> = match entries(dir) {
        Ok(now) => now.difference(before).map(|n| dir.join(n)).collect(),
        Err(e) => return format!("\n\nCouldn't check {} for leftovers: {e}", dir.display()),
    };
    let stuck: Vec<String> = added
        .iter()
        .filter_map(|p| {
            let removed = match p.symlink_metadata() {
                Ok(m) if m.is_dir() => std::fs::remove_dir_all(p),
                // A link (to a file or a folder) is removed itself, never followed.
                Ok(_) => std::fs::remove_file(p).or_else(|_| std::fs::remove_dir(p)),
                Err(_) => Ok(()), // already gone
            };
            removed.err().map(|e| format!("{} ({e})", p.display()))
        })
        .collect();
    if stuck.is_empty() {
        String::new()
    } else {
        format!("\n\nThese couldn't be removed:\n{}", stuck.join("\n"))
    }
}

/// `p.run` with every placeholder filled in. Every field needs a value.
fn bind(
    p: &Provisioner,
    root: &str,
    dir: &Path,
    name: &str,
    values: &HashMap<String, String>,
) -> Result<Vec<Vec<String>>, String> {
    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("dir", dir.to_string_lossy().into_owned());
    vars.insert("name", name.to_string());
    vars.insert("root", root.to_string());
    vars.insert("root_url", file_url(root));
    for f in &p.fields {
        let label = if f.label.is_empty() { &f.id } else { &f.label };
        let v = values.get(&f.id).map(|v| v.trim()).unwrap_or_default();
        if v.is_empty() {
            return Err(format!("Fill in \"{label}\"."));
        }
        vars.insert(f.id.as_str(), v.to_string());
    }
    p.run
        .iter()
        .map(|argv| argv.iter().map(|a| expand(a, &vars)).collect())
        .collect()
}

fn run_step(argv: &[String], cwd: &Path) -> Result<String, String> {
    let shown = display_command(argv);
    let program = resolve_program(&argv[0]).ok_or_else(|| format!("`{}` isn't on PATH.", argv[0]))?;
    run_captured(&program, &argv[1..], cwd, RUN_TIMEOUT).map_err(|e| format!("`{shown}` failed: {e}"))
}

fn display_command(argv: &[String]) -> String {
    argv.iter()
        .map(|a| if a.contains(' ') || a.is_empty() { format!("\"{a}\"") } else { a.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Run `program` to completion with stdin closed and output captured, killing
/// it after `timeout`. `Ok` is its stdout; `Err` explains the failure using
/// the tail of what it printed.
fn run_captured(program: &str, args: &[String], cwd: &Path, timeout: Duration) -> Result<String, String> {
    let mut cmd = command_for(program, args, cwd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start it ({e})"))?;

    // Drain both pipes on their own threads so a chatty command can't fill one
    // and block while we wait on it.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("it was still running after {}s and was stopped.", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("couldn't wait for it ({e})")),
        }
    };
    let stdout = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    if status.success() {
        return Ok(stdout);
    }

    let said = if stderr.trim().is_empty() { &stdout } else { &stderr };
    let lines: Vec<&str> = said.trim().lines().collect();
    let tail = lines[lines.len().saturating_sub(ERROR_TAIL_LINES)..].join("\n");
    let code = status.code().map(|c| format!("exit code {c}")).unwrap_or_else(|| "it was killed".into());
    Err(if tail.is_empty() { format!("{code}.") } else { format!("{code}.\n{tail}") })
}

#[cfg(windows)]
fn command_for(program: &str, args: &[String], cwd: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = Command::new(program);
    c.args(args).current_dir(cwd).creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(not(windows))]
fn command_for(program: &str, args: &[String], cwd: &Path) -> Command {
    // Inside Flatpak the version-control tools live on the host — same route
    // `launch.rs` takes, but waited on rather than detached.
    if crate::launch::in_flatpak() {
        let mut c = Command::new("flatpak-spawn");
        c.arg("--host")
            .arg(format!("--directory={}", cwd.display()))
            .arg("--")
            .arg(program)
            .args(args);
        return c;
    }
    let mut c = Command::new(program);
    c.args(args).current_dir(cwd);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProvisionField;

    fn vars(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn expand_fills_known_placeholders_and_rejects_unknown_ones() {
        let v = vars(&[("branch", "main"), ("name", "demo")]);
        assert_eq!(expand("--initial-branch={{branch}}", &v).unwrap(), "--initial-branch=main");
        assert_eq!(expand("{{ name }}/x", &v).unwrap(), "demo/x");
        assert_eq!(expand("plain", &v).unwrap(), "plain");
        assert!(expand("{{nope}}", &v).unwrap_err().contains("nope"));
        assert!(expand("{{branch", &v).unwrap_err().contains("unclosed"));
    }

    #[test]
    fn file_url_handles_drive_letters_unix_paths_and_spaces() {
        assert_eq!(file_url(r"D:\git"), "file:///D:/git");
        assert_eq!(file_url(r"D:\git\"), "file:///D:/git");
        assert_eq!(file_url("/home/me/git"), "file:///home/me/git");
        assert_eq!(file_url(r"C:\My Code"), "file:///C:/My%20Code");
        assert_eq!(file_url("/srv/ü"), "file:///srv/%C3%BC");
    }

    /// The Subversion recipe in docs/rules-engine.md, verbatim. Keep the two in
    /// step: this is what proves the documented YAML parses and expands.
    const SVN_RECIPE: &str = r#"
provision:
  - id: svn
    label: Subversion
    requires: [svnadmin, svn]
    run:
      - [svnadmin, create, "{{root}}/.svn-repos/{{name}}"]
      - [svn, checkout, "{{root_url}}/.svn-repos/{{name}}", "{{dir}}"]
"#;

    #[test]
    fn the_documented_svn_recipe_parses_and_expands() {
        let ov: config::RuleOverrides = serde_yaml_ng::from_str(SVN_RECIPE).unwrap();
        let p = &ov.provision[0];
        assert!(check_template(p).is_ok(), "{:?}", check_template(p));

        let cmds = bind(p, r"D:\git", Path::new(r"D:\git\demo"), "demo", &HashMap::new()).unwrap();
        assert_eq!(cmds[0], vec!["svnadmin", "create", r"D:\git/.svn-repos/demo"]);
        assert_eq!(
            cmds[1],
            vec!["svn", "checkout", "file:///D:/git/.svn-repos/demo", r"D:\git\demo"]
        );

        let cmds = bind(p, "/home/me/code", Path::new("/home/me/code/demo"), "demo", &HashMap::new()).unwrap();
        assert_eq!(cmds[1][2], "file:///home/me/code/.svn-repos/demo");
    }

    #[test]
    fn check_template_validates_fields_and_placeholders() {
        let mut p = Provisioner {
            id: "x".into(),
            label: "X".into(),
            run: vec![vec!["x".into(), "{{dir}}".into(), "{{flavor}}".into()]],
            fields: vec![ProvisionField { id: "flavor".into(), ..Default::default() }],
            ..Default::default()
        };
        assert!(check_template(&p).is_ok());

        p.fields[0].id = "dir".into(); // shadows a built-in
        assert!(check_template(&p).is_err());

        p.fields[0].id = "has space".into();
        assert!(check_template(&p).is_err());

        p.fields.clear(); // {{flavor}} no longer bound
        assert!(check_template(&p).is_err());

        p.run = vec![vec![]];
        assert!(check_template(&p).is_err());
    }

    #[test]
    fn check_name_rejects_paths_and_odd_names() {
        assert!(check_name("my-new-thing").is_ok());
        assert!(check_name("with space inside").is_ok());
        for bad in ["", " lead", "trail ", ".", "..", "a/b", "a\\b", "tab\there"] {
            assert!(check_name(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(check_name(&"x".repeat(256)).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn check_name_applies_windows_rules() {
        for bad in ["a:b", "what?", "dot.", "CON", "nul.txt", "Com1", "lpt9.md"] {
            assert!(check_name(bad).is_err(), "{bad:?} should be rejected");
        }
        for ok in ["console", "COM0", "COM10", "auxiliary"] {
            assert!(check_name(ok).is_ok(), "{ok:?} should be allowed");
        }
    }

    /// A config whose only root is a fresh temp dir and whose only provisioner
    /// runs `program` with `args`.
    fn sandbox(tag: &str, program: &str, args: &[&str]) -> (Config, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "dp-provision-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut cfg = config::bundled_defaults();
        cfg.roots = vec![root.to_string_lossy().into_owned()];
        let mut argv = vec![program.to_string()];
        argv.extend(args.iter().map(|a| a.to_string()));
        cfg.provision = vec![Provisioner {
            id: "t".into(),
            label: "Test".into(),
            run: vec![argv],
            ..Default::default()
        }];
        (cfg, root)
    }

    #[test]
    fn check_target_needs_a_known_root_and_a_free_name() {
        let (cfg, root) = sandbox("target", "git", &["init"]);
        let r = root.to_string_lossy().into_owned();
        assert!(check_target(&cfg, &r, "fresh").is_ok());
        assert_eq!(check_target(&cfg, "/not/a/root", "fresh").unwrap_err(), "Pick a location.");
        std::fs::create_dir(root.join("taken")).unwrap();
        assert!(check_target(&cfg, &r, "taken").unwrap_err().contains("already exists"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_runs_git_init_and_returns_the_repo() {
        if which("git").is_none() {
            return; // nothing to provision with on this machine
        }
        let (mut cfg, root) = sandbox("git", "git", &[]);
        cfg.provision = config::bundled_defaults().provision;
        let r = root.to_string_lossy().into_owned();
        let values = HashMap::from([("branch".to_string(), "trunk".to_string())]);

        let repo = create(&cfg, &r, "made", "git", &values).unwrap();
        assert_eq!(repo.name, "made");
        assert_eq!(repo.vcs.as_deref(), Some("Git"));
        let head = std::fs::read_to_string(root.join("made/.git/HEAD")).unwrap();
        assert!(head.contains("refs/heads/trunk"), "{head}");

        // Same name again: refused before anything runs.
        assert!(create(&cfg, &r, "made", "git", &values).is_err());
        // Missing field value: refused, nothing created.
        assert!(create(&cfg, &r, "other", "git", &HashMap::new()).is_err());
        assert!(!root.join("other").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failing_command_removes_the_folder_it_was_given() {
        if which("git").is_none() {
            return;
        }
        // `git init --bogus` writes nothing and exits non-zero.
        let (cfg, root) = sandbox("fail", "git", &["init", "--no-such-flag"]);
        let r = root.to_string_lossy().into_owned();
        let err = create(&cfg, &r, "doomed", "t", &HashMap::new()).unwrap_err().to_string();
        assert!(err.contains("git init --no-such-flag"), "{err}");
        assert!(!root.join("doomed").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_partial_setup_is_removed_too() {
        if which("git").is_none() {
            return;
        }
        // First command succeeds (leaves `.git`), the second fails.
        let (mut cfg, root) = sandbox("partial", "git", &["init"]);
        cfg.provision[0].run.push(vec!["git".into(), "no-such-subcommand".into()]);
        let r = root.to_string_lossy().into_owned();
        assert!(create(&cfg, &r, "half", "t", &HashMap::new()).is_err());
        assert!(!root.join("half").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn init_existing_versions_a_folder_and_keeps_its_contents() {
        if which("git").is_none() {
            return;
        }
        let (mut cfg, root) = sandbox("existing", "git", &[]);
        cfg.provision = config::bundled_defaults().provision;
        let dir = root.join("notes");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("todo.md"), "keep me").unwrap();
        let values = HashMap::from([("branch".to_string(), "main".to_string())]);

        let repo = init_existing(&cfg, &dir.to_string_lossy(), "git", &values).unwrap();
        assert_eq!(repo.vcs.as_deref(), Some("Git"));
        assert_eq!(std::fs::read_to_string(dir.join("todo.md")).unwrap(), "keep me");

        // A second time: it already has version control.
        let err = init_existing(&cfg, &dir.to_string_lossy(), "git", &values).unwrap_err();
        assert!(err.to_string().contains("already has version control"), "{err}");

        // Outside every root: refused.
        let outside = std::env::temp_dir();
        assert!(init_existing(&cfg, &outside.to_string_lossy(), "git", &values).is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn init_existing_failure_removes_only_what_setup_added() {
        if which("git").is_none() {
            return;
        }
        // `git init` succeeds (adds `.git`), then the second command fails.
        let (mut cfg, root) = sandbox("existing-fail", "git", &["init"]);
        cfg.provision[0].run.push(vec!["git".into(), "no-such-subcommand".into()]);
        let dir = root.join("photos");
        std::fs::create_dir_all(dir.join("2024")).unwrap();
        std::fs::write(dir.join("readme.txt"), "mine").unwrap();
        std::fs::write(dir.join("2024/a.jpg"), "jpg").unwrap();

        assert!(init_existing(&cfg, &dir.to_string_lossy(), "t", &HashMap::new()).is_err());
        assert!(!dir.join(".git").exists(), "the half-made .git should be gone");
        assert_eq!(std::fs::read_to_string(dir.join("readme.txt")).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(dir.join("2024/a.jpg")).unwrap(), "jpg");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_or_uninstalled_tools_are_refused() {
        let (cfg, root) = sandbox("missing", "dp-surely-not-installed", &[]);
        let r = root.to_string_lossy().into_owned();
        assert!(options(&cfg).tools.is_empty());
        assert!(create(&cfg, &r, "x", "t", &HashMap::new()).is_err());
        assert!(create(&cfg, &r, "x", "nope", &HashMap::new()).is_err());
        assert!(!root.join("x").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
