//! Everything that depends on the operating system, behind traits: the
//! global hotkey, the per-user control endpoint, the foreground window,
//! focus hand-off, opening and revealing paths, launching programs and
//! terminals, autostart, app discovery and the window list.
//!
//! The traits exist so the logic around them can be tested in CI with
//! fakes, because the behaviour itself can't be: whether a hotkey fires
//! or a window really comes to the front needs a desktop. Implementations
//! are selected with `cfg(windows)` (primary) and `cfg(unix)` (KDE Plasma 6
//! on Wayland, X11 best effort); a macOS port would add a third without
//! touching the callers. Policy stays out: this crate does what it is
//! asked and reports what happened, and deciding *what* to open or *when*
//! to grant focus is the caller's.
//!
//! Together with `flint-gui`, this is the only crate allowed to touch the
//! OS or the window system directly; a workspace test enforces that on
//! the dependency lists.
//!
//! Empty until M1 (control endpoint, hotkey, activation tokens).
