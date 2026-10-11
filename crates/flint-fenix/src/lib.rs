//! Everything Flint knows about Fenix, in one place: locating the `fenix`
//! executable, reading Fenix's state files (known projects, recent files)
//! and its theme and font settings, the control-protocol client with
//! capability detection, and the legacy hand-off fallback. It exposes
//! Fenix to the rest of Flint as a provider.
//!
//! The integration is specified in `docs/FENIX-INTEGRATION.md`. Two rules
//! from it shape this crate. Flint never writes Fenix's files: it reads
//! them defensively (missing or unreadable means no items, as Fenix itself
//! treats them), and anything that changes Fenix's data goes through Fenix.
//! And Flint asks what Fenix can do rather than checking versions, so an
//! older Fenix disables individual actions, each with its reason, instead
//! of breaking Flint.
//!
//! Fenix's state files are read by key through `fenix-storage`, not
//! through `fenix-project`, which would pull in Fenix's MIB and CCSDS
//! crates to read two JSON keys; F7 would turn those keys into a contract.
//!
//! Empty until M3 (reading state, the existing hand-off); the control
//! protocol arrives in M7, once Fenix has F1–F6.
