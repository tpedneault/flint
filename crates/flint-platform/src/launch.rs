//! Starting things: opening a path or URL with its default handler, and
//! running a program from an argument vector.
//!
//! - **No shell.** `run` spawns the program directly with its arguments,
//!   so nothing typed into Flint is ever interpreted by a shell.
//! - **Detached.** What Flint starts must outlive it and never hold on to
//!   its console: standard streams go to null, and on Linux the child gets
//!   its own process group and is reaped by a small thread, so no zombie
//!   is left behind.
//! - **Focus.** On Windows, Flint is the foreground process when the user
//!   presses `Enter`, so it grants the new process the right to take the
//!   foreground (`AllowSetForegroundWindow` with its pid) and the launched
//!   window comes to the front instead of flashing in the task bar.
//! - **No leaked tokens.** A Wayland activation token in Flint's own
//!   environment (from the `flint toggle` that started it) belongs to that
//!   one activation and is removed from the child's environment.
//!
//! Windows opens through `ShellExecuteExW`, the same call Explorer makes,
//! so a Start Menu shortcut's arguments, working directory and "run as"
//! settings are honoured. Linux opens through `xdg-open`.
//!
//! Revealing (showing a file selected in the file manager) is
//! `explorer.exe /select,` on Windows, and on Linux the
//! `org.freedesktop.FileManager1.ShowItems` D-Bus call that Dolphin,
//! Nautilus and others answer, sent with `dbus-send`; without `dbus-send`
//! the file's folder is opened instead.
//!
//! Errors come back as one line naming what couldn't be started and why.

use std::process::{Command, Stdio};

/// Opens `target` (a path or URL) with the OS's default handler.
pub fn open(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::shell_open(target)
    }
    #[cfg(not(windows))]
    {
        spawn(&["xdg-open".to_owned(), target.to_owned()], None)
            .map_err(|e| format!("couldn't open {target}: {e}"))
    }
}

/// Shows `path` selected in the file manager.
pub fn reveal(path: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::reveal(path).map_err(|e| format!("couldn't show {path}: {e}"))
    }
    #[cfg(not(windows))]
    {
        let result = if on_path("dbus-send") {
            spawn(&show_items(path), None)
        } else {
            let folder = std::path::Path::new(path)
                .parent()
                .map_or_else(|| path.to_owned(), |p| p.display().to_string());
            spawn(&["xdg-open".to_owned(), folder], None)
        };
        result.map_err(|e| format!("couldn't show {path}: {e}"))
    }
}

/// The `dbus-send` command asking the file manager to show `path`.
#[cfg_attr(windows, allow(dead_code))]
fn show_items(path: &str) -> Vec<String> {
    [
        "dbus-send",
        "--session",
        "--type=method_call",
        "--dest=org.freedesktop.FileManager1",
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1.ShowItems",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([
        format!("array:string:{}", file_uri(path)),
        "string:".to_owned(),
    ])
    .collect()
}

