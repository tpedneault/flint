//! The providers built into Flint: apps, quicklinks and Flint's own
//! commands (M2), then the calculator, the shell prefix and the window
//! list (M6).
//!
//! Each produces items and actions in exactly the shape an external
//! provider sends over `docs/PROTOCOL.md` (`flint_protocol::Item`), so the
//! index, the action handling and frecency treat built-in and external
//! results the same way, and a built-in provider could move out of process
//! without its callers noticing. Listing apps is file-system walking and
//! parsing, which lives here and is tested on any OS; launching what they
//! list is an OS call and belongs to `flint-platform`. Nothing here
//! interpolates a query into a shell string: `run` actions carry an argv.
//!
//! Fenix's projects, recent files and commands are not here; they are
//! `flint-fenix`'s, which owns everything that knows about Fenix.

pub mod apps;
pub mod commands;
pub mod desktop;
pub mod quicklinks;
