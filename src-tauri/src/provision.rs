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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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
pub(crate) fn resolve_program(program: &str) -> Option<String> {
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

pub(crate) fn display_command(argv: &[String]) -> String {
    argv.iter()
        .map(|a| if a.contains(' ') || a.is_empty() { format!("\"{a}\"") } else { a.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Run `program` to completion with stdin closed and output captured, killing
/// it after `timeout`. `Ok` is its stdout; `Err` explains the failure using
/// the tail of what it printed.
pub(crate) fn run_captured(program: &str, args: &[String], cwd: &Path, timeout: Duration) -> Result<String, String> {
    run_captured_env(program, args, cwd, timeout, &[])
}

// --- reads a screen is waiting on ------------------------------------------

thread_local! {
    /// Set while this thread runs a read some screen is waiting on (see
    /// [`cancellable`]); every captured command checks it.
    static CANCEL: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) };
}

fn cancelled() -> bool {
    CANCEL.with(|c| c.borrow().as_ref().is_some_and(|t| t.load(Ordering::Relaxed)))
}

/// Run `f` — a read whose every command goes through [`run_captured_env`] on
/// this thread — so that setting `token` stops the command under way (and all
/// it started) and skips the rest. For a read a screen is waiting on: leave the
/// screen, and nothing is left running in the repo.
pub(crate) fn cancellable<T>(token: Arc<AtomicBool>, f: impl FnOnce() -> T) -> T {
    // Cleared however `f` ends, a panic included: the thread goes back to a
    // pool, and a token left on it would cancel whatever runs there next.
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            CANCEL.with(|c| *c.borrow_mut() = None);
        }
    }
    CANCEL.with(|c| *c.borrow_mut() = Some(token));
    let _clear = Clear;
    f()
}

/// The reads screens are waiting on, by kind ("removal", "branches",
/// "details"): starting one cancels the last of its kind, and leaving the
/// screen cancels it.
#[derive(Default)]
pub struct Reads(Mutex<std::collections::HashMap<&'static str, Arc<AtomicBool>>>);

impl Reads {
    pub fn start(&self, kind: &'static str) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        if let Some(old) = self.0.lock().unwrap().insert(kind, token.clone()) {
            old.store(true, Ordering::Relaxed);
        }
        token
    }

    pub fn cancel(&self, kind: &str) {
        if let Some(t) = self.0.lock().unwrap().remove(kind) {
            t.store(true, Ordering::Relaxed);
        }
    }
}

/// [`run_captured`] with extra environment variables set for the command.
pub(crate) fn run_captured_env(
    program: &str,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
    env: &[(&str, &str)],
) -> Result<String, String> {
    if cancelled() {
        return Err("cancelled.".into());
    }
    let mut cmd = command_for(program, args, cwd, env);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start it ({e})"))?;
    let tree = ProcessTree::adopt(&child);

    // Drain both pipes on their own threads so a chatty command can't fill one
    // and block while we wait on it.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() >= timeout => {
                tree.stop();
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("it was still running after {}s and was stopped.", timeout.as_secs()));
            }
            Ok(None) if cancelled() => {
                tree.stop();
                let _ = child.kill();
                let _ = child.wait();
                return Err("cancelled.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("couldn't wait for it ({e})")),
        }
    };
    // The pipes close when the last process holding them exits — which can be
    // a helper the command started and left running. Waiting on that would
    // ignore the time limit, so the output gets what's left of it (and a
    // moment more), then counts as a failure.
    let deadline = started + timeout;
    let collect = |rx: std::sync::mpsc::Receiver<Vec<u8>>| {
        let left = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(500));
        rx.recv_timeout(left).ok()
    };
    let (Some(out), Some(err)) = (collect(out), collect(err)) else {
        tree.stop();
        return Err("it finished, but a process it started kept its output open past the time limit.".into());
    };
    let stdout = String::from_utf8_lossy(&out).into_owned();
    let stderr = String::from_utf8_lossy(&err).into_owned();
    if status.success() {
        return Ok(stdout);
    }

    let said = if stderr.trim().is_empty() { &stdout } else { &stderr };
    let lines: Vec<&str> = said.trim().lines().collect();
    let tail = lines[lines.len().saturating_sub(ERROR_TAIL_LINES)..].join("\n");
    let code = status.code().map(|c| format!("exit code {c}")).unwrap_or_else(|| "it was killed".into());
    Err(if tail.is_empty() { format!("{code}.") } else { format!("{code}.\n{tail}") })
}

