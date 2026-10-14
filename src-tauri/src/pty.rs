//! Built-in shell sessions behind "Open shell": the user's own shell running
//! in a pseudo-terminal (ConPTY on Windows, a Unix PTY on Linux), streamed to
//! xterm.js in the overlay.
//!
//! A PTY rather than plain pipes, because a program writing to a pipe decides
//! nobody is watching — no colour, no prompt, no line editing, no full-screen
//! tools. Behind a PTY the shell behaves exactly as it does in a terminal.
//!
//! Each session runs three threads:
//! - **reader** — PTY output → `on_data`, until EOF;
//! - **writer** — input queued by [`Sessions::write`] → PTY, in order, so a slow
//!   or busy shell never blocks the IPC call that delivered a keystroke;
//! - **waiter** — waits for the shell to exit, tears the session down, lets the
//!   reader drain, then reports the exit code — always after the last output.
//!
//! Ending a session matches closing a terminal tab: the shell is killed and
//! the PTY hung up, which takes down whatever runs attached to it (a dev
//! server). GUI apps started from the shell (`code .`) live on, as they would.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

/// How long the waiter lets the reader drain after the shell exits. A process
/// the shell left running in the background can hold the PTY open well past
/// this; the exit is reported anyway and the reader ends whenever it does.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// How long an ended session's shell gets to go after the hangup before it's
/// killed outright. On Linux the first signal is SIGHUP, which a shell can
/// ignore; `TerminateProcess` on Windows is already final.
const KILL_GRACE: Duration = Duration::from_secs(2);

/// How often the waiter checks on the shell.
const POLL: Duration = Duration::from_millis(50);

/// What starts a session: the program, its arguments, and a short name for
/// the overlay's header (`pwsh`, `bash`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpec {
    pub program: String,
    pub args: Vec<String>,
    pub label: String,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    input: mpsc::Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// When [`Sessions::close`] ended it — the waiter escalates from there.
    closed_at: Arc<Mutex<Option<Instant>>>,
}

/// Every live session, by id. Ids are never reused within a run, so a late
/// call for a session that already ended can't land on a newer one.
#[derive(Default)]
pub struct Sessions {
    next: AtomicU32,
    live: Mutex<HashMap<u32, Session>>,
}

impl Sessions {
    /// Start `shell` in `cwd` on a `cols`×`rows` PTY. Output goes to
    /// `on_data` as raw bytes (UTF-8 and escape codes, possibly split
    /// mid-character — xterm.js reassembles them); `on_exit` fires once, after
    /// the last output, with the exit code when there is one.
    pub fn open(
        self: &Arc<Self>,
        shell: &ShellSpec,
        cwd: &Path,
        cols: u16,
        rows: u16,
        on_data: impl Fn(Vec<u8>) + Send + 'static,
        on_exit: impl FnOnce(Option<i32>) + Send + 'static,
    ) -> anyhow::Result<u32> {
        let pair = native_pty_system().openpty(size(cols, rows))?;

        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(&shell.args);
        cmd.cwd(cwd);
        // Windows programs don't read TERM, and some (Windows ports of Unix
        // tools) misbehave when it's set, so it's Linux-only — as in Windows
        // Terminal. COLORTERM only advertises 24-bit colour, harmless anywhere.
        #[cfg(unix)]
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "dev-prompt");

        let mut child = pair.slave.spawn_command(cmd)?;
        // The child holds its own copy of the slave end. Ours must go, or the
        // reader never sees EOF on Linux once the shell exits.
        drop(pair.slave);

        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let (input, queued) = mpsc::channel::<Vec<u8>>();

        let closed_at = Arc::new(Mutex::new(None::<Instant>));
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.live.lock().unwrap().insert(
            id,
            Session {
                master: pair.master,
                input,
                killer,
                closed_at: Arc::clone(&closed_at),
            },
        );

        std::thread::Builder::new()
            .name(format!("pty-{id}-write"))
            .spawn(move || {
                // Ends when the session is dropped (the sender goes with it) or
                // the PTY stops taking input.
                for bytes in queued {
                    if writer.write_all(&bytes).and_then(|_| writer.flush()).is_err() {
                        break;
                    }
                }
            })?;