/// `file://` and the path, percent-encoded except for `/` and the
/// characters RFC 3986 leaves alone. A comma would end `dbus-send`'s
/// array element, so it is encoded like everything else.
#[cfg_attr(windows, allow(dead_code))]
fn file_uri(path: &str) -> String {
    let mut uri = String::from("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// Runs `argv` in `cwd`, in the user's terminal when `terminal`.
pub fn run(argv: &[String], cwd: Option<&str>, terminal: bool) -> Result<(), String> {
    let Some(program) = argv.first() else {
        return Err("nothing to run".into());
    };
    let argv = if terminal && !cfg!(windows) {
        in_terminal(argv)
    } else {
        argv.to_vec()
    };
    #[cfg(windows)]
    {
        windows::run(&argv, cwd, terminal).map_err(|e| format!("couldn't start {program}: {e}"))
    }
    #[cfg(not(windows))]
    {
        spawn(&argv, cwd).map_err(|e| format!("couldn't start {program}: {e}"))
    }
}

/// How a hidden command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    /// Whether it succeeded (exit code 0).
    pub success: bool,
    /// Its exit code, when it has one (not when killed by a signal).
    pub code: Option<i32>,
    /// The last non-empty line it printed, stderr's before stdout's: what
    /// a failing command says about why.
    pub last_line: Option<String>,
}

/// Runs `argv` in `cwd` with no window, and calls `done` from another
/// thread once it has exited. Its output is read as it comes and only the
/// last line kept, so a chatty command costs no memory and never blocks
/// on a full pipe. Errors starting it come back at once, as from [`run`].
pub fn run_hidden(
    argv: &[String],
    cwd: Option<&str>,
    done: impl FnOnce(Exit) + Send + 'static,
) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Read};
    use std::sync::mpsc;

    let Some(program) = argv.first() else {
        return Err("nothing to run".into());
    };
    let mut command = command(argv, cwd);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("couldn't start {program}: {e}"))?;
    // Bytes, not UTF-8: a console program's output may be in the OEM
    // code page, and one bad byte mustn't lose the line.
    fn last_line(stream: impl Read + Send + 'static) -> mpsc::Receiver<Option<String>> {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("flint-run-output".into())
            .spawn(move || {
                let mut reader = BufReader::new(stream);
                let mut last = None;
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match reader.read_until(b'\n', &mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let text = String::from_utf8_lossy(&line).trim().to_owned();
                            if !text.is_empty() {
                                last = Some(text);
                            }
                        }
                    }
                }
                let _ = tx.send(last);
            });
        rx
    }
    let stdout = child.stdout.take().map(last_line);
    let stderr = child.stderr.take().map(last_line);
    std::thread::Builder::new()
        .name("flint-run-wait".into())
        .spawn(move || {
            let status = child.wait();
            let take = |rx: Option<mpsc::Receiver<Option<String>>>| {
                rx.and_then(|rx| {
                    rx.recv_timeout(std::time::Duration::from_secs(1))
                        .ok()
                        .flatten()
                })
            };
            let (err, out) = (take(stderr), take(stdout));
            let last_line = err.or(out).map(|line| {
                let mut chars = line.chars();
                let short: String = chars.by_ref().take(160).collect();
                if chars.next().is_some() {
                    format!("{short}…")
                } else {
                    short
                }
            });
            done(match status {
                Ok(status) => Exit {
                    success: status.success(),
                    code: status.code(),
                    last_line,
                },
                Err(_) => Exit {
                    success: false,
                    code: None,
                    last_line,
                },
            });
        })
        .map(|_| ())
        .map_err(|e| format!("couldn't wait for {program}: {e}"))
}

/// `argv` wrapped to run in a terminal: `$TERMINAL` if set, else the first
/// of the usual ones that is installed. Most take `-e program args...`;
/// GNOME's takes `--`.
fn in_terminal(argv: &[String]) -> Vec<String> {
    let terminal = std::env::var("TERMINAL")
        .ok()
        .filter(|t| !t.is_empty())
        .or_else(|| {
            ["konsole", "x-terminal-emulator", "gnome-terminal", "xterm"]
                .into_iter()
                .find(|t| on_path(t))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "xterm".to_owned());
    let separator = if terminal.ends_with("gnome-terminal") {
        "--"
    } else {
        "-e"
    };
    let mut wrapped = vec![terminal, separator.to_owned()];
    wrapped.extend(argv.iter().cloned());
    wrapped
}

/// Whether `program` (a file name, with `.exe` on Windows) is in a
/// folder on the `PATH`.
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// The command for `argv`, with null streams and without our activation
/// token.
fn command(argv: &[String], cwd: Option<&str>) -> Command {
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("XDG_ACTIVATION_TOKEN")
        .env_remove("DESKTOP_STARTUP_ID");
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        command.current_dir(cwd);
    }
    command
}

#[cfg(not(windows))]
fn spawn(argv: &[String], cwd: Option<&str>) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut child = command(argv, cwd).process_group(0).spawn()?;
    // Reap it whenever it exits; nothing else waits for it.
    let _ = std::thread::Builder::new()
        .name("flint-reap".into())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

