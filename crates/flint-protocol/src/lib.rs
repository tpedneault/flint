//! The wire protocol between Flint and its providers and clients, as
//! specified in `docs/PROTOCOL.md`: message types, the `hello` handshake
//! and its state machine, error codes, and the size limits.
//!
//! This crate is the specification in code; when it and `PROTOCOL.md`
//! disagree, one of them is fixed in the same change. It is deliberately
//! pure: no threads, no sockets, no processes. It reads and writes
//! `BufRead`/`Write`, so the same code serves a spawned provider's stdio,
//! the control endpoint, and in-memory buffers in tests. Spawning, routing
//! and timeouts belong to `flint-host`.
//!
//! Framing is the `Content-Length` header LSP and DAP use, byte-for-byte
//! what `fenix-rpc` writes, so Flint, Fenix's language-server client and
//! Fenix's control protocol share one format. Writing goes through
//! `fenix_rpc::write_message`. Reading does not: `fenix-rpc`'s reader
//! trusts the header and allocates whatever it says, which is acceptable
//! for a child process Fenix chose to spawn but not for an endpoint other
//! processes can reach. Until Fenix gains a bounded reader (F16 in
//! `docs/FENIX-INTEGRATION.md`), this crate carries its own with the same
//! semantics plus the 4 MiB body and 1 KiB header-line limits, tested
//! against `fenix-rpc`'s.
//!
//! Empty until M5 (M1 adds the client messages the CLI needs).
