//! Finding and ordering candidates: the candidate store, scope prefixes
//! (`j ABC-123`), fuzzy matching, frecency, and the stable merge of
//! results that arrive while the list is on screen.
//!
//! This is where the 16 ms keystroke budget is won or lost, so it is pure
//! and synchronous: given candidates and a query it returns ranked hits
//! with match positions, and a benchmark over 100k synthetic candidates
//! gates CI (`examples/bench.rs`, run by `scripts/ci.ps1`). It knows nothing
//! about where candidates come from (`flint-providers`, `flint-host`), how
//! they are drawn (`flint-gui`), or where frecency is stored; it computes
//! with data handed to it, including the current time.
//!
//! Matching uses `nucleo-matcher` rather than `fenix-picker`: Flint needs
//! match positions for highlighting and must stay fast at 100k candidates,
//! and `fenix-picker`'s scorer is deliberately simple and returns neither.

pub mod frecency;
pub mod index;
pub mod merge;
pub mod query;

pub use frecency::Frecency;
pub use index::{Candidate, Hit, Index, Search};
pub use merge::stable_merge;
pub use query::split_scope;
