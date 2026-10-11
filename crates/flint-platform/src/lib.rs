//! Everything that depends on the operating system: the global hotkey,
//! the per-user control endpoint, focus hand-off, the console, and facts
//! about this process and session. Later milestones add the foreground
//! window, opening and revealing paths, launching programs and terminals,
//! autostart, app discovery and the window list.
//!
//! Where the daemon's logic depends on OS behaviour it reaches it through
//! a trait ([`hotkey::Hotkey`]) so that logic is tested in CI with fakes,
//! because the behaviour itself can't be: whether a hotkey fires or a
//! window really comes to the front needs a desktop. The endpoint has no
//! trait: it is tested for real on both CI platforms, and the code that
//! serves it (`flint-host`) works over any `Read`/`Write`.
//!
//! Implementations are selected with `cfg(windows)` (primary) and
//! `cfg(unix)` (KDE Plasma 6 on Wayland, X11 best effort); a macOS port
//! would add a third without touching the callers. Policy stays out: this
//! crate does what it is asked and reports what happened, and deciding
//! *what* to open or *when* to grant focus is the caller's.
//!
//! Together with `flint-gui`, this is the only crate allowed to touch the
//! OS or the window system directly; a workspace test enforces that on
//! the dependency lists.

pub mod console;
pub mod display;
pub mod endpoint;
pub mod focus;
pub mod hotkey;
pub mod process;

pub use endpoint::{BindError, Endpoint, Listener, Stream};
pub use hotkey::{Hotkey, HotkeyError, SystemHotkey};
