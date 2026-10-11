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

#[cfg(windows)]
mod windows {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        AllocConsole, AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS,
        STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

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
