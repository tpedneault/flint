//! Flint's window and everything drawn in it: the query line with its
//! scope chip, the result list with match highlights, the action menu, the
//! which-key popup, the footer, and theming.
//!
//! Built on the same stack and versions as Fenix (winit 0.30, wgpu 30,
//! glyphon 0.12) so fonts and themes look identical. The window and its
//! GPU surface are created once, hidden, when the daemon starts and are
//! never destroyed: showing is a visibility change, a redraw and a focus
//! request, which is how the 50 ms hotkey-to-frame budget is met. Two
//! differences from Fenix's `gpu.rs` are deliberate: the surface is
//! reconfigured at most once per frame, and when no hardware adapter is
//! usable (VMs, RDP sessions) Flint retries with the fallback adapter
//! rather than failing to start at login.
//!
//! This crate draws state and turns input into intents; ranking, config,
//! providers and OS calls live elsewhere. Together with `flint-platform` it
//! is the only crate that touches the window system.
//!
//! Empty until M1 (pre-created hidden window, query line, static list).
