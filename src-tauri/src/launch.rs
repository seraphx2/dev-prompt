use std::process::Command;

use crate::rules::Action;
use crate::error::{AppError, AppResult};
use crate::scan::Repo;

fn substitute(template: &str, repo: &Repo) -> String {
    template
        .replace("{{path}}", &repo.path)
        .replace("{{dir}}", &repo.path)
        .replace("{{file}}", "")
        .replace("{{name}}", &repo.name)
}

/// Spawn the action's process fully detached so it outlives the overlay window.
pub fn launch(action: &Action, repo: &Repo) -> AppResult<()> {
    if action.client_side {
        return Ok(()); // handled in the frontend
    }
    if action.program.is_empty() {
        return Err(AppError::msg("action has no program to run"));
    }

    let args: Vec<String> = action
        .args
        .iter()
        .map(|a| substitute(a, repo))
        .filter(|a| !a.is_empty())
        .collect();

    // Sub-project actions carry their own working directory; the rest run at the
    // repo root.
    let cwd = action
        .cwd
        .as_deref()
        .map(|c| substitute(c, repo))
        .unwrap_or_else(|| repo.path.clone());

    spawn_detached(&action.program, &args, &cwd, true)
}

/// Spawn an arbitrary program fully detached (no console, outlives the overlay).
/// Used by the app launcher, whose `program` is already a fully-resolved path —
/// so it spawns directly. An empty `cwd` leaves the working directory inherited
/// rather than set.
pub fn spawn(program: &str, args: &[String], cwd: &str) -> AppResult<()> {
    spawn_detached(program, args, cwd, false)
}

#[cfg(windows)]
fn spawn_detached(program: &str, args: &[String], cwd: &str, via_cmd: bool) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
    const FLAGS: u32 = 0x0000_0008 | 0x0000_0200 | 0x0800_0000;

    let mut cmd = if via_cmd {
        // Route through `cmd /c` so PATHEXT / .cmd shims (`code.cmd`) and Store
        // aliases (`wt.exe`) resolve the same way they do in a shell.
        let mut c = Command::new("cmd");
        c.arg("/c").arg(program).args(args);
        c
    } else {
        // `program` is a concrete path already; spawn it straight so Rust's own
        // argument quoting applies and `cmd`'s quote-stripping never sees the
        // command line (it corrupts a program path or arg that contains a space).
        let mut c = Command::new(program);
        c.args(args);
        c
    };
    if !cwd.is_empty() {
        cmd.current_dir(cwd);
    }
    cmd.creation_flags(FLAGS);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| AppError::msg(format!("failed to launch {program}: {e}")))
}

#[cfg(not(windows))]
fn spawn_detached(program: &str, args: &[String], cwd: &str, _via_cmd: bool) -> AppResult<()> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    if !cwd.is_empty() {
        cmd.current_dir(cwd);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Detach from the overlay's controlling terminal / process group.
        unsafe {
            cmd.pre_exec(|| {
                extern "C" {
                    fn setsid() -> i32;
                }
                setsid();
                Ok(())
            });
        }
    }

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| AppError::msg(format!("failed to launch {program}: {e}")))
}
