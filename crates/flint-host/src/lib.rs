//! The host side of every channel Flint serves: the control server for CLI
//! clients and connecting providers ([`control`]), and the provider host
//! ([`providers`]) — spawning declared providers and serving the ones that
//! connect, routing queries and `execute` requests, enforcing timeouts and
//! the 2 s result deadline, and isolating crashes (restart with 1 s, 5 s,
//! 30 s backoff; disabled after three crashes in a minute, with the reason
//! shown).
//!
//! The threads that talk to peers live here, so the concurrency between
//! Flint and its peers is reasoned about in one place. A peer may be slow,
//! hang, flood or send garbage, and none of that may reach the UI thread as
//! anything but a dropped result or a one-line reason. A provider runs only
//! if `settings.toml` declares it: there is no discovery and nothing loads
//! in-process.
//!
//! Messages, framing, limits and the handshake rules come from
//! `flint-protocol`. Transports are passed in as plain `Read`/`Write`
//! halves, so this crate never touches a socket and its tests run over
//! in-memory pipes.

pub mod control;
pub mod providers;

pub use control::{serve, serve_connection, ClientHandler};
pub use providers::{HostEvent, ProviderConfig, ProviderHost, Secret, State, Status, Timing};