        let (drained, reader_done) = mpsc::channel::<()>();
        std::thread::Builder::new()
            .name(format!("pty-{id}-read"))
            .spawn(move || {
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => on_data(buf[..n].to_vec()),
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        // Linux reports a hung-up PTY as EIO rather than EOF.
                        Err(_) => break,
                    }
                }
                let _ = drained.send(());
            })?;

        let sessions = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("pty-{id}-wait"))
            .spawn(move || {
                // Polled rather than a blocking `wait`, so a shell that
                // shrugged off an ended session's hangup can be killed for
                // good from here — the one thread that holds it un-reaped, so
                // its PID can't have been reused by then.
                let code = loop {
                    match child.try_wait() {
                        Ok(Some(status)) => break Some(status.exit_code() as i32),
                        Ok(None) => {}
                        Err(_) => break None,
                    }
                    let overdue = closed_at.lock().unwrap().is_some_and(|t| t.elapsed() >= KILL_GRACE);
                    if overdue {
                        // On Linux: SIGHUP, a moment's grace, then SIGKILL.
                        let _ = child.kill();
                        *closed_at.lock().unwrap() = None;
                        break child.wait().ok().map(|s| s.exit_code() as i32);
                    }
                    std::thread::sleep(POLL);
                };
                // Dropping the master hangs up the PTY. On Windows that closes
                // the pseudo-console, which is what finally ends the reader's
                // stream; it's already over on Linux unless something the
                // shell started still holds the terminal.
                let session = sessions.live.lock().unwrap().remove(&id);
                drop(session);
                let _ = reader_done.recv_timeout(DRAIN_TIMEOUT);
                on_exit(code);
            })?;

        Ok(id)
    }

    /// Queue input for session `id`. Never blocks on the shell.
    pub fn write(&self, id: u32, bytes: Vec<u8>) -> anyhow::Result<()> {
        let live = self.live.lock().unwrap();
        let session = live.get(&id).ok_or_else(|| gone(id))?;
        session.input.send(bytes).map_err(|_| gone(id))
    }

    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> anyhow::Result<()> {
        let live = self.live.lock().unwrap();
        let session = live.get(&id).ok_or_else(|| gone(id))?;
        session.master.resize(size(cols, rows))
    }

    /// End session `id`: kill the shell and hang up its PTY. The exit still
    /// arrives through `on_exit`. Unknown ids are a no-op — the session may
    /// have ended on its own a moment earlier.
    pub fn close(&self, id: u32) {
        let Some(mut session) = self.live.lock().unwrap().remove(&id) else {
            return;
        };
        let _ = session.killer.kill();
        *session.closed_at.lock().unwrap() = Some(Instant::now());
        drop(session);
    }

    /// End every session — on app exit, and when a reloaded frontend has
    /// forgotten the sessions it started.
    pub fn close_all(&self) {
        let ids: Vec<u32> = self.live.lock().unwrap().keys().copied().collect();
        for id in ids {
            self.close(id);
        }
    }

    #[cfg(test)]
    fn is_live(&self, id: u32) -> bool {
        self.live.lock().unwrap().contains_key(&id)
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn gone(id: u32) -> anyhow::Error {
    anyhow::anyhow!("shell session {id} has ended")
}

/// The shell a session runs: `choice` (a name from the shell picker or
/// `config.shell`, or a path), else the platform default.
#[cfg(windows)]
pub fn shell_spec(choice: Option<&str>, _cwd: &Path) -> ShellSpec {
    let name = choice
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(crate::rules::default_shell);
    let label = shell_label(&name);
    let program = crate::rules::which(&name)
        .or_else(|| (label == "bash").then(git_bash).flatten())
        .unwrap_or_else(|| name.clone());
    let args: &[&str] = match label.as_str() {
        "pwsh" | "powershell" => &["-NoLogo"],
        // Git for Windows' bash: a login shell is what reads its profile
        // (the git-aware prompt) — Windows Terminal's Git Bash profile does
        // the same.
        "bash" => &["--login", "-i"],
        _ => &[],
    };
    ShellSpec {
        program,
        args: args.iter().map(|s| s.to_string()).collect(),
        label,
    }
}

/// Git for Windows' bash, which usually isn't on PATH (`list_shells` offers
/// `bash` when it finds it here).
#[cfg(windows)]
fn git_bash() -> Option<String> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(|b| std::env::var(b).ok())
        .flat_map(|pf| {
            ["Git\\bin\\bash.exe", "Git\\usr\\bin\\bash.exe"].map(|rel| Path::new(&pf).join(rel))
        })
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(unix)]
pub fn shell_spec(choice: Option<&str>, cwd: &Path) -> ShellSpec {
    let choice = choice.map(str::trim).filter(|s| !s.is_empty());
    if crate::launch::in_flatpak() {
        // The shells live on the host. `flatpak-spawn --host` runs one there
        // with the PTY as its stdio; `--directory=` sets the host-side cwd.
        // With nothing chosen, the host's own login shell, read from its
        // `$SHELL` on that side.
        let mut args = vec!["--host".to_string(), format!("--directory={}", cwd.display())];
        args.push("--".to_string());
        let label = match choice {
            Some(name) => {
                args.push(name.to_string());
                shell_label(name)
            }
            None => {
                args.extend(["sh", "-c", "exec \"${SHELL:-/bin/sh}\""].map(String::from));
                "shell".to_string()
            }
        };
        return ShellSpec { program: "flatpak-spawn".to_string(), args, label };
    }
    let name = choice
        .map(str::to_string)
        .unwrap_or_else(|| unix_default_shell(std::env::var("SHELL").ok()));
    ShellSpec {
        program: crate::rules::which(&name).unwrap_or_else(|| name.clone()),
        args: Vec::new(),
        label: shell_label(&name),
    }
}