/// A captured command and everything it starts in turn, so a time limit
/// stops all of it. Git for Windows' `git.exe` on PATH is a launcher that runs
/// the real one: stopping only the launcher leaves the real Git running, its
/// working folder inside the repo — which then can't be moved to the Recycle
/// Bin or deleted. Windows groups them in a job object; Linux in a process
/// group (see `command_for`). Best effort: where neither can be had, the
/// command itself is still stopped.
struct ProcessTree {
    #[cfg(windows)]
    job: Option<windows::Win32::Foundation::HANDLE>,
    #[cfg(unix)]
    group: i32,
}

impl ProcessTree {
    #[cfg(windows)]
    fn adopt(child: &std::process::Child) -> Self {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};
        // It starts suspended (see `command_for`): in the job before it runs a
        // single instruction, so nothing it starts can be outside it. Then off
        // it goes — whether or not the job could be had.
        let job = unsafe { CreateJobObjectW(None, None) }.ok().filter(|job| unsafe {
            AssignProcessToJobObject(*job, HANDLE(child.as_raw_handle())).is_ok() || {
                let _ = CloseHandle(*job);
                false
            }
        });
        resume(child.id());
        Self { job }
    }

    #[cfg(windows)]
    /// Terminating a job only starts it: wait (a few seconds at most) until
    /// none of its processes are left, so they've let go of the folder before
    /// anyone tries to remove it.
    fn stop(&self) {
        use windows::Win32::System::JobObjects::{
            JobObjectBasicAccountingInformation, QueryInformationJobObject, TerminateJobObject,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        };
        let Some(job) = self.job else { return };
        let _ = unsafe { TerminateJobObject(job, 1) };
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            let asked = unsafe {
                QueryInformationJobObject(
                    Some(job),
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    None,
                )
            };
            if asked.is_err() || info.ActiveProcesses == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[cfg(unix)]
    fn adopt(child: &std::process::Child) -> Self {
        Self { group: child.id() as i32 }
    }

    /// The whole group: asked first (`flatpak-spawn` passes that on to the
    /// host side), then made.
    #[cfg(unix)]
    fn stop(&self) {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        const SIGTERM: i32 = 15;
        const SIGKILL: i32 = 9;
        unsafe { kill(-self.group, SIGTERM) };
        std::thread::sleep(Duration::from_millis(200));
        unsafe { kill(-self.group, SIGKILL) };
    }
}

/// Resume the threads of process `pid` (one, for a process started
/// suspended). The standard library doesn't hand out a child's thread, so it's
/// found in Windows' own list of them.
#[cfg(windows)]
fn resume(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return };
        let mut t = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut more = Thread32First(snap, &mut t).is_ok();
        while more {
            if t.th32OwnerProcessID == pid {
                if let Ok(thread) = OpenThread(THREAD_SUSPEND_RESUME, false, t.th32ThreadID) {
                    ResumeThread(thread);
                    let _ = CloseHandle(thread);
                }
            }
            more = Thread32Next(snap, &mut t).is_ok();
        }
        let _ = CloseHandle(snap);
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        if let Some(job) = self.job {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(job) };
        }
    }
}

#[cfg(windows)]
fn command_for(program: &str, args: &[String], cwd: &Path, env: &[(&str, &str)]) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Suspended until `ProcessTree::adopt` has it in its job and resumes it.
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    let mut c = Command::new(program);
    c.args(args)
        .current_dir(cwd)
        .envs(env.iter().copied())
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    c
}

