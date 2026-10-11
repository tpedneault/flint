//! Letting another process come to the front.
//!
//! Windows lets only the foreground process give focus away. When the
//! user runs `flint toggle` (from a terminal, a shortcut, a script), that
//! client process is the one in the foreground, not the daemon, so before
//! asking the daemon to show itself the client grants it the right with
//! `AllowSetForegroundWindow(daemon_pid)`; the pid comes from the
//! daemon's `hello` answer. Later milestones grant Fenix and launched
//! programs the same way. Elsewhere this does nothing: X11 and Wayland
//! have their own rules (activation tokens, on Wayland).

/// Allows process `pid` to take the foreground. Best effort: a refusal
/// only means the window may appear behind others, which is reported
/// nowhere because nothing better could be done about it.
pub fn allow_foreground(pid: u32) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
        // SAFETY: plain call with an integer argument.
        unsafe { AllowSetForegroundWindow(pid) };
    }
    #[cfg(not(windows))]
    let _ = pid;
}

/// Allows any process to take the foreground, until the user's next input.
///
/// For hand-offs where the process that will show a window isn't the one
/// Flint starts: `fenix <file>` passes the file to the Fenix already
/// running and exits, and it is that Fenix which must come forward. Until
/// Fenix reports its pid over a control protocol (F1, F3), Flint can't
/// name it, so it lifts the lock for everyone for that one moment; the
/// grant ends at the next key press or click.
pub fn allow_any_foreground() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
        // SAFETY: plain call with an integer argument.
        unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    }
}
