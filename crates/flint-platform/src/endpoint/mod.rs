//! The control endpoint: a channel only this user can reach, which the
//! daemon listens on and `flint toggle` and its siblings connect to.
//!
//! Windows: a named pipe whose name holds the user's SID and the login
//! session, created with a DACL granting that SID alone and with
//! `FILE_FLAG_FIRST_PIPE_INSTANCE`, so nothing else can create the name
//! first and receive our requests. The session is in the name because pipe
//! names are machine-wide: the same user logged in twice on a Remote
//! Desktop host must get two daemons, each showing its window in its own
//! session.
//!
//! Linux: a Unix socket in `$XDG_RUNTIME_DIR/flint/` (a `0700` directory),
//! or `/tmp/flint-$UID/` checked to be ours when there is no runtime dir.
//! A lock file beside the socket, held for the daemon's life, decides who
//! the daemon is, so a socket left behind by a crash is replaced safely
//! and two daemons starting together can't both bind.
//!
//! Single-instance negotiation is the caller's loop: [`Endpoint::connect`];
//! if nobody answers, [`Endpoint::bind`]; if that reports
//! [`BindError::InUse`], another daemon is starting, so connect again.
//!
//! Not Fenix's fixed loopback TCP port: one port per machine lets the first
//! user's daemon receive every other user's requests (F2 in
//! FENIX-INTEGRATION.md).

use std::ffi::OsStr;
use std::fmt;
use std::io;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::{Listener, Stream};
#[cfg(windows)]
pub use self::windows::{Listener, Stream};

/// Why the daemon couldn't take the endpoint.
#[derive(Debug)]
pub enum BindError {
    /// Another daemon has it (or is taking it right now).
    InUse,
    /// Anything else, which the daemon reports and exits on.
    Io(io::Error),
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindError::InUse => f.write_str("another Flint daemon is running"),
            BindError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BindError {}

impl From<io::Error> for BindError {
    fn from(e: io::Error) -> Self {
        BindError::Io(e)
    }
}

/// Which endpoint: this user's, or a separate one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Appended to the endpoint's name; empty for the user's own.
    instance: String,
}

impl Endpoint {
    /// This user's endpoint. When `FLINT_HOME` is set, the endpoint is
    /// that home's own, so a portable or test Flint runs beside the
    /// installed one instead of handing its requests to it.
    pub fn for_user(flint_home: Option<&OsStr>) -> Self {
        let instance = match flint_home.filter(|h| !h.is_empty()) {
            Some(home) => format!("{:016x}", fnv1a(home.as_encoded_bytes())),
            None => String::new(),
        };
        Endpoint { instance }
    }

    /// A separate endpoint named by `instance`, for tests.
    pub fn instance(instance: &str) -> Self {
        Endpoint {
            instance: instance.to_owned(),
        }
    }

    /// Takes the endpoint and starts listening.
    pub fn bind(&self) -> Result<Listener, BindError> {
        Listener::bind(self)
    }

    /// Connects to a running daemon; `None` when there isn't one.
    pub fn connect(&self) -> io::Result<Option<Stream>> {
        Stream::connect(self)
    }

    fn suffix(&self) -> String {
        if self.instance.is_empty() {
            String::new()
        } else {
            format!("-{}", self.instance)
        }
    }
}

/// FNV-1a, 64-bit: a stable, dependency-free hash for naming an endpoint
/// after a folder. Not for security; the endpoint's access control is.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn unique() -> Endpoint {
        static N: AtomicU32 = AtomicU32::new(0);
        Endpoint::instance(&format!(
            "test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn a_portable_home_gets_its_own_endpoint() {
        let a = Endpoint::for_user(Some(OsStr::new("/a")));
        let b = Endpoint::for_user(Some(OsStr::new("/b")));
        assert_ne!(a, b);
        assert_eq!(a, Endpoint::for_user(Some(OsStr::new("/a"))));
        assert_eq!(
            Endpoint::for_user(Some(OsStr::new(""))),
            Endpoint::for_user(None)
        );
    }

    #[test]
    fn nobody_listening_is_none() {
        assert!(unique().connect().unwrap().is_none());
    }

    #[test]
    fn a_second_bind_is_in_use() {
        let endpoint = unique();
        let _first = endpoint.bind().unwrap();
        assert!(matches!(endpoint.bind(), Err(BindError::InUse)));
    }

    #[test]
    fn the_endpoint_is_free_again_after_the_listener_goes() {
        let endpoint = unique();
        drop(endpoint.bind().unwrap());
        drop(endpoint.bind().unwrap());
    }

    #[test]
    fn clients_and_the_listener_talk_both_ways() {
        let endpoint = unique();
        let mut listener = endpoint.bind().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let stream = listener.accept().unwrap();
                let mut writer = stream.try_clone().unwrap();
                let mut line = String::new();
                BufReader::new(stream).read_line(&mut line).unwrap();
                writer.write_all(format!("echo {line}").as_bytes()).unwrap();
                writer.flush().unwrap();
            }
        });
        for word in ["one", "two"] {
            let stream = endpoint.connect().unwrap().expect("listening");
            let mut writer = stream.try_clone().unwrap();
            writer.write_all(format!("{word}\n").as_bytes()).unwrap();
            writer.flush().unwrap();
            let mut reply = String::new();
            BufReader::new(stream).read_line(&mut reply).unwrap();
            assert_eq!(reply, format!("echo {word}\n"));
        }
        server.join().unwrap();
    }
}
