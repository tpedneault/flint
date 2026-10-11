//! Finding and ordering candidates: the candidate store, query parsing
//! (scope prefixes such as `j ABC-123`), fuzzy matching, frecency, and the
//! stable merge of results that arrive while the list is on screen.
//!
//! This is where the 16 ms keystroke budget is won or lost, so it is pure
//! and synchronous: given candidates and a query it returns ranked rows
//! with match positions, and a benchmark over 100k synthetic candidates
//! gates CI (from M2). It knows nothing about where candidates come from
//! (that is `flint-providers` and `flint-host`), how they are drawn
//! (`flint-gui`), or where frecency is stored; it computes frecency from
//! data handed to it and hands back what changed.
//!
//! Matching uses `nucleo-matcher` rather than `fenix-picker`: Flint needs
//! match positions for highlighting and must stay fast at 100k candidates,
//! and `fenix-picker`'s scorer is deliberately simple and returns neither.
//!
//! Empty until M2.