#[cfg(not(windows))]
fn command_for(program: &str, args: &[String], cwd: &Path, env: &[(&str, &str)]) -> Command {
    use std::os::unix::process::CommandExt;
    // Inside Flatpak the version-control tools live on the host — same route
    // `launch.rs` takes, but waited on rather than detached. The sandbox's own
    // environment doesn't reach the host, so the variables go as flags.
    if crate::launch::in_flatpak() {
        let mut c = Command::new("flatpak-spawn");
        c.arg("--host").arg(format!("--directory={}", cwd.display()));
        for (k, v) in env {
            c.arg(format!("--env={k}={v}"));
        }
        c.arg("--").arg(program).args(args);
        c.process_group(0);
        return c;
    }
    let mut c = Command::new(program);
    // Its own process group, so a time limit can stop what it started too.
    c.args(args).current_dir(cwd).envs(env.iter().copied()).process_group(0);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProvisionField;

    /// A helper the command leaves running keeps its output pipes open; the
    /// time limit must still hold.
    #[test]
    fn a_process_left_running_does_not_outlast_the_time_limit() {
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/c".to_string(), "start /b ping -n 30 127.0.0.1".to_string()]);
        #[cfg(not(windows))]
        let (program, args) = ("sh", vec!["-c".to_string(), "sleep 30 &".to_string()]);
        let started = Instant::now();
        let got = run_captured(program, &args, &std::env::temp_dir(), Duration::from_secs(2));
        assert!(got.is_err(), "{got:?}");
        assert!(started.elapsed() < Duration::from_secs(8), "took {:?}", started.elapsed());
    }

    /// A command that's only a launcher (Git for Windows' `git.exe` on PATH)
    /// waits on what it started; the time limit stops that too, so nothing is
    /// left holding the folder it ran in.
    #[test]
    fn the_time_limit_stops_what_the_command_started() {
        let dir = std::env::temp_dir().join(format!(
            "dp-tree-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/c".to_string(), "ping -n 37 127.0.0.1 >nul".to_string()]);
        #[cfg(not(windows))]
        let (program, args) = ("sh", vec!["-c".to_string(), "sleep 37; true".to_string()]);
        let got = run_captured(program, &args, &dir, Duration::from_secs(1));
        assert!(got.unwrap_err().contains("stopped"));

        // Windows: a process left behind (the ping, for its 37 s) would keep
        // the folder from going. A virus scanner looking at the new folder
        // holds it for a moment too, so this gives it a few seconds — far
        // short of the ping's.
        #[cfg(windows)]
        {
            let started = Instant::now();
            while std::fs::remove_dir_all(&dir).is_err() {
                assert!(started.elapsed() < Duration::from_secs(5), "something still holds {}", dir.display());
                std::thread::sleep(Duration::from_millis(100));
            }
            if started.elapsed() > Duration::from_millis(50) {
                eprintln!("held for {:?}", started.elapsed());
            }
        }
        // Linux: the folder could go regardless — look for the process.
        #[cfg(not(windows))]
        {
            std::thread::sleep(Duration::from_millis(300));
            let left = std::process::Command::new("pgrep").args(["-f", "sleep 37"]).output().unwrap();
            assert!(left.stdout.is_empty(), "sleep 37 is still running");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Leaving the screen a read was for stops it at once — and what it
    /// started — and the read's later commands don't start.
    #[test]
    fn a_cancelled_read_stops_its_command_and_skips_the_rest() {
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/c".to_string(), "ping -n 38 127.0.0.1 >nul".to_string()]);
        #[cfg(not(windows))]
        let (program, args) = ("sh", vec!["-c".to_string(), "sleep 38; true".to_string()]);
        let reads = Reads::default();
        let token = reads.start("removal");
        let started = Instant::now();
        let (first, second) = std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                reads.cancel("removal");
            });
            cancellable(token, || {
                let dir = std::env::temp_dir();
                let long = Duration::from_secs(60);
                (run_captured(program, &args, &dir, long), run_captured(program, &args, &dir, long))
            })
        });
        assert_eq!(first.unwrap_err(), "cancelled.");
        assert_eq!(second.unwrap_err(), "cancelled.");
        assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
        // A panic inside clears the token all the same, so it can't cancel
        // whatever runs on this thread next.
        let set = Arc::new(AtomicBool::new(true));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cancellable(set, || panic!("boom"))));
        assert!(!cancelled());
        // A new read of the same kind cancels the one before it.
        let a = reads.start("details");
        let _b = reads.start("details");
        assert!(a.load(Ordering::Relaxed));
    }

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
