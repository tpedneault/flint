//! Flint's `settings.toml` and folders: every setting declared once in a
//! schema with its default, its allowed range or choices and a description;
//! reading, validating and live-reloading the file; and resolving Flint's
//! settings, state and log folders (`FLINT_HOME` included).
//!
//! It mirrors `fenix-config`'s conventions so someone who edits one file
//! can edit the other. A bad value costs only that setting: it falls back
//! to its default and produces one line of explanation for the footer and
//! `flint reload`, never an error that stops Flint starting. The settings
//! table in `docs/CONFIGURATION.md` is generated from the schema, and a test
//! fails when the two drift.
//!
//! Folder resolution copies `fenix_storage::paths`'s rules for `flint`
//! instead of `fenix` (a dozen lines) because that module hard-codes the
//! app name; F14 in `docs/FENIX-INTEGRATION.md` would let both share one
//! implementation. This crate never reads Fenix's settings; following
//! Fenix's theme is `flint-fenix`'s job.
//!
//! Empty until M1.