/// The user's login shell from `$SHELL`, else bash, else sh.
#[cfg(unix)]
pub fn unix_default_shell(shell_env: Option<String>) -> String {
    shell_env
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Path::new("/bin/bash").is_file().then(|| "/bin/bash".to_string()))
        .unwrap_or_else(|| "/bin/sh".to_string())
}

/// `C:\…\pwsh.exe` → `pwsh`, `/usr/bin/zsh` → `zsh`.
fn shell_label(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).to_lowercase();
    base.strip_suffix(".exe").unwrap_or(&base).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Collects a session's output and exit, for assertions.
    #[derive(Default)]
    struct Capture {
        out: Mutex<Vec<u8>>,
        exit: Mutex<Option<Option<i32>>>,
        /// Output length when the exit arrived — proves exit comes last.
        out_at_exit: Mutex<Option<usize>>,
    }

    impl Capture {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.out.lock().unwrap()).into_owned()
        }
        fn exited(&self) -> Option<Option<i32>> {
            *self.exit.lock().unwrap()
        }
    }

    fn open(sessions: &Arc<Sessions>, spec: &ShellSpec) -> (u32, Arc<Capture>) {
        let cap = Arc::new(Capture::default());
        let (c1, c2) = (Arc::clone(&cap), Arc::clone(&cap));
        let id = sessions
            .open(
                spec,
                &std::env::temp_dir(),
                100,
                30,
                move |b| c1.out.lock().unwrap().extend(b),
                move |code| {
                    *c2.out_at_exit.lock().unwrap() = Some(c2.out.lock().unwrap().len());
                    *c2.exit.lock().unwrap() = Some(code);
                },
            )
            .expect("spawn shell");
        // ConPTY opens by asking where the cursor is (`ESC[6n`) and holds the
        // program until the terminal answers. xterm.js answers it in the app;
        // here the test plays terminal.
        if cfg!(windows) {
            wait_until("ConPTY's cursor query", || cap.text().contains("\x1b[6n"));
            sessions.write(id, b"\x1b[1;1R".to_vec()).unwrap();
        }
        (id, cap)
    }

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn spec(program: &str, args: &[&str]) -> ShellSpec {
        ShellSpec {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            label: shell_label(program),
        }
    }

    /// A one-shot command that prints `hello` and exits 3.
    fn hello_exit_3() -> ShellSpec {
        if cfg!(windows) {
            spec("cmd", &["/d", "/c", "echo hello& exit 3"])
        } else {
            spec("sh", &["-c", "echo hello; exit 3"])
        }
    }

    /// An interactive shell with no user profile in the way.
    fn interactive() -> ShellSpec {
        if cfg!(windows) {
            spec("cmd", &["/d"])
        } else {
            spec("sh", &[])
        }
    }

    #[test]
    fn output_then_exit_code_arrive_in_order() {
        let sessions = Arc::new(Sessions::default());
        let (id, cap) = open(&sessions, &hello_exit_3());
        wait_until("exit", || cap.exited().is_some());
        assert!(cap.text().contains("hello"), "output: {:?}", cap.text());
        assert_eq!(cap.exited(), Some(Some(3)));
        // Nothing arrived after the exit was reported.
        assert_eq!(*cap.out_at_exit.lock().unwrap(), Some(cap.out.lock().unwrap().len()));
        assert!(!sessions.is_live(id), "an exited session is torn down");
        assert!(sessions.write(id, b"x".to_vec()).is_err());
    }

    #[test]
    fn typed_input_reaches_the_shell() {
        let sessions = Arc::new(Sessions::default());
        let (id, cap) = open(&sessions, &interactive());
        // The result (42) never appears in what was typed, so finding it
        // proves the shell ran the line rather than just echoing it.
        let line = if cfg!(windows) { "set /a 6*7\r" } else { "echo $((6*7))\r" };
        sessions.write(id, line.as_bytes().to_vec()).unwrap();
        wait_until("the shell's answer", || cap.text().contains("42"));
        sessions.write(id, b"exit\r".to_vec()).unwrap();
        wait_until("exit", || cap.exited().is_some());
        assert_eq!(cap.exited(), Some(Some(0)));
    }

    #[test]
    fn close_ends_a_running_shell() {
        let sessions = Arc::new(Sessions::default());
        let (id, cap) = open(&sessions, &interactive());
        assert!(sessions.is_live(id));
        sessions.close(id);
        assert!(!sessions.is_live(id), "close removes the session at once");
        wait_until("exit after close", || cap.exited().is_some());
        sessions.close(id); // already gone — a no-op, not a panic
    }

    #[test]
    fn close_all_ends_every_session() {
        let sessions = Arc::new(Sessions::default());
        let (_, a) = open(&sessions, &interactive());
        let (_, b) = open(&sessions, &interactive());
        sessions.close_all();
        wait_until("both exits", || a.exited().is_some() && b.exited().is_some());
        assert!(sessions.live.lock().unwrap().is_empty());
    }

    #[test]
    fn calls_for_an_unknown_session_fail_cleanly() {
        let sessions = Sessions::default();
        assert!(sessions.write(99, b"x".to_vec()).is_err());
        assert!(sessions.resize(99, 80, 24).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resize_reaches_the_shell() {
        let sessions = Arc::new(Sessions::default());
        let (id, cap) = open(&sessions, &interactive());
        sessions.resize(id, 123, 45).unwrap();
        sessions.write(id, b"stty size\r".to_vec()).unwrap();
        wait_until("stty's answer", || cap.text().contains("45 123"));
        sessions.close(id);
    }

    #[test]
    fn labels_drop_the_folder_and_exe() {
        assert_eq!(shell_label("pwsh"), "pwsh");
        assert_eq!(shell_label(r"C:\Program Files\PowerShell\7\pwsh.exe"), "pwsh");
        assert_eq!(shell_label("/usr/bin/zsh"), "zsh");
        assert_eq!(shell_label("CMD.EXE"), "cmd");
    }

    #[cfg(windows)]
    #[test]
    fn windows_shells_get_their_interactive_flags() {
        let cwd = std::env::temp_dir();
        assert_eq!(shell_spec(Some("cmd"), &cwd).args, Vec::<String>::new());
        assert_eq!(shell_spec(Some("cmd"), &cwd).label, "cmd");
        assert_eq!(shell_spec(Some("powershell"), &cwd).args, ["-NoLogo"]);
        assert_eq!(shell_spec(Some("bash"), &cwd).args, ["--login", "-i"]);
        // Nothing chosen → the same default `rules::default_shell` names.
        assert_eq!(shell_spec(None, &cwd).label, crate::rules::default_shell());
        assert_eq!(shell_spec(Some("  "), &cwd).label, crate::rules::default_shell());
    }

    #[cfg(unix)]
    #[test]
    fn unix_default_prefers_the_login_shell() {
        assert_eq!(unix_default_shell(Some("/usr/bin/fish".into())), "/usr/bin/fish");
        let fallback = unix_default_shell(None);
        assert!(fallback == "/bin/bash" || fallback == "/bin/sh");
        assert_eq!(unix_default_shell(Some(" ".into())), fallback);
    }
}
