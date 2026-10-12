//! The window in front: what the user was doing when they summoned Flint,
//! sent to providers with each query (`context.foreground`) so they can
//! rank for it (a terminal's project, Fenix's current file).
//!
//! Read just before Flint shows itself, while the other window is still in
//! front. Windows only for now: Wayland deliberately tells applications
//! nothing about other windows, and on X11 it would need the window
//! manager's properties, which no provider uses yet.

/// The foreground window, if it can be known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    /// Its program's file name (`fenix.exe`).
    pub process: String,
    /// Its title.
    pub title: String,
    /// Its process id.
    pub pid: u32,
}

/// The window in front now, or `None` where that can't be known.
pub fn current() -> Option<Foreground> {
    #[cfg(windows)]
    {
        windows::current()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
pub(crate) mod windows {
    use windows_sys::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    use super::Foreground;

    pub fn current() -> Option<Foreground> {
        // SAFETY: plain Win32 calls; every buffer is sized as passed, and
        // the process handle is closed before returning.
        unsafe {
            let window = GetForegroundWindow();
            if window.is_null() {
                return None;
            }
            let mut title = [0u16; 512];
            let length =
                GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32).max(0) as usize;
            let mut pid = 0;
            GetWindowThreadProcessId(window, &mut pid);
            Some(Foreground {
                process: process_name(pid),
                title: String::from_utf16_lossy(&title[..length]),
                pid,
            })
        }
    }

    /// Process `pid`'s program file name (`fenix.exe`), or empty when it
    /// can't be read (an elevated process, one that just exited).
    pub(crate) fn process_name(pid: u32) -> String {
        let mut path = [0u16; MAX_PATH as usize];
        let mut size = path.len() as u32;
        // SAFETY: the buffer is sized as passed; the handle is closed.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return String::new();
            }
            let mut name = String::new();
            if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut size)
                != 0
            {
                let full = String::from_utf16_lossy(&path[..size as usize]);
                name = full
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or_default()
                    .to_owned();
            }
            CloseHandle(process);
            name
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn asking_never_fails() {
        // In CI there may be no foreground window at all; either answer is
        // fine, a panic isn't.
        let _ = super::current();
    }
}
