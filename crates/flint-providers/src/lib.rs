//! The providers built into Flint: apps, quicklinks, Flint's own commands
//! (M2), then the calculator, the shell prefix and the window list (M6).
//!
//! Each produces items and actions in exactly the shape an external
//! provider sends over `docs/PROTOCOL.md`, so the index, the action menu
//! and frecency treat built-in and external results the same way, and a
//! built-in provider could move out of process without its callers
//! noticing. Discovering apps and listing windows are OS questions and are
//! asked through `flint-platform`'s traits; this crate turns the answers
//! into items. Nothing here interpolates a query into a shell string:
//! `run` actions carry an argv array.
//!
//! Fenix's projects, recent files and commands are not here; they are
//! `flint-fenix`'s, which owns everything that knows about Fenix.
//!
//! Empty until M2.
