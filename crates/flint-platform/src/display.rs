//! Which display server this session runs on, where it changes what Flint
//! can do: Wayland has no global hotkeys and no hidden windows.

/// Whether this is a Wayland session. Decided from the environment, as
/// winit itself decides which backend to use, so the two agree.
pub fn is_wayland() -> bool {
    if cfg!(windows) {
        return false;
    }
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty())
}
