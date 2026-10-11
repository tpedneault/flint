//! The control endpoint on Linux: a Unix socket in a private directory,
//! with a lock file deciding who the daemon is. See the parent module.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use super::{BindError, Endpoint};

/// The daemon's end.
pub struct Listener {
    listener: UnixListener,
    socket: PathBuf,
    /// Held, locked, for the daemon's life: the lock is what makes this
    /// process the daemon, not the socket file.
    _lock: File,
}

/// One connected client or daemon.
pub struct Stream {
    stream: UnixStream,
}

impl Listener {
    pub(super) fn bind(endpoint: &Endpoint) -> Result<Listener, BindError> {
        let dir = private_dir()?;
        let (socket, lock_path) = paths(&dir, endpoint);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(&lock_path)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(BindError::InUse),
            Err(fs::TryLockError::Error(e)) => return Err(BindError::Io(e)),
        }
        // Holding the lock, any socket file here belongs to a daemon that
        // is gone.
        match fs::remove_file(&socket) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(BindError::Io(e)),
        }
        let listener = UnixListener::bind(&socket)?;
        Ok(Listener {
            listener,
            socket,
            _lock: lock,
        })
    }

    /// Waits for the next client.
    pub fn accept(&mut self) -> io::Result<Stream> {
        let (stream, _) = self.listener.accept()?;
        Ok(Stream { stream })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

impl Stream {
    pub(super) fn connect(endpoint: &Endpoint) -> io::Result<Option<Stream>> {
        let dir = private_dir()?;
        let (socket, _) = paths(&dir, endpoint);
        match UnixStream::connect(&socket) {
            Ok(stream) => Ok(Some(Stream { stream })),
            // No socket, or one nobody is listening on (left by a crash).
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// A second handle to the same connection.
    pub fn try_clone(&self) -> io::Result<Stream> {
        Ok(Stream {
            stream: self.stream.try_clone()?,
        })
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn paths(dir: &Path, endpoint: &Endpoint) -> (PathBuf, PathBuf) {
    let suffix = endpoint.suffix();
    (
        dir.join(format!("control{suffix}.sock")),
        dir.join(format!("control{suffix}.lock")),
    )
}

/// `$XDG_RUNTIME_DIR/flint`, or `/tmp/flint-$UID`, created `0700` and
/// checked to be a real directory owned by us and closed to others before
/// anything is put in it. `/tmp` is shared, so someone else could have
/// made the directory first; that is refused rather than used.
fn private_dir() -> io::Result<PathBuf> {
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let dir = match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(runtime) => PathBuf::from(runtime).join("flint"),
        None => std::env::temp_dir().join(format!("flint-{uid}")),
    };
    match DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = fs::symlink_metadata(&dir)?;
    let problem = if !meta.file_type().is_dir() {
        Some("isn't a directory")
    } else if meta.uid() != uid {
        Some("belongs to another user")
    } else if meta.permissions().mode() & 0o077 != 0 {
        Some("is open to other users")
    } else {
        None
    };
    match problem {
        Some(problem) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} {problem}; Flint won't use it", dir.display()),
        )),
        None => Ok(dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directory_is_private() {
        let dir = private_dir().unwrap();
        let mode = fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{}", dir.display());
    }

    #[test]
    fn a_socket_left_by_a_crash_is_replaced() {
        let endpoint = Endpoint::instance(&format!("stale-{}", std::process::id()));
        let (socket, _) = paths(&private_dir().unwrap(), &endpoint);
        // A bound-then-dropped std listener leaves its file behind, as a
        // crashed daemon would.
        drop(UnixListener::bind(&socket).unwrap());
        assert!(socket.exists());
        assert!(endpoint.connect().unwrap().is_none());
        let _listener = endpoint.bind().unwrap();
        assert!(endpoint.connect().unwrap().is_some());
    }
}
