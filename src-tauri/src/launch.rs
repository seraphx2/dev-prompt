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

/// The arguments and working directory `action` runs with: placeholders
/// filled, empty arguments (an absent `{{file}}`) dropped, the repo root when
/// the action names no folder.
fn launch_args(action: &Action, repo: &Repo) -> (Vec<String>, String) {
    let args = action
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
    (args, cwd)
}

/// Spawn the action's process fully detached so it outlives the overlay window.
pub fn launch(action: &Action, repo: &Repo) -> AppResult<()> {
    if action.client_side {
        return Ok(()); // handled in the frontend
    }
    if action.program.is_empty() {
        return Err(AppError::msg("action has no program to run"));
    }
    let (args, cwd) = launch_args(action, repo);
    spawn_detached(&action.program, &args, &cwd, true)
}

/// [`launch`], as administrator (Windows: the UAC prompt) or root (Linux: the
/// terminal asks for the password) — for an action whose `elevated` says how.
pub fn launch_elevated(action: &Action, repo: &Repo) -> AppResult<()> {
    let Some(how) = action.elevated else {
        return Err(AppError::msg("this action can't run elevated"));
    };
    let (args, cwd) = launch_args(action, repo);
    #[cfg(windows)]
    {
        use crate::rules::Elevation;
        // The "runas" verb wants a file, not a name to look up (`wt.exe` is an
        // App Execution Alias on PATH).
        let program = if action.program.contains(['/', '\\']) {
            action.program.clone()
        } else {
            crate::rules::which(&action.program).unwrap_or_else(|| action.program.clone())
        };
        let line = |args: &[String]| args.iter().map(|a| crate::rules::win_quote(a)).collect::<Vec<_>>().join(" ");
        match how {
            // The terminal takes the folder on its own command line (`wt -d`).
            Elevation::Terminal if action.cwd.is_none() => run_as_admin(&program, &line(&args), &cwd, false),
            Elevation::Program if program.to_lowercase().ends_with(".exe") => {
                run_as_admin(&program, &line(&args), &cwd, false)
            }
            // The folder has to travel in the command (an elevated process
            // doesn't reliably keep its working directory): a terminal that
            // only gets one, or a script shim (`code.cmd`) — whose own window
            // stays hidden.
            how => run_as_admin("powershell.exe", &in_folder(&cwd, &program, &args), &cwd, how == Elevation::Program),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (how, args);
        let root = crate::rules::root_shell_args(&action.program, &cwd)
            .ok_or_else(|| AppError::msg("this terminal can't open a root shell"))?;
        spawn_detached(&action.program, &root, &cwd, false)
    }
}

/// PowerShell's arguments to run `program args` in `dir`. Every part is a
/// single-quoted string, where only a quote mark means anything (it's
/// doubled) — so nothing in a folder or argument name (`&`, `%`, `;`, `$`)
/// can become a command of its own, elevated. `cmd`'s quoting can't promise
/// that.
#[cfg(windows)]
fn in_folder(dir: &str, program: &str, args: &[String]) -> String {
    let lit = |s: &str| {
        let mut out = String::from("'");
        for c in s.chars() {
            // PowerShell takes the typographic single quotes as quote marks too.
            if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
                out.push(c);
            }
            out.push(c);
        }
        out.push('\'');
        out
    };
    let call: Vec<String> = std::iter::once(lit(program)).chain(args.iter().map(|a| lit(a))).collect();
    let script = format!("Set-Location -LiteralPath {}; & {}", lit(dir), call.join(" "));
    format!("-NoProfile -NoLogo -Command {}", crate::rules::win_quote(&script))
}

/// Start `program` through the "runas" verb: Windows shows its UAC prompt,
/// and a "No" there is the user's answer, not a failure. `params` is the raw
/// command line. The working directory is passed too, but Windows doesn't
/// reliably honour it for an elevated process — callers carry the folder in
/// the command line as well. `hidden`: start it without a window (a `cmd`
/// that only starts a GUI app).
#[cfg(windows)]
pub fn run_as_admin(program: &str, params: &str, cwd: &str, hidden: bool) -> AppResult<()> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::ERROR_CANCELLED;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};

    let (file, params, dir) = (HSTRING::from(program), HSTRING::from(params), HSTRING::from(cwd));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        lpDirectory: if cwd.is_empty() { PCWSTR::null() } else { PCWSTR(dir.as_ptr()) },
        nShow: if hidden { SW_HIDE.0 } else { SW_SHOWNORMAL.0 },
        ..Default::default()
    };
    // ShellExecuteEx may hand off to shell extensions, which want COM on the
    // calling thread (a blocking-pool thread here, so nothing else owns it).
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let ran = unsafe { ShellExecuteExW(&mut info) };
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
    match ran {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => Ok(()),
        Err(e) => Err(AppError::msg(format!("failed to launch {program} as administrator: {e}"))),
    }
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn in_folder_keeps_names_literal() {
        let args = vec!["it's".to_string(), "a & b".to_string(), "%PATH%".to_string()];
        let params = in_folder("D:\\git\\R&D", "C:\\x\\tool.exe", &args);
        let split = crate::rules::win_split_args(&params);
        assert_eq!(&split[..3], &["-NoProfile", "-NoLogo", "-Command"]);
        assert_eq!(
            split[3],
            "Set-Location -LiteralPath 'D:\\git\\R&D'; & 'C:\\x\\tool.exe' 'it''s' 'a & b' '%PATH%'"
        );
    }
}
