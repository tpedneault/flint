//! Flint's window and everything drawn in it: the query line with its
//! scope chip and mode badge, the result list with match highlights and
//! provider badges, the footer, and theming; the action menu and which-key
//! popup arrive in M4.
//!
//! Built on the same stack and versions as Fenix (winit 0.30, wgpu 30,
//! glyphon 0.12) so fonts and themes look identical. On Windows and X11
//! the window and its GPU surface are created once, hidden, when the
//! daemon starts and are never destroyed: showing is a visibility change,
//! a focus request and a frame, which is how the 50 ms hotkey-to-frame
//! budget is met. Wayland can't hide a window, so there it is made on each
//! show and dropped on hide, keeping the GPU device and fonts ([`app`]
//! explains). Two differences from Fenix's `gpu.rs` are deliberate: the
//! surface is reconfigured at most once per frame, and when no hardware
//! adapter is usable (VMs, RDP sessions) Flint retries with the fallback
//! adapter rather than failing to start at login ([`gpu`]).
//!
//! This crate draws state and turns input into changes of that state;
//! config, providers, the hotkey and the control endpoint live elsewhere,
//! and the daemon drives the window through [`Controller`] and [`Ui`].
//! Together with `flint-platform` it is the only crate that touches the
//! window system. What can be tested without a display is kept in plain
//! modules ([`launcher`], [`layout`], [`keys`], [`theme`]).

pub mod app;
pub mod gpu;
pub mod keys;
pub mod launcher;
pub mod layout;
mod rect;
mod render;
pub mod theme;

pub use app::{Controller, Gui, Proxy, ShowRequest, Shown, Ui};
pub use gpu::AdapterSummary;
pub use launcher::{Chip, FooterLine, Mode, Row};
pub use theme::Palette;
