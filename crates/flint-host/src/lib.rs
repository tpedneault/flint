//! The provider host: spawning declared providers and accepting the ones
//! that connect, running the `hello` handshake, routing queries and
//! `execute` requests, enforcing timeouts and the 2 s result deadline, and
//! isolating crashes (restart with 1 s, 5 s, 30 s backoff; disabled after
//! three crashes in a minute, with the reason shown).
//!
//! The threads that talk to peers live here, so the concurrency between
//! Flint and its providers is reasoned about in one place. A provider is a
//! peer that may be slow, hang, flood or send garbage, and none of that may
//! reach the UI thread as anything but a dropped result or a one-line
//! reason. A provider runs only if `settings.toml` declares it: there is no
//! discovery and nothing loads in-process.
//!
//! Messages, framing and their limits come from `flint-protocol`; the
//! control endpoint's transport comes from `flint-platform`.
//!
//! Empty until M5.
