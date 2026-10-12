//! Reaching the running Fenix: a connection to its control endpoint
//! (Fenix's `docs/CONTROL.md`), for `flint-fenix` to speak the protocol
//! over.
//!
//! The endpoint's name and the way to open it are Fenix's, taken from its
//! own `fenix-control` crate rather than rebuilt here: a named pipe per
//! user SID and login session on Windows, opened with identification-only
//! impersonation and checked to be served by this user; a socket in a
//! private directory on Linux. With `FENIX_HOME` set (a portable or test
//! Fenix) the endpoint is that Fenix's, as Fenix itself computes it, the
//! same variable Flint reads Fenix's state files by.
//!
//! What goes over the connection is no business of this module's: it
//! hands back a reader and a writer, and the protocol lives in
//! `flint-fenix`, tested without any OS.

use std::io::{self, Read, Write};

/// A connection to Fenix: what to read its replies from, and where to
/// write requests.
pub type Connection = (Box<dyn Read + Send>, Box<dyn Write + Send>);

/// Connects to the Fenix running for this user (and `FENIX_HOME`), or
/// `None` when no Fenix serves the endpoint: none is running, or one too
/// old to have the control channel.
pub fn connect() -> io::Result<Option<Connection>> {
    let home = std::env::var_os("FENIX_HOME");
    let endpoint = fenix_control::Endpoint::for_home(home.as_deref());
    let Some(stream) = endpoint.connect()? else {
        return Ok(None);
    };
    let writer = stream.try_clone()?;
    Ok(Some((Box::new(stream), Box::new(writer))))
}

#[cfg(test)]
mod tests {
    #[test]
    fn no_fenix_is_none_not_an_error() {
        // A FENIX_HOME nobody runs: nothing serves its endpoint.
        let endpoint =
            fenix_control::Endpoint::instance(&format!("flint-test-{}", std::process::id()));
        assert!(endpoint.connect().unwrap().is_none());
    }
}
