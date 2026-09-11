use std::process::Command;

use crate::rules::Action;
use crate::error::{AppError, AppResult};
use crate::scan::Repo;

/// True when running inside a Flatpak sandbox. `/.flatpak-info` is always
/// present there (unlike `$FLATPAK_ID`, which `flatpak run` doesn't always set).
pub fn in_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

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
    let child = cmd
        .spawn()
        .map_err(|e| AppError::msg(format!("failed to launch {program}: {e}")))?;

    // Windows blocks background processes from stealing focus (the foreground
    // lock timeout), so a detached child's first window can open behind
    // whatever's already active. Grant it a one-shot exemption so its own
    // SetForegroundWindow call (made implicitly when its main window is
    // created) succeeds. Best-effort: locked-down machines (GPO, endpoint
    // security) may still ignore it, so a failure here isn't fatal.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(child.id());
    }

    Ok(())
}

#[cfg(not(windows))]
fn spawn_detached(program: &str, args: &[String], cwd: &str, _via_cmd: bool) -> AppResult<()> {
    let mut cmd = if in_flatpak() {
        // Inside the sandbox the editors / terminals / CLIs we launch live on
        // the host, not in the runtime. `flatpak-spawn --host` runs them there;
        // `--directory=` sets the host-side cwd (the sandbox's own cwd is
        // meaningless to the target). Needs the `--talk-name=org.freedesktop.Flatpak`
        // hole — see packaging/flatpak/. No `--watch-bus`: the launched process
        // should outlive the overlay.
        let mut c = Command::new("flatpak-spawn");
        c.arg("--host");
        if !cwd.is_empty() {
            c.arg(format!("--directory={cwd}"));
        }
        c.arg("--").arg(program).args(args);
        c
    } else {
        let mut c = Command::new(program);
        c.args(args);
        if !cwd.is_empty() {
            c.current_dir(cwd);
        }
        c
    };

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
