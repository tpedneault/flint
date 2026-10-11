//! The control endpoint on Windows: a per-user, per-session named pipe.
//! See the parent module for why it is shaped this way.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr;

use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY,
    ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, WaitNamedPipeW, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcessToken,
};

use super::{BindError, Endpoint};

const BUFFER: u32 = 64 * 1024;

/// The daemon's end: hands out one [`Stream`] per client.
pub struct Listener {
    name: Vec<u16>,
    security: Security,
    /// The instance the next client will connect to. Created ahead, so a
    /// client arriving between two `accept`s finds one waiting.
    pending: HANDLE,
}

// The handle is only used from whichever thread owns the listener.
unsafe impl Send for Listener {}

/// One connected client or daemon.
pub struct Stream {
    file: File,
}

impl Listener {
    pub(super) fn bind(endpoint: &Endpoint) -> Result<Listener, BindError> {
        let user = UserIdentity::current()?;
        let name = wide(&pipe_name(endpoint, &user));
        // Full access for this user's SID and nobody else; `P` stops
        // inherited entries from widening it.
        let security = Security::from_sddl(&format!("D:P(A;;GA;;;{})", user.sid))?;
        let pending = match create_instance(&name, &security, true) {
            Ok(handle) => handle,
            // `FILE_FLAG_FIRST_PIPE_INSTANCE` fails with access denied
            // when an instance of the name exists: another daemon.
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                return Err(BindError::InUse)
            }
            Err(e) => return Err(BindError::Io(e)),
        };
        Ok(Listener {
            name,
            security,
            pending,
        })
    }

    /// Waits for the next client.
    pub fn accept(&mut self) -> io::Result<Stream> {
        if self.pending == INVALID_HANDLE_VALUE {
            self.pending = create_instance(&self.name, &self.security, false)?;
        }
        let handle = self.pending;
        // SAFETY: `handle` is a pipe instance this listener created and
        // still owns; a null OVERLAPPED makes this a blocking wait.
        let connected = unsafe { ConnectNamedPipe(handle, ptr::null_mut()) } != 0;
        if !connected {
            let error = io::Error::last_os_error();
            // A client that connected between creation and this call is
            // connected all the same.
            if error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
                return Err(error);
            }
        }
        self.pending = INVALID_HANDLE_VALUE;
        // SAFETY: ownership of the connected instance moves to the File.
        let file = unsafe { File::from_raw_handle(handle as _) };
        // Have the next instance ready before this client is served, so a
        // second client never finds the name missing and concludes there's
        // no daemon.
        self.pending =
            create_instance(&self.name, &self.security, false).unwrap_or(INVALID_HANDLE_VALUE);
        Ok(Stream { file })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if self.pending != INVALID_HANDLE_VALUE {
            // SAFETY: an instance this listener created and never handed out.
            unsafe { CloseHandle(self.pending) };
        }
    }
}

impl Stream {
    pub(super) fn connect(endpoint: &Endpoint) -> io::Result<Option<Stream>> {
        let user = UserIdentity::current()?;
        let name = wide(&pipe_name(endpoint, &user));
        // A busy pipe means every instance is taken for a moment; the
        // daemon creates the next one right after accepting.
        for _ in 0..20 {
            // SAFETY: `name` is NUL-terminated. Identification-level QoS
            // stops the server from acting as us beyond knowing who we are.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                // SAFETY: a handle we just opened and own.
                let file = unsafe { File::from_raw_handle(handle as _) };
                return Ok(Some(Stream { file }));
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|c| c as u32) {
                Some(ERROR_FILE_NOT_FOUND) => return Ok(None),
                Some(ERROR_PIPE_BUSY) => {
                    // SAFETY: `name` is NUL-terminated.
                    unsafe { WaitNamedPipeW(name.as_ptr(), 250) };
                }
                _ => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Flint's control pipe stayed busy",
        ))
    }

    /// A second handle to the same connection, for reading on one side
    /// and writing on the other.
    pub fn try_clone(&self) -> io::Result<Stream> {
        Ok(Stream {
            file: self.file.try_clone()?,
        })
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // `std` already reads a broken pipe as end of file.
        self.file.read(buf)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    /// Waits until the other side has read everything written. Without
    /// it, a daemon that answers `quit` and exits could take the answer
    /// with it; `File::flush` is a no-op on Windows.
    fn flush(&mut self) -> io::Result<()> {
        // SAFETY: the handle is owned by `self.file` and open.
        if unsafe { FlushFileBuffers(self.file.as_raw_handle() as HANDLE) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

fn create_instance(name: &[u16], security: &Security, first: bool) -> io::Result<HANDLE> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let open_mode = PIPE_ACCESS_DUPLEX
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    // SAFETY: `name` is NUL-terminated and `attributes` outlives the call.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER,
            BUFFER,
            0,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(handle)
}

/// `\\.\pipe\flint-<SID>-<session>[-<instance>]`.
fn pipe_name(endpoint: &Endpoint, user: &UserIdentity) -> String {
    format!(
        r"\\.\pipe\flint-{}-{}{}",
        user.sid,
        user.session,
        endpoint.suffix()
    )
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Who we are: the user's SID as a string, and the login session.
struct UserIdentity {
    sid: String,
    session: u32,
}

impl UserIdentity {
    fn current() -> io::Result<Self> {
        let mut session = 0;
        // SAFETY: plain out-parameter call.
        if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(UserIdentity {
            sid: current_user_sid()?,
            session,
        })
    }
}

fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo-handle for this process needs no closing.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle(token);
    let mut needed = 0;
    // SAFETY: a size query; failing with "insufficient buffer" is expected.
    unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // `u64`s keep the buffer aligned for `TOKEN_USER`.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: `buffer` has at least `needed` bytes.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call filled `buffer` with a TOKEN_USER.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `sid` points into `buffer`, alive for this call.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `text` is a NUL-terminated string the call allocated with
    // LocalAlloc; it is read once and freed.
    let sid = unsafe {
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        s
    };
    Ok(sid)
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: a handle we opened.
        unsafe { CloseHandle(self.0) };
    }
}

/// A security descriptor from SDDL, freed on drop.
struct Security(PSECURITY_DESCRIPTOR);

impl Security {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let sddl = wide(sddl);
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; the descriptor is allocated by
        // the call and freed in `drop`.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Security(descriptor))
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_holds_the_user_and_session() {
        let user = UserIdentity::current().unwrap();
        assert!(user.sid.starts_with("S-1-"), "{}", user.sid);
        let name = pipe_name(&Endpoint::instance("x"), &user);
        assert!(name.starts_with(r"\\.\pipe\flint-S-1-"), "{name}");
        assert!(name.ends_with(&format!("-{}-x", user.session)), "{name}");
        let own = pipe_name(&Endpoint::for_user(None), &user);
        assert!(own.ends_with(&format!("-{}", user.session)), "{own}");
    }
}
