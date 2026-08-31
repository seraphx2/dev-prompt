use std::process::Command;

use crate::actions::Action;
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

    spawn_detached(&action.program, &args, &repo.path)
}

#[cfg(windows)]
fn spawn_detached(program: &str, args: &[String], cwd: &str) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
    const FLAGS: u32 = 0x0000_0008 | 0x0000_0200 | 0x0800_0000;

    // Route through `cmd /c` so PATHEXT / .cmd shims (`code.cmd`) and Store
    // aliases (`wt.exe`) resolve the same way they do in a shell.
    let mut cmd = Command::new("cmd");
    cmd.arg("/c").arg(program).args(args);
    cmd.current_dir(cwd);
    cmd.creation_flags(FLAGS);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| AppError::msg(format!("failed to launch {program}: {e}")))
}

#[cfg(not(windows))]
fn spawn_detached(program: &str, args: &[String], cwd: &str) -> AppResult<()> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.current_dir(cwd);

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
