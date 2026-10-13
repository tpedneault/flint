//! Somewhere to print, for a program with no console of its own.
//!
//! On Windows Flint is a GUI-subsystem program, as Fenix is, so the daemon
//! started at login or from a shortcut opens no console window. The cost
//! is that `flint status` typed in a terminal would print nowhere. Two
//! remedies, the same as Fenix's `--console`:
//!
//! - [`attach_parent`]: CLI subcommands attach to the console of the
//!   terminal they were started from, if there is one, and print there.
//!   Output already redirected (`flint status > out.txt`, a pipe) is left
//!   alone. One wart remains: cmd and PowerShell don't wait for GUI
//!   programs, so the prompt can come back before the output; piping
//!   (`flint status | more`) makes them wait.
//! - [`open`]: `--console` on the daemon attaches, or opens a console
//!   window of its own, to watch what the daemon logs.
//!
//! Elsewhere a program started from a terminal already writes to it, so
//! both do nothing.

/// Attaches to the parent's console, if it has one, and points stdout and
/// stderr at it unless they are redirected. Returns whether output now has
/// somewhere to go.
pub fn attach_parent() -> bool {
    #[cfg(windows)]
    {
        windows::attach(false)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Attaches to the parent's console, or opens a new console window when
/// there is none.
pub fn open() {
    #[cfg(windows)]
    windows::attach(true);
}

/// Reads one line from the user without showing it: a token typed for
/// `flint credential set`. From a terminal, with echo off (Windows: the
/// console's input buffer, `CONIN$`, since a GUI-subsystem program has no
/// standard input of its own); when standard input is a pipe or a file
/// (`… | flint credential set x`), its first line, so scripts work.
/// `prompt` goes to standard error, where it can't end up in a pipe.
pub fn read_secret(prompt: &str) -> std::io::Result<String> {
    use std::io::Write as _;
    let piped = {
        #[cfg(windows)]
        {
            windows::stdin_is_redirected()
        }
        #[cfg(not(windows))]
        {
            // SAFETY: isatty on a file descriptor number is always safe.
            unsafe { libc::isatty(0) == 0 }
        }
    };
    let mut line = String::new();
    if piped {
        std::io::stdin().read_line(&mut line)?;
    } else {
        eprint!("{prompt}");
        let _ = std::io::stderr().flush();
        #[cfg(windows)]
        {
            line = windows::read_hidden()?;
        }
        #[cfg(not(windows))]
        {
            line = unix::read_hidden()?;
        }
        eprintln!();
    }
    Ok(clean_secret(&line))
}

/// A secret as typed or piped, without what came with it: the line end,
/// and the byte-order mark PowerShell puts in front of text it pipes to
/// a program (`"tok" | flint credential set x`), which would otherwise
/// become the token's first character.
fn clean_secret(line: &str) -> String {
    line.trim_start_matches('\u{feff}')
        .trim_end_matches(['\r', '\n'])
        .to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_piped_secret_loses_its_byte_order_mark_and_line_end() {
        assert_eq!(super::clean_secret("\u{feff}t0ken\r\n"), "t0ken");
        assert_eq!(super::clean_secret("t0ken\n"), "t0ken");
        assert_eq!(
            super::clean_secret(" spaced "),
            " spaced ",
            "the rest is the user's"
        );
    }
}

#[cfg(not(windows))]
mod unix {
    use std::io::BufRead;

    pub fn read_hidden() -> std::io::Result<String> {
        // SAFETY: termios is plain data; fd 0 is a terminal (checked by
        // the caller), and the old settings are restored before returning.
        unsafe {
            let mut old: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut old) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut quiet = old;
            quiet.c_lflag &= !libc::ECHO;
            quiet.c_lflag |= libc::ECHONL;
            libc::tcsetattr(0, libc::TCSANOW, &quiet);
            let mut line = String::new();
            let read = std::io::stdin().lock().read_line(&mut line);
            libc::tcsetattr(0, libc::TCSANOW, &old);
            read.map(|_| line)
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        AllocConsole, AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS,
        STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

    /// Whether standard input is a pipe or a file rather than nothing or
    /// a console.
    pub fn stdin_is_redirected() -> bool {
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileType, FILE_TYPE_CHAR, FILE_TYPE_UNKNOWN,
        };
        use windows_sys::Win32::System::Console::STD_INPUT_HANDLE;
        // SAFETY: plain queries on a handle that may be null.
        unsafe {
            let h = GetStdHandle(STD_INPUT_HANDLE);
            if h.is_null() || h == INVALID_HANDLE_VALUE {
                return false;
            }
            let kind = GetFileType(h);
            kind != FILE_TYPE_CHAR && kind != FILE_TYPE_UNKNOWN
        }
    }

    /// A line from the console's input buffer with echo off.
    pub fn read_hidden() -> std::io::Result<String> {
        use std::io::{BufRead, BufReader};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Console::{
            GetConsoleMode, SetConsoleMode, ENABLE_ECHO_INPUT,
        };
        let conin = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("CONIN$")
            .map_err(|e| {
                std::io::Error::new(e.kind(), "no terminal to read it from; pipe it in instead")
            })?;
        let handle = conin.as_raw_handle() as _;
        let mut mode = 0;
        // SAFETY: `handle` is the console input buffer opened above;
        // the mode is restored before returning.
        unsafe {
            GetConsoleMode(handle, &mut mode);
            SetConsoleMode(handle, mode & !ENABLE_ECHO_INPUT);
        }
        let mut line = String::new();
        let read = BufReader::new(&conin).read_line(&mut line);
        // SAFETY: as above.
        unsafe { SetConsoleMode(handle, mode) };
        read.map(|_| line)
    }

    pub fn attach(allocate: bool) -> bool {
        // SAFETY: plain Win32 calls without pointer arguments.
        let has_output = |which| unsafe {
            let h = GetStdHandle(which);
            !h.is_null() && h != INVALID_HANDLE_VALUE
        };
        if has_output(STD_OUTPUT_HANDLE) && has_output(STD_ERROR_HANDLE) {
            return true; // redirected, or a console program after all
        }
        // SAFETY: as above.
        let attached = unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS) != 0 || (allocate && AllocConsole() != 0)
        };
        if !attached {
            return false;
        }
        // A GUI-subsystem process gets no standard handles from attaching;
        // open the console's output buffer and install it where none is
        // set. `std` looks the handle up on every write.
        let Ok(conout) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("CONOUT$")
        else {
            return false;
        };
        // Never closed: it is stdout/stderr for the rest of the process.
        let conout = conout.into_raw_handle();
        for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            if !has_output(which) {
                // SAFETY: `conout` is a valid handle nobody else owns.
                unsafe { SetStdHandle(which, conout as _) };
            }
        }
        true
    }
}
