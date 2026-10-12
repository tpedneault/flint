//! The open top-level windows, and switching to or closing one: what the
//! windows provider (`w`) lists and acts on.
//!
//! Windows: `EnumWindows`, front to back, keeping what the task bar and
//! Alt+Tab would show: visible, unowned, not a tool window, with a title,
//! and not *cloaked* (DWM hides windows on other virtual desktops and
//! suspended Store apps that way while they still report visible). Flint's
//! own windows and the desktop (`Progman`) are left out. A Store app's
//! window belongs to `ApplicationFrameHost.exe`, so that is the program
//! it shows; finding the app behind it is a later refinement.
//!
//! Switching restores a minimised window and calls `SetForegroundWindow`,
//! which Windows allows because Flint is the foreground process when the
//! user picks a row. Closing posts `WM_CLOSE`, exactly what the window's
//! close button does, so the app may still ask to save.
//!
//! Linux: Wayland doesn't let an ordinary client see or raise other
//! windows; KWin scripting over D-Bus is the way there, later. Listing
//! says so instead of returning nothing silently.

/// A window, as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// Its handle, valid while it is open.
    pub handle: u64,
    /// Its title.
    pub title: String,
    /// Its program's file name (`firefox.exe`); empty when unreadable.
    pub process: String,
}

/// The windows the user could switch to, front to back, or why they
/// can't be listed here.
pub fn list() -> Result<Vec<Window>, String> {
    #[cfg(windows)]
    {
        Ok(windows::list())
    }
    #[cfg(not(windows))]
    {
        Err("listing windows isn't available on Linux yet".into())
    }
}

/// Brings window `handle` to the front, restoring it if minimised.
pub fn focus(handle: u64) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::focus(handle)
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
        Err("switching windows isn't available on Linux yet".into())
    }
}

/// Asks window `handle` to close, as its close button would.
pub fn close(handle: u64) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::close(handle)
    }
    #[cfg(not(windows))]
    {
        let _ = handle;
        Err("closing windows isn't available on Linux yet".into())
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindow, GetWindowLongW, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
        PostMessageW, SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_RESTORE, WM_CLOSE,
        WS_EX_TOOLWINDOW,
    };

    use super::Window;
    use crate::foreground::windows::process_name;

    pub fn list() -> Vec<Window> {
        let mut handles: Vec<HWND> = Vec::new();
        unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
            // SAFETY: `found` is the `&mut Vec` passed below, alive for
            // the whole of EnumWindows.
            let handles = unsafe { &mut *(found as *mut Vec<HWND>) };
            handles.push(window);
            1
        }
        // SAFETY: the callback only pushes to the vector it is given.
        unsafe { EnumWindows(Some(each), &mut handles as *mut Vec<HWND> as LPARAM) };
        let own = std::process::id();
        handles
            .into_iter()
            .filter_map(|window| {
                // SAFETY: plain queries on a window handle; a window that
                // closed meanwhile just fails them.
                unsafe {
                    if IsWindowVisible(window) == 0 || !GetWindow(window, GW_OWNER).is_null() {
                        return None;
                    }
                    if GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0 {
                        return None;
                    }
                    let mut cloaked: u32 = 0;
                    let got = DwmGetWindowAttribute(
                        window,
                        DWMWA_CLOAKED as u32,
                        (&mut cloaked as *mut u32).cast(),
                        std::mem::size_of::<u32>() as u32,
                    );
                    if got == 0 && cloaked != 0 {
                        return None;
                    }
                    let mut class = [0u16; 64];
                    let n = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32);
                    if String::from_utf16_lossy(&class[..n.max(0) as usize]) == "Progman" {
                        return None;
                    }
                    let length = GetWindowTextLengthW(window);
                    if length <= 0 {
                        return None;
                    }
                    let mut title = vec![0u16; length as usize + 1];
                    let n = GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32);
                    let title = String::from_utf16_lossy(&title[..n.max(0) as usize]);
                    if title.trim().is_empty() {
                        return None;
                    }
                    let mut pid = 0;
                    GetWindowThreadProcessId(window, &mut pid);
                    if pid == own {
                        return None;
                    }
                    Some(Window {
                        handle: window as u64,
                        title,
                        process: process_name(pid),
                    })
                }
            })
            .collect()
    }

    fn window(handle: u64) -> Result<HWND, String> {
        let window = handle as HWND;
        // SAFETY: IsWindow accepts any value and says whether it names a
        // window.
        if unsafe { IsWindow(window) } == 0 {
            return Err("that window has closed".into());
        }
        Ok(window)
    }

    pub fn focus(handle: u64) -> Result<(), String> {
        let window = window(handle)?;
        // SAFETY: calls on a window handle checked just above.
        unsafe {
            if IsIconic(window) != 0 {
                ShowWindow(window, SW_RESTORE);
            }
            if SetForegroundWindow(window) == 0 {
                return Err("Windows didn't let Flint bring that window forward".into());
            }
        }
        Ok(())
    }

    pub fn close(handle: u64) -> Result<(), String> {
        let window = window(handle)?;
        // SAFETY: posting a message with no pointers in it.
        if unsafe { PostMessageW(window, WM_CLOSE, 0, 0) } == 0 {
            return Err(format!(
                "couldn't ask the window to close: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn listing_never_fails_badly() {
        // In CI there may be no windows at all, and on Linux it says why;
        // either way, no panic and no window of Flint's own.
        match super::list() {
            Ok(windows) => assert!(windows.iter().all(|w| !w.title.trim().is_empty())),
            Err(why) => assert!(!why.contains('\n')),
        }
        // A handle that isn't a window is an error, not a crash.
        assert!(super::focus(0).is_err());
    }
}
