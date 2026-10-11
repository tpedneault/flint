//! Everything Flint knows about Fenix, in one place: reading Fenix's state
//! files (known projects, recent files) and its theme and font settings,
//! turning them into items, locating the `fenix` executable, and carrying
//! out Fenix actions through the hand-off Fenix has today. From M7, the
//! control-protocol client with capability detection.
//!
//! The integration is specified in `docs/FENIX-INTEGRATION.md`. Two rules
//! from it shape this crate. Flint never writes Fenix's files: it reads
//! them defensively (missing or unreadable means no items, as Fenix itself
//! treats them), and anything that changes Fenix's data goes through Fenix.
//! And Flint asks what Fenix can do rather than checking versions, so an
//! older Fenix disables individual actions, each with its reason, instead
//! of breaking Flint.
//!
//! Fenix's folders are resolved by `fenix_storage::paths` (so a portable
//! `FENIX_HOME` install is found), and its state files are read by key
//! through `fenix_storage::state`, not through `fenix-project`, which would
//! pull in Fenix's MIB and CCSDS crates to read two JSON keys; F7 would
//! turn those keys into a contract.
//!
//! M3 is "level 0": everything here works with Fenix as it is at the
//! pinned revision, and nothing needs a Fenix change.

pub mod handoff;
pub mod items;
pub mod locate;
pub mod look;
pub mod state;

pub use fenix_storage::paths::Roots;
pub use look::Look;
pub use state::{FenixState, Stamps};
