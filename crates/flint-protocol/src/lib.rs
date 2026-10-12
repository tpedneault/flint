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
//! `docs/FENIX-INTEGRATION.md`), this crate carries its own ([`frame`])
//! with the same semantics plus the 4 MiB body and 1 KiB header-line
//! limits, tested against `fenix-rpc`'s.
//!
//! The envelope, `hello` and the client messages arrived in M1, items and
//! actions in M2 (the built-in providers produce them), and the provider
//! messages (`provider/register`, `query`, `results`, ...) in M5.

pub mod client;
pub mod connection;
pub mod frame;
pub mod hello;
pub mod item;
pub mod message;
pub mod provider;
pub mod session;

pub use client::ClientRequest;
pub use connection::{CallError, ClientConnection};
pub use hello::{Hello, HelloResult, HostInfo, Role, PROTOCOL_VERSION};
pub use item::Item;
pub use message::{code, Id, Message, RpcError};
pub use session::{Action, HostSession};