#[cfg(windows)]
mod windows {
    use std::os::windows::process::CommandExt;
    use std::ptr;

    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetProcessId, CREATE_NEW_CONSOLE, CREATE_NO_WINDOW,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use crate::focus::allow_foreground;

    pub fn shell_open(target: &str) -> Result<(), String> {
        let file: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        // SAFETY: a zeroed SHELLEXECUTEINFOW is valid; the fields set below
        // point at `file`, which outlives the call.
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        // No error dialogs: the reason goes to Flint's footer instead.
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI;
        info.lpFile = file.as_ptr();
        info.lpVerb = ptr::null();
        info.nShow = SW_SHOWNORMAL;
        // SAFETY: `info` is initialised as the call requires.
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            return Err(format!(
                "couldn't open {target}: {}",
                std::io::Error::last_os_error()
            ));
        }
        if !info.hProcess.is_null() {
            // SAFETY: a process handle the call returned to us to close.
            unsafe {
                allow_foreground(GetProcessId(info.hProcess));
                CloseHandle(info.hProcess);
            }
        }
        Ok(())
    }

    pub fn reveal(path: &str) -> std::io::Result<()> {
        // `explorer /select,"path"` must reach Explorer exactly so: the
        // standard argument quoting would wrap the whole of it in quotes,
        // which Explorer doesn't understand. A running Explorer may do the
        // showing, so any process is allowed the foreground once.
        let mut command = std::process::Command::new("explorer.exe");
        command.raw_arg(format!("/select,\"{path}\""));
        crate::focus::allow_any_foreground();
        command.spawn().map(|_| ())
    }

    pub fn run(argv: &[String], cwd: Option<&str>, terminal: bool) -> std::io::Result<()> {
        let flags = if terminal {
            CREATE_NEW_CONSOLE
        } else {
            CREATE_NO_WINDOW
        };
        let child = super::command(argv, cwd).creation_flags(flags).spawn()?;
        allow_foreground(child.id());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_get_their_own_separator() {
        std::env::set_var("TERMINAL", "/usr/bin/gnome-terminal");
        assert_eq!(
            in_terminal(&["htop".into()]),
            ["/usr/bin/gnome-terminal", "--", "htop"]
        );
        std::env::set_var("TERMINAL", "konsole");
        assert_eq!(
            in_terminal(&["htop".into(), "-d".into()]),
            ["konsole", "-e", "htop", "-d"]
        );
        std::env::remove_var("TERMINAL");
    }

    #[test]
    fn show_items_sends_an_encoded_file_uri() {
        let argv = show_items("/home/me/My Notes,v2.md");
        assert_eq!(argv[0], "dbus-send");
        assert_eq!(argv[6], "array:string:file:///home/me/My%20Notes%2Cv2.md");
        assert_eq!(argv[7], "string:");
        assert_eq!(file_uri("/a/é"), "file:///a/%C3%A9");
    }

    #[test]
    fn a_hidden_command_reports_its_status_and_last_line() {
        let (tx, rx) = std::sync::mpsc::channel();
        let parts: &[&str] = if cfg!(windows) {
            &["cmd", "/d", "/c", "echo first & echo second 1>&2 & exit 3"]
        } else {
            &["sh", "-c", "echo first; echo second >&2; exit 3"]
        };
        let argv: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
        run_hidden(&argv, None, move |exit| {
            let _ = tx.send(exit);
        })
        .unwrap();
        let exit = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(!exit.success);
        assert_eq!(exit.code, Some(3));
        assert_eq!(exit.last_line.as_deref(), Some("second"), "stderr wins");
        let err = run_hidden(&["definitely-not-a-program-xyz".into()], None, |_| {}).unwrap_err();
        assert!(err.starts_with("couldn't start definitely-not-a-program-xyz: "));
    }

    #[test]
    fn running_nothing_or_a_missing_program_is_one_line() {
        assert_eq!(run(&[], None, false).unwrap_err(), "nothing to run");
        let err = run(&["definitely-not-a-program-xyz".into()], None, false).unwrap_err();
        assert!(
            err.starts_with("couldn't start definitely-not-a-program-xyz: "),
            "{err}"
        );
        assert!(!err.contains('\n'));
    }
}
